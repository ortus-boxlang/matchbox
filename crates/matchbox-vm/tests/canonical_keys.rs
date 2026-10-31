use matchbox_vm::{
    types::{
        BxCompiledFunction, BxVM, BxValue, Constant, FunctionKind, FutureStatus, NativeFutureValue,
        box_string::BoxString,
    },
    vm::{
        VM,
        chunk::{Chunk, ChunkProgram},
        gc::GcObject,
        opcode::op,
    },
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

// Per-thread allocation accounting: measure public VM calls, not test-runner threads.
thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
    static ALLOCATION_BYTES: Cell<Option<usize>> = const { Cell::new(None) };
}
struct CountedSystem;
#[global_allocator]
static ALLOCATOR: CountedSystem = CountedSystem;
unsafe impl GlobalAlloc for CountedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value + 1));
            }
        });
        let _ = ALLOCATION_BYTES.try_with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value + layout.size()));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[test]
fn repeated_local_bindings_and_member_writes_do_not_allocate_operand_strings() {
    for bind in [true, false] {
        let mut allocations = Vec::new();
        for repeats in [1, 100] {
            let mut vm = VM::new();
            let record = vm.struct_new();
            vm.struct_set(record, "value", BxValue::new_int(0));
            vm.insert_global("target".into(), BxValue::new_ptr(record));
            let mut body = Chunk::new("key-operands.bxs");
            let name = body.add_constant(Constant::String(BoxString::new("value")));
            let target = body.add_constant(Constant::String(BoxString::new("target")));
            let local = body.add_constant(Constant::String(BoxString::new("local")));
            let answer = body.add_constant(Constant::Number(42.0));
            body.emit0(op::FUNCTION_ENTER, 1);
            if bind { body.emit1(op::CONSTANT, answer, 1); }
            for _ in 0..repeats {
                if bind {
                    body.emit2(op::BIND_LOCAL, 1, name, 1);
                } else {
                    body.emit1(op::GET_GLOBAL, target, 1);
                    body.emit1(op::CONSTANT, answer, 1);
                    body.emit1(op::SET_MEMBER, name, 1);
                    body.emit0(op::POP, 1);
                }
            }
            body.emit1(if bind { op::GET_PRIVATE } else { op::GET_GLOBAL }, if bind { local } else { target }, 1);
            body.emit0(op::RETURN, 1);
            let function = BxCompiledFunction {
                name: "keyOperands".into(), kind: FunctionKind::Udf,
                arity: 1, min_arity: 0, params: Vec::new(), bif: None,
                modifiers: Default::default(), captured_receiver: None,
                captured_scopes: Vec::new(), chunk: body,
            };
            let mut script = Chunk::new("key-operands-owner.bxs");
            let function = script.add_constant(Constant::CompiledFunction(function));
            script.emit1(op::CONSTANT, function, 1); script.emit0(op::RETURN, 1);
            let function = vm.interpret(script).unwrap();
            let _root = vm.retain_value(function).unwrap();
            vm.call_function_value(function, Vec::new(), None).unwrap();
            ALLOCATIONS.with(|count| count.set(Some(0)));
            let result = vm.call_function_value(function, Vec::new(), None).unwrap();
            allocations.push(ALLOCATIONS.with(|count| count.replace(None).unwrap()));
            assert_eq!(vm.struct_get(result.as_gc_id().unwrap(), "value").as_number(), 42.0);
        }
        eprintln!("Operand allocations bind={bind}, 1/100 operations: {allocations:?}");
        assert!(allocations[1] <= allocations[0] + 8, "Operand names allocate per instruction: {allocations:?}");
    }
}

