use super::{ScopeMode, SlotBindings, VM};
use crate::types::{BxCompiledFunction, BxVM, BxValue, FunctionKind, FunctionModifiers};
use crate::vm::{BxFiber, CallFrame, chunk::Chunk};
use std::{cell::RefCell, rc::Rc};

fn fixture() -> VM {
    let mut vm = VM::new();
    let future_id = vm.future_new().as_gc_id().unwrap();
    let chunk = Rc::new(RefCell::new(Chunk::default()));
    let function = Rc::new(BxCompiledFunction {
        name: "unbound-state-test".into(),
        kind: FunctionKind::Udf,
        arity: 0,
        min_arity: 0,
        params: Vec::new(),
        modifiers: FunctionModifiers::default(),
        captured_receiver: None,
        captured_scopes: Vec::new(),
        bif: None,
        chunk: Chunk::default(),
    });
    vm.fibers.push(BxFiber {
        // Nonzero base distinguishes frame-relative from absolute slot addressing.
        stack: vec![BxValue::new_int(91), BxValue::new_number(2.0)],
        frames: vec![CallFrame {
            function, chunk, ip: 0, stack_base: 1, receiver: None,
            handlers: Vec::new(), pending_completion: None,
            promoted_constants: Vec::new(), local_scope: None, context: None,
            local_names: SlotBindings::new(), scope_mode: ScopeMode::Binding,
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
    vm
}

fn mode(vm: &VM) -> ScopeMode {
    vm.fibers[0].frames.last().unwrap().scope_mode
}

#[test]
fn unbound_checked_slots_survive_reallocation_and_gc() {
    let mut vm = fixture();
    vm.prepare_unbound_frame(0);
    assert!(mode(&vm) == ScopeMode::Unbound);
    assert_eq!(vm.local_value(0, 0).as_number(), 2.0);
    let value = BxValue::new_ptr(vm.string_new("retained".into()));
    vm.set_local_value(0, 0, value);
    vm.fibers[0].stack.reserve(4096);
    vm.collect_garbage_now();
    assert_eq!(vm.local_value(0, 0), value);
    assert_eq!(vm.to_string(value), "retained");
    assert_eq!(vm.fibers[0].stack[0].as_int(), 91);
    vm.set_local_value(0, 0, BxValue::new_null());
    assert!(vm.local_value(0, 0).is_null());
}

#[test]
fn unbound_late_binding_restores_live_dictionary_path() {
    let mut vm = fixture();
    vm.prepare_unbound_frame(0);
    vm.bind_local(0, 0, "value");
    assert!(mode(&vm) == ScopeMode::Binding);
    vm.prepare_unbound_frame(0);
    assert!(mode(&vm) == ScopeMode::Binding);
    let scope = vm.local_scope(0).as_gc_id().unwrap();
    vm.struct_set(scope, "value", BxValue::new_number(7.0));
    assert_eq!(vm.local_value(0, 0).as_number(), 7.0);
    vm.set_local_value(0, 0, BxValue::new_number(11.0));
    assert_eq!(vm.struct_get(scope, "value").as_number(), 11.0);
    assert_eq!(vm.fibers[0].stack[1].as_number(), 11.0);
}

#[test]
fn unbound_exposure_and_capture_cannot_be_reclassified() {
    let mut vm = fixture();
    vm.prepare_unbound_frame(0);
    assert!(vm.capture_scopes(0).is_empty());
    assert!(mode(&vm) == ScopeMode::Shared);
    vm.prepare_unbound_frame(0);
    assert!(mode(&vm) == ScopeMode::Shared);
    vm.set_local_value(0, 0, BxValue::new_number(9.0));
    assert_eq!(vm.local_value(0, 0).as_number(), 9.0);
}

#[test]
fn unbound_truncation_reload_and_wide_binding_keep_invariant() {
    let mut vm = fixture();
    vm.prepare_unbound_frame(0);
    vm.truncate_scope_stack(0, 1);
    vm.fibers[0].stack.push(BxValue::new_number(4.0));
    vm.prepare_unbound_frame(0);
    assert!(mode(&vm) == ScopeMode::Unbound);
    assert_eq!(vm.local_value(0, 0).as_number(), 4.0);
    vm.fibers[0].stack.resize(35, BxValue::new_null());
    vm.bind_local(0, 33, "wide");
    assert!(mode(&vm) == ScopeMode::Shared);
    vm.truncate_scope_stack(0, 1);
    vm.prepare_unbound_frame(0);
    assert!(mode(&vm) == ScopeMode::Shared);
}

#[test]
fn unbound_admission_excludes_parameters_implicit_arguments_and_components() {
    for case in 0..3 {
        let mut vm = fixture();
        let function = Rc::make_mut(&mut vm.fibers[0].frames[0].function);
        match case {
            0 => function.params.push("value".into()),
            1 => function.arity = 1, // A zero-declared-argument UDF still has arguments.
            _ => function.kind = FunctionKind::ComponentBody,
        }
        vm.prepare_unbound_frame(0);
        assert!(mode(&vm) == ScopeMode::Binding);
    }
    let mut vm = fixture();
    vm.fibers[0].frames[0].scope_mode = ScopeMode::Stack;
    vm.prepare_unbound_frame(0);
    assert!(mode(&vm) == ScopeMode::Stack);
}

#[test]
#[should_panic]
fn unbound_read_keeps_bounds_check() {
    let mut vm = fixture();
    vm.prepare_unbound_frame(0);
    vm.local_value(0, 1);
}

#[test]
#[should_panic]
fn unbound_write_keeps_bounds_check() {
    let mut vm = fixture();
    vm.prepare_unbound_frame(0);
    vm.set_local_value(0, 1, BxValue::new_null());
}

fn borrowed_read(vm: &mut VM, slot: usize) -> BxValue {
    vm.fibers[0].local_value(slot, &vm.heap, &vm.shapes, &vm.interner)
}

fn borrowed_write(vm: &mut VM, slot: usize, value: BxValue) {
    vm.fibers[0].set_local_value(slot, value, &mut vm.heap, &mut vm.shapes, &mut vm.interner);
}

#[test]
fn active_local_view_keeps_roots_through_stack_frame_and_fiber_relocation() {
    let mut vm = fixture();
    vm.prepare_unbound_frame(0);
    let value = BxValue::new_ptr(vm.string_new("active-root".into()));
    {
        let fiber = &mut vm.fibers[0];
        fiber.set_local_value(0, value, &mut vm.heap, &mut vm.shapes, &mut vm.interner);
        fiber.frames.last_mut().unwrap().promoted_constants.push(Some(value));
        fiber.stack.reserve(4096);
        fiber.frames.reserve(64);
        assert_eq!(fiber.local_value(0, &vm.heap, &vm.shapes, &vm.interner), value);
    }
    // The view ends before whole-VM work; roots never leave the owning fiber.
    vm.fibers.reserve(64);
    vm.collect_garbage_now();
    assert_eq!(borrowed_read(&mut vm, 0), value);
    assert_eq!(vm.to_string(value), "active-root");
    assert_eq!(vm.fibers[0].frames.last().unwrap().promoted_constants[0], Some(value));
    assert_eq!(vm.fibers[0].stack[0].as_int(), 91);
}

#[test]
fn active_local_view_preserves_shared_alias_delete_fallback_and_null() {
    let mut vm = fixture();
    vm.bind_local(0, 0, "MiXeD");
    vm.expose_frame_scopes(0);
    let scope = vm.local_scope(0).as_gc_id().unwrap();
    vm.struct_set(scope, "mixed", BxValue::new_number(7.0));
    assert_eq!(borrowed_read(&mut vm, 0).as_number(), 7.0);
    vm.struct_set(scope, "other", BxValue::new_int(19)); // Invalidate cached shape.
    assert_eq!(borrowed_read(&mut vm, 0).as_number(), 7.0);
    assert!(vm.struct_delete(scope, "MIXED"));
    assert_eq!(borrowed_read(&mut vm, 0).as_number(), 2.0); // Original stack fallback.
    borrowed_write(&mut vm, 0, BxValue::new_null());
    assert!(vm.struct_key_exists(scope, "mixed"));
    assert!(vm.struct_get(scope, "mixed").is_null());
    assert!(borrowed_read(&mut vm, 0).is_null());
    assert!(vm.fibers[0].stack[1].is_null());
}

#[test]
fn active_argument_view_keeps_read_only_lookup_write_spelling_and_case_rules() {
    for case_sensitive in [false, true] {
        let mut vm = fixture();
        let arguments = vm.struct_new();
        let crate::vm::gc::GcObject::Struct(record) = vm.heap.get_mut(arguments) else {
            panic!("expected struct");
        };
        record.case_sensitive = case_sensitive;
        vm.struct_set(arguments, "mixed", BxValue::new_number(7.0));
        let function = Rc::make_mut(&mut vm.fibers[0].frames[0].function);
        function.params = vec!["MiXeD".into()];
        function.arity = 2;
        vm.fibers[0].stack.push(BxValue::new_ptr(arguments));
        let expected = if case_sensitive { 2.0 } else { 7.0 };
        assert_eq!(borrowed_read(&mut vm, 0).as_number(), expected);
        assert_eq!(vm.struct_key_array(arguments), vec!["mixed"]);
        assert!(vm.interner.get_exact_id("MiXeD").is_none());
        borrowed_write(&mut vm, 0, BxValue::new_number(11.0));
        assert_eq!(borrowed_read(&mut vm, 0).as_number(), 11.0);
        assert_eq!(vm.struct_get(arguments, "MiXeD").as_number(), 11.0);
        assert_eq!(vm.struct_key_array(arguments).len(), if case_sensitive { 2 } else { 1 });
        assert!(vm.struct_key_array(arguments).contains(&"MiXeD".to_string()));
        if case_sensitive {
            assert_eq!(vm.struct_get(arguments, "mixed").as_number(), 7.0);
        }
    }
}
