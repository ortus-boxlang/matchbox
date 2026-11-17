//! Live-read contracts: cached metadata is not a cached value or scope identity.
use super::{ScopeMode, SlotBindings, VM};
use crate::types::{BxCompiledFunction, BxVM, BxValue, Constant, FunctionKind, FunctionModifiers};
use crate::vm::{BxFiber, CallFrame, chunk::Chunk, gc::GcObject, opcode::op};
use std::{cell::RefCell, rc::Rc};

fn fixture() -> VM {
    let mut vm = VM::new();
    let future_id = vm.future_new().as_gc_id().unwrap();
    let function = Rc::new(BxCompiledFunction {
        name: "binding-read-contract".into(), kind: FunctionKind::Udf,
        arity: 0, min_arity: 0, params: Vec::new(),
        modifiers: FunctionModifiers::default(), captured_receiver: None,
        captured_scopes: Vec::new(), bif: None, chunk: Chunk::default(),
    });
    vm.fibers.push(BxFiber {
        stack: vec![BxValue::new_int(91), BxValue::new_number(2.0)],
        frames: vec![CallFrame {
            function, chunk: Rc::new(RefCell::new(Chunk::default())), ip: 0,
            stack_base: 1, receiver: None, handlers: Vec::new(),
            pending_completion: None, promoted_constants: Vec::new(),
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
    vm
}

fn arguments_fixture(name: &str, key: &str, case_sensitive: bool) -> VM {
    let mut vm = fixture();
    let arguments = vm.struct_new();
    let GcObject::Struct(record) = vm.heap.get_mut(arguments) else { unreachable!() };
    record.case_sensitive = case_sensitive;
    vm.struct_set(arguments, key, BxValue::new_number(7.0));
    let function = Rc::make_mut(&mut vm.fibers[0].frames[0].function);
    function.params = vec![name.into()];
    function.arity = 2;
    vm.fibers[0].stack.push(BxValue::new_ptr(arguments));
    vm
}

fn hit(vm: &VM, slot: usize) -> Option<BxValue> {
    let fiber = &vm.fibers[0];
    fiber.frames.last()?.cached_binding_value(slot, &fiber.stack, &vm.heap)
}

fn local(vm: &mut VM) -> usize {
    vm.bind_local(0, 0, "kept");
    assert!(hit(vm, 0).is_none());
    assert_eq!(vm.local_value(0, 0).as_number(), 2.0);
    assert_eq!(hit(vm, 0).unwrap().as_number(), 2.0);
    vm.local_scope(0).as_gc_id().unwrap()
}

#[test]
fn warm_local_reads_live_values_and_null_without_changing_fallback_or_metadata() {
    for mode in [ScopeMode::Binding, ScopeMode::Shared] {
        let mut vm = fixture();
        let scope = local(&mut vm);
        vm.fibers[0].frames[0].scope_mode = mode;
        let before = *vm.fibers[0].frames[0].local_names.get(&0).unwrap();
        for value in [BxValue::new_int(19), BxValue::new_number(-0.0), BxValue::new_bool(false), BxValue::new_null()] {
            vm.struct_set(scope, "kept", value);
            assert_eq!(hit(&vm, 0), Some(value));
            assert_eq!(vm.local_value(0, 0), value);
            assert_eq!(vm.fibers[0].stack[1].as_number(), 2.0);
            let after = vm.fibers[0].frames[0].local_names.get(&0).unwrap();
            assert_eq!((before.key, before.local, before.shape, before.index, before.write_ready),
                (after.key, after.local, after.shape, after.index, after.write_ready));
            assert!(vm.fibers[0].frames[0].scope_mode == mode);
        }
    }
}

#[test]
fn shape_change_delete_negative_index_and_reinsert_use_original_resolver() {
    let mut vm = fixture();
    let scope = local(&mut vm);
    vm.struct_set(scope, "extra", BxValue::new_int(1));
    assert!(hit(&vm, 0).is_none());
    assert_eq!(vm.local_value(0, 0).as_number(), 2.0);
    assert!(hit(&vm, 0).is_some());
    assert!(vm.struct_delete(scope, "kept"));
    assert!(hit(&vm, 0).is_none());
    assert_eq!(vm.local_value(0, 0).as_number(), 2.0);
    assert!(vm.fibers[0].frames[0].local_names.get(&0).unwrap().index.is_none());
    assert!(hit(&vm, 0).is_none());
    vm.struct_set(scope, "kept", BxValue::new_number(11.0));
    assert!(hit(&vm, 0).is_none());
    assert_eq!(vm.local_value(0, 0).as_number(), 11.0);
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 11.0);
}

#[test]
fn missing_binding_does_not_create_a_scope_or_upgrade_to_shared() {
    let mut vm = fixture();
    assert!(hit(&vm, 0).is_none());
    assert_eq!(vm.local_value(0, 0).as_number(), 2.0);
    let frame = &vm.fibers[0].frames[0];
    assert!(frame.local_scope.is_none());
    assert!(frame.local_names.get(&0).is_none());
    assert!(frame.scope_mode == ScopeMode::Binding);
}

#[test]
fn argument_hits_keep_read_spelling_case_and_write_readiness_rules() {
    for sensitive in [false, true] {
        let mut vm = arguments_fixture("MiXeD", "mixed", sensitive);
        let scope = vm.arguments_scope(0).unwrap().as_gc_id().unwrap();
        let expected = if sensitive { 2.0 } else { 7.0 };
        assert_eq!(vm.local_value(0, 0).as_number(), expected);
        assert_eq!(hit(&vm, 0).map(|v| v.as_number()), if sensitive { None } else { Some(7.0) });
        assert_eq!(vm.struct_key_array(scope), vec!["mixed"]);
        assert!(vm.interner.get_exact_id("MiXeD").is_none());
        vm.set_local_value(0, 0, BxValue::new_number(11.0));
        assert_eq!(vm.local_value(0, 0).as_number(), 11.0);
        assert_eq!(hit(&vm, 0).unwrap().as_number(), 11.0);
        assert!(vm.struct_key_array(scope).contains(&"MiXeD".to_string()));
        assert_eq!(vm.struct_key_array(scope).len(), if sensitive { 2 } else { 1 });
        if sensitive { assert_eq!(vm.struct_get(scope, "mixed").as_number(), 7.0); }
    }
}

#[test]
fn argument_hit_selects_current_scope_object_not_cached_scope_identity() {
    let mut vm = arguments_fixture("value", "value", false);
    assert_eq!(vm.local_value(0, 0).as_number(), 7.0);
    let other = vm.struct_new();
    vm.struct_set(other, "value", BxValue::new_number(19.0));
    vm.fibers[0].stack[2] = BxValue::new_ptr(other);
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 19.0);
    assert_eq!(vm.local_value(0, 0).as_number(), 19.0);
    vm.struct_set(other, "extra", BxValue::new_int(3));
    assert!(hit(&vm, 0).is_none());
    assert_eq!(vm.local_value(0, 0).as_number(), 19.0);
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 19.0);
}

#[test]
fn local_hit_selects_current_scope_object_and_current_value_type() {
    let mut vm = fixture();
    local(&mut vm);
    let other = vm.struct_new();
    vm.struct_set(other, "kept", BxValue::new_number(23.0));
    vm.fibers[0].frames[0].local_scope = Some(BxValue::new_ptr(other));
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 23.0);
    let text = BxValue::new_ptr(vm.string_new("fresh".into()));
    vm.struct_set(other, "kept", text);
    assert_eq!(hit(&vm, 0), Some(text));
    let value = vm.local_value(0, 0);
    assert_eq!(vm.to_string(value), "fresh");
}

