//! Bounded append-only loop kernels. All computation remains compiler-generated
//! Cranelift IR; helpers perform ordinary VM operations, never analyzer logic.
use super::super::gc::GcObject;
use super::*;
use crate::types::{BxValue, Constant, FunctionKind};

#[derive(Clone)]
pub(super) struct RecordCall {
    owner: Arc<ChunkProgram>,
    keys: Vec<crate::types::box_string::BoxString>,
    slots: Vec<usize>,
    arity: usize,
}

pub(super) struct Region {
    owner: Arc<ChunkProgram>,
    code: Vec<u32>,
    constants: Vec<Constant>,
    pub(super) callees: HashMap<usize, (u64, u32)>,
    records: Vec<RecordCall>,
    exit: usize,
    count: u64,
    compiled: Option<HotFnFn>,
    executed: bool,
}

#[repr(C)]
pub(super) struct Environment<'a> {
    read: FnReadEnv,
    records: &'a [RecordCall],
    undo: Vec<(usize, usize)>,
}

/// No callbacks, stack growth or collection. Heap::alloc does not collect;
/// returned records reach a rooted output array before the next VM safepoint.
pub(super) unsafe extern "C" fn call(
    env: *mut Environment<'_>,
    kind: u64,
    first: u64,
    second: u64,
    out: *mut u64,
) -> u64 {
    let env = unsafe { &mut *env };
    let vm = unsafe { &mut *(env.read.vm as *mut super::super::VM) };
    let first: BxValue = unsafe { std::mem::transmute(first) };
    let second: BxValue = unsafe { std::mem::transmute(second) };
    let result = if kind == 8 {
        let Some(id) = first.as_gc_id() else {
            return 1;
        };
        let Some(GcObject::String(value)) = vm.heap.get_opt(id) else {
            return 1;
        };
        let mut value = value.clone();
        let Some(character) = value.as_flat_str().chars().next() else {
            return 1;
        };
        BxValue::new_number(character as u32 as f64)
    } else if kind == 16 {
        let Some(id) = first.as_gc_id() else {
            return 1;
        };
        let Some(GcObject::Array(array)) = vm.heap.get_opt(id) else {
            return 1;
        };
        if !env.undo.iter().any(|(target, _)| *target == id) {
            // ponytail: at most 32 append targets per 1024-iteration transaction;
            // use rooted dynamic checkpoints only when a workload needs more.
            if env.undo.len() == 32 {
                return 1;
            }
            env.undo.push((id, array.len()));
        }
        vm.array_push(id, second);
        BxValue::new_bool(true)
    } else {
        let Some(record) = env.records.get(kind.wrapping_sub(32) as usize) else {
            return 1;
        };
        let arguments = [first, second];
        // Ordinary prologues apply defaults even to explicitly null arguments.
        // Only non-NaN floats bypass those prologues; other values replay normally.
        if arguments[..record.arity]
            .iter()
            .any(|value| !value.is_float() || value.as_number().is_nan())
        {
            return 1;
        }
        let mut pairs = [(0, BxValue::new_null()); 2];
        for (index, key) in record.keys.iter().enumerate().rev() {
            let mut key = key.clone();
            pairs[index] = (
                vm.interner.intern(key.as_flat_str()),
                arguments[record.slots[index]],
            );
        }
        vm.literal_struct(&pairs[..record.keys.len()])
    };
    unsafe {
        *out = result.to_bits();
    }
    0
}

impl super::super::VM {
    fn construct_record(&mut self, fiber: usize, name: &str, arity: u32) -> Option<RecordCall> {
        let value = self.private_value(fiber, name)?;
        let id = value.as_gc_id()?;
        let GcObject::CompiledFunction(function) = self.heap.get_opt(id)? else {
            return None;
        };
        if function.kind != FunctionKind::Udf
            || function.captured_receiver.is_some()
            || function.params.len() != arity as usize
            || arity > 2
        {
            return None;
        }
        let owner = Arc::clone(&function.chunk.program);
        let mut entry = 0;
        loop {
            let opcode = (*owner.code.get(entry)? & 0xff) as u8;
            if opcode == op::FUNCTION_ENTER {
                entry += 1;
                break;
            }
            entry += match opcode {
                op::COMPARE_JUMP
                | op::CALL_NAMED
                | op::INVOKE
                | op::PUSH_HANDLER
                | op::BIND_LOCAL => 2,
                op::LOCAL_COMPARE_JUMP
                | op::GLOBAL_COMPARE_JUMP
                | op::INVOKE_NAMED
                | op::INVOKE_NAMED_SPREAD
                | op::ITER_NEXT
                | op::LOCAL_JUMP_IF_NE_CONST
                | op::FOR_LOOP_STEP => 3,
                _ => 1,
            };
        }
        let body = &owner.code[entry..];
        let keys = JitState::record_return(body, &owner.constants, function.arity)?;
        let slots = body[..keys.len() * 2]
            .chunks_exact(2)
            .map(|pair| (pair[1] >> 8) as usize)
            .collect::<Vec<_>>();
        if slots.iter().any(|slot| *slot >= arity as usize) {
            return None;
        }
        Some(RecordCall {
            owner,
            keys,
            slots,
            arity: arity as usize,
        })
    }

