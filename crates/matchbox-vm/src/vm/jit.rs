use super::chunk::ChunkProgram;
use crate::types::BxVM;
/// JIT compilation of hot bytecode loops using Cranelift.
///
/// Two tiers:
///  1. `FOR_LOOP_STEP` self-loops (empty body, offset == 3) — O(1) via
///     `select(fcmp, limit, start)`.  This is dead-code elimination: if the
///     loop body is empty there is nothing to run, so we can jump to the end.
///
///  2. `FOR_LOOP_STEP` loops with a numeric-only body — real code generation.
///     We translate each bytecode instruction 1:1 into Cranelift SSA IR and
///     emit a native loop with a backward `brif`.  Locals are kept in SSA
///     block-params (registers) throughout; only the final values are written
///     back to the VM's locals array on exit.  This eliminates interpreter
///     dispatch overhead without cheating on the iteration count.
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Weak};

use cranelift_codegen::Context;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::types::{F64, I32, I64};
use cranelift_codegen::ir::{AbiParam, BlockArg, InstBuilder, MemFlags, UserFuncName};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

use super::opcode::op;

/// Threshold before JIT-compiling a loop site.
const JIT_THRESHOLD: u64 = 10_000;

// ── Type aliases for the two compiled function kinds ─────────────────────────

/// Tier-1 (empty loop): skips to the final counter value in one call.
/// `fn(i_current: f64, limit: f64) -> f64`
pub type JitLoopFn = unsafe extern "C" fn(f64, f64) -> f64;

/// Tier-2 (generic numeric body): runs the native loop, writes modified locals
/// back in-place through the pointer. Returns 0 if fully completed, or 1 if it
/// bailed out early (deoptimized) so the interpreter can resume.
/// `fn(locals_ptr: *mut u64, heap_ptr: *const std::ffi::c_void) -> u64`
pub type GenericJitLoopFn = unsafe extern "C" fn(*mut u64, *const std::ffi::c_void) -> u64;

/// Tier-3 (array iteration): iterates a float-only array, writes locals back.
/// fn(locals_ptr: *mut u64, array_data: *const u64, array_len: u64) -> u64
/// Returns 0 = loop completed, 1 = deoptimized (type guard failed).
pub type ArrayIterJitFn = unsafe extern "C" fn(*mut u64, *const u64, u64) -> u64;

/// Tier-4 (hot function): compiles an entire function body to native code.
/// fn(locals_ptr, environment, out_val) -> u64   (0 = ok, 1 = deopt, 2 = yield)
/// - locals_ptr: function's local slots (args at [0..arity], rest are additional locals)
/// - environment: readonly heap/VM access for nonallocating guarded reads
/// - out_val:    write the NaN-boxed return BxValue here on success
pub type HotFnFn = unsafe extern "C" fn(*mut u64, *const std::ffi::c_void, *mut u64) -> u64;

const JIT_FN_THRESHOLD: u64 = 100;
mod construct;

#[derive(Clone)]
pub struct CompiledFn {
    call: HotFnFn,
    record_keys: Option<Arc<[crate::types::box_string::BoxString]>>,
    executed: bool,
    intrinsics: u8,
}

pub(super) enum FnJitOutcome {
    Return(crate::types::BxValue),
    Yield,
    Resume(usize),
}

#[repr(C)]
struct FnReadEnv {
    vm: *const super::VM,
    local_scope: Option<usize>,
}

// Read-only guards: no methods/proxies, coercion, VM allocation or collection.
// Native objects may explicitly opt into allocation-free scalar properties.
fn native_scalar_property(object: &std::rc::Rc<std::cell::RefCell<dyn crate::types::BxNativeObject>>, name: &str) -> Option<crate::types::BxValue> {
    let value = object.try_borrow().ok()?.readonly_jit_property(name)?;
    (value.is_number() || value.is_bool() || value.is_null()).then_some(value)
}
// Temporary Rust string normalization cannot invalidate rooted VM values.
unsafe extern "C" fn jit_read_index(
    env: *const FnReadEnv,
    value: u64,
    index: u64,
    out: *mut u64,
) -> u64 {
    use crate::types::BxValue;
    let env = unsafe { &*env };
    let vm = unsafe { &*env.vm };
    let value: BxValue = unsafe { std::mem::transmute(value) };
    let index: BxValue = unsafe { std::mem::transmute(index) };
    let Some(id) = value.as_gc_id() else { return 1; };
    if let Some(super::gc::GcObject::NativeObject(object)) = vm.heap.get_opt(id) {
        let Some(super::gc::GcObject::String(key)) = index.as_gc_id().and_then(|key| vm.heap.get_opt(key)) else { return 1; };
        let mut key = key.clone();
        let Some(value) = native_scalar_property(object, &key.as_flat_str().to_ascii_lowercase()) else { return 1; };
        unsafe { *out = value.to_bits(); } return 0;
    }
    if matches!(vm.heap.get_opt(id), Some(super::gc::GcObject::Struct(_))) {
        if env.local_scope == Some(id) { return 1; }
        let Some(key_id) = index.as_gc_id() else { return 1; };
        let Some(super::gc::GcObject::String(key)) = vm.heap.get_opt(key_id) else { return 1; };
        let mut key = key.clone();
        unsafe { *out = vm.struct_get(id, key.as_flat_str()).to_bits(); }
        return 0;
    }
    if !index.is_number() {
        return 1;
    }
    let index = index.as_number();
    if !index.is_finite() || index < 1.0 || index != index.trunc() {
        return 1;
    }
    let Some(id) = value.as_gc_id() else {
        return 1;
    };
    let Some(super::gc::GcObject::Array(values)) = vm.heap.get_opt(id) else {
        return 1;
    };
    if index > values.len() as f64 {
        return 1;
    }
    unsafe {
        *out = values[index as usize - 1].to_bits();
    }
    0
}

unsafe extern "C" fn jit_read_member(
    env: *const FnReadEnv,
    value: u64,
    folded: u32,
    exact: u32,
    out: *mut u64,
) -> u64 {
    let env = unsafe { &*env };
    let vm = unsafe { &*env.vm };
    let value: crate::types::BxValue = unsafe { std::mem::transmute(value) };
    let Some(id) = value.as_gc_id() else {
        return 1;
    };
    // A retained execution context can expose this frame's locals through
    // parameters. Its dictionary must not observe staged numeric stack writes.
    if env.local_scope == Some(id) {
        return 1;
    }
    if let Some(super::gc::GcObject::NativeObject(object)) = vm.heap.get_opt(id) {
        let Some(value) = native_scalar_property(object, vm.interner.resolve(folded)) else { return 1; };
        unsafe { *out = value.to_bits(); } return 0;
    }
    let Some(super::gc::GcObject::Struct(record)) = vm.heap.get_opt(id) else {
        return 1;
    };
    let key = if record.case_sensitive { exact } else { folded };
    let Some(index) = vm.shapes.get_index(record.shape_id, key) else {
        return 1;
    };
    unsafe {
        *out = record.properties[index as usize].to_bits();
    }
    0
}

// Each intrinsic is identity-guarded at ordinary function entry, not resolved
// from a forged boxed tag or called through arbitrary user code under raw pointers.
fn intrinsic(name: &str) -> Option<(u8, u32, crate::types::BxNativeFunction)> {
    if name.eq_ignore_ascii_case("int") { Some((1, 1, crate::bifs::int_bif)) }
    else if name.eq_ignore_ascii_case("isStruct") { Some((2, 1, crate::bifs::is_struct_bif)) }
    else if name.eq_ignore_ascii_case("structKeyExists") { Some((4, 2, crate::bifs::struct_key_exists_bif)) }
    else { None }
}

unsafe extern "C" fn jit_parameter_kind(env: *const FnReadEnv, value: u64, kind: u64) -> u64 {
    let env = unsafe { &*env };
    let vm = unsafe { &*env.vm };
    let value: crate::types::BxValue = unsafe { std::mem::transmute(value) };
    let object = value.as_gc_id().and_then(|id| vm.heap.get_opt(id));
    match (kind, object) {
        (3, Some(super::gc::GcObject::Array(_)))
        | (4, Some(super::gc::GcObject::Struct(_)))
        | (5, Some(super::gc::GcObject::String(_))) => 1,
        _ => 0,
    }
}

unsafe extern "C" fn jit_core_read(env: *const FnReadEnv, kind: u64, first: u64, second: u64, out: *mut u64) -> u64 {
    use crate::types::BxValue;
    let env = unsafe { &*env };
    let vm = unsafe { &*env.vm };
    let first: BxValue = unsafe { std::mem::transmute(first) };
    let value = if kind == 2 {
        vm.is_struct_value(first)
    } else if kind == 4 {
        let Some(id) = first.as_gc_id() else { return 1; };
        if env.local_scope == Some(id) { return 1; }
        let second: BxValue = unsafe { std::mem::transmute(second) };
        let Some(key_id) = second.as_gc_id() else { return 1; };
        let Some(super::gc::GcObject::String(key)) = vm.heap.get_opt(key_id) else { return 1; };
        let mut key = key.clone();
        vm.struct_key_exists(id, key.as_flat_str())
    } else { return 1; };
    unsafe { *out = BxValue::new_bool(value).to_bits(); }
    0
}

pub unsafe extern "C" fn jit_ic_member_fallback(
    heap_ptr: *const std::ffi::c_void,
    gc_id: u64,
    expected_shape: u32,
    prop_idx: u32,
    out_val: *mut u64,
) -> u64 {
    // Check if it's a pointer
    if (gc_id & !crate::types::BxValue::PAYLOAD_MASK)
        != (crate::types::BxValue::TAGGED_BASE
            | (crate::types::BxValue::TAG_PTR << crate::types::BxValue::TAG_SHIFT))
    {
        return 1;
    }

    let heap = unsafe { &*(heap_ptr as *const crate::vm::gc::Heap) };
    let id = (gc_id & crate::types::BxValue::PAYLOAD_MASK) as usize;
    // Basic bounds check, although BxValue guarantees validity unless collected
    // but better safe than sorry in JIT.
    if let Some(obj) = heap.get_opt(id) {
        match obj {
            crate::vm::gc::GcObject::Struct(s) => {
                if s.shape_id == expected_shape {
                    if let Some(v) = s.properties.get(prop_idx as usize) {
                        unsafe {
                            *out_val = v.to_bits();
                        }
                        return 0; // Success
                    }
                }
            }
            crate::vm::gc::GcObject::Instance(i) => {
                if i.shape_id == expected_shape {
                    if let Some(v) = i.properties.get(prop_idx as usize) {
                        unsafe {
                            *out_val = v.to_bits();
                        }
                        return 0; // Success
                    }
                }
            }
            _ => {}
        }
    }
    1 // Deopt
}

pub unsafe extern "C" fn jit_get_shape_id(heap_ptr: *const std::ffi::c_void, gc_id: u64) -> u32 {
    if (gc_id & !crate::types::BxValue::PAYLOAD_MASK)
        != (crate::types::BxValue::TAGGED_BASE
            | (crate::types::BxValue::TAG_PTR << crate::types::BxValue::TAG_SHIFT))
    {
        return u32::MAX;
    }
    let heap = unsafe { &*(heap_ptr as *const crate::vm::gc::Heap) };
    let id = (gc_id & crate::types::BxValue::PAYLOAD_MASK) as usize;
    match heap.get_opt(id) {
        Some(crate::vm::gc::GcObject::Struct(s)) => s.shape_id as u32,
        Some(crate::vm::gc::GcObject::Instance(i)) => i.shape_id as u32,
        _ => u32::MAX,
    }
}

pub unsafe extern "C" fn jit_load_prop_at(
    heap_ptr: *const std::ffi::c_void,
    gc_id: u64,
    prop_idx: u32,
    out_val: *mut u64,
) -> u64 {
    let heap = unsafe { &*(heap_ptr as *const crate::vm::gc::Heap) };
    let id = (gc_id & crate::types::BxValue::PAYLOAD_MASK) as usize;
    let props = match heap.get_opt(id) {
        Some(crate::vm::gc::GcObject::Struct(s)) => &s.properties,
        Some(crate::vm::gc::GcObject::Instance(i)) => &i.properties,
        _ => return 1,
    };
    match props.get(prop_idx as usize) {
        Some(v) => {
            unsafe {
                *out_val = v.to_bits();
            }
            0
        }
        None => 1,
    }
}

impl super::VM {
    fn jit_intrinsics_match(&mut self, fiber: usize, mask: u8) -> bool {
        ["int", "isStruct", "structKeyExists"].iter().all(|name| {
            let (kind, _, expected) = intrinsic(name).unwrap();
            if mask & kind == 0 { return true; }
            // Method binding allocates; deopt before the readonly intrinsic guard.
            if self.fibers[fiber].frames.last().unwrap().receiver.is_some_and(|receiver| self.instance_has_method(receiver, name)) { return false; }
            let Some(value) = self.private_value(fiber, name) else { return false; };
            let Some(id) = value.as_gc_id() else { return false; };
            matches!(self.heap.get_opt(id), Some(super::gc::GcObject::NativeFunction(function)) if std::ptr::fn_addr_eq(*function, expected))
        })
    }