#[test]
fn local_operand_folding_preserves_presentation_through_gc() {
    for spelling in ["MiXeD", "İ", "ǅ", "Élève中😀_identifier_long_tail"] {
        let mut vm = VM::new();
        let folded = spelling.to_lowercase();
        let presentation = vm.struct_new();
        vm.struct_set(presentation, &folded, BxValue::new_int(7));
        let _presentation_root = vm.retain_value(BxValue::new_ptr(presentation)).unwrap();
        let mut body = Chunk::new("local-operand-spelling.bxs");
        let name = body.add_constant(Constant::String(BoxString::new(spelling).concat(&BoxString::new(""))));
        let local = body.add_constant(Constant::String(BoxString::new("local")));
        let answer = body.add_constant(Constant::Number(42.0));
        body.emit0(op::FUNCTION_ENTER, 1);
        body.emit1(op::CONSTANT, answer, 1);
        body.emit2(op::BIND_LOCAL, 1, name, 1);
        body.emit1(op::GET_PRIVATE, local, 1); body.emit0(op::RETURN, 1);
        let function = BxCompiledFunction {
            name: "localSpelling".into(), kind: FunctionKind::Udf,
            arity: 1, min_arity: 0, params: Vec::new(), bif: None,
            modifiers: Default::default(), captured_receiver: None,
            captured_scopes: Vec::new(), chunk: body,
        };
        let mut script = Chunk::new("local-spelling-owner.bxs");
        let function = script.add_constant(Constant::CompiledFunction(function));
        script.emit1(op::CONSTANT, function, 1); script.emit0(op::RETURN, 1);
        let function = vm.interpret(script).unwrap();
        let _root = vm.retain_value(function).unwrap();
        for _ in 0..10 {
            vm.collect_garbage_now();
            let scope = vm.call_function_value(function, Vec::new(), None).unwrap().as_gc_id().unwrap();
            assert_eq!(vm.struct_key_array(scope), [folded.clone()]);
            assert_eq!(vm.struct_key_array(presentation), [folded.clone()]);
            assert_eq!(vm.struct_get(scope, spelling).as_number(), 42.0);
        }
    }
}

#[test]
fn string_operands_preserve_exact_member_keys_in_independent_vms() {
    for spelling in ["VaLuE", "İ", "ǅ", "Élève中😀_identifier_long_tail"] {
        let mut script = Chunk::new("exact-member-operands.bxs");
        let target = script.add_constant(Constant::String(BoxString::new("target")));
        let key = script.add_constant(Constant::String(BoxString::new(spelling).concat(&BoxString::new(""))));
        let answer = script.add_constant(Constant::Number(42.0));
        script.emit1(op::GET_GLOBAL, target, 1); script.emit1(op::CONSTANT, answer, 1);
        script.emit1(op::SET_MEMBER, key, 1); script.emit0(op::POP, 1);
        script.emit1(op::GET_GLOBAL, target, 1); script.emit1(op::MEMBER, key, 1);
        script.emit0(op::RETURN, 1);
        for padding in [0, 17] {
            let mut vm = VM::new();
            for i in 0..padding { vm.insert_global(format!("padding{i}"), BxValue::new_int(i)); }
            let record = vm.struct_new_with_options(&["casesensitive".into(), "ordered".into()]);
            vm.struct_set(record, &spelling.to_lowercase(), BxValue::new_int(7));
            vm.insert_global("target".into(), BxValue::new_ptr(record));
            for _ in 0..10 {
                vm.collect_garbage_now();
                assert_eq!(vm.interpret_chunk_borrowed(&script).unwrap().as_number(), 42.0);
                assert_eq!(vm.struct_get(record, &spelling.to_lowercase()).as_number(), 7.0);
                assert_eq!(vm.struct_key_array(record), [spelling.to_lowercase(), spelling.to_string()]);
            }
        }
    }
}

#[test]
fn existing_canonical_struct_reads_do_not_allocate_temporary_keys() {
    let mut vm = VM::new();
    let data = vm.struct_new();
    let value = BxValue::new_int(42);
    for key in ["name", "élève中😀"] {
        vm.struct_set(data, key, value);
    }
    ALLOCATIONS.with(|count| count.set(Some(0)));
    for _ in 0..100 {
        for key in ["name", "élève中😀"] {
            assert_eq!(vm.struct_get(data, key), value);
            assert!(vm.struct_key_exists(data, key));
        }
    }
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    assert_eq!(
        allocations, 0,
        "Existing canonical reads should not allocate temporary keys"
    );
}