    fn construct_region(
        &mut self,
        fiber: usize,
        owner: Arc<ChunkProgram>,
        entry: usize,
        slots: u32,
        writable: u32,
    ) -> Option<Region> {
        let backedge = (entry..owner.code.len()).find(|&ip| {
            let word = owner.code[ip];
            (word & 0xff) as u8 == op::LOOP
                && (word >> 8) as usize <= ip + 1
                && ip + 1 - (word >> 8) as usize == entry
        })?;
        let exit = backedge + 1;
        if owner.code.get(exit).copied()? != op::POP as u32 {
            return None;
        }
        // Lower fused comparisons without interpreting them. Rebase all branch
        // targets so a multiword operand can never masquerade as an instruction.
        let mut code = Vec::new();
        let mut labels = HashMap::new();
        let mut patches = Vec::new();
        let mut ip = entry;
        while ip <= backedge {
            labels.insert(ip, code.len());
            let word = owner.code[ip];
            let opcode = (word & 0xff) as u8;
            let arg = (word >> 8) as usize;
            // Kernel completion resumes after this loop, never returns a function.
            // Early returns and nonlocal exits remain interpreted.
            if opcode == op::RETURN {
                return None;
            }
            if opcode == op::LOCAL_JUMP_IF_NE_CONST {
                let constant = *owner.code.get(ip + 1)?;
                let offset = *owner.code.get(ip + 2)? as usize;
                let start = code.len();
                code.extend([
                    op::GET_LOCAL as u32 | ((arg as u32) << 8),
                    op::CONSTANT as u32 | (constant << 8),
                    op::EQUAL as u32,
                    op::JUMP_IF_FALSE as u32 | (2 << 8),
                    op::POP as u32,
                    op::JUMP as u32 | (2 << 8),
                    op::POP as u32,
                    op::JUMP as u32,
                ]);
                patches.push((start + 7, ip + 3 + offset));
                ip += 3;
            } else {
                if matches!(
                    opcode,
                    op::CALL_NAMED
                        | op::INVOKE
                        | op::PUSH_HANDLER
                        | op::BIND_LOCAL
                        | op::COMPARE_JUMP
                        | op::LOCAL_COMPARE_JUMP
                        | op::GLOBAL_COMPARE_JUMP
                        | op::INVOKE_NAMED
                        | op::INVOKE_NAMED_SPREAD
                        | op::ITER_NEXT
                        | op::FOR_LOOP_STEP
                ) {
                    return None;
                }
                if matches!(opcode, op::JUMP | op::JUMP_IF_FALSE | op::LOOP) {
                    let target = if opcode == op::LOOP {
                        (ip + 1).checked_sub(arg)?
                    } else {
                        ip + 1 + arg
                    };
                    patches.push((code.len(), target));
                }
                code.push(word);
                ip += 1;
            }
        }
        labels.insert(exit, code.len());
        for (position, target) in patches {
            let target = *labels.get(&target)?;
            let opcode = (code[position] & 0xff) as u8;
            let offset = if opcode == op::LOOP {
                (position + 1).checked_sub(target)?
            } else {
                target.checked_sub(position + 1)?
            };
            code[position] = opcode as u32 | ((offset as u32) << 8);
        }
        let mut constants = owner.constants.clone();
        let zero = constants.len();
        constants.push(Constant::Number(0.0));
        code.extend([
            op::POP as u32,
            op::CONSTANT as u32 | ((zero as u32) << 8),
            op::RETURN as u32,
        ]);
        let mut callees = HashMap::new();
        let mut records = Vec::new();
        for (ip, &word) in code.iter().enumerate() {
            if (word & 0xff) as u8 != op::CALL {
                continue;
            }
            let origin = JitState::call_origin(&code, ip)?;
            let index = (code[origin] >> 8) as usize;
            let Constant::String(mut name) = constants.get(index)?.clone() else {
                return None;
            };
            let name = name.as_flat_str();
            let arity = word >> 8;
            let kind = if let Some((kind, count, _)) = intrinsic(name) {
                if count != arity {
                    return None;
                }
                kind as u64
            } else if name.eq_ignore_ascii_case("ascii") && arity == 1 {
                8
            } else if name.eq_ignore_ascii_case("arrayAppend") && arity == 2 {
                16
            } else {
                let record = self.construct_record(fiber, name, arity)?;
                // Uppercase spelling may change observable key presentation on
                // a later rollback. Such helpers remain ordinary interpreted calls.
                if record.keys.iter().any(|key| {
                    let mut key = key.clone();
                    let text = key.as_flat_str();
                    text != text.to_lowercase()
                }) {
                    return None;
                }
                records.push(record);
                31 + records.len() as u64
            };
            callees.insert(index, (kind, arity));
        }
        if code.iter().any(|word| (*word & 0xff) as u8 == op::MEMBER) {
            return None;
        }
        if !callees.values().any(|(kind, _)| *kind == 16)
            || !JitState::fn_is_translatable_with_calls(
                &code,
                &constants,
                slots,
                writable,
                Some(&callees),
            )
        {
            return None;
        }
        Some(Region {
            owner,
            code,
            constants,
            callees,
            records,
            exit: exit + 1,
            count: 0,
            compiled: None,
            executed: false,
        })
    }

