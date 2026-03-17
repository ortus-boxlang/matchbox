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

use cranelift_codegen::ir::{AbiParam, BlockArg, InstBuilder, MemFlags, UserFuncName};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::types::{F64, I64, I32};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
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

pub unsafe extern "C" fn jit_ic_member_fallback(
    heap_ptr: *const std::ffi::c_void,
    gc_id: u64,
    expected_shape: u32,
    prop_idx: u32,
    out_val: *mut u64,
) -> u64 {
    // Check if it's a pointer
    if (gc_id & !crate::types::BxValue::PAYLOAD_MASK) != (crate::types::BxValue::TAGGED_BASE | (crate::types::BxValue::TAG_PTR << crate::types::BxValue::TAG_SHIFT)) {
        return 1;
    }

    let heap = &*(heap_ptr as *const crate::vm::gc::Heap);
    let id = (gc_id & crate::types::BxValue::PAYLOAD_MASK) as usize;
    // Basic bounds check, although BxValue guarantees validity unless collected
    // but better safe than sorry in JIT.
    if let Some(obj) = heap.get_opt(id) {
        match obj {
            crate::vm::gc::GcObject::Struct(s) => {
                if s.shape_id == expected_shape {
                    if let Some(v) = s.properties.get(prop_idx as usize) {
                        *out_val = v.to_bits();
                        return 0; // Success
                    }
                }
            }
            crate::vm::gc::GcObject::Instance(i) => {
                if i.shape_id == expected_shape {
                    if let Some(v) = i.properties.get(prop_idx as usize) {
                        *out_val = v.to_bits();
                        return 0; // Success
                    }
                }
            }
            _ => {}
        }
    }
    1 // Deopt
}

// ── JitState ──────────────────────────────────────────────────────────────────

pub struct JitState {
    module: JITModule,
    ctx: Context,
    func_ctx: FunctionBuilderContext,
    func_counter: u32,

    // Tier-1: empty self-loops
    loop_counts: HashMap<(usize, usize), u64>,
    compiled_loops: HashMap<(usize, usize), JitLoopFn>,

    // Tier-2: generic numeric-body loops
    generic_counts: HashMap<(usize, usize), u64>,
    compiled_generic: HashMap<(usize, usize), GenericJitLoopFn>,

    // Tier-3: array iterator body loops
    iter_counts:    HashMap<(usize, usize), u64>,
    compiled_iters: HashMap<(usize, usize), ArrayIterJitFn>,
}

impl JitState {
    pub fn new() -> anyhow::Result<Self> {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").unwrap();

        let isa_builder = cranelift_native::builder()
            .map_err(|e| anyhow::anyhow!("cranelift-native: {}", e))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| anyhow::anyhow!("ISA finish: {}", e))?;

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        builder.symbol("jit_ic_member_fallback", jit_ic_member_fallback as *const u8);
        let module = JITModule::new(builder);
        let ctx = module.make_context();

        Ok(Self {
            module,
            ctx,
            func_ctx: FunctionBuilderContext::new(),
            func_counter: 0,
            loop_counts: HashMap::new(),
            compiled_loops: HashMap::new(),
            generic_counts: HashMap::new(),
            compiled_generic: HashMap::new(),
            iter_counts:    HashMap::new(),
            compiled_iters: HashMap::new(),
        })
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
                    eprintln!("[JIT] compiled empty loop @ code=0x{:x} ip={}", code_ptr, ip);
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
        let func_id = self.module.declare_function(&func_name, Linkage::Local, &sig)?;