#[test]
fn missing_canonical_ascii_lookups_do_not_allocate_temporary_keys() {
    let mut vm = VM::new();
    let data = vm.struct_new();
    let shape = vm.get_root_shape();
    ALLOCATIONS.with(|count| count.set(Some(0)));
    for _ in 0..100 {
        for key in ["missing", "__mb_interceptor_states", "absent_2!?"] {
            assert!(vm.struct_get(data, key).is_null());
            assert!(!vm.struct_key_exists(data, key));
            assert_eq!(vm.get_global(key), None);
            assert_eq!(vm.get_shape_index(shape, key), None);
        }
    }
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    assert_eq!(
        allocations, 0,
        "Missing canonical ASCII lookups allocate: {allocations}"
    );
    // A miss must not insert a folded ID or change later presentation/case behavior.
    for (spelling, folded) in [
        ("MiSsInG", "missing"),
        ("ÉLÈVE中😀", "élève中😀"),
        ("İ", "i\u{307}"),
    ] {
        vm.struct_set(data, spelling, BxValue::new_int(42));
        assert_eq!(vm.struct_get(data, folded), BxValue::new_int(42));
        assert_eq!(vm.struct_get(data, spelling), BxValue::new_int(42));
    }
    assert!(vm.struct_get(data, "NOT_DEFINED").is_null());
    assert!(vm.struct_get(data, "ÉLÈVE_ABSENT").is_null());
}

#[test]
fn existing_canonical_struct_writes_do_not_allocate_temporary_keys() {
    let mut vm = VM::new();
    let data = vm.struct_new();
    for key in ["name", "élève中😀"] {
        vm.struct_set(data, key, BxValue::new_int(0));
    }
    ALLOCATIONS.with(|count| count.set(Some(0)));
    for number in 1..=100 {
        for key in ["name", "élève中😀"] {
            let value = BxValue::new_int(number);
            vm.struct_set(data, key, value);
            assert_eq!(vm.struct_get(data, key), value);
        }
    }
    let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
    assert_eq!(
        allocations, 0,
        "Existing canonical writes should not allocate temporary keys"
    );
}