    /// Defaults/arguments/interception have already run in the ordinary frame.
    /// A readonly, nonallocating body can safely deopt back to this entry.
    pub(super) fn run_jit_fn_body(
        &mut self,
        fiber: usize,
        entry: usize,
    ) -> Option<crate::types::BxValue> {
        self.jit.as_ref()?;
        #[cfg(feature = "debugger")]
        if self.debug_run.is_some() {
            return None;
        }
        let frame = self.fibers[fiber].frames.last().unwrap();
        let program = Arc::clone(&frame.chunk.borrow().program);
        let key = (
            Arc::as_ptr(&program) as usize,
            entry,
            frame.function.arity,
            0,
        );
        let compiled = self.jit.as_mut()?.profile_fn(
            program,
            entry,
            frame.function.arity,
            0,
            &frame.function.name,
            &mut self.interner,
        )?;
        if !self.jit_intrinsics_match(fiber, compiled.intrinsics) { return None; }
        let base = self.fibers[fiber].frames.last().unwrap().stack_base;
        let parameters = self.fibers[fiber]
            .frames
            .last()
            .unwrap()
            .function
            .params
            .len();
        for slot in 0..parameters {
            let value = self.local_value(fiber, slot);
            self.fibers[fiber].stack[base + slot] = value;
        }
        // Eligibility bounds every GET_LOCAL and rejects writes/calls/allocations.
        // The VM stack cannot reallocate or collect during this native leaf.
        let locals = unsafe { self.fibers[fiber].stack.as_mut_ptr().add(base) as *mut u64 };
        // ponytail: at most two terminal record fields; wider/nested records
        // need GC-rooted spills before enabling their bytecodes.
        let mut results = [0u64; 2];
        let env = FnReadEnv { vm: self as *const _, local_scope: None };
        let status = unsafe {
            (compiled.call)(
                locals,
                &env as *const _ as *const std::ffi::c_void,
                results.as_mut_ptr(),
            )
        };
        if status == 0 {
            let jit = self.jit.as_mut().unwrap();
            if jit.trace {
                let cached = &mut jit.compiled_fns.get_mut(&key).unwrap().1;
                if !cached.executed {
                    cached.executed = true;
                    eprintln!(
                        "[JIT] executed hot fn name={}",
                        self.fibers[fiber].frames.last().unwrap().function.name
                    );
                }
            }
            if let Some(keys) = compiled.record_keys {
                let mut pairs = [(0, crate::types::BxValue::new_null()); 2];
                // Match STRUCT's reverse key interning/presentation, then source order.
                for (i, key) in keys.iter().enumerate().rev() {
                    let mut key = key.clone();
                    pairs[i] = (self.interner.intern(key.as_flat_str()), unsafe {
                        std::mem::transmute::<u64, crate::types::BxValue>(results[i])
                    });
                }
                // Inputs still belong to live frame scopes. Heap::alloc does not
                // collect; ordinary return handling roots the constructed result.
                Some(self.literal_struct(&pairs[..keys.len()]))
            } else {
                Some(unsafe { std::mem::transmute::<u64, crate::types::BxValue>(results[0]) })
            }
        } else {
            if self.jit.as_ref().is_some_and(|jit| jit.trace) {
                eprintln!("[JIT] deopt hot fn body (type guard)");
            }
            None
        }
    }
}

impl super::VM {
    /// OSR after ordinary declarations and at least one interpreted iteration.
    /// Only numeric named-local writes are staged; heap/parameters stay readonly.
    pub(super) fn run_jit_fn_loop(&mut self, fiber: usize, entry: usize) -> Option<FnJitOutcome> {
        use crate::types::BxValue;
        self.jit.as_ref()?;
        #[cfg(feature = "debugger")]
        if self.debug_run.is_some() {
            return None;
        }
        // One feasibility/profiling/guard attempt per invocation, not per
        // interpreted iteration of unsupported loops. Yields reopen the gate.
        let frame = self.fibers[fiber].frames.last_mut().unwrap();
        if frame.jit_loop_attempted {
            return None;
        }
        frame.jit_loop_attempted = true;
        let frame = self.fibers[fiber].frames.last().unwrap();
        let base = frame.stack_base;
        let slots = frame.local_names.iter()
            .filter_map(|(slot, binding)| binding.local.then_some(slot))
            .max().map(|slot| slot + 1)?;
        // ponytail: fixed 32-slot rollback checkpoint; add dynamically rooted
        // spills only if a measured workload needs larger nonallocating kernels.
        if slots > 32 || self.fibers[fiber].stack.len() != base + slots {
            return None;
        }
        let writable = frame
            .local_names
            .iter()
            .filter_map(|(slot, binding)| binding.local.then_some(slot))
            .fold(0u32, |mask, slot| mask | (1 << slot));
        let program = Arc::clone(&frame.chunk.borrow().program);
        let key = (
            Arc::as_ptr(&program) as usize,
            entry,
            slots as u32,
            writable,
        );
        let compiled = self.jit.as_mut()?.profile_fn(
            program,
            entry,
            slots as u32,
            writable,
            &frame.function.name,
            &mut self.interner,
        )?;
        if !self.jit_intrinsics_match(fiber, compiled.intrinsics)
            || self.fibers[fiber].frames.last().unwrap().local_names.values().any(|binding| {
                binding.local && intrinsic(self.interner.resolve(binding.key))
                    .is_some_and(|(kind, _, _)| compiled.intrinsics & kind != 0)
            }) {
            return None; // A staged local write could change the callee binding.
        }
        self.sync_jit_locals(fiber, false);
        let mut saved = [BxValue::new_null(); 32];
        saved[..slots].copy_from_slice(&self.fibers[fiber].stack[base..base + slots]);
        let locals = unsafe { self.fibers[fiber].stack.as_mut_ptr().add(base) as *mut u64 };
        let mut result = 0u64;
        // No calls/effects/GC in this kernel. Array/record inputs stay rooted by
        // the unchanged frame; numeric writes cannot introduce unrooted objects.
        let env = FnReadEnv {
            vm: self as *const _,
            local_scope: self.fibers[fiber]
                .frames
                .last()
                .unwrap()
                .local_scope
                .and_then(|scope| scope.as_gc_id()),
        };
        let status = unsafe {
            (compiled.call)(
                locals,
                &env as *const _ as *const std::ffi::c_void,
                &mut result,
            )
        };
        if status == 1 {
            // Discard the entire attempt, including completed iterations. Since
            // scopes/heap were untouched, replay from this OSR entry is safe.
            self.fibers[fiber].stack[base..base + slots].copy_from_slice(&saved[..slots]);
            if self.jit.as_ref().unwrap().trace {
                eprintln!("[JIT] deopt hot loop fn (guard)");
            }
            return None;
        }
        // Commit only named locals, never parameters/arguments aliases.
        for slot in 0..slots {
            if writable & (1 << slot) != 0 {
                self.set_local_value(fiber, slot, self.fibers[fiber].stack[base + slot]);
            }
        }
        if status == 2 {
            self.fibers[fiber]
                .frames
                .last_mut()
                .unwrap()
                .jit_loop_attempted = false;
            return Some(FnJitOutcome::Yield);
        }
        let jit = self.jit.as_mut().unwrap();
        if jit.trace {
            let cached = &mut jit.compiled_fns.get_mut(&key).unwrap().1;
            if !cached.executed {
                cached.executed = true;
                eprintln!(
                    "[JIT] executed hot loop fn name={}",
                    self.fibers[fiber].frames.last().unwrap().function.name
                );
            }
        }
        Some(FnJitOutcome::Return(unsafe {
            std::mem::transmute::<u64, BxValue>(result)
        }))
    }
}

// ── JitState ──────────────────────────────────────────────────────────────────

pub struct JitState {
    module: JITModule,
    ctx: Context,
    func_ctx: FunctionBuilderContext,
    func_counter: u32,
    trace: bool,

    // Tier-1: empty self-loops
    loop_counts: HashMap<(usize, usize), u64>,
    compiled_loops: HashMap<(usize, usize), JitLoopFn>,

    // Tier-2: generic numeric-body loops
    generic_counts: HashMap<(usize, usize), u64>,
    compiled_generic: HashMap<(usize, usize), GenericJitLoopFn>,

    // Tier-3: array iterator body loops
    iter_counts: HashMap<(usize, usize), u64>,
    compiled_iters: HashMap<(usize, usize), ArrayIterJitFn>,

    // Tier-4: hot function compilation
    fn_counts: HashMap<(usize, usize, u32, u32), (Weak<ChunkProgram>, u64)>,
    compiled_fns: HashMap<(usize, usize, u32, u32), (Arc<ChunkProgram>, CompiledFn)>,
    construct_regions: HashMap<(usize, usize, u32, u32), (Weak<ChunkProgram>, Option<construct::Region>)>,

    // OSR: persistent per-site iteration counters keyed on (fn_id, ip_at_start).
    // Survive across run_fiber quanta so fiber-scheduled loops can cross time-slice
    // boundaries and still accumulate enough iterations to trigger compilation.
    /// Tier-2 loop iteration counts (persists across quanta).
    loop_profiles: HashMap<(usize, usize), u64>,
    /// Tier-3 array-iter iteration counts (persists across quanta).
    iter_profiles: HashMap<(usize, usize), u64>,
}

impl JitState {
    pub fn new() -> anyhow::Result<Self> {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").unwrap();

        let isa_builder =
            cranelift_native::builder().map_err(|e| anyhow::anyhow!("cranelift-native: {}", e))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| anyhow::anyhow!("ISA finish: {}", e))?;

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        builder.symbol(
            "jit_ic_member_fallback",
            jit_ic_member_fallback as *const u8,
        );
        builder.symbol("jit_get_shape_id", jit_get_shape_id as *const u8);
        builder.symbol("jit_load_prop_at", jit_load_prop_at as *const u8);
        builder.symbol("jit_read_index", jit_read_index as *const u8);
        builder.symbol("jit_core_read", jit_core_read as *const u8);
        builder.symbol("jit_parameter_kind", jit_parameter_kind as *const u8);
        builder.symbol("jit_construct_call", construct::call as *const u8);
        builder.symbol("jit_read_member", jit_read_member as *const u8);
        let module = JITModule::new(builder);
        let ctx = module.make_context();
        let trace = std::env::var_os("MATCHBOX_JIT_TRACE").is_some();
        if trace {
            eprintln!("[JIT] Cranelift initialized");
        }

