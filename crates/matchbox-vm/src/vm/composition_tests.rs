//! Per-VM, test-only observations/injection: no global interrupt races or
//! production instrumentation, phase state, feature switches or helper ABI.
use super::*;
use crate::types::{BxCompiledFunction, FunctionKind, FunctionModifiers, NativeFutureValue};
use super::scopes::{ScopeMode, SlotBindings};

#[derive(Default)]
pub(super) struct Probe {
    pub(super) initial_counter: u32,
    interrupt_at: Option<u8>,
    stop_after_hits: Option<usize>,
    events: Vec<(u8, usize, u32, Vec<BxValue>)>,
}

impl Probe {
    pub(super) fn interrupt(&mut self, phase: u8) -> bool {
        if self.interrupt_at == Some(phase) {
            self.interrupt_at = None;
            true
        } else {
            false
        }
    }

    pub(super) fn record(&mut self, phase: u8, ip: usize, counter: u32, stack: &[BxValue]) {
        self.events.push((phase, ip, counter, stack.to_vec()));
        if let Some(limit) = self.stop_after_hits
            && self.events.iter().filter(|event| event.0 == 0).count() >= limit
        {
            self.interrupt_at = Some(1);
        }
    }
}

fn function(chunk: Chunk) -> BxCompiledFunction {
    BxCompiledFunction {
        name: "composition-contract".into(), kind: FunctionKind::Udf,
        arity: 0, min_arity: 0, params: Vec::new(), bif: None,
        modifiers: FunctionModifiers::default(), captured_receiver: None,
        captured_scopes: Vec::new(), chunk,
    }
}

fn fixture(left: BxValue, right: f64) -> VM {
    let mut vm = VM::new();
    let future_id = vm.future_new().as_gc_id().unwrap();
    let mut chunk = Chunk::default();
    let index = chunk.add_constant(Constant::Number(right));
    assert_eq!(index, 0);
    chunk.emit1(op::GET_LOCAL, 0, 1);
    chunk.emit1(op::CONSTANT, index, 2);
    chunk.emit0(op::ADD, 3);
    let function = Rc::new(function(chunk.clone()));
    vm.fibers.push(BxFiber {
        stack: vec![BxValue::new_int(91), left],
        frames: vec![CallFrame {
            function, chunk: Rc::new(RefCell::new(chunk)), ip: 0,
            stack_base: 1, receiver: None, handlers: Vec::new(),
            pending_completion: None, promoted_constants: vec![Some(BxValue::new_number(right))],
            local_scope: None, context: None, local_names: SlotBindings::new(),
            scope_mode: ScopeMode::Binding,
            #[cfg(feature = "jit")]
            jit_loop_attempted: false,
            #[cfg(feature = "jit")]
            jit_construct_unsupported: None,
        }],
        variables: VM::new_variables_scope(), future_id, wait_until: None,
        yield_requested: false, running: false, blocked_on_future: false,
        finished: false, priority: 0, root_stack: Vec::new(),
    });
    vm.current_fiber_idx = Some(0);
    vm.composition_probe = Some(Probe::default());
    assert!(!vm.heap.should_collect());
    vm
}

fn hits(vm: &VM) -> usize {
    vm.composition_probe.as_ref().unwrap().events.iter().filter(|e| e.0 == 0).count()
}

fn run(vm: &mut VM) -> BxValue {
    vm.run_fiber_inner(0, None).unwrap();
    *vm.fibers[0].stack.last().unwrap()
}

fn restart(vm: &mut VM) {
    vm.fibers[0].frames[0].ip = 0;
    vm.fibers[0].stack.truncate(2);
}

#[test]
fn cached_float_composition_keeps_prefix_and_logical_ticks() {
    let mut vm = fixture(BxValue::new_number(7.0), 2.0);
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 1);
    assert_eq!(vm.fibers[0].stack, vec![BxValue::new_int(91), BxValue::new_number(7.0), BxValue::new_number(9.0)]);
    let event = &vm.composition_probe.as_ref().unwrap().events[0];
    assert_eq!((event.0, event.1, event.2), (0, 3, 3));
}