#[test]
fn completion_delivery_preserves_idle_pending_gc_and_scheduled_work() {
    let mut vm = VM::new();
    let mut script = Chunk::new("completion-delivery.bxs");
    let answer = script.add_constant(Constant::Number(42.0));
    script.emit1(op::CONSTANT, answer, 1);
    script.emit0(op::RETURN, 1);
    assert_eq!(
        vm.interpret_chunk_borrowed(&script).unwrap().as_number(),
        42.0
    );

    let resolved_handle = vm.native_future_new();
    let resolved = resolved_handle.future();
    let rejected_handle = vm.native_future_new();
    let rejected = rejected_handle.future();
    let abandoned_handle = vm.native_future_new();
    let abandoned = abandoned_handle.future();
    let abandoned_root = vm.retain_value(abandoned).unwrap();
    // Empty polls with live senders must keep futures rooted and pending.
    vm.pump_until_blocked().unwrap();
    vm.collect_garbage_now();
    assert!(matches!(
        vm.future_status(resolved),
        Some(FutureStatus::Pending)
    ));
    assert!(matches!(
        vm.future_status(rejected),
        Some(FutureStatus::Pending)
    ));
    std::thread::spawn(move || {
        resolved_handle
            .resolve(NativeFutureValue::String("é中😀".into()))
            .unwrap();
        rejected_handle
            .reject(NativeFutureValue::Error {
                message: "observed failure".into(),
            })
            .unwrap();
        drop(abandoned_handle);
    })
    .join()
    .unwrap();
    // Queued channel payloads are still protected by their pending-future roots.
    vm.collect_garbage_now();
    assert_eq!(
        vm.interpret_chunk_borrowed(&script).unwrap().as_number(),
        42.0
    );
    assert!(matches!(
        vm.future_status(resolved),
        Some(FutureStatus::Completed)
    ));
    assert_eq!(vm.to_string(vm.future_value(resolved).unwrap()), "é中😀");
    let Some(FutureStatus::Failed(error)) = vm.future_status(rejected) else {
        panic!("Expected rejection");
    };
    assert_eq!(
        vm.to_string(vm.struct_get(error.as_gc_id().unwrap(), "message")),
        "observed failure"
    );
    assert!(matches!(
        vm.future_status(vm.read_retained_value(&abandoned_root).unwrap()),
        Some(FutureStatus::Pending)
    ));

    // The VM-local scheduled queue is independent of native sender registration.
    let scheduled = vm.future_new();
    let scheduled_error = vm.future_new();
    let value = BxValue::new_ptr(vm.string_new("scheduled é😀".into()));
    vm.future_schedule_resolve(scheduled, value).unwrap();
    vm.future_schedule_reject(scheduled_error, BxValue::new_int(7))
        .unwrap();
    vm.collect_garbage_now();
    vm.pump_until_blocked().unwrap();
    assert_eq!(
        vm.to_string(vm.future_value(scheduled).unwrap()),
        "scheduled é😀"
    );
    assert!(
        matches!(vm.future_status(scheduled_error), Some(FutureStatus::Failed(value)) if value == BxValue::new_int(7))
    );
    // Once both queues are drained, later registrations must resume delivery.
    assert_eq!(
        vm.interpret_chunk_borrowed(&script).unwrap().as_number(),
        42.0
    );
    let later = vm.native_future_new();
    let future = later.future();
    later.resolve(NativeFutureValue::Int(64)).unwrap();
    vm.pump_until_blocked().unwrap();
    assert_eq!(vm.future_value(future), Some(BxValue::new_int(64)));
}

#[test]
fn cold_ordinary_members_do_not_allocate_name_roundtrips() {
    let mut allocations = Vec::new();
    for repeats in [1, 100] {
        let mut vm = VM::new();
        let record = vm.struct_new();
        vm.struct_set(record, "value", BxValue::new_int(42));
        vm.insert_global("record".into(), BxValue::new_ptr(record));
        let mut script = Chunk::new("cold-members.bxs");
        let record = script.add_constant(Constant::String(BoxString::new("record")));
        let field = script.add_constant(Constant::String(BoxString::new("value")));
        for repeat in 0..repeats {
            script.emit1(op::GET_GLOBAL, record, 1);
            script.emit1(op::MEMBER, field, 1);
            if repeat + 1 < repeats {
                script.emit0(op::POP, 1);
            }
        }
        script.emit0(op::RETURN, 1);
        ALLOCATIONS.with(|count| count.set(Some(0)));
        let result = vm.interpret_chunk_borrowed(&script).unwrap();
        allocations.push(ALLOCATIONS.with(|count| count.replace(None).unwrap()));
        assert_eq!(result, BxValue::new_int(42));
    }
    eprintln!("Cold member allocations, 1/100 sites: {allocations:?}");
    assert!(
        allocations[1] <= allocations[0] + 8,
        "Member name roundtrips allocate: {allocations:?}"
    );
}