#[test]
fn old_scope_young_values_and_relocated_fiber_frame_stack_remain_rooted() {
    let mut vm = fixture();
    let scope = local(&mut vm);
    vm.collect_garbage_now();
    let first = BxValue::new_ptr(vm.string_new("dictionary-only-young".into()));
    vm.struct_set(scope, "kept", first);
    vm.fibers[0].stack.reserve(4096);
    vm.fibers[0].frames.reserve(64);
    vm.fibers.reserve(64);
    vm.collect_garbage_now();
    assert_eq!(hit(&vm, 0), Some(first));
    assert_eq!(vm.to_string(first), "dictionary-only-young");
    let second = BxValue::new_ptr(vm.string_new("setter-barrier".into()));
    vm.set_local_value(0, 0, second);
    vm.collect_garbage_now();
    assert_eq!(hit(&vm, 0), Some(second));
    assert_eq!(vm.local_value(0, 0), second);
    assert_eq!(vm.to_string(second), "setter-barrier");
}

#[test]
fn actual_struct_type_and_property_bounds_are_checked_even_with_matching_cache() {
    for short in [false, true] {
        let mut vm = fixture();
        let scope = local(&mut vm);
        if short {
            let GcObject::Struct(record) = vm.heap.get_mut(scope) else { unreachable!() };
            record.properties.clear();
        } else {
            *vm.heap.get_mut(scope) = GcObject::Array(Vec::new());
        }
        assert!(hit(&vm, 0).is_none());
        assert_eq!(vm.local_value(0, 0).as_number(), 2.0);
    }
}