    pub(in crate::vm) fn run_jit_construct_loop(
        &mut self,
        fiber: usize,
        entry: usize,
    ) -> Option<FnJitOutcome> {
        self.jit.as_ref()?;
        let frame=self.fibers[fiber].frames.last_mut()?;
        if frame.jit_construct_unsupported==Some(entry) {return None;}
        // Invocation-owned negative feasibility latch: unsupported hot loops
        // must not rescan bindings/interception state on every backedge.
        frame.jit_construct_unsupported=Some(entry);
        #[cfg(feature = "debugger")]
        if self.debug_run.is_some() {
            return None;
        }
        if [
            "preFunctionInvoke",
            "postFunctionInvoke",
            "onFunctionException",
        ]
        .iter()
        .any(|name| self.has_interception_state(name))
        {
            return None;
        }
        let frame = self.fibers[fiber].frames.last()?;
        let base = frame.stack_base;
        let slots = frame
            .local_names
            .iter()
            .filter_map(|(slot, binding)| binding.local.then_some(slot + 1))
            .max()?;
        if slots > 32 || self.fibers[fiber].stack.len() != base + slots {
            return None;
        }
        let writable = frame
            .local_names
            .iter()
            .filter_map(|(slot, binding)| binding.local.then_some(slot))
            .fold(0u32, |mask, slot| mask | (1 << slot));
        let owner = Arc::clone(&frame.chunk.borrow().program);
        let key = (Arc::as_ptr(&owner) as usize, entry, slots as u32, writable);
        if !self.jit.as_ref()?.construct_regions.contains_key(&key) {
            let region =
                self.construct_region(fiber, Arc::clone(&owner), entry, slots as u32, writable);
            self.jit
                .as_mut()?
                .construct_regions
                .insert(key, (Arc::downgrade(&owner), region));
        }
        let (weak, region) = self.jit.as_mut()?.construct_regions.get_mut(&key)?;
        if !weak
            .upgrade()
            .is_some_and(|cached| Arc::ptr_eq(&cached, &owner))
        {
            self.jit.as_mut()?.construct_regions.remove(&key);
            return None;
        }
        let region = region.as_mut()?;
        self.fibers[fiber].frames.last_mut()?.jit_construct_unsupported=None;
        region.count += 1;
        if region.compiled.is_none() && region.count != JIT_THRESHOLD {
            return None;
        }
        // Move the plan out while compiling/guarding so no JitState reference
        // aliases VM operations. Restore it on every normal failure path.
        let mut region = self.jit.as_mut()?.construct_regions.remove(&key)?.1?;
        let result = self.execute_construct_region(fiber, base, slots, writable, &mut region);
        self.jit
            .as_mut()?
            .construct_regions
            .insert(key, (Arc::downgrade(&owner), Some(region)));
        result
    }