#[test]
fn warmed_members_do_not_renormalize_operand_names() {
    let mut allocations = Vec::new();
    for repeats in [1, 100] {
        let mut vm = VM::new();
        let record = vm.struct_new();
        vm.struct_set(record, "value", BxValue::new_int(42));
        let _root = vm.retain_value(BxValue::new_ptr(record)).unwrap();
        let mut body = Chunk::new("warm-members.bxs");
        let field = body.add_constant(Constant::String(BoxString::new("VaLuE")));
        for repeat in 0..repeats {
            body.emit1(op::GET_LOCAL, 0, 1);
            body.emit1(op::MEMBER, field, 1);
            if repeat + 1 < repeats {
                body.emit0(op::POP, 1);
            }
        }
        body.emit0(op::RETURN, 1);
        let function = BxCompiledFunction {
            name: "readMembers".into(),
            kind: FunctionKind::Udf,
            arity: 1,
            min_arity: 1,
            params: vec!["record".into()],
            bif: None,
            modifiers: Default::default(),
            captured_receiver: None,
            captured_scopes: Vec::new(),
            chunk: body,
        };
        let mut script = Chunk::new("warm-members-owner.bxs");
        let slot = script.add_constant(Constant::CompiledFunction(function));
        script.emit1(op::CONSTANT, slot, 1);
        script.emit0(op::RETURN, 1);
        let function = vm.interpret(script).unwrap();
        let args = vec![BxValue::new_ptr(record)];
        assert_eq!(
            vm.call_function_value(function, args.clone(), None)
                .unwrap(),
            BxValue::new_int(42)
        );
        assert_eq!(vm.struct_key_array(record), ["VaLuE"]);
        ALLOCATIONS.with(|count| count.set(Some(0)));
        let result = vm.call_function_value(function, args, None).unwrap();
        allocations.push(ALLOCATIONS.with(|count| count.replace(None).unwrap()));
        assert_eq!(result, BxValue::new_int(42));
    }
    eprintln!("Warm member allocations, 1/100 sites: {allocations:?}");
    assert!(
        allocations[1] <= allocations[0] + 8,
        "Cached members renormalize names: {allocations:?}"
    );
}

#[test]
fn repeated_parameter_writes_do_not_allocate_binding_names() {
    let mut allocations = Vec::new();
    for repeats in [1, 100] {
        let mut vm = VM::new();
        let mut body = Chunk::new("parameter-writes.bxs");
        let answer = body.add_constant(Constant::Number(42.0));
        for _ in 0..repeats {
            body.emit1(op::CONSTANT, answer, 1);
            body.emit1(op::SET_LOCAL_POP, 0, 1);
        }
        body.emit1(op::GET_LOCAL, 0, 1);
        body.emit0(op::RETURN, 1);
        let function = BxCompiledFunction {
            name: "writeParameter".into(),
            kind: FunctionKind::Udf,
            arity: 2,
            min_arity: 1,
            params: vec!["VaLuE".into()],
            bif: None,
            modifiers: Default::default(),
            captured_receiver: None,
            captured_scopes: Vec::new(),
            chunk: body,
        };
        let mut script = Chunk::new("parameter-writes-owner.bxs");
        let slot = script.add_constant(Constant::CompiledFunction(function));
        script.emit1(op::CONSTANT, slot, 1);
        script.emit0(op::RETURN, 1);
        let function = vm.interpret(script).unwrap();
        let args = vec![BxValue::new_int(1)];
        ALLOCATIONS.with(|count| count.set(Some(0)));
        let result = vm.call_function_value(function, args, None).unwrap();
        allocations.push(ALLOCATIONS.with(|count| count.replace(None).unwrap()));
        assert_eq!(result.as_number(), 42.0);
    }
    eprintln!("Parameter write allocations, 1/100 stores: {allocations:?}");
    assert!(
        allocations[1] <= allocations[0] + 8,
        "Parameter writes allocate names: {allocations:?}"
    );
}