#[test]
fn checked_invalid_collected_and_scalar_scope_peeks_keep_original_handling() {
    for case in 0..3 {
        let mut vm = fixture();
        local(&mut vm);
        let scope = match case {
            0 => BxValue::new_ptr(1 << 30),
            1 => {
                let dead = vm.struct_new();
                vm.collect_garbage_now();
                assert!(vm.heap.get_opt(dead).is_none());
                BxValue::new_ptr(dead)
            }
            _ => BxValue::new_int(9),
        };
        vm.fibers[0].frames[0].local_scope = Some(scope);
        assert!(hit(&vm, 0).is_none());
        if case < 2 {
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.local_value(0, 0))).is_err());
            // A deliberately malformed-host VM is discarded, never resumed.
        } else { assert_eq!(vm.local_value(0, 0).as_number(), 2.0); }
    }
}

#[test]
fn dense_sparse_and_maximum_metadata_slots_keep_live_reads() {
    for slot in [0, 31, 32, 65_535] {
        let mut vm = fixture();
        vm.fibers[0].stack.resize(slot + 2, BxValue::new_number(2.0));
        vm.bind_local(0, slot, "wide");
        assert_eq!(vm.local_value(0, slot).as_number(), 2.0);
        let scope = vm.local_scope(0).as_gc_id().unwrap();
        vm.struct_set(scope, "wide", BxValue::new_number(29.0));
        assert_eq!(hit(&vm, slot).unwrap().as_number(), 29.0);
        assert_eq!(vm.local_value(0, slot).as_number(), 29.0);
    }
    let mut vm = fixture();
    local(&mut vm);
    let binding = *vm.fibers[0].frames[0].local_names.get(&0).unwrap();
    vm.fibers[0].frames[0].local_names.insert(usize::MAX, binding);
    // Existing successful dictionary reads do not require a fallback stack slot.
    assert_eq!(hit(&vm, usize::MAX).unwrap().as_number(), 2.0);
    assert_eq!(vm.local_value(0, usize::MAX).as_number(), 2.0);
}

#[test]
fn aliases_from_block_slots_keep_live_authority_after_binding_is_removed() {
    let mut vm = fixture();
    let scope = local(&mut vm);
    vm.fibers[0].stack.push(BxValue::new_number(5.0));
    vm.bind_local(0, 1, "kept");
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 5.0);
    assert_eq!(vm.local_value(0, 1).as_number(), 5.0);
    vm.truncate_scope_stack(0, 2);
    assert!(vm.fibers[0].frames[0].local_names.get(&1).is_none());
    assert_eq!(vm.local_value(0, 0).as_number(), 5.0);
    vm.struct_set(scope, "kept", BxValue::new_null());
    assert_eq!(hit(&vm, 0), Some(BxValue::new_null()));
}