#[test]
fn floats_preserve_signed_zero_infinity_nan_and_operand_order() {
    for (left, right) in [(-0.0, -0.0), (0.0, -0.0), (f64::INFINITY, f64::NEG_INFINITY),
        (f64::NAN, 3.0), (5.0, -8.0), (1e300, -1e300)] {
        let left_value = BxValue::new_number(left);
        let right_value = BxValue::new_number(right);
        let mut vm = fixture(left_value, right);
        let expected = BxValue::new_number(left_value.as_number() + right_value.as_number());
        let actual = run(&mut vm);
        if expected.as_number().is_nan() {
            assert!(actual.as_number().is_nan());
            assert!(actual.is_float());
        } else {
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
        assert_eq!(hits(&vm), 1);
    }
}

#[test]
fn integer_string_and_array_left_keep_generic_add() {
    let mut integer = fixture(BxValue::new_int(7), 2.0);
    assert_eq!(run(&mut integer).as_number(), 9.0);
    assert_eq!(hits(&integer), 0);
    for text in ["7", "word"] {
        let mut vm = fixture(BxValue::new_null(), 2.0);
        let left = BxValue::new_ptr(vm.string_new(text.into()));
        vm.fibers[0].stack[1] = left;
        let result = run(&mut vm);
        if text == "7" { assert_eq!(result.as_number(), 9.0); }
        else { assert_eq!(vm.to_string(result), "word2"); }
        assert_eq!(hits(&vm), 0);
    }
    let mut vm = fixture(BxValue::new_null(), 2.0);
    let left = BxValue::new_ptr(vm.heap.alloc(GcObject::Array(vec![BxValue::new_int(3)])));
    let expected = format!("{}2", vm.to_string(left));
    vm.fibers[0].stack[1] = left;
    let result = run(&mut vm);
    assert_eq!(vm.to_string(result), expected);
    assert_eq!(hits(&vm), 0);
}

#[test]
fn cache_miss_uses_authority_then_can_compose() {
    let mut vm = fixture(BxValue::new_number(7.0), 2.0);
    vm.fibers[0].frames[0].promoted_constants.clear();
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 0);
    assert!(vm.fibers[0].frames[0].promoted_constants[0].is_some());
    restart(&mut vm);
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 1);
}

#[test]
fn current_constant_class_and_cached_value_are_both_checked() {
    let mut vm = fixture(BxValue::new_number(7.0), 2.0);
    vm.fibers[0].frames[0].chunk.borrow_mut().constants_mut()[0] = Constant::String(BoxString::new("changed"));
    // read_constant intentionally returns the existing cached number for a
    // noncapturing slot. Composition still must inspect current classification.
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 0);
    restart(&mut vm);
    vm.fibers[0].frames[0].chunk.borrow_mut().constants_mut()[0] = Constant::Number(2.0);
    vm.fibers[0].frames[0].promoted_constants[0] = Some(BxValue::new_int(2));
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 0);
}

#[test]
fn changed_closure_and_component_constants_still_capture_fresh() {
    for kind in [FunctionKind::Closure, FunctionKind::ComponentBody] {
        let mut vm = fixture(BxValue::new_number(7.0), 2.0);
        let mut captured = function(Chunk::default());
        captured.kind = kind;
        vm.fibers[0].frames[0].chunk.borrow_mut().constants_mut()[0] = Constant::CompiledFunction(captured);
        let result = run(&mut vm);
        assert!(result.is_ptr()); // Existing generic concatenation, not numeric9.
        assert_eq!(hits(&vm), 0);
        let first = vm.read_constant(0, 0).unwrap();
        let second = vm.read_constant(0, 0).unwrap();
        assert_ne!(first, second);
        assert!(matches!(vm.heap.get(first.as_gc_id().unwrap()), GcObject::CompiledFunction(_)));
    }
}