    fn execute_construct_region(
        &mut self,
        fiber: usize,
        base: usize,
        slots: usize,
        writable: u32,
        region: &mut Region,
    ) -> Option<FnJitOutcome> {
        // Each entry uses fresh bindings; changing a helper/default/capture can
        // never silently continue with stale machine-code call assumptions.
        for (&index, &(kind, arity)) in &region.callees {
            let Constant::String(mut name) = region.constants[index].clone() else {
                return None;
            };
            let name = name.as_flat_str();
            if kind >= 32 {
                let current = self.construct_record(fiber, name, arity)?;
                if !Arc::ptr_eq(&current.owner, &region.records[(kind - 32) as usize].owner) {
                    return None;
                }
            } else {
                let expected = match kind {
                    8 => crate::bifs::ascii_bif,
                    16 => crate::bifs::array_append,
                    _ => intrinsic(name)?.2,
                };
                let value = self.private_value(fiber, name)?;
                let id = value.as_gc_id()?;
                if !matches!(self.heap.get_opt(id),Some(GcObject::NativeFunction(function)) if std::ptr::fn_addr_eq(*function,expected as crate::types::BxNativeFunction))
                {
                    return None;
                }
            }
        }
        if self.fibers[fiber].frames.last()?.local_names.values().any(|binding|binding.local && region.callees.keys().any(|index|matches!(&region.constants[*index],Constant::String(name) if name.clone().as_flat_str().eq_ignore_ascii_case(self.interner.resolve(binding.key))))) {return None;}
        if region.compiled.is_none() {
            let jit = self.jit.as_mut()?;
            let compiled = jit.compile_hot_fn(
                Arc::as_ptr(&region.owner) as usize,
                &region.code,
                &region.constants,
                writable,
                &HashMap::new(),
                &[],
                Some(&region.callees),
            );
            match compiled {
                Ok(code) => {
                    region.compiled = Some(code);
                    if jit.trace {
                        eprintln!(
                            "[JIT] compiled constructing loop fn name={}",
                            self.fibers[fiber].frames.last()?.function.name
                        );
                    }
                }
                Err(error) => {
                    if jit.trace {
                        eprintln!("[JIT] constructing compile failed: {}", error);
                    }
                    jit.module.clear_context(&mut jit.ctx);
                    jit.func_ctx = FunctionBuilderContext::new();
                    return None;
                }
            }
        }
        self.sync_jit_locals(fiber, false);
        let mut saved = [BxValue::new_null(); 32];
        saved[..slots].copy_from_slice(&self.fibers[fiber].stack[base..base + slots]);
        let locals = unsafe { self.fibers[fiber].stack.as_mut_ptr().add(base) as *mut u64 };
        let scope = self.fibers[fiber]
            .frames
            .last()?
            .local_scope
            .and_then(|value| value.as_gc_id());
        let mut environment = Environment {
            read: FnReadEnv {
                vm: self as *const _,
                local_scope: scope,
            },
            records: &region.records,
            undo: Vec::new(),
        };
        let mut output = 0u64;
        let status = unsafe {
            region.compiled?(
                locals,
                &mut environment as *mut _ as *const std::ffi::c_void,
                &mut output,
            )
        };
        if status == 1 {
            for (id, length) in environment.undo {
                if let GcObject::Array(array) = self.heap.get_mut(id) {
                    array.truncate(length);
                }
            }
            self.fibers[fiber].stack[base..base + slots].copy_from_slice(&saved[..slots]);
            if self.jit.as_ref()?.trace {
                eprintln!("[JIT] deopt constructing loop (rolled back appends)");
            }
            return None;
        }
        for slot in 0..slots {
            if writable & (1 << slot) != 0 {
                self.set_local_value(fiber, slot, self.fibers[fiber].stack[base + slot]);
            }
        }
        if !region.executed {
            region.executed = true;
            if self.jit.as_ref()?.trace {
                eprintln!(
                    "[JIT] executed constructing loop fn name={}",
                    self.fibers[fiber].frames.last()?.function.name
                );
            }
        }
        if status == 2 {
            Some(FnJitOutcome::Yield)
        } else {
            Some(FnJitOutcome::Resume(region.exit))
        }
    }
}