#[test]
fn component_implicit_arguments_precede_cached_binding_and_use_captured_scope() {
    let mut vm = arguments_fixture("value", "value", false);
    let captured_local = vm.struct_new();
    let captured_args = vm.struct_new();
    vm.struct_set(captured_args, "value", BxValue::new_number(31.0));
    let function = Rc::make_mut(&mut vm.fibers[0].frames[0].function);
    function.kind = FunctionKind::ComponentBody;
    function.captured_scopes = vec![BxValue::new_ptr(captured_local), BxValue::new_ptr(captured_args)];
    assert_eq!(vm.local_value(0, 0).as_number(), 31.0);
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 31.0);
    let binding = *vm.fibers[0].frames[0].local_names.get(&0).unwrap();
    vm.fibers[0].frames[0].local_names.insert(1, binding);
    assert_eq!(vm.local_value(0, 1), BxValue::new_ptr(captured_args));
}

#[test]
fn mode_authority_and_real_implicit_argument_exposure_precede_cached_value() {
    let mut vm = arguments_fixture("value", "value", false);
    vm.fibers[0].stack.push(BxValue::new_number(2.0));
    vm.bind_local(0, 2, "kept");
    let scope = vm.local_scope(0).as_gc_id().unwrap();
    assert_eq!(vm.local_value(0, 2).as_number(), 2.0);
    vm.struct_set(scope, "kept", BxValue::new_number(37.0));
    for mode in [ScopeMode::Stack, ScopeMode::Unbound] {
        // Exercise mode priority directly, not a new admission capability.
        vm.fibers[0].frames[0].scope_mode = mode;
        assert_eq!(hit(&vm, 2).unwrap().as_number(), 37.0);
        assert_eq!(vm.local_value(0, 2).as_number(), 2.0);
    }
    vm.fibers[0].frames[0].scope_mode = ScopeMode::Stack;
    assert_eq!(vm.local_value(0, 1), vm.arguments_scope(0).unwrap());
    assert!(vm.fibers[0].frames[0].scope_mode == ScopeMode::Shared);
    assert_eq!(vm.local_value(0, 2).as_number(), 37.0);
    vm.prepare_unbound_frame(0);
    vm.enter_frame_scopes(0);
    assert!(vm.fibers[0].frames[0].scope_mode == ScopeMode::Shared);
}

#[test]
fn captured_scope_aliases_stay_live_and_exposure_remains_permanent() {
    let mut vm = arguments_fixture("value", "value", false);
    vm.fibers[0].stack.push(BxValue::new_number(2.0));
    vm.bind_local(0, 2, "kept");
    assert_eq!(vm.local_value(0, 2).as_number(), 2.0);
    let captured = vm.capture_scopes(0);
    assert!(vm.fibers[0].frames[0].scope_mode == ScopeMode::Shared);
    vm.struct_set(captured[0].as_gc_id().unwrap(), "kept", BxValue::new_number(41.0));
    assert_eq!(hit(&vm, 2).unwrap().as_number(), 41.0);
    assert_eq!(vm.local_value(0, 2).as_number(), 41.0);
    vm.enter_frame_scopes(0);
    assert!(vm.fibers[0].frames[0].scope_mode == ScopeMode::Shared);
}