#[test]
fn numeric_parameter_bindings_preserve_read_and_first_write_spelling() {
    for (spelling, incoming) in [
        ("SpellingProbe", "spellingprobe"),
        ("Élève中😀", "élève中😀"),
    ] {
        for write in [false, true] {
            let mut vm = VM::new();
            let mut body = Chunk::new("parameter-spelling.bxs");
            body.emit1(op::GET_LOCAL, 0, 1);
            body.emit0(op::POP, 1);
            if write {
                let answer = body.add_constant(Constant::Number(42.0));
                body.emit1(op::CONSTANT, answer, 1);
                body.emit1(op::SET_LOCAL_POP, 0, 1);
            }
            body.emit1(op::GET_LOCAL, 1, 1);
            body.emit0(op::RETURN, 1);
            let function = BxCompiledFunction {
                name: "parameterSpelling".into(),
                kind: FunctionKind::Udf,
                arity: 2,
                min_arity: 1,
                params: vec![spelling.into()],
                bif: None,
                modifiers: Default::default(),
                captured_receiver: None,
                captured_scopes: Vec::new(),
                chunk: body,
            };
            let mut script = Chunk::new("parameter-spelling-owner.bxs");
            let slot = script.add_constant(Constant::CompiledFunction(function));
            script.emit1(op::CONSTANT, slot, 1);
            script.emit0(op::RETURN, 1);
            let function = vm.interpret(script).unwrap();
            let scope = vm
                .call_function_value_named(
                    function,
                    vec![BxValue::new_int(7)],
                    Some(vec![incoming.into()]),
                )
                .unwrap();
            let scope = scope.as_gc_id().unwrap();
            assert_eq!(
                vm.struct_key_array(scope),
                [if write { spelling } else { incoming }]
            );
            assert_eq!(
                vm.struct_get(scope, incoming).as_number(),
                if write { 42.0 } else { 7.0 }
            );
        }
    }
}

#[test]
fn identifier_bytecode_reads_do_not_allocate_per_instruction() {
    let mut allocations = Vec::new();
    for repeats in [1, 100] {
        let mut vm = VM::new();
        vm.insert_global("knownoperand".into(), BxValue::new_int(42));
        let mut chunk = Chunk::new("operand-read.bxs");
        let name = chunk.add_constant(Constant::String(BoxString::new("knownoperand")));
        for _ in 1..repeats {
            chunk.emit1(op::GET_GLOBAL, name, 1);
            chunk.emit0(op::POP, 1);
        }
        chunk.emit1(op::GET_GLOBAL, name, 1);
        chunk.emit0(op::RETURN, 1);
        ALLOCATIONS.with(|count| count.set(Some(0)));
        let result = vm.interpret_chunk_borrowed(&chunk).unwrap();
        let count = ALLOCATIONS.with(|count| count.replace(None).unwrap());
        assert_eq!(result, BxValue::new_int(42));
        allocations.push(count);
    }
    eprintln!("Identifier allocations, 1/100 reads: {allocations:?}");
    // Interpreter startup may allocate; reading the same metadata must not add
    // a temporary owned string for every cold instruction/cache site.
    assert!(
        allocations[1] <= allocations[0] + 8,
        "Repeated identifier reads allocate: {allocations:?}"
    );
}

#[test]
fn warmed_function_calls_do_not_reallocate_instruction_sized_buffers() {
    let mut bytes = Vec::new();
    for padding in [0, 2000] {
        let mut vm = VM::new();
        let mut body = Chunk::new("function-buffer.bxs");
        let null = body.add_constant(Constant::Null);
        for _ in 0..padding {
            body.emit1(op::CONSTANT, null, 1);
            body.emit0(op::POP, 1);
        }
        let answer = body.add_constant(Constant::Number(42.0));
        body.emit1(op::CONSTANT, answer, 1);
        body.emit0(op::RETURN, 1);
        let function = BxCompiledFunction {
            name: "answer".into(),
            kind: FunctionKind::Udf,
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            bif: None,
            modifiers: Default::default(),
            captured_receiver: None,
            captured_scopes: Vec::new(),
            chunk: body,
        };
        let mut script = Chunk::new("function-owner.bxs");
        let function = script.add_constant(Constant::CompiledFunction(function));
        script.emit1(op::CONSTANT, function, 1);
        script.emit0(op::RETURN, 1);
        let function = vm.interpret(script).unwrap();
        assert_eq!(
            vm.call_function_value(function, Vec::new(), None)
                .unwrap()
                .as_number(),
            42.0
        );
        ALLOCATION_BYTES.with(|count| count.set(Some(0)));
        for _ in 0..10 {
            assert_eq!(
                vm.call_function_value(function, Vec::new(), None)
                    .unwrap()
                    .as_number(),
                42.0
            );
        }
        bytes.push(ALLOCATION_BYTES.with(|count| count.replace(None).unwrap()));
    }
    eprintln!("Warm call allocation bytes, 0/2000 padding pairs: {bytes:?}");
    assert!(
        bytes[1] <= bytes[0] + 8192,
        "Warm calls reallocate buffers sized by function instructions: {bytes:?}"
    );
}