        Ok(Self {
            module,
            ctx,
            func_ctx: FunctionBuilderContext::new(),
            func_counter: 0,
            trace,
            loop_counts: HashMap::new(),
            compiled_loops: HashMap::new(),
            generic_counts: HashMap::new(),
            compiled_generic: HashMap::new(),
            iter_counts: HashMap::new(),
            compiled_iters: HashMap::new(),
            fn_counts: HashMap::new(),
            compiled_fns: HashMap::new(),
            construct_regions: HashMap::new(),
            loop_profiles: HashMap::new(),
            iter_profiles: HashMap::new(),
        })
    }

    // ── OSR: persistent profile counters ─────────────────────────────────────

    /// Increment and return the new cumulative count for a Tier-2 loop site.
    /// Survives across `run_fiber` quanta so long-running loops can cross time-slice
    /// boundaries and eventually reach the compilation threshold.
    pub fn inc_loop_profile(&mut self, fn_id: usize, ip: usize) -> u64 {
        let c = self.loop_profiles.entry((fn_id, ip)).or_insert(0);
        *c += 1;
        *c
    }

    /// Increment and return the new cumulative count for a Tier-3 iter site.
    pub fn inc_iter_profile(&mut self, fn_id: usize, ip: usize) -> u64 {
        let c = self.iter_profiles.entry((fn_id, ip)).or_insert(0);
        *c += 1;
        *c
    }

    // ── Tier-1: empty loop ────────────────────────────────────────────────────

    #[inline]
    pub fn get_compiled_loop(&self, code_ptr: usize, ip: usize) -> Option<JitLoopFn> {
        self.compiled_loops.get(&(code_ptr, ip)).copied()
    }

    pub fn profile_loop(&mut self, code_ptr: usize, ip: usize, iters: u64) -> bool {
        let (prev, new_count) = {
            let count = self.loop_counts.entry((code_ptr, ip)).or_insert(0);
            let prev = *count;
            *count = prev + iters;
            (prev, prev + iters)
        };
        if prev < JIT_THRESHOLD && new_count >= JIT_THRESHOLD {
            match self.compile_empty_loop(code_ptr, ip) {
                Ok(_) => {
                    eprintln!(
                        "[JIT] compiled empty loop @ code=0x{:x} ip={}",
                        code_ptr, ip
                    );
                    return true;
                }
                Err(e) => eprintln!("[JIT] empty loop failed: {}", e),
            }
        }
        false
    }

    fn compile_empty_loop(&mut self, code_ptr: usize, ip: usize) -> anyhow::Result<()> {
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(F64)); // i (already incremented)
        sig.params.push(AbiParam::new(F64)); // limit
        sig.returns.push(AbiParam::new(F64));

        let func_name = format!("jit_empty_x{:x}_ip{}", code_ptr, ip);
        let func_id = self
            .module
            .declare_function(&func_name, Linkage::Local, &sig)?;

        self.ctx.func.name = UserFuncName::user(0, self.func_counter);
        self.func_counter += 1;
        self.ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_ctx);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);

            let v_i = builder.block_params(block)[0];
            let v_limit = builder.block_params(block)[1];
            let cmp = builder.ins().fcmp(FloatCC::LessThan, v_i, v_limit);
            let result = builder.ins().select(cmp, v_limit, v_i);
            builder.ins().return_(&[result]);
            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;

        let code = self.module.get_finalized_function(func_id);
        let fn_ptr: JitLoopFn = unsafe { std::mem::transmute(code) };
        self.compiled_loops.insert((code_ptr, ip), fn_ptr);
        Ok(())
    }

    // ── Tier-2: generic numeric-body loop ─────────────────────────────────────

    #[inline]
    pub fn get_compiled_generic(&self, code_ptr: usize, ip: usize) -> Option<GenericJitLoopFn> {
        self.compiled_generic.get(&(code_ptr, ip)).copied()
    }

    pub fn profile_generic(
        &mut self,
        code_ptr: usize,
        ip: usize,
        iters: u64,
        body_code: &[u32],
        ic_entries: &[Option<crate::vm::chunk::IcEntry>],
        i_slot: u32,
        limit_val: f64,
        constants: &HashMap<u32, f64>,
    ) -> bool {
        let (prev, new_count) = {
            let count = self.generic_counts.entry((code_ptr, ip)).or_insert(0);
            let prev = *count;
            *count = prev + iters;
            (prev, prev + iters)
        };
        if prev < JIT_THRESHOLD && new_count >= JIT_THRESHOLD {
            if !Self::body_is_translatable(body_code, ic_entries, constants) {
                return false;
            }
            match self.compile_generic_loop(
                code_ptr, ip, body_code, ic_entries, i_slot, limit_val, constants,
            ) {
                Ok(_) => {
                    eprintln!(
                        "[JIT] compiled generic loop @ code=0x{:x} ip={} after {} iters",
                        code_ptr, ip, new_count
                    );
                    return true;
                }
                Err(e) => {
                    eprintln!("[JIT] generic loop failed: {}", e);
                    // Also print the context's function for debugging
                    eprintln!("{}", self.ctx.func.display());
                }
            }
        }
        false
    }

    fn body_is_translatable(
        body_code: &[u32],
        ic_entries: &[Option<crate::vm::chunk::IcEntry>],
        constants: &HashMap<u32, f64>,
    ) -> bool {
        for (idx, &word) in body_code.iter().enumerate() {
            let opcode = (word & 0xFF) as u8;
            let op0 = word >> 8;
            match opcode {
                op::GET_LOCAL | op::SET_LOCAL_POP => {}
                op::CONSTANT => {
                    if !constants.contains_key(&op0) {
                        return false; // non-numeric constant
                    }
                }
                op::ADD
                | op::ADD_FLOAT
                | op::ADD_INT
                | op::SUBTRACT
                | op::MULTIPLY
                | op::DIVIDE => {}
                op::MEMBER => match &ic_entries[idx] {
                    Some(crate::vm::chunk::IcEntry::Monomorphic { .. }) => {}
                    Some(crate::vm::chunk::IcEntry::Polymorphic { count, .. }) if *count <= 2 => {}
                    _ => return false,
                },
                _ => return false,
            }
        }
        true
    }

    fn compile_generic_loop(
        &mut self,
        code_ptr: usize,
        ip: usize,
        body_code: &[u32],
        ic_entries: &[Option<crate::vm::chunk::IcEntry>],
        i_slot: u32,
        limit_val: f64,
        constants: &HashMap<u32, f64>,
    ) -> anyhow::Result<()> {
        let mut slot_set: BTreeSet<u32> = BTreeSet::new();
        slot_set.insert(i_slot);
        for &word in body_code {
            let opcode = (word & 0xFF) as u8;
            let op0 = word >> 8;
            if opcode == op::GET_LOCAL || opcode == op::SET_LOCAL_POP {
                slot_set.insert(op0);
            }
        }
        let referenced: Vec<u32> = slot_set.into_iter().collect();
        let n_ref = referenced.len();
        let slot_idx: HashMap<u32, usize> = referenced
            .iter()
            .enumerate()
            .map(|(i, &s)| (s, i))
            .collect();

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type)); // locals_ptr
        sig.params.push(AbiParam::new(ptr_type)); // heap_ptr
        sig.returns.push(AbiParam::new(I64)); // Return status (0=OK, 1=Deopt)

        let func_name = format!("jit_gloop_x{:x}_ip{}", code_ptr, ip);
        let func_id = self
            .module
            .declare_function(&func_name, Linkage::Local, &sig)?;

        self.ctx.func.name = UserFuncName::user(0, self.func_counter);
        self.func_counter += 1;
        self.ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_ctx);

            let entry_block = builder.create_block();
            let loop_header = builder.create_block();
            let loop_body = builder.create_block();
            let loop_exit = builder.create_block();
            let deopt_exit = builder.create_block();

            builder.append_block_params_for_function_params(entry_block);

            // Add heap_ptr as the first parameter to these blocks
            builder.append_block_param(loop_header, ptr_type);
            builder.append_block_param(loop_body, ptr_type);
            builder.append_block_param(loop_exit, ptr_type);
            builder.append_block_param(deopt_exit, ptr_type);

            for _ in 0..n_ref {
                builder.append_block_param(loop_header, I64);
                builder.append_block_param(loop_body, I64);
                builder.append_block_param(loop_exit, I64);
                builder.append_block_param(deopt_exit, I64);
            }

            let mut ext_sig = self.module.make_signature();
            ext_sig.params.push(AbiParam::new(ptr_type)); // heap_ptr
            ext_sig.params.push(AbiParam::new(I64)); // gc_id
            ext_sig.params.push(AbiParam::new(I32)); // expected_shape
            ext_sig.params.push(AbiParam::new(I32)); // prop_idx
            ext_sig.params.push(AbiParam::new(ptr_type)); // out_val
            ext_sig.returns.push(AbiParam::new(I64)); // status
            let ext_func_id = self
                .module
                .declare_function("jit_ic_member_fallback", Linkage::Import, &ext_sig)
                .unwrap();
            let ext_func_ref = self
                .module
                .declare_func_in_func(ext_func_id, &mut builder.func);

            // jit_get_shape_id(heap_ptr, gc_id) -> u32
            let mut get_shape_sig = self.module.make_signature();
            get_shape_sig.params.push(AbiParam::new(ptr_type));
            get_shape_sig.params.push(AbiParam::new(I64));
            get_shape_sig.returns.push(AbiParam::new(I32));
            let get_shape_id = self
                .module
                .declare_function("jit_get_shape_id", Linkage::Import, &get_shape_sig)
                .unwrap();
            let get_shape_ref = self
                .module
                .declare_func_in_func(get_shape_id, &mut builder.func);

            // jit_load_prop_at(heap_ptr, gc_id, prop_idx, out_val) -> u64
            let mut load_prop_sig = self.module.make_signature();
            load_prop_sig.params.push(AbiParam::new(ptr_type));
            load_prop_sig.params.push(AbiParam::new(I64));
            load_prop_sig.params.push(AbiParam::new(I32));
            load_prop_sig.params.push(AbiParam::new(ptr_type));
            load_prop_sig.returns.push(AbiParam::new(I64));
            let load_prop_id = self
                .module
                .declare_function("jit_load_prop_at", Linkage::Import, &load_prop_sig)
                .unwrap();
            let load_prop_ref = self
                .module
                .declare_func_in_func(load_prop_id, &mut builder.func);

            // ── entry_block ──────────────────────────────────────────────────
            builder.switch_to_block(entry_block);
            let locals_ptr = builder.block_params(entry_block)[0];
            let heap_ptr = builder.block_params(entry_block)[1];

            let mut init_vals: Vec<cranelift_codegen::ir::Value> = Vec::new();
            init_vals.push(heap_ptr);
            for &slot in &referenced {
                let offset = (slot * 8) as i32;
                let v = builder.ins().load(I64, MemFlags::new(), locals_ptr, offset);
                init_vals.push(v);
            }
            let init_args: Vec<BlockArg> = init_vals.into_iter().map(BlockArg::from).collect();
            builder.ins().jump(loop_header, &init_args);
            builder.seal_block(entry_block);

            // ── loop_header ──────────────────────────────────────────────────
            builder.switch_to_block(loop_header);
            let header_vals: Vec<cranelift_codegen::ir::Value> =
                builder.block_params(loop_header).to_vec();
            let heap_ptr = header_vals[0];
            let v_i_i64 = header_vals[1 + slot_idx[&i_slot]];

            // Check if i is float (NaN-boxing: < 0xFFF8... is a float)
            let is_float = builder.ins().icmp_imm(
                IntCC::UnsignedLessThan,
                v_i_i64,
                0xFFF8000000000000_u64 as i64,
            );
            let check_limit_block = builder.create_block();

            let header_args: Vec<BlockArg> =
                header_vals.iter().map(|&v| BlockArg::from(v)).collect();
            builder
                .ins()
                .brif(is_float, check_limit_block, &[], deopt_exit, &header_args);

            builder.switch_to_block(check_limit_block);
            builder.seal_block(check_limit_block);

            let v_i_f64 = builder.ins().bitcast(F64, MemFlags::new(), v_i_i64);
            let v_limit = builder.ins().f64const(limit_val);
            let cmp = builder.ins().fcmp(FloatCC::LessThan, v_i_f64, v_limit);
            builder
                .ins()
                .brif(cmp, loop_body, &header_args, loop_exit, &header_args);

            // ── loop_body (bytecode translation) ─────────────────────────────
            builder.switch_to_block(loop_body);
            let body_vals: Vec<cranelift_codegen::ir::Value> =
                builder.block_params(loop_body).to_vec();

            let mut slot_val: HashMap<u32, cranelift_codegen::ir::Value> = HashMap::new();
            for (&slot, &idx) in &slot_idx {
                slot_val.insert(slot, body_vals[idx + 1]);
            }

            let mut vstack: Vec<cranelift_codegen::ir::Value> = Vec::new();

            for (idx, &word) in body_code.iter().enumerate() {
                let opcode = (word & 0xFF) as u8;
                let op0 = word >> 8;
                match opcode {
                    op::GET_LOCAL => {
                        vstack.push(*slot_val.get(&op0).unwrap());
                    }
                    op::SET_LOCAL_POP => {
                        let v = vstack.pop().unwrap();
                        slot_val.insert(op0, v);
                    }
                    op::CONSTANT => {
                        let val = constants[&op0];
                        let val_f64 = builder.ins().f64const(val);
                        let val_i64 = builder.ins().bitcast(I64, MemFlags::new(), val_f64);
                        vstack.push(val_i64);
                    }
                    op::ADD | op::ADD_FLOAT | op::SUBTRACT | op::MULTIPLY | op::DIVIDE => {
                        let b_i64 = vstack.pop().unwrap();
                        let a_i64 = vstack.pop().unwrap();

                        // Type guard both to float
                        let a_is_float = builder.ins().icmp_imm(
                            IntCC::UnsignedLessThan,
                            a_i64,
                            0xFFF8000000000000_u64 as i64,
                        );
                        let b_is_float = builder.ins().icmp_imm(
                            IntCC::UnsignedLessThan,
                            b_i64,
                            0xFFF8000000000000_u64 as i64,
                        );
                        let both_float = builder.ins().band(a_is_float, b_is_float);

                        let op_block = builder.create_block();
                        let mut current_header_args: Vec<BlockArg> = vec![BlockArg::from(heap_ptr)];
                        current_header_args.extend(
                            referenced
                                .iter()
                                .map(|s| BlockArg::from(*slot_val.get(s).unwrap())),
                        );
                        builder.ins().brif(
                            both_float,
                            op_block,
                            &[],
                            deopt_exit,
                            &current_header_args,
                        );

                        builder.switch_to_block(op_block);
                        builder.seal_block(op_block);

                        let a_f64 = builder.ins().bitcast(F64, MemFlags::new(), a_i64);
                        let b_f64 = builder.ins().bitcast(F64, MemFlags::new(), b_i64);

                        let res_f64 = match opcode {
                            op::ADD | op::ADD_FLOAT => builder.ins().fadd(a_f64, b_f64),
                            op::SUBTRACT => builder.ins().fsub(a_f64, b_f64),
                            op::MULTIPLY => builder.ins().fmul(a_f64, b_f64),
                            op::DIVIDE => builder.ins().fdiv(a_f64, b_f64),
                            _ => unreachable!(),
                        };
                        let res_i64 = builder.ins().bitcast(I64, MemFlags::new(), res_f64);
                        vstack.push(res_i64);
                    }
                    op::ADD_INT => {
                        let b_i64 = vstack.pop().unwrap();
                        let a_i64 = vstack.pop().unwrap();

                        let mask_imm = 0xFFFF000000000000_u64 as i64;
                        let target_imm = 0xFFF8000000000000_u64 as i64;

                        let a_masked = builder.ins().band_imm(a_i64, mask_imm);
                        let a_is_int = builder.ins().icmp_imm(IntCC::Equal, a_masked, target_imm);
                        let b_masked = builder.ins().band_imm(b_i64, mask_imm);
                        let b_is_int = builder.ins().icmp_imm(IntCC::Equal, b_masked, target_imm);
                        let both_int = builder.ins().band(a_is_int, b_is_int);

                        let op_block = builder.create_block();
                        let mut current_header_args: Vec<BlockArg> = vec![BlockArg::from(heap_ptr)];
                        current_header_args.extend(
                            referenced
                                .iter()
                                .map(|s| BlockArg::from(*slot_val.get(s).unwrap())),
                        );
                        builder.ins().brif(
                            both_int,
                            op_block,
                            &[],
                            deopt_exit,
                            &current_header_args,
                        );

                        builder.switch_to_block(op_block);
                        builder.seal_block(op_block);

                        let a_payload =
                            builder.ins().band_imm(a_i64, 0x0000FFFFFFFFFFFF_u64 as i64);
                        let a_32 = builder.ins().ireduce(I32, a_payload);
                        let b_payload =
                            builder.ins().band_imm(b_i64, 0x0000FFFFFFFFFFFF_u64 as i64);
                        let b_32 = builder.ins().ireduce(I32, b_payload);
                        let res_32 = builder.ins().iadd(a_32, b_32);
                        let res_64 = builder.ins().uextend(I64, res_32);
                        vstack.push(builder.ins().bor_imm(res_64, target_imm));
                    }
                    op::MEMBER => {
                        let base_val = vstack.pop().unwrap();
                        let out_val_slot = builder.create_sized_stack_slot(
                            cranelift_codegen::ir::StackSlotData::new(
                                cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
                                8,
                                3,
                            ),
                        );
                        let out_val_ptr = builder.ins().stack_addr(ptr_type, out_val_slot, 0);

                        match &ic_entries[idx] {
                            Some(crate::vm::chunk::IcEntry::Monomorphic { shape_id, index }) => {
                                let expected_shape = *shape_id;
                                let prop_idx = *index;

                                let shape_arg = builder.ins().iconst(I32, expected_shape as i64);
                                let idx_arg = builder.ins().iconst(I32, prop_idx as i64);

                                let call_inst = builder.ins().call(
                                    ext_func_ref,
                                    &[heap_ptr, base_val, shape_arg, idx_arg, out_val_ptr],
                                );
                                let status = builder.inst_results(call_inst)[0];

                                let is_deopt = builder.ins().icmp_imm(IntCC::Equal, status, 1);

                                let op_block = builder.create_block();
                                let mut current_header_args: Vec<BlockArg> =
                                    vec![BlockArg::from(heap_ptr)];
                                current_header_args.extend(
                                    referenced
                                        .iter()
                                        .map(|s| BlockArg::from(*slot_val.get(s).unwrap())),
                                );

                                builder.ins().brif(
                                    is_deopt,
                                    deopt_exit,
                                    &current_header_args,
                                    op_block,
                                    &[],
                                );

                                builder.switch_to_block(op_block);
                                builder.seal_block(op_block);

                                let loaded_val =
                                    builder.ins().load(I64, MemFlags::new(), out_val_ptr, 0);
                                vstack.push(loaded_val);
                            }
                            Some(crate::vm::chunk::IcEntry::Polymorphic { entries, count }) => {
                                let (shape0, idx0) = entries[0];
                                let (shape1, idx1) =
                                    if *count >= 2 { entries[1] } else { entries[0] };

                                // get actual shape_id from heap
                                let get_call =
                                    builder.ins().call(get_shape_ref, &[heap_ptr, base_val]);
                                let actual_shape = builder.inst_results(get_call)[0]; // I32

                                let fast0 = builder.create_block();
                                let check1 = builder.create_block();
                                let fast1 = builder.create_block();
                                let pic_done = builder.create_block();
                                builder.append_block_param(pic_done, I64); // merged result

                                let mut current_header_args: Vec<BlockArg> =
                                    vec![BlockArg::from(heap_ptr)];
                                current_header_args.extend(
                                    referenced
                                        .iter()
                                        .map(|s| BlockArg::from(*slot_val.get(s).unwrap())),
                                );

                                // branch: shape0 hit -> fast0, else -> check1
                                let s0_const = builder.ins().iconst(I32, shape0 as i64);
                                let hit0 = builder.ins().icmp(IntCC::Equal, actual_shape, s0_const);
                                builder.ins().brif(hit0, fast0, &[], check1, &[]);

                                // fast0: load prop at idx0
                                builder.switch_to_block(fast0);
                                builder.seal_block(fast0);
                                let i0_arg = builder.ins().iconst(I32, idx0 as i64);
                                let lp0_call = builder.ins().call(
                                    load_prop_ref,
                                    &[heap_ptr, base_val, i0_arg, out_val_ptr],
                                );
                                let lp0_status = builder.inst_results(lp0_call)[0];
                                let lp0_fail =
                                    builder.ins().icmp_imm(IntCC::NotEqual, lp0_status, 0);
                                let lp0_ok = builder.create_block();
                                builder.ins().brif(
                                    lp0_fail,
                                    deopt_exit,
                                    &current_header_args,
                                    lp0_ok,
                                    &[],
                                );
                                builder.switch_to_block(lp0_ok);
                                builder.seal_block(lp0_ok);
                                let v0 = builder.ins().load(I64, MemFlags::new(), out_val_ptr, 0);
                                builder.ins().jump(pic_done, &[BlockArg::from(v0)]);

                                // check1: compare shape1, deopt on miss
                                builder.switch_to_block(check1);
                                builder.seal_block(check1);
                                let s1_const = builder.ins().iconst(I32, shape1 as i64);
                                let hit1 = builder.ins().icmp(IntCC::Equal, actual_shape, s1_const);
                                builder.ins().brif(
                                    hit1,
                                    fast1,
                                    &[],
                                    deopt_exit,
                                    &current_header_args,
                                );

                                // fast1: load prop at idx1
                                builder.switch_to_block(fast1);
                                builder.seal_block(fast1);
                                let i1_arg = builder.ins().iconst(I32, idx1 as i64);
                                let lp1_call = builder.ins().call(
                                    load_prop_ref,
                                    &[heap_ptr, base_val, i1_arg, out_val_ptr],
                                );
                                let lp1_status = builder.inst_results(lp1_call)[0];
                                let lp1_fail =
                                    builder.ins().icmp_imm(IntCC::NotEqual, lp1_status, 0);
                                let lp1_ok = builder.create_block();
                                builder.ins().brif(
                                    lp1_fail,
                                    deopt_exit,
                                    &current_header_args,
                                    lp1_ok,
                                    &[],
                                );
                                builder.switch_to_block(lp1_ok);
                                builder.seal_block(lp1_ok);
                                let v1 = builder.ins().load(I64, MemFlags::new(), out_val_ptr, 0);
                                builder.ins().jump(pic_done, &[BlockArg::from(v1)]);

                                // pic_done: block param carries merged result
                                builder.switch_to_block(pic_done);
                                builder.seal_block(pic_done);
                                let result = builder.block_params(pic_done)[0];
                                vstack.push(result);
                            }
                            _ => unreachable!("body_is_translatable checked this"),
                        }
                    }
                    _ => unreachable!("body_is_translatable checked this"),
                }
            }

            // Increment i by 1.0 (assuming i is float)
            let v_i_cur_i64 = *slot_val.get(&i_slot).unwrap();
            let i_is_float = builder.ins().icmp_imm(
                IntCC::UnsignedLessThan,
                v_i_cur_i64,
                0xFFF8000000000000_u64 as i64,
            );
            let inc_block = builder.create_block();
            let mut current_header_args: Vec<BlockArg> = vec![BlockArg::from(heap_ptr)];
            current_header_args.extend(
                referenced
                    .iter()
                    .map(|s| BlockArg::from(*slot_val.get(s).unwrap())),
            );
            builder
                .ins()
                .brif(i_is_float, inc_block, &[], deopt_exit, &current_header_args);

            builder.switch_to_block(inc_block);
            builder.seal_block(inc_block);

            let v_i_cur_f64 = builder.ins().bitcast(F64, MemFlags::new(), v_i_cur_i64);
            let v_one_f64 = builder.ins().f64const(1.0);
            let v_i_next_f64 = builder.ins().fadd(v_i_cur_f64, v_one_f64);
            slot_val.insert(
                i_slot,
                builder.ins().bitcast(I64, MemFlags::new(), v_i_next_f64),
            );

            let mut updated_args: Vec<BlockArg> = vec![BlockArg::from(heap_ptr)];
            updated_args.extend(
                referenced
                    .iter()
                    .map(|s| BlockArg::from(*slot_val.get(s).unwrap())),
            );
            builder.ins().jump(loop_header, &updated_args);
            builder.seal_block(loop_body);
            builder.seal_block(loop_header);

            // ── loop_exit (normal completion) ────────────────────────────────
            builder.switch_to_block(loop_exit);
            builder.seal_block(loop_exit);
            let exit_vals = builder.block_params(loop_exit).to_vec();
            for (idx, &slot) in referenced.iter().enumerate() {
                builder.ins().store(
                    MemFlags::new(),
                    exit_vals[idx + 1],
                    locals_ptr,
                    (slot * 8) as i32,
                );
            }
            let ret_0 = builder.ins().iconst(I64, 0);
            builder.ins().return_(&[ret_0]);

            // ── deopt_exit (bailing out early) ───────────────────────────────
            builder.switch_to_block(deopt_exit);
            builder.seal_block(deopt_exit);
            let deopt_vals = builder.block_params(deopt_exit).to_vec();
            for (idx, &slot) in referenced.iter().enumerate() {
                builder.ins().store(
                    MemFlags::new(),
                    deopt_vals[idx + 1],
                    locals_ptr,
                    (slot * 8) as i32,
                );
            }
            let ret_1 = builder.ins().iconst(I64, 1);
            builder.ins().return_(&[ret_1]);

            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;
        let fn_ptr: GenericJitLoopFn =
            unsafe { std::mem::transmute(self.module.get_finalized_function(func_id)) };
        self.compiled_generic.insert((code_ptr, ip), fn_ptr);
        Ok(())
    }

    // ── Tier-3: array iterator loop ───────────────────────────────────────────

    /// Returns Some(item_slot) if body[0] is SET_LOCAL_POP and body[1..] is
    /// translatable (numeric ops only, no MEMBER). Returns None otherwise.
    fn body_is_iter_translatable(body_code: &[u32], constants: &HashMap<u32, f64>) -> Option<u32> {
        if body_code.is_empty() {
            return None;
        }
        if (body_code[0] & 0xFF) as u8 != op::SET_LOCAL_POP {
            return None;
        }
        let item_slot = body_code[0] >> 8;
        for &word in &body_code[1..] {
            let opcode = (word & 0xFF) as u8;
            let op0 = word >> 8;
            match opcode {
                op::GET_LOCAL | op::SET_LOCAL_POP => {}
                op::CONSTANT => {
                    if !constants.contains_key(&op0) {
                        return None;
                    }
                }
                op::ADD
                | op::ADD_FLOAT
                | op::ADD_INT
                | op::SUBTRACT
                | op::MULTIPLY
                | op::DIVIDE => {}
                _ => return None,
            }
        }
        Some(item_slot)
    }

    #[inline]
    pub fn get_compiled_iter(&self, code_ptr: usize, ip: usize) -> Option<ArrayIterJitFn> {
        self.compiled_iters.get(&(code_ptr, ip)).copied()
    }

    pub fn profile_iter(
        &mut self,
        code_ptr: usize,
        ip: usize,
        cursor_slot: u32,
        iters: u64,
        body_code: &[u32],
        constants: &HashMap<u32, f64>,
    ) -> bool {
        const JIT_ITER_COMPILE_THRESHOLD: u64 = 5_000;
        let (prev, new_count) = {
            let count = self.iter_counts.entry((code_ptr, ip)).or_insert(0);
            let prev = *count;
            *count = prev + iters;
            (prev, prev + iters)
        };
        if prev < JIT_ITER_COMPILE_THRESHOLD && new_count >= JIT_ITER_COMPILE_THRESHOLD {
            let Some(item_slot) = Self::body_is_iter_translatable(body_code, constants) else {
                return false;
            };
            match self.compile_iter_loop(code_ptr, ip, cursor_slot, item_slot, body_code, constants)
            {
                Ok(_) => {
                    eprintln!(
                        "[JIT] compiled iter loop @ code=0x{:x} ip={} after {} iters",
                        code_ptr, ip, new_count
                    );
                    return true;
                }
                Err(e) => {
                    eprintln!("[JIT] iter loop failed: {}", e);
                    eprintln!("{}", self.ctx.func.display());
                }
            }
        }
        false
    }

    fn compile_iter_loop(
        &mut self,
        code_ptr: usize,
        ip: usize,
        cursor_slot: u32,
        item_slot: u32,
        body_code: &[u32],
        constants: &HashMap<u32, f64>,
    ) -> anyhow::Result<()> {
        // Collect all referenced local slots
        let mut slot_set: BTreeSet<u32> = BTreeSet::new();
        slot_set.insert(cursor_slot);
        slot_set.insert(item_slot);
        for &word in &body_code[1..] {
            let opcode = (word & 0xFF) as u8;
            let op0 = word >> 8;
            if opcode == op::GET_LOCAL || opcode == op::SET_LOCAL_POP {
                slot_set.insert(op0);
            }
        }
        let referenced: Vec<u32> = slot_set.into_iter().collect();
        let n_ref = referenced.len();
        let slot_idx: HashMap<u32, usize> = referenced
            .iter()
            .enumerate()
            .map(|(i, &s)| (s, i))
            .collect();

        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type)); // locals_ptr
        sig.params.push(AbiParam::new(ptr_type)); // array_data
        sig.params.push(AbiParam::new(I64)); // array_len
        sig.returns.push(AbiParam::new(I64)); // 0=done, 1=deopt

        let func_name = format!("jit_iter_x{:x}_ip{}", code_ptr, ip);
        let func_id = self
            .module
            .declare_function(&func_name, Linkage::Local, &sig)?;

        self.ctx.func.name = UserFuncName::user(0, self.func_counter);
        self.func_counter += 1;
        self.ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_ctx);

            let entry_block = builder.create_block();
            let loop_header = builder.create_block();
            let loop_body = builder.create_block();
            let loop_exit = builder.create_block();
            let deopt_exit = builder.create_block();

            builder.append_block_params_for_function_params(entry_block);

            // Block params: n_ref × I64 (slot values); no heap_ptr — array_data/array_len
            // are loop-invariant SSA values from entry_block, valid in all dominated blocks.
            for _ in 0..n_ref {
                builder.append_block_param(loop_header, I64);
                builder.append_block_param(loop_body, I64);
                builder.append_block_param(loop_exit, I64);
                builder.append_block_param(deopt_exit, I64);
            }

            // ── entry_block ──────────────────────────────────────────────────
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);
            let entry_params = builder.block_params(entry_block).to_vec();
            let locals_ptr = entry_params[0];
            let array_data = entry_params[1];
            let array_len = entry_params[2];

            let mut init_vals: Vec<cranelift_codegen::ir::Value> = Vec::new();
            for &slot in &referenced {
                let byte_off = (slot * 8) as i32;
                let v = builder
                    .ins()
                    .load(I64, MemFlags::new(), locals_ptr, byte_off);
                init_vals.push(v);
            }
            let init_args: Vec<BlockArg> = init_vals.into_iter().map(BlockArg::from).collect();
            builder.ins().jump(loop_header, &init_args);

            // ── loop_header ──────────────────────────────────────────────────
            builder.switch_to_block(loop_header);
            let header_vals: Vec<cranelift_codegen::ir::Value> =
                builder.block_params(loop_header).to_vec();
            let v_cursor_i64 = header_vals[slot_idx[&cursor_slot]];

            // Type guard: cursor must be a valid float (NaN-box < 0xFFF8... means float)
            let cursor_is_float = builder.ins().icmp_imm(
                IntCC::UnsignedLessThan,
                v_cursor_i64,
                0xFFF8000000000000_u64 as i64,
            );
            let check_bounds_block = builder.create_block();
            let header_args: Vec<BlockArg> =
                header_vals.iter().map(|&v| BlockArg::from(v)).collect();
            builder.ins().brif(
                cursor_is_float,
                check_bounds_block,
                &[],
                deopt_exit,
                &header_args,
            );

            // check_bounds_block: single predecessor (loop_header) → seal immediately
            builder.switch_to_block(check_bounds_block);
            builder.seal_block(check_bounds_block);

            // v_cursor_i64 is from loop_header which dominates this block — valid SSA use
            let v_cursor_f64 = builder.ins().bitcast(F64, MemFlags::new(), v_cursor_i64);
            let v_cursor_uint = builder.ins().fcvt_to_uint(I64, v_cursor_f64);
            let in_bounds = builder
                .ins()
                .icmp(IntCC::UnsignedLessThan, v_cursor_uint, array_len);
            builder
                .ins()
                .brif(in_bounds, loop_body, &header_args, loop_exit, &header_args);

            // ── loop_body ────────────────────────────────────────────────────
            builder.switch_to_block(loop_body);
            let body_vals: Vec<cranelift_codegen::ir::Value> =
                builder.block_params(loop_body).to_vec();

            // Slot 0 = slot_idx[slot0], no heap_ptr offset (unlike Tier-2)
            let mut slot_val: HashMap<u32, cranelift_codegen::ir::Value> = HashMap::new();
            for (&slot, &idx) in &slot_idx {
                slot_val.insert(slot, body_vals[idx]);
            }

            // Re-derive cursor index from the slot value
            let v_cur_i64 = *slot_val.get(&cursor_slot).unwrap();
            let v_cur_f64 = builder.ins().bitcast(F64, MemFlags::new(), v_cur_i64);
            let v_cur_uint = builder.ins().fcvt_to_uint(I64, v_cur_f64);

            // Load arr[cursor]
            let byte_offset = builder.ins().imul_imm(v_cur_uint, 8);
            let elem_ptr = builder.ins().iadd(array_data, byte_offset);
            let elem_raw = builder.ins().load(I64, MemFlags::new(), elem_ptr, 0);

            // Type guard: element must be a float
            let elem_is_float = builder.ins().icmp_imm(
                IntCC::UnsignedLessThan,
                elem_raw,
                0xFFF8000000000000_u64 as i64,
            );
            let body_op_block = builder.create_block();
            {
                let deopt_args: Vec<BlockArg> = referenced
                    .iter()
                    .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                    .collect();
                builder
                    .ins()
                    .brif(elem_is_float, body_op_block, &[], deopt_exit, &deopt_args);
            }
            builder.switch_to_block(body_op_block);
            builder.seal_block(body_op_block);

            // Word 0 of body (SET_LOCAL_POP item_slot) handled directly:
            // elem_raw from loop_body (dominator) is valid here.
            slot_val.insert(item_slot, elem_raw);

            // Translate body_code[1..] (skip the SET_LOCAL_POP at index 0)
            let mut vstack: Vec<cranelift_codegen::ir::Value> = Vec::new();
            for &word in &body_code[1..] {
                let opcode = (word & 0xFF) as u8;
                let op0 = word >> 8;
                match opcode {
                    op::GET_LOCAL => {
                        vstack.push(*slot_val.get(&op0).unwrap());
                    }
                    op::SET_LOCAL_POP => {
                        let v = vstack.pop().unwrap();
                        slot_val.insert(op0, v);
                    }
                    op::CONSTANT => {
                        let val = constants[&op0];
                        let val_f64 = builder.ins().f64const(val);
                        let val_i64 = builder.ins().bitcast(I64, MemFlags::new(), val_f64);
                        vstack.push(val_i64);
                    }
                    op::ADD | op::ADD_FLOAT | op::SUBTRACT | op::MULTIPLY | op::DIVIDE => {
                        let b_i64 = vstack.pop().unwrap();
                        let a_i64 = vstack.pop().unwrap();
                        let a_is_float = builder.ins().icmp_imm(
                            IntCC::UnsignedLessThan,
                            a_i64,
                            0xFFF8000000000000_u64 as i64,
                        );
                        let b_is_float = builder.ins().icmp_imm(
                            IntCC::UnsignedLessThan,
                            b_i64,
                            0xFFF8000000000000_u64 as i64,
                        );
                        let both_float = builder.ins().band(a_is_float, b_is_float);
                        let arith_block = builder.create_block();
                        {
                            let deopt_args: Vec<BlockArg> = referenced
                                .iter()
                                .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                                .collect();
                            builder.ins().brif(
                                both_float,
                                arith_block,
                                &[],
                                deopt_exit,
                                &deopt_args,
                            );
                        }
                        builder.switch_to_block(arith_block);
                        builder.seal_block(arith_block);
                        let a_f64 = builder.ins().bitcast(F64, MemFlags::new(), a_i64);
                        let b_f64 = builder.ins().bitcast(F64, MemFlags::new(), b_i64);
                        let res_f64 = match opcode {
                            op::ADD | op::ADD_FLOAT => builder.ins().fadd(a_f64, b_f64),
                            op::SUBTRACT => builder.ins().fsub(a_f64, b_f64),
                            op::MULTIPLY => builder.ins().fmul(a_f64, b_f64),
                            op::DIVIDE => builder.ins().fdiv(a_f64, b_f64),
                            _ => unreachable!(),
                        };
                        let res_i64 = builder.ins().bitcast(I64, MemFlags::new(), res_f64);
                        vstack.push(res_i64);
                    }
                    op::ADD_INT => {
                        let b_i64 = vstack.pop().unwrap();
                        let a_i64 = vstack.pop().unwrap();
                        let mask_imm = 0xFFFF000000000000_u64 as i64;
                        let target_imm = 0xFFF8000000000000_u64 as i64;
                        let a_masked = builder.ins().band_imm(a_i64, mask_imm);
                        let a_is_int = builder.ins().icmp_imm(IntCC::Equal, a_masked, target_imm);
                        let b_masked = builder.ins().band_imm(b_i64, mask_imm);
                        let b_is_int = builder.ins().icmp_imm(IntCC::Equal, b_masked, target_imm);
                        let both_int = builder.ins().band(a_is_int, b_is_int);
                        let int_block = builder.create_block();
                        {
                            let deopt_args: Vec<BlockArg> = referenced
                                .iter()
                                .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                                .collect();
                            builder
                                .ins()
                                .brif(both_int, int_block, &[], deopt_exit, &deopt_args);
                        }
                        builder.switch_to_block(int_block);
                        builder.seal_block(int_block);
                        let a_payload =
                            builder.ins().band_imm(a_i64, 0x0000FFFFFFFFFFFF_u64 as i64);
                        let a_32 = builder.ins().ireduce(I32, a_payload);
                        let b_payload =
                            builder.ins().band_imm(b_i64, 0x0000FFFFFFFFFFFF_u64 as i64);
                        let b_32 = builder.ins().ireduce(I32, b_payload);
                        let res_32 = builder.ins().iadd(a_32, b_32);
                        let res_64 = builder.ins().uextend(I64, res_32);
                        vstack.push(builder.ins().bor_imm(res_64, target_imm));
                    }
                    _ => unreachable!("body_is_iter_translatable checked this"),
                }
            }

            // Increment cursor: type-guard it is still float, then cursor += 1.0
            let v_cur_now_i64 = *slot_val.get(&cursor_slot).unwrap();
            let cur_still_float = builder.ins().icmp_imm(
                IntCC::UnsignedLessThan,
                v_cur_now_i64,
                0xFFF8000000000000_u64 as i64,
            );
            let inc_block = builder.create_block();
            {
                let deopt_args: Vec<BlockArg> = referenced
                    .iter()
                    .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                    .collect();
                builder
                    .ins()
                    .brif(cur_still_float, inc_block, &[], deopt_exit, &deopt_args);
            }
            builder.switch_to_block(inc_block);
            builder.seal_block(inc_block);

            let v_cur_f64_now = builder.ins().bitcast(F64, MemFlags::new(), v_cur_now_i64);
            let v_one = builder.ins().f64const(1.0);
            let v_cur_next_f64 = builder.ins().fadd(v_cur_f64_now, v_one);
            slot_val.insert(
                cursor_slot,
                builder.ins().bitcast(I64, MemFlags::new(), v_cur_next_f64),
            );

            let updated_args: Vec<BlockArg> = referenced
                .iter()
                .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                .collect();
            builder.ins().jump(loop_header, &updated_args);
            builder.seal_block(loop_body);
            builder.seal_block(loop_header);

            // ── loop_exit (normal completion) ────────────────────────────────
            builder.switch_to_block(loop_exit);
            builder.seal_block(loop_exit);
            let exit_vals = builder.block_params(loop_exit).to_vec();
            for (idx, &slot) in referenced.iter().enumerate() {
                builder.ins().store(
                    MemFlags::new(),
                    exit_vals[idx],
                    locals_ptr,
                    (slot * 8) as i32,
                );
            }
            let ret_0 = builder.ins().iconst(I64, 0);
            builder.ins().return_(&[ret_0]);

            // ── deopt_exit ───────────────────────────────────────────────────
            builder.switch_to_block(deopt_exit);
            builder.seal_block(deopt_exit);
            let deopt_vals = builder.block_params(deopt_exit).to_vec();
            for (idx, &slot) in referenced.iter().enumerate() {
                builder.ins().store(
                    MemFlags::new(),
                    deopt_vals[idx],
                    locals_ptr,
                    (slot * 8) as i32,
                );
            }
            let ret_1 = builder.ins().iconst(I64, 1);
            builder.ins().return_(&[ret_1]);

            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;
        let fn_ptr: ArrayIterJitFn =
            unsafe { std::mem::transmute(self.module.get_finalized_function(func_id)) };
        self.compiled_iters.insert((code_ptr, ip), fn_ptr);
        Ok(())
    }

    // ── Tier-4: hot function compilation ─────────────────────────────────────

    pub fn profile_fn(
        &mut self,
        program: Arc<ChunkProgram>,
        entry: usize,
        arity: u32,
        writable: u32,
        name: &str,
        interner: &mut super::intern::StringInterner,
    ) -> Option<CompiledFn> {
        let fn_id = Arc::as_ptr(&program) as usize;
        let key = (fn_id, entry, arity, writable);
        if let Some((_, compiled)) = self.compiled_fns.get(&key) {
            return Some(compiled.clone());
        }
        let (owner, count) = self
            .fn_counts
            .entry(key)
            .or_insert_with(|| (Arc::downgrade(&program), 0));
        // GC may recycle addresses of uncompiled programs; never inherit heat.
        if !owner
            .upgrade()
            .is_some_and(|owner| Arc::ptr_eq(&owner, &program))
        {
            *owner = Arc::downgrade(&program);
            *count = 0;
        }
        *count += 1;
        if *count == JIT_FN_THRESHOLD {
            let code = &program.code[entry..];
            let record_keys = (writable == 0)
                .then(|| Self::record_return(code, &program.constants, arity))
                .flatten();
            if record_keys.is_none()
                && !Self::fn_is_translatable(code, &program.constants, arity, writable)
            {
                if self.trace {
                    eprintln!("[JIT] skipped hot fn name={} (unsupported bytecode)", name);
                }
                return None;
            }
            let mut members = HashMap::new();
            let mut intrinsics = 0;
            for &word in code {
                let opcode = (word & 0xFF) as u8;
                if opcode == op::GET_PRIVATE {
                    let crate::types::Constant::String(mut name) = program.constants[(word >> 8) as usize].clone() else { unreachable!() };
                    intrinsics |= intrinsic(name.as_flat_str()).unwrap().0;
                }
                if opcode == op::MEMBER {
                    let constant = (word >> 8) as usize;
                    let crate::types::Constant::String(mut name) =
                        program.constants[constant].clone()
                    else {
                        unreachable!()
                    };
                    let text = name.as_flat_str();
                    members.insert(
                        constant,
                        (interner.intern(text), interner.intern_case_sensitive(text)),
                    );
                }
            }
            let hints = Self::parameter_hints(&program);
            if hints.len() > arity as usize { return None; }
            match self.compile_hot_fn(fn_id, code, &program.constants, writable, &members, hints, None) {
                Ok(compiled) => {
                    eprintln!(
                        "[JIT] compiled hot {}fn name={} fn_id=0x{:x} entry={} type_hints={}",
                        if writable != 0 { "loop " } else { "" },
                        name,
                        fn_id,
                        entry,
                        hints.iter().filter(|hint| Self::hint_kind(hint).is_some()).count()
                    );
                    let compiled = CompiledFn {
                        call: compiled,
                        record_keys: record_keys.map(Arc::from),
                        executed: false,
                        intrinsics,
                    };
                    self.compiled_fns.insert(key, (program, compiled.clone()));
                    return Some(compiled);
                }
                Err(e) => {
                    eprintln!("[JIT] hot fn compile failed: {}", e);
                    self.module.clear_context(&mut self.ctx);
                    self.func_ctx = FunctionBuilderContext::new();
                }
            }
        }
        None
    }

    fn hint_kind(name: &str) -> Option<u64> {
        match name { "numeric" | "number" => Some(1), "boolean" | "bool" => Some(2), "array" => Some(3), "struct" => Some(4), "string" => Some(5), _ => None }
    }

    fn parameter_hints(program: &ChunkProgram) -> &[String] {
        let mut ip = 0;
        while let Some(&word) = program.code.get(ip) {
            let opcode = (word & 0xff) as u8;
            if opcode == op::FUNCTION_ENTER {
                return match (word >> 8).checked_sub(1).and_then(|index| program.constants.get(index as usize)) {
                    Some(crate::types::Constant::StringArray(hints)) => hints,
                    _ => &[],
                };
            }
            // Skip operand words; a named-default argument count can resemble an opcode.
            ip += match opcode {
                op::COMPARE_JUMP | op::CALL_NAMED | op::INVOKE | op::PUSH_HANDLER | op::BIND_LOCAL => 2,
                op::LOCAL_COMPARE_JUMP | op::GLOBAL_COMPARE_JUMP | op::INVOKE_NAMED | op::INVOKE_NAMED_SPREAD | op::ITER_NEXT | op::LOCAL_JUMP_IF_NE_CONST | op::FOR_LOOP_STEP => 3,
                _ => 1,
            };
        }
        &[]
    }

    /// Terminal small STRUCT literals whose values are parameters, not allocations.
    fn record_return(
        code: &[u32],
        consts: &[crate::types::Constant],
        arity: u32,
    ) -> Option<Vec<crate::types::box_string::BoxString>> {
        use crate::types::Constant;
        let end = code
            .iter()
            .position(|word| (word & 0xFF) as u8 == op::STRUCT)?;
        let count = (code[end] >> 8) as usize;
        if count > 2 || end != count * 2 || code.get(end + 1).copied()? != op::RETURN as u32 {
            return None;
        }
        let tail = &code[end + 2..];
        if !tail.is_empty()
            && !(tail.len() == 2
                && (tail[0] & 0xFF) as u8 == op::CONSTANT
                && matches!(consts.get((tail[0] >> 8) as usize), Some(Constant::Null))
                && tail[1] == op::RETURN as u32)
        {
            return None;
        }
        code[..end]
            .chunks_exact(2)
            .map(|pair| {
                if (pair[0] & 0xFF) as u8 != op::CONSTANT
                    || (pair[1] & 0xFF) as u8 != op::GET_LOCAL
                    || pair[1] >> 8 >= arity
                {
                    return None;
                }
                match consts.get((pair[0] >> 8) as usize)? {
                    Constant::String(key) => Some(key.clone()),
                    _ => None,
                }
            })
            .collect()
    }

    fn fn_is_translatable(
        code: &[u32],
        consts: &[crate::types::Constant],
        arity: u32,
        writable: u32,
    ) -> bool {
        Self::fn_is_translatable_with_calls(code,consts,arity,writable,None)
    }

    fn fn_is_translatable_with_calls(code:&[u32],consts:&[crate::types::Constant],arity:u32,writable:u32,calls:Option<&HashMap<usize,(u64,u32)>>)->bool {
        // Every intrinsic reference must be consumed by a proven call within
        // one expression block; references cannot escape through branch spills.
        let callees: BTreeSet<usize> = code.iter().enumerate()
            .filter(|(_, word)| (**word & 0xff) as u8 == op::CALL)
            .filter_map(|(ip, _)| if let Some(calls)=calls {
                let origin=Self::call_origin(code,ip)?;
                calls.get(&((code[origin]>>8) as usize)).filter(|(_,arity)|*arity==code[ip]>>8).map(|_|origin)
            } else {Self::intrinsic_call(code,consts,ip)}).collect();
        // Loops stage numeric named-local writes, rollback on guards, and yield
        // every 1024 backedges. No heap writes/general calls/allocations.
        code.iter().enumerate().all(|(ip, &word)| {
            let opcode = (word & 0xFF) as u8;
            let arg = word >> 8;
            match opcode {
                op::GET_LOCAL => arg < arity,
                op::SET_LOCAL | op::SET_LOCAL_POP => arg < 32 && writable & (1 << arg) != 0,
                op::INC_LOCAL => calls.is_some() && arg < 32 && writable & (1 << arg) != 0,
                op::LOOP => writable != 0 && (arg as usize) == ip + 1,
                op::INDEX => true,
                // Compiling an untaken read must not upgrade key presentation.
                op::MEMBER => matches!(consts.get(arg as usize), Some(crate::types::Constant::String(name)) if { let mut name=name.clone(); let text=name.as_flat_str(); text==text.to_lowercase() }),
                op::GET_PRIVATE => callees.contains(&ip),
                op::CALL => Self::call_origin(code,ip).is_some_and(|origin|callees.contains(&origin)),
                op::CONSTANT => matches!(
                    consts.get(arg as usize),
                    Some(
                        crate::types::Constant::Number(_)
                            | crate::types::Constant::Boolean(_)
                            | crate::types::Constant::Null
                    )
                ),
                op::JUMP | op::JUMP_IF_FALSE => ip + 1 + (arg as usize) < code.len(),
                op::ADD
                | op::ADD_FLOAT
                | op::SUBTRACT
                | op::SUB_FLOAT
                | op::MULTIPLY
                | op::MUL_FLOAT
                | op::DIVIDE
                | op::DIV_FLOAT
                | op::EQUAL
                | op::NOT_EQUAL
                | op::LESS
                | op::LESS_EQUAL
                | op::GREATER
                | op::GREATER_EQUAL
                | op::NOT
                | op::RETURN
                | op::POP
                | op::DUP => true,
                _ => false,
            }
        })
    }

    /// Prove the callee within one expression block before touching Cranelift.
    fn intrinsic_call(code: &[u32], consts: &[crate::types::Constant], ip: usize) -> Option<usize> {
        let origin=Self::call_origin(code,ip)?;
        matches!(consts.get((code[origin]>>8) as usize),Some(crate::types::Constant::String(name)) if intrinsic(name.clone().as_flat_str()).is_some_and(|(_,count,_)|count==code[ip]>>8)).then_some(origin)
    }

    fn call_origin(code:&[u32],ip:usize)->Option<usize> {
        let mut needed = (code[ip]>>8) as i32;
        for (distance, &word) in code[..ip].iter().rev().enumerate() {
            let opcode = (word & 0xFF) as u8;
            if needed == 0 {
                return (opcode==op::GET_PRIVATE).then_some(ip-distance-1);
            }
            needed += match opcode {
                op::GET_LOCAL | op::GET_PRIVATE | op::CONSTANT => -1,
                op::ADD
                | op::ADD_FLOAT
                | op::SUBTRACT
                | op::SUB_FLOAT
                | op::MULTIPLY
                | op::MUL_FLOAT
                | op::DIVIDE
                | op::DIV_FLOAT
                | op::EQUAL
                | op::NOT_EQUAL
                | op::LESS
                | op::LESS_EQUAL
                | op::GREATER
                | op::GREATER_EQUAL
                | op::INDEX => 1,
                op::CALL => (word >> 8) as i32,
                op::NOT | op::MEMBER => 0,
                _ => return None, // calls across branches/stores stay interpreted
            };
        }
        None
    }

    fn compile_hot_fn(
        &mut self,
        fn_id: usize,
        code: &[u32],
        consts: &[crate::types::Constant],
        writable: u32,
        members: &HashMap<usize, (u32, u32)>,
        hints: &[String],
        calls: Option<&HashMap<usize,(u64,u32)>>,
    ) -> anyhow::Result<HotFnFn> {
        use cranelift_codegen::ir::{Block, StackSlot, StackSlotData, StackSlotKind};

        // ── Pass 1a: collect branch targets ──────────────────────────────────
        let mut branch_targets: BTreeSet<usize> = BTreeSet::new();
        {
            let mut ip = 0usize;
            while ip < code.len() {
                let word = code[ip];
                let opcode = (word & 0xFF) as u8;
                let op0 = (word >> 8) as usize;
                match opcode {
                    op::JUMP_IF_FALSE => {
                        branch_targets.insert(ip + 1); // fallthrough
                        branch_targets.insert(ip + 1 + op0); // jump target
                    }
                    op::JUMP => {
                        branch_targets.insert(ip + 1 + op0);
                    }
                    op::LOOP => {
                        branch_targets.insert(ip + 1 - op0);
                    }
                    _ => {}
                }
                ip += 1; // all accepted opcodes are 1-word
            }
        }

        // ── Pass 1b: BFS to compute stack depth at each block start ───────────
        let mut stack_depth_at: HashMap<usize, i32> = HashMap::new();
        stack_depth_at.insert(0, 0);
        let mut worklist: Vec<usize> = vec![0];
        let mut max_stack: usize = 0;

        while let Some(block_start) = worklist.pop() {
            let mut depth = stack_depth_at[&block_start];
            let mut ip2 = block_start;
            loop {
                if ip2 >= code.len() {
                    break;
                }
                // Hit a new block boundary (not our own start)
                if ip2 != block_start && branch_targets.contains(&ip2) {
                    if !stack_depth_at.contains_key(&ip2) {
                        stack_depth_at.insert(ip2, depth);
                        worklist.push(ip2);
                    }
                    break;
                }
                let word2 = code[ip2];
                let opcode2 = (word2 & 0xFF) as u8;
                let op02 = (word2 >> 8) as usize;
                match opcode2 {
                    op::JUMP_IF_FALSE => {
                        // Peek: stack depth unchanged at both successors
                        for &target in &[ip2 + 1, ip2 + 1 + op02] {
                            if !stack_depth_at.contains_key(&target) {
                                stack_depth_at.insert(target, depth);
                                worklist.push(target);
                            }
                        }
                        break;
                    }
                    op::JUMP | op::LOOP => {
                        let target = if opcode2 == op::LOOP {
                            ip2 + 1 - op02
                        } else {
                            ip2 + 1 + op02
                        };
                        if opcode2 == op::LOOP && depth != 0 {
                            return Err(anyhow::anyhow!(
                                "loop OSR requires an empty expression stack"
                            ));
                        }
                        if !stack_depth_at.contains_key(&target) {
                            stack_depth_at.insert(target, depth);
                            worklist.push(target);
                        }
                        break;
                    }
                    op::RETURN => {
                        break;
                    }
                    op::GET_LOCAL | op::GET_PRIVATE | op::CONSTANT | op::DUP => depth += 1,
                    op::INDEX => depth -= 1,
                    op::STRUCT => depth += 1 - 2 * op02 as i32,
                    op::SET_LOCAL_POP | op::POP => depth -= 1,
                    op::CALL => {
                        // pops (op02 + 1) values (args + func), pushes 1 → net -op02
                        depth -= op02 as i32;
                    }
                    op::ADD
                    | op::ADD_INT
                    | op::ADD_FLOAT
                    | op::SUBTRACT
                    | op::SUB_INT
                    | op::SUB_FLOAT
                    | op::MULTIPLY
                    | op::MUL_INT
                    | op::MUL_FLOAT
                    | op::DIVIDE
                    | op::DIV_FLOAT
                    | op::MODULO
                    | op::EQUAL
                    | op::NOT_EQUAL
                    | op::LESS
                    | op::LESS_EQUAL
                    | op::GREATER
                    | op::GREATER_EQUAL
                    | op::STRING_CONCAT => depth -= 1,
                    // SET_LOCAL, NOT: no net stack change
                    _ => {}
                }
                if depth > 0 && depth as usize > max_stack {
                    max_stack = depth as usize;
                }
                ip2 += 1;
            }
        }

        // ── Setup Cranelift function ──────────────────────────────────────────
        let ptr_type = self.module.isa().pointer_type();
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type)); // locals_ptr
        sig.params.push(AbiParam::new(ptr_type)); // heap_ptr
        sig.params.push(AbiParam::new(ptr_type)); // out_val_ptr
        sig.returns.push(AbiParam::new(I64)); // status (0=ok, 1=rollback, 2=yield)

        let func_name = format!("jit_hotfn_x{:x}_v{}", fn_id, self.func_counter);
        let func_id = self
            .module
            .declare_function(&func_name, Linkage::Local, &sig)?;

        self.ctx.func.name = UserFuncName::user(0, self.func_counter);
        self.func_counter += 1;
        self.ctx.func.signature = sig;

        let readers = if code.iter().any(|word| matches!((word & 0xFF) as u8, op::INDEX | op::MEMBER)) {
            let mut sig = self.module.make_signature();
            sig.params.extend([
                AbiParam::new(ptr_type),
                AbiParam::new(I64),
                AbiParam::new(I64),
                AbiParam::new(ptr_type),
            ]);
            sig.returns.push(AbiParam::new(I64));
            let array = self
                .module
                .declare_function("jit_read_index", Linkage::Import, &sig)?;
            let array = self.module.declare_func_in_func(array, &mut self.ctx.func);
            let mut sig = self.module.make_signature();
            sig.params.extend([
                AbiParam::new(ptr_type),
                AbiParam::new(I64),
                AbiParam::new(I32),
                AbiParam::new(I32),
                AbiParam::new(ptr_type),
            ]);
            sig.returns.push(AbiParam::new(I64));
            let member = self
                .module
                .declare_function("jit_read_member", Linkage::Import, &sig)?;
            let member = self.module.declare_func_in_func(member, &mut self.ctx.func);
            Some((array, member))
        } else {
            None
        };

        let mut read_sig = self.module.make_signature();
        read_sig.params.extend([AbiParam::new(ptr_type), AbiParam::new(I64), AbiParam::new(I64), AbiParam::new(I64), AbiParam::new(ptr_type)]);
        read_sig.returns.push(AbiParam::new(I64));
        let core_read = self.module.declare_function("jit_core_read", Linkage::Import, &read_sig)?;
        let core_read = self.module.declare_func_in_func(core_read, &mut self.ctx.func);
        let construct_call=self.module.declare_function("jit_construct_call",Linkage::Import,&read_sig)?;
        let construct_call=self.module.declare_func_in_func(construct_call,&mut self.ctx.func);
        let mut kind_sig = self.module.make_signature();
        kind_sig.params.extend([AbiParam::new(ptr_type), AbiParam::new(I64), AbiParam::new(I64)]);
        kind_sig.returns.push(AbiParam::new(I64));
        let kind_check = self.module.declare_function("jit_parameter_kind", Linkage::Import, &kind_sig)?;
        let kind_check = self.module.declare_func_in_func(kind_check, &mut self.ctx.func);
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_ctx);

            // Create blocks
            let entry_block = builder.create_block();
            let normal_exit = builder.create_block();
            let deopt_exit = builder.create_block();

            // Only create blocks for branch targets reachable from the BFS (live code).
            // Dead-code targets (e.g. the implicit `CONSTANT null; RETURN` after
            // a function whose every path already returns) are excluded so they
            // don't produce empty Cranelift blocks or trigger spurious deopt paths.
            let mut target_blocks: HashMap<usize, Block> = HashMap::new();
            for &target_ip in &branch_targets {
                if stack_depth_at.contains_key(&target_ip) {
                    target_blocks.insert(target_ip, builder.create_block());
                }
            }

            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            let locals_ptr = builder.block_params(entry_block)[0];
            let vm_ptr = builder.block_params(entry_block)[1];
            let out_val_ptr = builder.block_params(entry_block)[2];
            let yield_exit = builder.create_block();
            let budget_slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                8,
                3,
            ));
            let budget = builder.ins().iconst(I64, 1024);
            builder.ins().stack_store(budget, budget_slot, 0);

            // Allocate spill slots for expression stack (8 bytes each, 8-byte aligned)
            let spill_slots: Vec<StackSlot> = (0..max_stack.max(1))
                .map(|_| {
                    builder.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        8,
                        3,
                    ))
                })
                .collect();

            // Guard declared parameters once, after defaults/events and at every
            // native (re-)entry. No GC IDs, layouts or unchecked promises in metadata.
            let mut parameter_values = HashMap::new();
            let mut numeric_values = BTreeSet::new();
            for (slot, hint) in hints.iter().enumerate() {
                let Some(kind) = Self::hint_kind(hint) else { continue; };
                let value = builder.ins().load(I64, MemFlags::new(), locals_ptr, (slot * 8) as i32);
                let valid = if kind == 1 {
                    let tagged = builder.ins().icmp_imm(IntCC::UnsignedLessThan, value, 0xFFF8000000000000_u64 as i64);
                    let float = builder.ins().bitcast(F64, MemFlags::new(), value);
                    let ordered = builder.ins().fcmp(FloatCC::Ordered, float, float);
                    builder.ins().band(tagged, ordered)
                } else if kind == 2 {
                    let tag = builder.ins().band_imm(value, 0xffff000000000000_u64 as i64);
                    builder.ins().icmp_imm(IntCC::Equal, tag, 0xfff9000000000000_u64 as i64)
                } else {
                    let kind = builder.ins().iconst(I64, kind as i64);
                    let call = builder.ins().call(kind_check, &[vm_ptr, value, kind]);
                    let result = builder.inst_results(call)[0];
                    builder.ins().icmp_imm(IntCC::Equal, result, 1)
                };
                let accepted = builder.create_block();
                builder.ins().brif(valid, accepted, &[], deopt_exit, &[]);
                builder.switch_to_block(accepted);
                builder.seal_block(accepted);
                parameter_values.insert(slot, value);
                if kind == 1 { numeric_values.insert(value); }
            }

            // If ip=0 is itself a branch target, jump to it from entry
            if let Some(&t0_block) = target_blocks.get(&0) {
                builder.ins().jump(t0_block, &[]);
            }

            // ── Per-instruction translation ───────────────────────────────────
            let mut vstack: Vec<cranelift_codegen::ir::Value> = Vec::new();
            // Proven callee SSA values, not forgeable boxed-value tags.
            // Calls across control-flow/spill boundaries remain interpreted.
            let mut functions = HashMap::new();
            let mut block_terminated = target_blocks.contains_key(&0);

            let mut ip = 0usize;
            while ip < code.len() {
                // ── Block boundary ────────────────────────────────────────────
                if let Some(&target_block) = target_blocks.get(&ip) {
                    if !block_terminated {
                        // Spill vstack values before the implicit fall-through jump
                        for (i, &v) in vstack.iter().enumerate() {
                            builder.ins().stack_store(v, spill_slots[i], 0);
                        }
                        builder.ins().jump(target_block, &[]);
                    }
                    builder.switch_to_block(target_block);

                    // Reload vstack from spill slots (based on known depth at this ip)
                    let depth = *stack_depth_at.get(&ip).unwrap_or(&0) as usize;
                    vstack.clear();
                    for i in 0..depth {
                        let v = builder.ins().stack_load(I64, spill_slots[i], 0);
                        vstack.push(v);
                    }
                    block_terminated = false;
                }

                if block_terminated {
                    ip += 1;
                    continue; // skip dead code after unconditional branches
                }

                let word = code[ip];
                let opcode = (word & 0xFF) as u8;
                let op0 = (word >> 8) as usize;

                macro_rules! emit_deopt_brif {
                    ($cond:expr) => {{
                        let ok_block = builder.create_block();
                        builder.ins().brif($cond, ok_block, &[], deopt_exit, &[]);
                        builder.switch_to_block(ok_block);
                        builder.seal_block(ok_block);
                    }};
                }

                macro_rules! require_float {
                    ($value:expr) => {{
                        let value = $value;
                        let valid = builder.ins().icmp_imm(
                            IntCC::UnsignedLessThan,
                            value,
                            0xFFF8000000000000_u64 as i64,
                        );
                        emit_deopt_brif!(valid);
                        let float = builder.ins().bitcast(F64, MemFlags::new(), value);
                        let ordered = builder.ins().fcmp(FloatCC::Ordered, float, float);
                        emit_deopt_brif!(ordered);
                        float
                    }};
                }

                match opcode {
                    op::GET_LOCAL => {
                        let value = if let Some(&value) = parameter_values.get(&op0) { value } else {
                            builder.ins().load(I64, MemFlags::new(), locals_ptr, (op0 * 8) as i32)
                        };
                        vstack.push(value);
                    }
                    op::SET_LOCAL | op::SET_LOCAL_POP => {
                        let value = *vstack.last().unwrap();
                        require_float!(value);
                        builder
                            .ins()
                            .store(MemFlags::new(), value, locals_ptr, (op0 * 8) as i32);
                        if opcode == op::SET_LOCAL_POP {
                            vstack.pop();
                        }
                    }
                    op::INC_LOCAL => {
                        let value=builder.ins().load(I64,MemFlags::new(),locals_ptr,(op0*8) as i32);
                        let float=require_float!(value);
                        let one=builder.ins().f64const(1.0);
                        let next=builder.ins().fadd(float,one);
                        let raw=builder.ins().bitcast(I64,MemFlags::new(),next);
                        builder.ins().store(MemFlags::new(),raw,locals_ptr,(op0*8) as i32);
                    }
                    op::GET_PRIVATE => {
                        let value = builder
                            .ins()
                            .iconst(I64, crate::types::BxValue::new_null().to_bits() as i64);
                        let crate::types::Constant::String(mut name) = consts[op0].clone() else { unreachable!() };
                        let kind=if let Some(calls)=calls {calls[&op0].0} else {intrinsic(name.as_flat_str()).unwrap().0 as u64};
                        functions.insert(value,kind);
                        vstack.push(value);
                    }
                    op::CALL => {
                        let second = if op0 == 2 { vstack.pop().unwrap() } else { builder.ins().iconst(I64, 0) };
                        let first = vstack.pop().unwrap();
                        let function = vstack.pop().unwrap();
                        let Some(&kind) = functions.get(&function) else {
                            return Err(anyhow::anyhow!("only proven readonly core calls are eligible"));
                        };
                        if kind == 1 {
                            let float = require_float!(first);
                            let result = builder.ins().trunc(float);
                            vstack.push(builder.ins().bitcast(I64, MemFlags::new(), result));
                        } else {
                            let out = builder.ins().stack_addr(ptr_type, spill_slots[0], 0);
                            let kind = builder.ins().iconst(I64, kind as i64);
                            let helper=if calls.is_some() && functions[&function]>=8 {construct_call} else {core_read};
                            let call = builder.ins().call(helper, &[vm_ptr, kind, first, second, out]);
                            let status = builder.inst_results(call)[0];
                            let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                            emit_deopt_brif!(success);
                            vstack.push(builder.ins().stack_load(I64, spill_slots[0], 0));
                        }
                    }
                    op::INDEX | op::MEMBER => {
                        let out = builder.ins().stack_addr(ptr_type, spill_slots[0], 0);
                        let call = if opcode == op::INDEX {
                            let index = vstack.pop().unwrap();
                            let value = vstack.pop().unwrap();
                            builder
                                .ins()
                                .call(readers.unwrap().0, &[vm_ptr, value, index, out])
                        } else {
                            let value = vstack.pop().unwrap();
                            let (folded, exact) = members[&op0];
                            let folded = builder.ins().iconst(I32, folded as i64);
                            let exact = builder.ins().iconst(I32, exact as i64);
                            builder
                                .ins()
                                .call(readers.unwrap().1, &[vm_ptr, value, folded, exact, out])
                        };
                        let status = builder.inst_results(call)[0];
                        let success = builder.ins().icmp_imm(IntCC::Equal, status, 0);
                        emit_deopt_brif!(success);
                        vstack.push(builder.ins().stack_load(I64, spill_slots[0], 0));
                    }
                    op::CONSTANT => {
                        let idx = op0;
                        let value = match consts.get(idx) {
                            Some(crate::types::Constant::Number(f)) => {
                                crate::types::BxValue::new_number(*f)
                            }
                            Some(crate::types::Constant::Boolean(b)) => {
                                crate::types::BxValue::new_bool(*b)
                            }
                            Some(crate::types::Constant::Null) => crate::types::BxValue::new_null(),
                            // record_return validates these are only literal keys.
                            Some(crate::types::Constant::String(_)) => {
                                crate::types::BxValue::new_null()
                            }
                            _ => {
                                return Err(anyhow::anyhow!("unsupported constant at idx {}", idx));
                            }
                        };
                        let raw = builder.ins().iconst(I64, value.to_bits() as i64);
                        if matches!(consts.get(idx), Some(crate::types::Constant::Number(number)) if !number.is_nan()) { numeric_values.insert(raw); }
                        vstack.push(raw);
                    }
                    op::ADD
                    | op::ADD_FLOAT
                    | op::SUBTRACT
                    | op::SUB_FLOAT
                    | op::MULTIPLY
                    | op::MUL_FLOAT
                    | op::DIVIDE
                    | op::DIV_FLOAT
                    | op::MODULO => {
                        let b_i64 = vstack.pop().unwrap();
                        let a_i64 = vstack.pop().unwrap();

                        let a_f64 = builder.ins().bitcast(F64, MemFlags::new(), a_i64);
                        let b_f64 = builder.ins().bitcast(F64, MemFlags::new(), b_i64);
                        if !numeric_values.contains(&a_i64) || !numeric_values.contains(&b_i64) {
                            let a_ok = if numeric_values.contains(&a_i64) { builder.ins().iconst(cranelift_codegen::ir::types::I8, 1) } else { builder.ins().icmp_imm(IntCC::UnsignedLessThan, a_i64, 0xFFF8000000000000_u64 as i64) };
                            let b_ok = if numeric_values.contains(&b_i64) { builder.ins().iconst(cranelift_codegen::ir::types::I8, 1) } else { builder.ins().icmp_imm(IntCC::UnsignedLessThan, b_i64, 0xFFF8000000000000_u64 as i64) };
                            let both = builder.ins().band(a_ok, b_ok);
                            emit_deopt_brif!(both);
                            let ordered = builder.ins().fcmp(FloatCC::Ordered, a_f64, b_f64);
                            emit_deopt_brif!(ordered);
                        }
                        if matches!(opcode, op::DIVIDE | op::DIV_FLOAT) {
                            let zero = builder.ins().f64const(0.0);
                            let nonzero = builder.ins().fcmp(FloatCC::NotEqual, b_f64, zero);
                            emit_deopt_brif!(nonzero);
                        }
                        let res_f64 = match opcode {
                            op::ADD | op::ADD_FLOAT => builder.ins().fadd(a_f64, b_f64),
                            op::SUBTRACT | op::SUB_FLOAT => builder.ins().fsub(a_f64, b_f64),
                            op::MULTIPLY | op::MUL_FLOAT => builder.ins().fmul(a_f64, b_f64),
                            op::DIVIDE | op::DIV_FLOAT => builder.ins().fdiv(a_f64, b_f64),
                            _ => unreachable!(),
                        };
                        // Keep VM canonical NaN boxing and comparisons on the interpreter path.
                        let ordered = builder.ins().fcmp(FloatCC::Ordered, res_f64, res_f64);
                        emit_deopt_brif!(ordered);
                        let raw = builder.ins().bitcast(I64, MemFlags::new(), res_f64);
                        numeric_values.insert(raw);
                        vstack.push(raw);
                    }
                    op::EQUAL
                    | op::NOT_EQUAL
                    | op::LESS
                    | op::LESS_EQUAL
                    | op::GREATER
                    | op::GREATER_EQUAL => {
                        let b_i64 = vstack.pop().unwrap();
                        let a_i64 = vstack.pop().unwrap();
                        let a_f64 = builder.ins().bitcast(F64, MemFlags::new(), a_i64);
                        let b_f64 = builder.ins().bitcast(F64, MemFlags::new(), b_i64);
                        if !numeric_values.contains(&a_i64) || !numeric_values.contains(&b_i64) {
                            let a_ok = if numeric_values.contains(&a_i64) { builder.ins().iconst(cranelift_codegen::ir::types::I8, 1) } else { builder.ins().icmp_imm(IntCC::UnsignedLessThan, a_i64, 0xFFF8000000000000_u64 as i64) };
                            let b_ok = if numeric_values.contains(&b_i64) { builder.ins().iconst(cranelift_codegen::ir::types::I8, 1) } else { builder.ins().icmp_imm(IntCC::UnsignedLessThan, b_i64, 0xFFF8000000000000_u64 as i64) };
                            let both = builder.ins().band(a_ok, b_ok);
                            emit_deopt_brif!(both);
                            let ordered = builder.ins().fcmp(FloatCC::Ordered, a_f64, b_f64);
                            emit_deopt_brif!(ordered);
                        }
                        let float_cc = match opcode {
                            op::EQUAL => FloatCC::Equal,
                            op::NOT_EQUAL => FloatCC::NotEqual,
                            op::LESS => FloatCC::LessThan,
                            op::LESS_EQUAL => FloatCC::LessThanOrEqual,
                            op::GREATER => FloatCC::GreaterThan,
                            op::GREATER_EQUAL => FloatCC::GreaterThanOrEqual,
                            _ => unreachable!(),
                        };
                        let cmp = builder.ins().fcmp(float_cc, a_f64, b_f64);
                        let true_bits = builder.ins().iconst(I64, 0xFFF9000000000001_u64 as i64);
                        let false_bits = builder.ins().iconst(I64, 0xFFF9000000000000_u64 as i64);
                        let result = builder.ins().select(cmp, true_bits, false_bits);
                        vstack.push(result);
                    }
                    op::NOT => {
                        let v = vstack.pop().unwrap();
                        // Check tag bits == bool tag (0xFFF9...)
                        let tag = builder.ins().band_imm(v, 0xFFFF000000000000_u64 as i64);
                        let is_bool = builder.ins().icmp_imm(
                            IntCC::Equal,
                            tag,
                            0xFFF9000000000000_u64 as i64,
                        );
                        emit_deopt_brif!(is_bool);
                        // Invert payload bit 0
                        let inverted = builder.ins().bxor_imm(v, 1);
                        vstack.push(inverted);
                    }
                    op::JUMP_IF_FALSE => {
                        let offset = op0;
                        let target_ip = ip + 1 + offset;
                        let fallthru_ip = ip + 1;

                        // Non-Boolean truthiness belongs to the VM, not raw bit equality.
                        let cond = *vstack.last().unwrap();
                        let tag = builder.ins().band_imm(cond, 0xFFFF000000000000_u64 as i64);
                        let is_bool = builder.ins().icmp_imm(
                            IntCC::Equal,
                            tag,
                            0xFFF9000000000000_u64 as i64,
                        );
                        emit_deopt_brif!(is_bool);

                        // Spill entire vstack to spill slots (condition included)
                        for (i, &v) in vstack.iter().enumerate() {
                            builder.ins().stack_store(v, spill_slots[i], 0);
                        }

                        // cond == false → 0xFFF9000000000000 → take jump
                        let false_val = 0xFFF9000000000000_u64 as i64;
                        let is_false = builder.ins().icmp_imm(IntCC::Equal, cond, false_val);

                        let tgt_block = target_blocks[&target_ip];
                        let fth_block = target_blocks[&fallthru_ip];
                        builder.ins().brif(is_false, tgt_block, &[], fth_block, &[]);

                        vstack.clear();
                        block_terminated = true;
                    }
                    op::JUMP | op::LOOP => {
                        let target_ip = if opcode == op::LOOP {
                            ip + 1 - op0
                        } else {
                            ip + 1 + op0
                        };
                        if opcode == op::LOOP {
                            // Bound native execution without collecting under raw pointers.
                            let left = builder.ins().stack_load(I64, budget_slot, 0);
                            let left = builder.ins().iadd_imm(left, -1);
                            builder.ins().stack_store(left, budget_slot, 0);
                            let remaining =
                                builder.ins().icmp_imm(IntCC::UnsignedGreaterThan, left, 0);
                            let resume = builder.create_block();
                            builder.ins().brif(remaining, resume, &[], yield_exit, &[]);
                            builder.switch_to_block(resume);
                            builder.seal_block(resume);
                        }

                        // Spill vstack before unconditional jump
                        for (i, &v) in vstack.iter().enumerate() {
                            builder.ins().stack_store(v, spill_slots[i], 0);
                        }

                        let tgt_block = target_blocks[&target_ip];
                        builder.ins().jump(tgt_block, &[]);

                        vstack.clear();
                        block_terminated = true;
                    }
                    op::STRUCT => {
                        for field in 0..op0 {
                            builder.ins().store(
                                MemFlags::new(),
                                vstack[field * 2 + 1],
                                out_val_ptr,
                                (field * 8) as i32,
                            );
                        }
                        // Terminal allocation uses the same VM service as STRUCT.
                        builder.ins().jump(normal_exit, &[]);
                        block_terminated = true;
                    }
                    op::RETURN => {
                        let ret_val = vstack.pop().unwrap();
                        if functions.contains_key(&ret_val) { return Err(anyhow::anyhow!("intrinsic references must not escape")); }
                        if writable != 0 {
                            require_float!(ret_val);
                        }
                        builder
                            .ins()
                            .store(MemFlags::new(), ret_val, out_val_ptr, 0);
                        builder.ins().jump(normal_exit, &[]);
                        block_terminated = true;
                    }
                    op::POP => {
                        vstack.pop();
                    }
                    op::DUP => {
                        let v = *vstack.last().unwrap();
                        vstack.push(v);
                    }
                    _ => return Err(anyhow::anyhow!("unexpected opcode {} in hot fn", opcode)),
                }

                ip += 1;
            }

            // ── normal_exit ───────────────────────────────────────────────────
            builder.switch_to_block(normal_exit);
            let ret_0 = builder.ins().iconst(I64, 0);
            builder.ins().return_(&[ret_0]);

            builder.switch_to_block(yield_exit);
            let yielded = builder.ins().iconst(I64, 2);
            builder.ins().return_(&[yielded]);

            // ── deopt_exit ────────────────────────────────────────────────────
            builder.switch_to_block(deopt_exit);
            let zero = builder.ins().iconst(I64, 0);
            builder.ins().store(MemFlags::new(), zero, out_val_ptr, 0);
            let ret_1 = builder.ins().iconst(I64, 1);
            builder.ins().return_(&[ret_1]);

            builder.seal_all_blocks();
            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;
        let fn_ptr: HotFnFn =
            unsafe { std::mem::transmute(self.module.get_finalized_function(func_id)) };
        Ok(fn_ptr)
    }
}