#[test]
fn nested_frame_return_and_relocation_select_current_frame_metadata() {
    let mut vm = fixture();
    let outer = local(&mut vm);
    let mut child = CallFrame {
        function: Rc::clone(&vm.fibers[0].frames[0].function),
        chunk: Rc::new(RefCell::new(Chunk::default())), ip: 0,
        stack_base: vm.fibers[0].stack.len(), receiver: None, handlers: Vec::new(),
        pending_completion: None, promoted_constants: Vec::new(), local_scope: None,
        context: None, local_names: SlotBindings::new(), scope_mode: ScopeMode::Binding,
        #[cfg(feature = "jit")]
        jit_loop_attempted: false,
        #[cfg(feature = "jit")]
        jit_construct_unsupported: None,
    };
    let base = child.stack_base;
    child.ip = 7;
    vm.fibers[0].stack.push(BxValue::new_number(43.0));
    vm.fibers[0].frames.push(child);
    vm.bind_local(0, 0, "kept");
    assert_eq!(vm.local_value(0, 0).as_number(), 43.0);
    vm.struct_set(outer, "kept", BxValue::new_number(47.0));
    vm.fibers[0].frames.reserve(64);
    vm.fibers[0].frames.pop();
    vm.fibers[0].stack.truncate(base);
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 47.0);
    assert_eq!(vm.local_value(0, 0).as_number(), 47.0);
}

#[test]
fn dispatch_composition_consumes_current_dictionary_number_and_changed_type() {
    let mut vm = fixture();
    let scope = local(&mut vm);
    let mut chunk = Chunk::default();
    let index = chunk.add_constant(Constant::Number(2.0));
    chunk.emit1(op::GET_LOCAL, 0, 1);
    chunk.emit1(op::CONSTANT, index, 1);
    chunk.emit0(op::ADD, 1);
    vm.fibers[0].frames[0].chunk = Rc::new(RefCell::new(chunk));
    vm.fibers[0].frames[0].promoted_constants = vec![Some(BxValue::new_number(2.0))];
    vm.struct_set(scope, "kept", BxValue::new_number(7.0));
    vm.run_fiber_inner(0, None).unwrap();
    assert_eq!(vm.fibers[0].stack.last().unwrap().as_number(), 9.0);
    let text = BxValue::new_ptr(vm.string_new("word".into()));
    vm.struct_set(scope, "kept", text);
    vm.fibers[0].frames[0].ip = 0;
    vm.fibers[0].stack.truncate(2);
    vm.run_fiber_inner(0, None).unwrap();
    assert_eq!(vm.to_string(*vm.fibers[0].stack.last().unwrap()), "word2");
}

#[test]
fn frame_relative_arguments_wrapper_keeps_no_frame_absence() {
    let mut vm = fixture();
    vm.fibers[0].frames.clear();
    assert_eq!(vm.fibers[0].arguments_scope(), None);
    assert_eq!(vm.arguments_scope(0), None);
    assert_eq!(hit(&vm, 0), None);
}

#[test]
fn frame_relative_arguments_checks_arity_before_stack_and_reads_current_slot() {
    let mut vm = fixture();
    let frame = &mut vm.fibers[0].frames[0];
    let function = Rc::make_mut(&mut frame.function);
    function.params = vec!["value".into()];
    function.arity = 1;
    frame.stack_base = usize::MAX;
    vm.fibers[0].stack.clear();
    assert_eq!(vm.arguments_scope(0), None);
    vm.fibers[0].frames[0].stack_base = 1;
    Rc::make_mut(&mut vm.fibers[0].frames[0].function).arity = 2;
    vm.fibers[0].stack = vec![BxValue::new_int(91), BxValue::new_int(2), BxValue::new_null()];
    assert_eq!(vm.arguments_scope(0), Some(BxValue::new_null()));
    vm.fibers[0].stack[2] = BxValue::new_int(53);
    assert_eq!(vm.arguments_scope(0), Some(BxValue::new_int(53)));
}

#[test]
fn frame_relative_component_arguments_never_fall_through_to_ordinary_stack() {
    let mut vm = arguments_fixture("value", "value", false);
    let local = BxValue::new_ptr(vm.struct_new());
    let captured = BxValue::new_ptr(vm.struct_new());
    let function = Rc::make_mut(&mut vm.fibers[0].frames[0].function);
    function.kind = FunctionKind::ComponentBody;
    function.captured_scopes = vec![local, captured];
    // Ordinary arity and a populated stack would select a different object.
    assert_ne!(captured, vm.fibers[0].stack[2]);
    assert_eq!(vm.arguments_scope(0), Some(captured));
    Rc::make_mut(&mut vm.fibers[0].frames[0].function).captured_scopes.truncate(1);
    assert_eq!(vm.arguments_scope(0), None);
    Rc::make_mut(&mut vm.fibers[0].frames[0].function).captured_scopes.push(BxValue::new_null());
    vm.fibers[0].frames[0].stack_base = usize::MAX;
    vm.fibers[0].stack.clear();
    assert_eq!(vm.arguments_scope(0), Some(BxValue::new_null()));
}