#[test]
fn stack_and_shared_scope_reads_keep_dictionary_authority() {
    let mut stack = fixture(BxValue::new_number(7.0), 2.0);
    stack.fibers[0].frames[0].scope_mode = ScopeMode::Stack;
    Rc::make_mut(&mut stack.fibers[0].frames[0].function).params.push("left".into());
    assert_eq!(run(&mut stack).as_number(), 9.0);
    assert_eq!(hits(&stack), 1);
    let mut shared = fixture(BxValue::new_number(7.0), 2.0);
    shared.bind_local(0, 0, "left");
    let scope = shared.local_scope(0).as_gc_id().unwrap();
    shared.capture_scopes(0);
    shared.struct_set(scope, "left", BxValue::new_number(19.0));
    assert_eq!(run(&mut shared).as_number(), 21.0);
    assert_eq!(hits(&shared), 1);
}

#[test]
fn native_pending_handle_excludes_fusion_until_terminal_drain() {
    let mut vm = fixture(BxValue::new_number(7.0), 2.0);
    let handle = vm.native_future_new();
    let future = handle.future();
    vm.collect_garbage_now();
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 0);
    handle.resolve(NativeFutureValue::Null).unwrap();
    vm.drain_native_completions();
    assert!(vm.pending_native_futures.is_empty());
    assert!(matches!(vm.heap.get(future.as_gc_id().unwrap()), GcObject::Future(f) if matches!(f.status, FutureStatus::Completed)));
    restart(&mut vm);
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 1);
}

#[test]
fn collection_due_preserves_internal_const_safepoint_and_roots() {
    let mut vm = fixture(BxValue::new_number(7.0), 2.0);
    vm.collect_garbage_now();
    while !vm.heap.should_collect() { vm.heap.alloc(GcObject::Array(Vec::new())); }
    assert!(!vm.heap.should_collect_promptly());
    vm.composition_probe.as_mut().unwrap().initial_counter = 1022;
    assert!(vm.run_fiber_inner(0, None).unwrap().is_none());
    assert_eq!(vm.fibers[0].frames[0].ip, 1);
    assert_eq!(vm.fibers[0].stack.len(), 3);
    assert_eq!(vm.fibers[0].stack[2].as_number(), 7.0);
    assert_eq!(hits(&vm), 0);
    vm.collect_garbage_now();
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 0); // Resume at CONST, not a hidden fused entry.
}

#[test]
fn logical_counter_wrap_is_preserved() {
    let mut vm = fixture(BxValue::new_number(7.0), 2.0);
    vm.composition_probe.as_mut().unwrap().initial_counter = u32::MAX - 1;
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(vm.composition_probe.as_ref().unwrap().events[0].2, 1);
}

#[test]
fn logical_ticks_keep_backedge_quantum_clock_phase() {
    for cached in [true, false] {
        let mut vm = fixture(BxValue::new_number(7.0), 2.0);
        {
            let mut chunk = vm.fibers[0].frames[0].chunk.borrow_mut();
            chunk.emit0(op::POP, 4);
            chunk.emit1(op::LOOP, 5, 4);
        }
        if !cached { vm.fibers[0].frames[0].promoted_constants.clear(); }
        let probe = vm.composition_probe.as_mut().unwrap();
        probe.initial_counter = 1018;
        probe.stop_after_hits = Some(4); // A broken phase fails, never spins forever.
        assert!(vm.run_fiber_inner(0, Some(Instant::now())).unwrap().is_none());
        assert_eq!(vm.fibers[0].frames[0].ip, 0);
        assert_eq!(vm.fibers[0].stack.len(), 2);
        assert_eq!(hits(&vm), usize::from(cached));
    }
}

