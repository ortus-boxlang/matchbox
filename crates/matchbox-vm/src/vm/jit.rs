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
use cranelift_codegen::ir::condcodes::FloatCC;
use cranelift_codegen::ir::types::F64;
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
/// back in-place through the pointer.
/// `fn(locals_ptr: *mut u64)`
pub type GenericJitLoopFn = unsafe extern "C" fn(*mut u64);

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

        let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
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

    /// Profile a non-empty loop body.  If threshold crossed and the body only
    /// uses supported opcodes, emit a real native loop.
    /// `body_code`  – bytecode words between the initial cond-check and FOR_LOOP_STEP.
    /// `i_slot`     – local slot for the loop counter.
    /// `limit_val`  – the numeric limit constant (embedded in emitted IR).
    /// `constants`  – map from const_idx → f64 for any CONSTANT opcodes in the body.
    pub fn profile_generic(
        &mut self,
        code_ptr: usize,
        ip: usize,
        iters: u64,
        body_code: &[u32],
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
            if !Self::body_is_translatable(body_code, constants) {
                return false;
            }
            match self.compile_generic_loop(code_ptr, ip, body_code, i_slot, limit_val, constants) {
                Ok(_) => {
                    eprintln!(
                        "[JIT] compiled generic loop @ code=0x{:x} ip={} after {} iters",
                        code_ptr, ip, new_count
                    );
                    return true;
                }
                Err(e) => eprintln!("[JIT] generic loop failed: {}", e),
            }
        }
        false
    }

    /// Quick pre-flight check: can every opcode in the body be translated?
    fn body_is_translatable(body_code: &[u32], constants: &HashMap<u32, f64>) -> bool {
        for &word in body_code {
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
                _ => return false,
            }
        }
        true
    }

    /// Compile the loop body into a real native loop using Cranelift SSA IR.
    ///
    /// The emitted function has signature `fn(*mut u64)` where the pointer
    /// is the VM's locals array (NaN-boxed f64 values stored as raw u64 bits).
    /// All referenced local slots are loaded at entry, kept in SSA block-params
    /// (registers) throughout the loop, and stored back only at loop exit.
    fn compile_generic_loop(
        &mut self,
        code_ptr: usize,
        ip: usize,
        body_code: &[u32],
        i_slot: u32,
        limit_val: f64,
        constants: &HashMap<u32, f64>,
    ) -> anyhow::Result<()> {
        // ── Collect every local slot referenced in the body, plus i_slot ──────
        let mut slot_set: BTreeSet<u32> = BTreeSet::new();
        slot_set.insert(i_slot);
        for &word in body_code {
            let opcode = (word & 0xFF) as u8;
            let op0    = word >> 8;
            if opcode == op::GET_LOCAL || opcode == op::SET_LOCAL_POP {
                slot_set.insert(op0);
            }
        }
        // Sorted vec so block-param order is deterministic.
        let referenced: Vec<u32> = slot_set.into_iter().collect();
        let n_ref = referenced.len();
        // Map slot → index into `referenced`.
        let slot_idx: HashMap<u32, usize> =
            referenced.iter().enumerate().map(|(i, &s)| (s, i)).collect();

        // ── Function signature: fn(locals_ptr: ptr_type) -> () ────────────────
        let ptr_type = self.module.isa().pointer_type();
        let mut sig  = self.module.make_signature();
        sig.params.push(AbiParam::new(ptr_type));

        let func_name = format!("jit_gloop_x{:x}_ip{}", code_ptr, ip);
        let func_id   = self.module.declare_function(&func_name, Linkage::Local, &sig)?;

        self.ctx.func.name      = UserFuncName::user(0, self.func_counter);
        self.func_counter      += 1;
        self.ctx.func.signature = sig;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.func_ctx);

            // ── Create blocks ──────────────────────────────────────────────────
            let entry_block  = builder.create_block();  // function entry: load locals
            let loop_header  = builder.create_block();  // loop condition check
            let loop_body    = builder.create_block();  // loop body + increment
            let loop_exit    = builder.create_block();  // store back + return

            // entry_block receives the function arguments.
            builder.append_block_params_for_function_params(entry_block);

            // loop_header, loop_body, loop_exit each carry one F64 per referenced local.
            for _ in 0..n_ref {
                builder.append_block_param(loop_header, F64);
                builder.append_block_param(loop_body,   F64);
                builder.append_block_param(loop_exit,   F64);
            }

            // ── entry_block: load locals from VM memory, jump to loop_header ──
            builder.switch_to_block(entry_block);
            let locals_ptr = builder.block_params(entry_block)[0];

            let mut init_vals: Vec<cranelift_codegen::ir::Value> = Vec::new();
            for &slot in &referenced {
                // BxValue is a NaN-boxed u64 stored at locals_ptr[slot].
                // Float BxValues have their raw f64 bits as the u64 payload,
                // so a plain F64 load gives the correct value.
                let offset = (slot * 8) as i32;
                let v = builder.ins().load(F64, MemFlags::new(), locals_ptr, offset);
                init_vals.push(v);
            }
            let init_args: Vec<BlockArg> = init_vals.into_iter().map(BlockArg::from).collect();
            builder.ins().jump(loop_header, &init_args);
            builder.seal_block(entry_block);

            // ── loop_header: check i < limit, branch or exit ──────────────────
            // Do NOT seal yet — loop_body has a back-edge into loop_header.
            builder.switch_to_block(loop_header);
            let header_vals: Vec<cranelift_codegen::ir::Value> =
                builder.block_params(loop_header).to_vec();
            let v_i     = header_vals[slot_idx[&i_slot]];
            let v_limit = builder.ins().f64const(limit_val);
            let cmp     = builder.ins().fcmp(FloatCC::LessThan, v_i, v_limit);
            let header_args: Vec<BlockArg> = header_vals.into_iter().map(BlockArg::from).collect();
            builder.ins().brif(cmp, loop_body, &header_args, loop_exit, &header_args);

            // ── loop_body: translate bytecode, increment i, jump to header ────
            builder.switch_to_block(loop_body);
            let body_vals: Vec<cranelift_codegen::ir::Value> =
                builder.block_params(loop_body).to_vec();

            // Current SSA value per slot (kept in registers via block-params).
            let mut slot_val: HashMap<u32, cranelift_codegen::ir::Value> = HashMap::new();
            for (&slot, &idx) in &slot_idx {
                slot_val.insert(slot, body_vals[idx]);
            }

            // Virtual operand stack — mirrors the interpreter's stack discipline.
            let mut vstack: Vec<cranelift_codegen::ir::Value> = Vec::new();

            for &word in body_code {
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
                        vstack.push(builder.ins().f64const(val));
                    }
                    op::ADD | op::ADD_FLOAT | op::ADD_INT => {
                        let b = vstack.pop().unwrap();
                        let a = vstack.pop().unwrap();
                        vstack.push(builder.ins().fadd(a, b));
                    }
                    op::SUBTRACT => {
                        let b = vstack.pop().unwrap();
                        let a = vstack.pop().unwrap();
                        vstack.push(builder.ins().fsub(a, b));
                    }
                    op::MULTIPLY => {
                        let b = vstack.pop().unwrap();
                        let a = vstack.pop().unwrap();
                        vstack.push(builder.ins().fmul(a, b));
                    }
                    op::DIVIDE => {
                        let b = vstack.pop().unwrap();
                        let a = vstack.pop().unwrap();
                        vstack.push(builder.ins().fdiv(a, b));
                    }
                    _ => unreachable!("body_is_translatable checked this"),
                }
            }

            // Increment i by 1.0 (this is what FOR_LOOP_STEP normally does).
            let v_i_cur  = *slot_val.get(&i_slot).unwrap();
            let v_one    = builder.ins().f64const(1.0);
            let v_i_next = builder.ins().fadd(v_i_cur, v_one);
            slot_val.insert(i_slot, v_i_next);

            // Build the updated values vec (same order as block params).
            let updated: Vec<cranelift_codegen::ir::Value> =
                referenced.iter().map(|s| *slot_val.get(s).unwrap()).collect();
            let updated_args: Vec<BlockArg> = updated.into_iter().map(BlockArg::from).collect();
            builder.ins().jump(loop_header, &updated_args);
            builder.seal_block(loop_body);
            // loop_header's predecessors are now all known (entry + body).
            builder.seal_block(loop_header);

            // ── loop_exit: store locals back, return ──────────────────────────
            builder.switch_to_block(loop_exit);
            builder.seal_block(loop_exit);
            let exit_vals: Vec<cranelift_codegen::ir::Value> =
                builder.block_params(loop_exit).to_vec();

            for (idx, &slot) in referenced.iter().enumerate() {
                let offset = (slot * 8) as i32;
                builder.ins().store(MemFlags::new(), exit_vals[idx], locals_ptr, offset);
            }
            builder.ins().return_(&[]);

            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        self.module.clear_context(&mut self.ctx);
        self.module.finalize_definitions()?;

        let code   = self.module.get_finalized_function(func_id);
        let fn_ptr: GenericJitLoopFn = unsafe { std::mem::transmute(code) };
        self.compiled_generic.insert((code_ptr, ip), fn_ptr);
        Ok(())
    }
}