        self.ctx.func.name = UserFuncName::user(0, self.func_counter);
        self.func_counter += 1;
        self.ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_ctx);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);

            let v_i     = builder.block_params(block)[0];
            let v_limit = builder.block_params(block)[1];
            let cmp     = builder.ins().fcmp(FloatCC::LessThan, v_i, v_limit);
            let result  = builder.ins().select(cmp, v_limit, v_i);
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
            let prev  = *count;
            *count = prev + iters;
            (prev, prev + iters)
        };
        if prev < JIT_THRESHOLD && new_count >= JIT_THRESHOLD {
            if !Self::body_is_translatable(body_code, ic_entries, constants) {
                return false;
            }
            match self.compile_generic_loop(code_ptr, ip, body_code, ic_entries, i_slot, limit_val, constants) {
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

    fn body_is_translatable(body_code: &[u32], ic_entries: &[Option<crate::vm::chunk::IcEntry>], constants: &HashMap<u32, f64>) -> bool {
        for (idx, &word) in body_code.iter().enumerate() {
            let opcode = (word & 0xFF) as u8;
            let op0    = word >> 8;
            match opcode {
                op::GET_LOCAL | op::SET_LOCAL_POP => {}
                op::CONSTANT => {
                    if !constants.contains_key(&op0) {
                        return false; // non-numeric constant
                    }
                }
                op::ADD | op::ADD_FLOAT | op::ADD_INT
                | op::SUBTRACT | op::MULTIPLY | op::DIVIDE => {}
                op::MEMBER => {
                    // Only support Monomorphic ICs
                    match &ic_entries[idx] {
                        Some(crate::vm::chunk::IcEntry::Monomorphic { .. }) => {}
                        _ => return false,
                    }
                }
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
            let op0    = word >> 8;
            if opcode == op::GET_LOCAL || opcode == op::SET_LOCAL_POP {
                slot_set.insert(op0);
            }
        }
        let referenced: Vec<u32> = slot_set.into_iter().collect();
        let n_ref = referenced.len();
        let slot_idx: HashMap<u32, usize> =
            referenced.iter().enumerate().map(|(i, &s)| (s, i)).collect();

        let ptr_type = self.module.isa().pointer_type();
        let mut sig  = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type)); // locals_ptr
        sig.params.push(AbiParam::new(ptr_type)); // heap_ptr
        sig.returns.push(AbiParam::new(I64)); // Return status (0=OK, 1=Deopt)

        let func_name = format!("jit_gloop_x{:x}_ip{}", code_ptr, ip);
        let func_id   = self.module.declare_function(&func_name, Linkage::Local, &sig)?;

        self.ctx.func.name      = UserFuncName::user(0, self.func_counter);
        self.func_counter      += 1;
        self.ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_ctx);

            let entry_block  = builder.create_block();
            let loop_header  = builder.create_block();
            let loop_body    = builder.create_block();
            let loop_exit    = builder.create_block();
            let deopt_exit   = builder.create_block();

            builder.append_block_params_for_function_params(entry_block);

            // Add heap_ptr as the first parameter to these blocks
            builder.append_block_param(loop_header, ptr_type);
            builder.append_block_param(loop_body, ptr_type);
            builder.append_block_param(loop_exit, ptr_type);
            builder.append_block_param(deopt_exit, ptr_type);

            for _ in 0..n_ref {
                builder.append_block_param(loop_header, I64);
                builder.append_block_param(loop_body,   I64);
                builder.append_block_param(loop_exit,   I64);
                builder.append_block_param(deopt_exit,  I64);
            }

            let mut ext_sig = self.module.make_signature();
            ext_sig.params.push(AbiParam::new(ptr_type)); // heap_ptr
            ext_sig.params.push(AbiParam::new(I64)); // gc_id
            ext_sig.params.push(AbiParam::new(I32)); // expected_shape
            ext_sig.params.push(AbiParam::new(I32)); // prop_idx
            ext_sig.params.push(AbiParam::new(ptr_type)); // out_val
            ext_sig.returns.push(AbiParam::new(I64)); // status
            let ext_func_id = self.module.declare_function("jit_ic_member_fallback", Linkage::Import, &ext_sig).unwrap();
            let ext_func_ref = self.module.declare_func_in_func(ext_func_id, &mut builder.func);

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
            let is_float = builder.ins().icmp_imm(IntCC::UnsignedLessThan, v_i_i64, 0xFFF8000000000000_u64 as i64);
            let check_limit_block = builder.create_block();
            
            let header_args: Vec<BlockArg> = header_vals.iter().map(|&v| BlockArg::from(v)).collect();
            builder.ins().brif(is_float, check_limit_block, &[], deopt_exit, &header_args);
            
            builder.switch_to_block(check_limit_block);
            builder.seal_block(check_limit_block);
            
            let v_i_f64 = builder.ins().bitcast(F64, MemFlags::new(), v_i_i64);
            let v_limit = builder.ins().f64const(limit_val);
            let cmp     = builder.ins().fcmp(FloatCC::LessThan, v_i_f64, v_limit);
            builder.ins().brif(cmp, loop_body, &header_args, loop_exit, &header_args);

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
                let op0    = word >> 8;
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
                        let a_is_float = builder.ins().icmp_imm(IntCC::UnsignedLessThan, a_i64, 0xFFF8000000000000_u64 as i64);
                        let b_is_float = builder.ins().icmp_imm(IntCC::UnsignedLessThan, b_i64, 0xFFF8000000000000_u64 as i64);
                        let both_float = builder.ins().band(a_is_float, b_is_float);
                        
                        let op_block = builder.create_block();
                        let mut current_header_args: Vec<BlockArg> = vec![BlockArg::from(heap_ptr)];
                        current_header_args.extend(referenced.iter().map(|s| BlockArg::from(*slot_val.get(s).unwrap())));
                        builder.ins().brif(both_float, op_block, &[], deopt_exit, &current_header_args);
                        
                        builder.switch_to_block(op_block);
                        builder.seal_block(op_block);
                        
                        let a_f64 = builder.ins().bitcast(F64, MemFlags::new(), a_i64);
                        let b_f64 = builder.ins().bitcast(F64, MemFlags::new(), b_i64);
                        
                        let res_f64 = match opcode {
                            op::ADD | op::ADD_FLOAT => builder.ins().fadd(a_f64, b_f64),
                            op::SUBTRACT => builder.ins().fsub(a_f64, b_f64),
                            op::MULTIPLY => builder.ins().fmul(a_f64, b_f64),
                            op::DIVIDE   => builder.ins().fdiv(a_f64, b_f64),
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
                        current_header_args.extend(referenced.iter().map(|s| BlockArg::from(*slot_val.get(s).unwrap())));
                        builder.ins().brif(both_int, op_block, &[], deopt_exit, &current_header_args);
                        
                        builder.switch_to_block(op_block);
                        builder.seal_block(op_block);
                        
                        let a_payload = builder.ins().band_imm(a_i64, 0x0000FFFFFFFFFFFF_u64 as i64);
                        let a_32 = builder.ins().ireduce(I32, a_payload);
                        let b_payload = builder.ins().band_imm(b_i64, 0x0000FFFFFFFFFFFF_u64 as i64);
                        let b_32 = builder.ins().ireduce(I32, b_payload);
                        let res_32 = builder.ins().iadd(a_32, b_32);
                        let res_64 = builder.ins().uextend(I64, res_32);
                        vstack.push(builder.ins().bor_imm(res_64, target_imm));
                    }
                    op::MEMBER => {
                        let base_val = vstack.pop().unwrap();
                        let (expected_shape, prop_idx) = match &ic_entries[idx] {
                            Some(crate::vm::chunk::IcEntry::Monomorphic { shape_id, index }) => (*shape_id, *index),
                            _ => unreachable!(),
                        };
                        
                        let out_val_slot = builder.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(cranelift_codegen::ir::StackSlotKind::ExplicitSlot, 8, 3));
                        let out_val_ptr = builder.ins().stack_addr(ptr_type, out_val_slot, 0);
                        
                        let shape_arg = builder.ins().iconst(I32, expected_shape as i64);
                        let idx_arg = builder.ins().iconst(I32, prop_idx as i64);
                        
                        let call_inst = builder.ins().call(ext_func_ref, &[heap_ptr, base_val, shape_arg, idx_arg, out_val_ptr]);
                        let status = builder.inst_results(call_inst)[0];
                        
                        let is_deopt = builder.ins().icmp_imm(IntCC::Equal, status, 1);
                        
                        let op_block = builder.create_block();
                        let mut current_header_args: Vec<BlockArg> = vec![BlockArg::from(heap_ptr)];
                        current_header_args.extend(referenced.iter().map(|s| BlockArg::from(*slot_val.get(s).unwrap())));
                        
                        builder.ins().brif(is_deopt, deopt_exit, &current_header_args, op_block, &[]);
                        
                        builder.switch_to_block(op_block);
                        builder.seal_block(op_block);
                        
                        let loaded_val = builder.ins().load(I64, MemFlags::new(), out_val_ptr, 0);
                        vstack.push(loaded_val);
                    }
                    _ => unreachable!("body_is_translatable checked this"),
                }
            }

            // Increment i by 1.0 (assuming i is float)
            let v_i_cur_i64 = *slot_val.get(&i_slot).unwrap();
            let i_is_float = builder.ins().icmp_imm(IntCC::UnsignedLessThan, v_i_cur_i64, 0xFFF8000000000000_u64 as i64);
            let inc_block = builder.create_block();
            let mut current_header_args: Vec<BlockArg> = vec![BlockArg::from(heap_ptr)];
            current_header_args.extend(referenced.iter().map(|s| BlockArg::from(*slot_val.get(s).unwrap())));
            builder.ins().brif(i_is_float, inc_block, &[], deopt_exit, &current_header_args);
            
            builder.switch_to_block(inc_block);
            builder.seal_block(inc_block);
            
            let v_i_cur_f64 = builder.ins().bitcast(F64, MemFlags::new(), v_i_cur_i64);
            let v_one_f64 = builder.ins().f64const(1.0);
            let v_i_next_f64 = builder.ins().fadd(v_i_cur_f64, v_one_f64);
            slot_val.insert(i_slot, builder.ins().bitcast(I64, MemFlags::new(), v_i_next_f64));

            let mut updated_args: Vec<BlockArg> = vec![BlockArg::from(heap_ptr)];
            updated_args.extend(referenced.iter().map(|s| BlockArg::from(*slot_val.get(s).unwrap())));
            builder.ins().jump(loop_header, &updated_args);
            builder.seal_block(loop_body);
            builder.seal_block(loop_header);

            // ── loop_exit (normal completion) ────────────────────────────────
            builder.switch_to_block(loop_exit);
            builder.seal_block(loop_exit);
            let exit_vals = builder.block_params(loop_exit).to_vec();
            for (idx, &slot) in referenced.iter().enumerate() {
                builder.ins().store(MemFlags::new(), exit_vals[idx + 1], locals_ptr, (slot * 8) as i32);
            }
            let ret_0 = builder.ins().iconst(I64, 0);
            builder.ins().return_(&[ret_0]);

            // ── deopt_exit (bailing out early) ───────────────────────────────
            builder.switch_to_block(deopt_exit);
            builder.seal_block(deopt_exit);
            let deopt_vals = builder.block_params(deopt_exit).to_vec();
            for (idx, &slot) in referenced.iter().enumerate() {
                builder.ins().store(MemFlags::new(), deopt_vals[idx + 1], locals_ptr, (slot * 8) as i32);
            }
            let ret_1 = builder.ins().iconst(I64, 1);
            builder.ins().return_(&[ret_1]);

            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;
        let fn_ptr: GenericJitLoopFn = unsafe { std::mem::transmute(self.module.get_finalized_function(func_id)) };
        self.compiled_generic.insert((code_ptr, ip), fn_ptr);
        Ok(())
    }

    // ── Tier-3: array iterator loop ───────────────────────────────────────────

    /// Returns Some(item_slot) if body[0] is SET_LOCAL_POP and body[1..] is
    /// translatable (numeric ops only, no MEMBER). Returns None otherwise.
    fn body_is_iter_translatable(
        body_code: &[u32],
        constants: &HashMap<u32, f64>,
    ) -> Option<u32> {
        if body_code.is_empty() { return None; }
        if (body_code[0] & 0xFF) as u8 != op::SET_LOCAL_POP { return None; }
        let item_slot = body_code[0] >> 8;
        for &word in &body_code[1..] {
            let opcode = (word & 0xFF) as u8;
            let op0    = word >> 8;
            match opcode {
                op::GET_LOCAL | op::SET_LOCAL_POP => {}
                op::CONSTANT => {
                    if !constants.contains_key(&op0) { return None; }
                }
                op::ADD | op::ADD_FLOAT | op::ADD_INT
                | op::SUBTRACT | op::MULTIPLY | op::DIVIDE => {}
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
            match self.compile_iter_loop(code_ptr, ip, cursor_slot, item_slot, body_code, constants) {
                Ok(_) => {
                    eprintln!("[JIT] compiled iter loop @ code=0x{:x} ip={} after {} iters", code_ptr, ip, new_count);
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
            let op0    = word >> 8;
            if opcode == op::GET_LOCAL || opcode == op::SET_LOCAL_POP {
                slot_set.insert(op0);
            }
        }
        let referenced: Vec<u32> = slot_set.into_iter().collect();
        let n_ref = referenced.len();
        let slot_idx: HashMap<u32, usize> =
            referenced.iter().enumerate().map(|(i, &s)| (s, i)).collect();

        let ptr_type = self.module.isa().pointer_type();
        let mut sig  = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type)); // locals_ptr
        sig.params.push(AbiParam::new(ptr_type)); // array_data
        sig.params.push(AbiParam::new(I64));       // array_len
        sig.returns.push(AbiParam::new(I64));      // 0=done, 1=deopt

        let func_name = format!("jit_iter_x{:x}_ip{}", code_ptr, ip);
        let func_id   = self.module.declare_function(&func_name, Linkage::Local, &sig)?;

        self.ctx.func.name      = UserFuncName::user(0, self.func_counter);
        self.func_counter      += 1;
        self.ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_ctx);

            let entry_block = builder.create_block();
            let loop_header = builder.create_block();
            let loop_body   = builder.create_block();
            let loop_exit   = builder.create_block();
            let deopt_exit  = builder.create_block();

            builder.append_block_params_for_function_params(entry_block);

            // Block params: n_ref × I64 (slot values); no heap_ptr — array_data/array_len
            // are loop-invariant SSA values from entry_block, valid in all dominated blocks.
            for _ in 0..n_ref {
                builder.append_block_param(loop_header, I64);
                builder.append_block_param(loop_body,   I64);
                builder.append_block_param(loop_exit,   I64);
                builder.append_block_param(deopt_exit,  I64);
            }

            // ── entry_block ──────────────────────────────────────────────────
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);
            let entry_params = builder.block_params(entry_block).to_vec();
            let locals_ptr   = entry_params[0];
            let array_data   = entry_params[1];
            let array_len    = entry_params[2];

            let mut init_vals: Vec<cranelift_codegen::ir::Value> = Vec::new();
            for &slot in &referenced {
                let byte_off = (slot * 8) as i32;
                let v = builder.ins().load(I64, MemFlags::new(), locals_ptr, byte_off);
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
                IntCC::UnsignedLessThan, v_cursor_i64, 0xFFF8000000000000_u64 as i64,
            );
            let check_bounds_block = builder.create_block();
            let header_args: Vec<BlockArg> = header_vals.iter().map(|&v| BlockArg::from(v)).collect();
            builder.ins().brif(cursor_is_float, check_bounds_block, &[], deopt_exit, &header_args);

            // check_bounds_block: single predecessor (loop_header) → seal immediately
            builder.switch_to_block(check_bounds_block);
            builder.seal_block(check_bounds_block);

            // v_cursor_i64 is from loop_header which dominates this block — valid SSA use
            let v_cursor_f64  = builder.ins().bitcast(F64, MemFlags::new(), v_cursor_i64);
            let v_cursor_uint = builder.ins().fcvt_to_uint(I64, v_cursor_f64);
            let in_bounds     = builder.ins().icmp(IntCC::UnsignedLessThan, v_cursor_uint, array_len);
            builder.ins().brif(in_bounds, loop_body, &header_args, loop_exit, &header_args);

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
            let v_cur_i64  = *slot_val.get(&cursor_slot).unwrap();
            let v_cur_f64  = builder.ins().bitcast(F64, MemFlags::new(), v_cur_i64);
            let v_cur_uint = builder.ins().fcvt_to_uint(I64, v_cur_f64);

            // Load arr[cursor]
            let byte_offset = builder.ins().imul_imm(v_cur_uint, 8);
            let elem_ptr    = builder.ins().iadd(array_data, byte_offset);
            let elem_raw    = builder.ins().load(I64, MemFlags::new(), elem_ptr, 0);

            // Type guard: element must be a float
            let elem_is_float = builder.ins().icmp_imm(
                IntCC::UnsignedLessThan, elem_raw, 0xFFF8000000000000_u64 as i64,
            );
            let body_op_block = builder.create_block();
            {
                let deopt_args: Vec<BlockArg> = referenced.iter()
                    .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                    .collect();
                builder.ins().brif(elem_is_float, body_op_block, &[], deopt_exit, &deopt_args);
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
                let op0    = word >> 8;
                match opcode {
                    op::GET_LOCAL => {
                        vstack.push(*slot_val.get(&op0).unwrap());
                    }
                    op::SET_LOCAL_POP => {
                        let v = vstack.pop().unwrap();
                        slot_val.insert(op0, v);
                    }
                    op::CONSTANT => {
                        let val     = constants[&op0];
                        let val_f64 = builder.ins().f64const(val);
                        let val_i64 = builder.ins().bitcast(I64, MemFlags::new(), val_f64);
                        vstack.push(val_i64);
                    }
                    op::ADD | op::ADD_FLOAT | op::SUBTRACT | op::MULTIPLY | op::DIVIDE => {
                        let b_i64 = vstack.pop().unwrap();
                        let a_i64 = vstack.pop().unwrap();
                        let a_is_float = builder.ins().icmp_imm(IntCC::UnsignedLessThan, a_i64, 0xFFF8000000000000_u64 as i64);
                        let b_is_float = builder.ins().icmp_imm(IntCC::UnsignedLessThan, b_i64, 0xFFF8000000000000_u64 as i64);
                        let both_float = builder.ins().band(a_is_float, b_is_float);
                        let arith_block = builder.create_block();
                        {
                            let deopt_args: Vec<BlockArg> = referenced.iter()
                                .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                                .collect();
                            builder.ins().brif(both_float, arith_block, &[], deopt_exit, &deopt_args);
                        }
                        builder.switch_to_block(arith_block);
                        builder.seal_block(arith_block);
                        let a_f64   = builder.ins().bitcast(F64, MemFlags::new(), a_i64);
                        let b_f64   = builder.ins().bitcast(F64, MemFlags::new(), b_i64);
                        let res_f64 = match opcode {
                            op::ADD | op::ADD_FLOAT => builder.ins().fadd(a_f64, b_f64),
                            op::SUBTRACT           => builder.ins().fsub(a_f64, b_f64),
                            op::MULTIPLY           => builder.ins().fmul(a_f64, b_f64),
                            op::DIVIDE             => builder.ins().fdiv(a_f64, b_f64),
                            _ => unreachable!(),
                        };
                        let res_i64 = builder.ins().bitcast(I64, MemFlags::new(), res_f64);
                        vstack.push(res_i64);
                    }
                    op::ADD_INT => {
                        let b_i64 = vstack.pop().unwrap();
                        let a_i64 = vstack.pop().unwrap();
                        let mask_imm   = 0xFFFF000000000000_u64 as i64;
                        let target_imm = 0xFFF8000000000000_u64 as i64;
                        let a_masked = builder.ins().band_imm(a_i64, mask_imm);
                        let a_is_int = builder.ins().icmp_imm(IntCC::Equal, a_masked, target_imm);
                        let b_masked = builder.ins().band_imm(b_i64, mask_imm);
                        let b_is_int = builder.ins().icmp_imm(IntCC::Equal, b_masked, target_imm);
                        let both_int = builder.ins().band(a_is_int, b_is_int);
                        let int_block = builder.create_block();
                        {
                            let deopt_args: Vec<BlockArg> = referenced.iter()
                                .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                                .collect();
                            builder.ins().brif(both_int, int_block, &[], deopt_exit, &deopt_args);
                        }
                        builder.switch_to_block(int_block);
                        builder.seal_block(int_block);
                        let a_payload = builder.ins().band_imm(a_i64, 0x0000FFFFFFFFFFFF_u64 as i64);
                        let a_32 = builder.ins().ireduce(I32, a_payload);
                        let b_payload = builder.ins().band_imm(b_i64, 0x0000FFFFFFFFFFFF_u64 as i64);
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
                IntCC::UnsignedLessThan, v_cur_now_i64, 0xFFF8000000000000_u64 as i64,
            );
            let inc_block = builder.create_block();
            {
                let deopt_args: Vec<BlockArg> = referenced.iter()
                    .map(|s| BlockArg::from(*slot_val.get(s).unwrap()))
                    .collect();
                builder.ins().brif(cur_still_float, inc_block, &[], deopt_exit, &deopt_args);
            }
            builder.switch_to_block(inc_block);
            builder.seal_block(inc_block);

            let v_cur_f64_now  = builder.ins().bitcast(F64, MemFlags::new(), v_cur_now_i64);
            let v_one          = builder.ins().f64const(1.0);
            let v_cur_next_f64 = builder.ins().fadd(v_cur_f64_now, v_one);
            slot_val.insert(cursor_slot, builder.ins().bitcast(I64, MemFlags::new(), v_cur_next_f64));

            let updated_args: Vec<BlockArg> = referenced.iter()
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
                builder.ins().store(MemFlags::new(), exit_vals[idx], locals_ptr, (slot * 8) as i32);
            }
            let ret_0 = builder.ins().iconst(I64, 0);
            builder.ins().return_(&[ret_0]);

            // ── deopt_exit ───────────────────────────────────────────────────
            builder.switch_to_block(deopt_exit);
            builder.seal_block(deopt_exit);
            let deopt_vals = builder.block_params(deopt_exit).to_vec();
            for (idx, &slot) in referenced.iter().enumerate() {
                builder.ins().store(MemFlags::new(), deopt_vals[idx], locals_ptr, (slot * 8) as i32);
            }
            let ret_1 = builder.ins().iconst(I64, 1);
            builder.ins().return_(&[ret_1]);

            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;
        let fn_ptr: ArrayIterJitFn = unsafe {
            std::mem::transmute(self.module.get_finalized_function(func_id))
        };
        self.compiled_iters.insert((code_ptr, ip), fn_ptr);
        Ok(())
    }
}