#[test]
fn frame_relative_malformed_ordinary_stack_keeps_panic_and_vm_is_discarded() {
    for overflow in [false, true] {
        let mut vm = arguments_fixture("value", "value", false);
        if overflow {
            vm.fibers[0].frames[0].stack_base = usize::MAX;
        } else {
            vm.fibers[0].stack.clear();
        }
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.arguments_scope(0))).is_err());
        // No promise to resume an intentionally malformed host VM after panic.
    }
}

#[test]
fn frame_relative_argument_reads_follow_current_nested_frame_after_relocation_return() {
    let mut vm = arguments_fixture("value", "value", false);
    let outer_args = vm.arguments_scope(0).unwrap();
    assert_eq!(vm.local_value(0, 0).as_number(), 7.0);
    let child_args = vm.struct_new();
    vm.struct_set(child_args, "value", BxValue::new_number(59.0));
    let base = vm.fibers[0].stack.len();
    let child = CallFrame {
        function: Rc::clone(&vm.fibers[0].frames[0].function),
        chunk: Rc::new(RefCell::new(Chunk::default())), ip: 0, stack_base: base,
        receiver: None, handlers: Vec::new(), pending_completion: None,
        promoted_constants: Vec::new(), local_scope: None, context: None,
        local_names: SlotBindings::new(), scope_mode: ScopeMode::Binding,
        #[cfg(feature = "jit")]
        jit_loop_attempted: false,
        #[cfg(feature = "jit")]
        jit_construct_unsupported: None,
    };
    vm.fibers[0].stack.extend([BxValue::new_number(2.0), BxValue::new_ptr(child_args)]);
    vm.fibers[0].frames.push(child);
    assert_eq!(vm.local_value(0, 0).as_number(), 59.0);
    vm.struct_set(outer_args.as_gc_id().unwrap(), "value", BxValue::new_number(61.0));
    vm.fibers[0].frames.reserve(64);
    vm.fibers[0].stack.reserve(4096);
    vm.fibers.reserve(64);
    vm.collect_garbage_now();
    assert_eq!(vm.arguments_scope(0), Some(BxValue::new_ptr(child_args)));
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 59.0);
    vm.fibers[0].frames.pop();
    vm.fibers[0].stack.truncate(base);
    assert_eq!(vm.arguments_scope(0), Some(outer_args));
    assert_eq!(hit(&vm, 0).unwrap().as_number(), 61.0);
    assert_eq!(vm.local_value(0, 0).as_number(), 61.0);
}

#[test]
fn frame_relative_hit_uses_post_exposure_mode_and_live_dictionary() {
    let mut vm = arguments_fixture("value", "value", false);
    vm.bind_local(0, 1, "shadow");
    let args = vm.arguments_scope(0).unwrap();
    assert_eq!(vm.local_value(0, 1), args);
    let scope = vm.local_scope(0).as_gc_id().unwrap();
    vm.struct_set(scope, "shadow", BxValue::new_number(67.0));
    // Direct mode-priority fixture, not a new Stack admission capability.
    vm.fibers[0].frames[0].scope_mode = ScopeMode::Stack;
    assert_eq!(vm.local_value(0, 1).as_number(), 67.0);
    assert!(vm.fibers[0].frames[0].scope_mode == ScopeMode::Shared);
    assert_eq!(hit(&vm, 1).unwrap().as_number(), 67.0);
    assert_eq!(vm.fibers[0].stack[2], args);
}