#[test]
fn both_interrupt_points_materialize_prefix_ip_ticks_and_use_handler() {
    for phase in [1, 2] {
        let mut vm = fixture(BxValue::new_number(7.0), 2.0);
        {
            let mut chunk = vm.fibers[0].frames[0].chunk.borrow_mut();
            let result = chunk.add_constant(Constant::Number(99.0));
            chunk.emit0(op::POP, 4); // Catch's exception value.
            chunk.emit1(op::CONSTANT, result, 4);
            chunk.emit0(op::RETURN, 4);
        }
        vm.fibers[0].frames[0].handlers.push(ExceptionHandler {
            catch_ip: Some(3), finally_ip: None, saved_stack_len: 2, catch_enabled: true,
        });
        vm.composition_probe.as_mut().unwrap().interrupt_at = Some(phase);
        assert_eq!(vm.run_fiber_inner(0, None).unwrap().unwrap().as_number(), 99.0);
        let event = &vm.composition_probe.as_ref().unwrap().events[0];
        assert_eq!((event.0, event.1, event.2), (phase, phase as usize + 1, phase as u32 + 1));
        assert_eq!(event.3.len(), phase as usize + 2);
        assert_eq!(event.3[0].as_int(), 91);
        assert_eq!(event.3[2].as_number(), 7.0);
        if phase == 2 { assert_eq!(event.3[3].as_number(), 2.0); }
        assert_eq!(hits(&vm), 0);
    }
}

#[test]
fn truncated_lookahead_is_an_ordinary_local_read() {
    let mut vm = fixture(BxValue::new_number(7.0), 2.0);
    vm.fibers[0].frames[0].chunk.borrow_mut().code_mut().truncate(1);
    assert_eq!(run(&mut vm).as_number(), 7.0);
    assert_eq!(hits(&vm), 0);
}

#[cfg(feature = "debugger")]
#[test]
fn debugger_budget_one_and_two_preserve_intermediate_stack_and_resume() {
    for budget in [1, 2] {
        let mut vm = fixture(BxValue::new_number(7.0), 2.0);
        // One source line isolates instruction-budget stopping from line stepping.
        vm.fibers[0].frames[0].chunk.borrow_mut().lines_mut().fill(1);
        vm.debug_run = Some(DebugRunState { start: None, executed: 0, budget, pause: None });
        assert!(vm.run_fiber_inner(0, None).unwrap().is_none());
        assert_eq!(vm.debug_run.as_ref().unwrap().executed, budget);
        assert_eq!(vm.fibers[0].frames[0].ip, budget as usize);
        assert_eq!(vm.fibers[0].stack.len(), 2 + budget as usize);
        assert_eq!(hits(&vm), 0);
        vm.debug_run = None;
        assert_eq!(run(&mut vm).as_number(), 9.0);
        assert_eq!(hits(&vm), 0);
    }
}

#[test]
fn tracing_uses_original_instructions_in_isolated_process() {
    const CHILD: &str = "MATCHBOX_COMPOSITION_TRACE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "vm::composition_tests::tracing_uses_original_instructions_in_isolated_process", "--nocapture"])
            .env(CHILD, "1").env("BX_TRACE", "1").output().unwrap();
        assert!(output.status.success(), "{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let trace = String::from_utf8_lossy(&output.stderr);
        for name in ["GET_LOCAL", "CONSTANT", "ADD"] { assert!(trace.contains(name), "{trace}"); }
    } else {
        let mut vm = fixture(BxValue::new_number(7.0), 2.0);
        assert_eq!(run(&mut vm).as_number(), 9.0);
        assert_eq!(hits(&vm), 0);
    }
}

#[cfg(feature = "jit")]
#[test]
fn interpreter_composition_does_not_disable_jit() {
    let mut vm = fixture(BxValue::new_number(7.0), 2.0);
    vm.enable_jit();
    assert!(vm.jit.is_some());
    assert_eq!(run(&mut vm).as_number(), 9.0);
    assert_eq!(hits(&vm), 1);
    assert!(vm.jit.is_some());
}