#[test]
fn identifier_operands_preserve_unicode_case_folding_and_rope_text() {
    let key = BoxString::new("Élève中😀_identifier_").concat(&BoxString::new("long_tail"));
    let mut vm = VM::new();
    vm.insert_global(
        "élève中😀_identifier_long_tail".into(),
        BxValue::new_int(42),
    );
    let mut script = Chunk::new("unicode-operand.bxs");
    let key = script.add_constant(Constant::String(key));
    script.emit1(op::GET_GLOBAL, key, 1);
    script.emit0(op::RETURN, 1);
    assert_eq!(vm.interpret(script).unwrap().as_int(), 42);
}

#[test]
fn shared_script_programs_do_not_share_vm_global_indices() {
    let mut script = Chunk::new("shared-script.bxs");
    let key = script.add_constant(Constant::String(BoxString::new("answer")));
    script.emit1(op::GET_GLOBAL, key, 1);
    script.emit0(op::RETURN, 1);
    let mut first = VM::new();
    first.insert_global("answer".into(), BxValue::new_int(42));
    assert_eq!(first.interpret(script.clone()).unwrap().as_int(), 42);
    let mut second = VM::new();
    second.insert_global("padding".into(), BxValue::new_int(-1));
    second.insert_global("answer".into(), BxValue::new_int(64));
    assert_eq!(second.interpret(script).unwrap().as_int(), 64);
}

#[test]
fn immutable_programs_remain_thread_shareable() {
    fn thread_shareable<T: Send + Sync>() {}
    thread_shareable::<ChunkProgram>();
}

#[test]
fn copied_function_metadata_does_not_reuse_another_vms_global_indices() {
    let mut first = VM::new();
    first.insert_global("answer".into(), BxValue::new_int(42));
    let mut body = Chunk::new("vm-local-cache.bxs");
    let key = body.add_constant(Constant::String(BoxString::new("answer")));
    body.emit1(op::GET_GLOBAL, key, 1);
    body.emit0(op::RETURN, 1);
    let function = BxCompiledFunction {
        name: "readAnswer".into(),
        kind: FunctionKind::Udf,
        arity: 0,
        min_arity: 0,
        params: Vec::new(),
        bif: None,
        modifiers: Default::default(),
        captured_receiver: None,
        captured_scopes: Vec::new(),
        chunk: body,
    };
    let mut script = Chunk::new("cache-transfer.bxs");
    let slot = script.add_constant(Constant::CompiledFunction(function));
    script.emit1(op::CONSTANT, slot, 1);
    script.emit0(op::RETURN, 1);
    let value = first.interpret_chunk_borrowed(&script).unwrap();
    assert_eq!(
        first
            .call_function_value(value, Vec::new(), None)
            .unwrap()
            .as_int(),
        42
    );
    // Stage a consumer's copy of actual warmed metadata, not a cache assertion.
    let GcObject::CompiledFunction(function) = first.heap.get(value.as_gc_id().unwrap()) else {
        panic!("Expected compiled function");
    };
    let mut transferred = Chunk::new("transferred-function.bxs");
    let slot = transferred.add_constant(Constant::CompiledFunction((**function).clone()));
    transferred.emit1(op::CONSTANT, slot, 1);
    transferred.emit0(op::RETURN, 1);
    let mut second = VM::new();
    second.insert_global("padding".into(), BxValue::new_int(-1));
    second.insert_global("answer".into(), BxValue::new_int(64));
    let value = second.interpret(transferred).unwrap();
    assert_eq!(
        second
            .call_function_value(value, Vec::new(), None)
            .unwrap()
            .as_int(),
        64
    );
}
