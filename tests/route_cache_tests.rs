use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::{
    Chunk,
    types::{BxVM, BxValue},
    vm::VM,
};
use std::{cell::RefCell, panic::AssertUnwindSafe, rc::Rc, sync::Arc};
use matchbox_vm::{types::NativeCallback, vm::chunk::IcEntry};

fn compile(source: &str) -> Chunk {
    let ast = parser::parse(source, Some("route_cache.bxs")).unwrap();
    Compiler::new("route_cache.bxs")
        .compile(&ast, source)
        .unwrap()
}

fn route(vm: &mut VM, chunk: &Chunk) {
    vm.interpret_chunk_with_receiver(chunk, None).unwrap();
}

// Debugger-only work windows, never used as release timing measurements.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn route_cache_work_start(phase: u32) {
    std::hint::black_box(phase);
}

#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn route_cache_work_end() {
    std::hint::black_box(());
}

#[test]
fn route_cache_work_preserves_recursive_function_control() {
    let chunk = compile("total=0; for(i=1;i<=100;i++){ total=total+i; } return total;");
    let functions = compile(
        r#"
        function sumGlobals(depth) {
            if(depth>0) { return sumGlobals(depth-1); }
            total=0;
            for(i=1;i<=100;i++){ total=total+i; }
            return total;
        }
    "#,
    );
    let mut vm = VM::new();
    route(&mut vm, &functions);
    let function = vm
        .retain_value(vm.get_global("sumGlobals").unwrap())
        .unwrap();
    for _ in 0..2 {
        route(&mut vm, &chunk);
        assert_eq!(
            vm.call_function("sumGlobals", vec![BxValue::new_number(3.0)], None)
                .unwrap()
                .as_number(),
            5050.0
        );
    }

    route_cache_work_start(1);
    for _ in 0..5 {
        vm.with_isolated_globals(|vm| {
            route(vm, &chunk);
            assert_eq!(vm.get_global("total").unwrap().as_number(), 5050.0);
            Ok(())
        })
        .unwrap();
    }
    route_cache_work_end();

    vm.collect_garbage_now();
    route_cache_work_start(2);
    for _ in 0..5 {
        let function = vm.read_retained_value(&function).unwrap();
        assert_eq!(
            vm.call_function_value(function, vec![BxValue::new_number(3.0)], None)
                .unwrap()
                .as_number(),
            5050.0
        );
    }
    route_cache_work_end();
}

#[test]
fn shared_source_routes_keep_vm_namespace_shapes_and_retained_closures_independent() {
    let chunk = compile(
        r#"
        function makeReader() { return function() { return input; }; }
        function field(s) { return s.value; }
        answer=input;
        for(i=1;i<=6;i++) {
            if(i==1) { s={value:1}; }
            else if(i==2) { s={a:0,value:2}; }
            else if(i==3) { s={b:0,value:3}; }
            else if(i==4) { s={c:0,value:4}; }
            else if(i==5) { s={d:0,value:5}; }
            else { s={e:0,value:6}; }
            answer=answer+field(s);
        }
        reader=makeReader();
    "#,
    );
    let original = (*chunk.program).clone();
    let source_caches = chunk.cache_slice(0, chunk.code().len());
    let program = Arc::clone(&chunk.program);
    let changed = compile("answer=input+99;");
    let mut vms = [VM::new(), VM::new()];
    for pass in 0..4 {
        for (index, vm) in vms.iter_mut().enumerate() {
            let input = (pass * 100 + index * 10) as f64;
            let reader = vm
                .with_isolated_globals(|vm| {
                    if pass % 2 == 0 {
                        vm.insert_global("padding".into(), BxValue::new_bool(true));
                    }
                    vm.insert_global("input".into(), BxValue::new_number(input));
                    route(vm, &chunk);
                    assert_eq!(vm.get_global("answer").unwrap().as_number(), input + 21.0);
                    vm.retain_value(vm.get_global("reader").unwrap())
                        .map_err(anyhow::Error::msg)
                })
                .unwrap();
            vm.collect_garbage_now();
            vm.with_isolated_globals(|vm| {
                vm.insert_global("differentPadding".into(), BxValue::new_bool(false));
                vm.insert_global("input".into(), BxValue::new_number(input + 7.0));
                let reader = vm.read_retained_value(&reader).unwrap();
                assert_eq!(
                    vm.call_function_value(reader, vec![], None)?.as_number(),
                    input + 7.0
                );
                route(vm, &changed);
                assert_eq!(vm.get_global("answer").unwrap().as_number(), input + 106.0);
                route(vm, &chunk);
                assert_eq!(vm.get_global("answer").unwrap().as_number(), input + 28.0);
                Ok(())
            })
            .unwrap();
            assert!(vm.get_global("input").is_none());
            assert!(vm.get_global("reader").is_none());
        }
    }
    assert!(Arc::ptr_eq(&chunk.program, &program));
    assert_eq!(*chunk.program, original);
    assert_eq!(
        chunk.cache_slice(0, chunk.code().len()),
        source_caches,
        "borrowed routes cannot mutate source caches"
    );
}

#[test]
fn fresh_route_entrypoints_preserve_top_level_shape_caches_and_error_recovery() {
    let chunk = compile(
        r#"
        answer=input;
        for(i=1;i<=60;i++) {
            if(i%6==0) { s={value:1}; }
            else if(i%6==1) { s={a:0,value:2}; }
            else if(i%6==2) { s={b:0,value:3}; }
            else if(i%6==3) { s={c:0,value:4}; }
            else if(i%6==4) { s={d:0,value:5}; }
            else { s={e:0,value:6}; }
            answer=answer+s.value;
        }
        return answer;
        "#,
    );
    let source_caches = chunk.cache_slice(0, chunk.code().len());
    for entry in 0..3 {
        let mut vm = VM::new();
        for pass in 0..3 {
            vm.with_isolated_globals(|vm| {
                vm.insert_global("padding".into(), BxValue::new_bool(true));
                vm.insert_global("input".into(), BxValue::new_number(pass as f64));
                let value = match entry {
                    0 => vm.interpret_chunk_borrowed(&chunk)?,
                    1 => vm.interpret_chunk_borrowed_current_task(&chunk)?,
                    _ => vm.interpret_chunk_with_receiver(&chunk, None)?,
                };
                assert_eq!(value.as_number(), 210.0 + pass as f64);
                Ok(())
            }).unwrap();
            assert!(vm.get_global("answer").is_none());
            let failure = compile("throw 'route failed';");
            assert!(vm.with_isolated_globals(|vm| {
                vm.interpret_chunk_with_receiver(&failure, None)
            }).is_err());
            vm.collect_garbage_now();
        }
    }
    assert_eq!(chunk.cache_slice(0, chunk.code().len()), source_caches);
}

#[test]
fn compiled_route_metadata_remains_thread_shareable_and_serializable() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Chunk>();
    send_sync::<matchbox_vm::vm::chunk::ChunkProgram>();
    let chunk = Arc::new(compile("answer=input+1;"));
    let workers: Vec<_> = (0..2)
        .map(|index| {
            let chunk = Arc::clone(&chunk);
            std::thread::spawn(move || {
                let bytes = bincode::serialize(&*chunk).unwrap();
                let decoded: Chunk = bincode::deserialize(&bytes).unwrap();
                let mut vm = VM::new();
                vm.insert_global("input".into(), BxValue::new_number(index as f64));
                route(&mut vm, &chunk);
                assert_eq!(
                    vm.get_global("answer").unwrap().as_number(),
                    index as f64 + 1.0
                );
                route(&mut vm, &decoded);
                assert_eq!(
                    vm.get_global("answer").unwrap().as_number(),
                    index as f64 + 1.0
                );
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}

// A deliberate, caught Rust panic leaves live frames available for ownership
// assertions without adding production hooks. These VMs are discarded, never
// resumed: this helper does not promise recovery from arbitrary native panics.
fn stopped_owning_entry(source: &str) -> (VM, Chunk) {
    let chunk = compile(source);
    let mut vm = VM::new();
    vm.register_callback(
        "cacheProbe",
        NativeCallback { parameters: vec![], callback: Rc::new(|_, _| panic!("entry-cache-probe")) },
    )
    .unwrap();
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| vm.interpret(chunk.clone())))
        .expect_err("the probe must stop inside the live entry");
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"entry-cache-probe"));
    (vm, chunk)
}

#[test]
fn owning_entry_has_one_private_cache_and_separate_empty_pseudo_metadata() {
    let (vm, source) = stopped_owning_entry(
        "answer=0; for(i=1;i<=9;i++){ answer=answer+i; } cacheProbe();",
    );
    let frame = vm.fibers.iter().flat_map(|fiber| &fiber.frames)
        .find(|frame| frame.function.name == "script").unwrap();
    let active = frame.chunk.borrow();
    assert!(Arc::ptr_eq(&active.program, &source.program));
    assert!(active.cache_slice(0, active.code().len()).iter()
        .any(|entry| matches!(entry, Some(IcEntry::Global { .. }))));
    assert!(active.runtime.caches.lock().unwrap().is_empty(),
        "active entries live in the private store, not the inactive shared store");
    assert!(!Arc::ptr_eq(&active.runtime.caches, &frame.function.chunk.runtime.caches));
    assert!(frame.function.chunk.runtime.caches.lock().unwrap().is_empty());
    assert_eq!(frame.function.chunk, frame.function.chunk.clone_without_runtime_caches(),
        "metadata has neither a shared dense store nor a cloned private vector");
    assert!(source.cache_slice(0, source.code().len()).iter().all(Option::is_none));
}

#[test]
fn private_entry_cache_variants_shape_transitions_clone_and_serde_are_mode_aware() {
    let (vm, _) = stopped_owning_entry("answer=1; cacheProbe();");
    let frame = vm.fibers.iter().flat_map(|fiber| &fiber.frames)
        .find(|frame| frame.function.name == "script").unwrap();
    let mut chunk = frame.chunk.borrow_mut();
    let site = chunk.code().len() + 3;
    assert_eq!(chunk.cache_get(site), None);
    let entries = [
        IcEntry::Monomorphic { shape_id: 1, index: 3 },
        IcEntry::Polymorphic { entries: [(1, 3), (2, 4), (0, 0), (0, 0)], count: 2 },
        IcEntry::Megamorphic,
        IcEntry::Global { index: 5, namespace: 83 },
    ];
    for entry in entries {
        chunk.cache_set(site, entry.clone());
        assert_eq!(chunk.cache_get(site), Some(entry));
    }
    let mut snapshot = chunk.clone();
    assert_eq!(*chunk, snapshot);
    snapshot.cache_set(site, IcEntry::Megamorphic);
    assert_ne!(*chunk, snapshot, "full Chunk cloning copies private cache state");
    assert_eq!(chunk.cache_get(site), Some(IcEntry::Global { index: 5, namespace: 83 }));
    assert!(Arc::ptr_eq(&chunk.program, &snapshot.program));
    let bytes = bincode::serialize(&*chunk).unwrap();
    let decoded: Chunk = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded, chunk.clone_without_runtime_caches());
    assert_eq!(decoded.cache_get(site), None);
    for (shape, index) in [(10, 2), (20, 3), (30, 4), (40, 5)] {
        chunk.cache_add_shape(site + 1, shape, index);
    }
    assert_eq!(chunk.cache_get(site + 1), Some(IcEntry::Polymorphic {
        entries: [(10, 2), (20, 3), (30, 4), (40, 5)], count: 4,
    }));
    chunk.cache_add_shape(site + 1, 20, 999);
    assert!(matches!(chunk.cache_get(site + 1), Some(IcEntry::Polymorphic { count: 4, .. })));
    chunk.cache_add_shape(site + 1, 50, 6);
    assert_eq!(chunk.cache_get(site + 1), Some(IcEntry::Megamorphic));
    chunk.cache_add_shape(site, 99, 99);
    assert_eq!(chunk.cache_get(site), Some(IcEntry::Global { index: 5, namespace: 83 }));
    assert!(chunk.runtime.caches.lock().unwrap().is_empty());
    assert!(frame.function.chunk.cache_slice(site, 2).iter().all(Option::is_none));
}

#[test]
fn owning_entries_preserve_source_vm_layout_namespace_and_retained_reader_contracts() {
    let chunk = compile(
        "function makeReader(){ return function(){ return input; }; } \
         answer=input; for(i=1;i<=6;i++){ s={value:i}; answer=answer+s.value; } \
         reader=makeReader(); return answer;",
    );
    let before = chunk.clone();
    let failure = compile("throw 'owning failed';");
    let mut vms = [VM::new(), VM::new()];
    for pass in 0..4 {
        for (index, vm) in vms.iter_mut().enumerate() {
            let input = (pass * 100 + index * 10) as f64;
            let reader = vm.with_isolated_globals(|vm| {
                if pass % 2 == 0 {
                    vm.insert_global("padding".into(), BxValue::new_bool(true));
                }
                vm.insert_global("input".into(), BxValue::new_number(input));
                assert_eq!(vm.interpret(chunk.clone())?.as_number(), input + 21.0);
                vm.retain_value(vm.get_global("reader").unwrap()).map_err(anyhow::Error::msg)
            }).unwrap();
            vm.collect_garbage_now();
            vm.with_isolated_globals(|vm| {
                vm.insert_global("differentPadding".into(), BxValue::new_bool(false));
                vm.insert_global("input".into(), BxValue::new_number(input + 7.0));
                let reader = vm.read_retained_value(&reader).unwrap();
                assert_eq!(vm.call_function_value(reader, vec![], None)?.as_number(), input + 7.0);
                assert_eq!(vm.interpret(chunk.clone())?.as_number(), input + 28.0);
                Ok(())
            }).unwrap();
            assert!(vm.with_isolated_globals(|vm| vm.interpret(failure.clone())).is_err());
            assert!(vm.get_global("input").is_none());
            assert!(vm.get_global("reader").is_none());
        }
    }
    assert_eq!(chunk, before);
    assert!(chunk.cache_slice(0, chunk.code().len()).iter().all(Option::is_none));
}

#[test]
fn owning_entry_keeps_recursive_function_cache_sharing_and_program_identity_guards() {
    let (mut vm, _) = stopped_owning_entry(
        "function recurse(depth){ if(depth>0){return recurse(depth-1);} cacheProbe(); return 1; } recurse(3);",
    );
    let frames: Vec<_> = vm.fibers.iter().flat_map(|fiber| &fiber.frames)
        .filter(|frame| frame.function.name == "recurse").collect();
    assert_eq!(frames.len(), 4);
    let function = Rc::clone(&frames[0].function);
    let site = function.chunk.code().len() + 2;
    frames[0].chunk.borrow_mut().cache_set(site, IcEntry::Megamorphic);
    for frame in &frames {
        let active = frame.chunk.borrow();
        assert!(Arc::ptr_eq(&active.runtime.caches, &function.chunk.runtime.caches));
        assert_eq!(active.cache_get(site), Some(IcEntry::Megamorphic));
    }
    assert_eq!(function.chunk.cache_get(site), Some(IcEntry::Megamorphic));

    // Check another invocation's construction in the SAME VM, without
    // resuming the deliberately stopped VM or transferring VM-owned values.
    drop(frames);
    vm.spawn(Rc::clone(&function), vec![BxValue::new_number(0.0)], 0,
        Rc::new(RefCell::new(Chunk::default())), None);
    let spawned = &vm.fibers.last().unwrap().frames[0];
    assert_eq!(spawned.chunk.borrow().cache_get(site), Some(IcEntry::Megamorphic));
    assert!(Arc::ptr_eq(&spawned.chunk.borrow().runtime.caches, &function.chunk.runtime.caches));

    // Separately exercise the weak-program guard using host-constructed,
    // capture-free metadata, not a function associated with a different VM.
    let template = compile("function replacement(){return 1;}");
    let replacement = template.constants().iter().find_map(|constant| match constant {
        matchbox_vm::types::Constant::CompiledFunction(function) => Some(function),
        _ => None,
    }).unwrap();
    let mut replacement = (*replacement).clone();
    replacement.chunk = replacement.chunk.clone_without_runtime_caches();
    replacement.chunk.cache_set(site, IcEntry::Megamorphic);
    let old_caches = Arc::clone(&replacement.chunk.runtime.caches);
    replacement.chunk.program = compile("return 77;").program;
    let mut next = VM::new();
    let future = next.spawn(Rc::new(replacement), vec![], 0,
        Rc::new(RefCell::new(Chunk::default())), None);
    assert!(!Arc::ptr_eq(&next.fibers[0].frames[0].chunk.borrow().runtime.caches, &old_caches));
    assert_eq!(next.fibers[0].frames[0].chunk.borrow().cache_get(site), None);
    assert_eq!(next.run_future_to_completion(future).unwrap().as_number(), 77.0);
    assert_eq!(function.chunk.cache_get(site), Some(IcEntry::Megamorphic));
}

#[test]
fn owning_entry_reentry_releases_borrows_and_retained_chunk_lifetime_is_explicit() {
    let nested = compile("nestedAnswer=input+7; return nestedAnswer;");
    let source = compile("input=10; answer=cacheReenter()+input; return answer;");
    let saved = Rc::new(RefCell::new(None));
    let observed = Rc::clone(&saved);
    let mut vm = VM::new();
    vm.register_callback("cacheReenter", NativeCallback { parameters: vec![], callback: Rc::new(move |vm, _| {
        let chunk = vm.current_chunk().unwrap();
        assert!(chunk.try_borrow_mut().is_ok());
        let value = vm.interpret_chunk_with_receiver(nested.clone(), None)?;
        assert!(chunk.try_borrow_mut().is_ok());
        assert!(Rc::ptr_eq(&chunk, &vm.current_chunk().unwrap()));
        *observed.borrow_mut() = Some(chunk);
        Ok(value)
    }) }).unwrap();
    assert_eq!(vm.interpret(source.clone()).unwrap().as_number(), 27.0);
    vm.collect_garbage_now();
    let retained = saved.borrow().as_ref().cloned().unwrap();
    assert!(retained.borrow().cache_slice(0, source.code().len()).iter()
        .any(|entry| matches!(entry, Some(IcEntry::Global { .. }))));
    assert!(retained.borrow().runtime.caches.lock().unwrap().is_empty());
    assert!(source.cache_slice(0, source.code().len()).iter().all(Option::is_none));
}

#[test]
fn shared_chunk_clone_and_poison_behavior_are_unchanged() {
    let mut source = compile("return 1;");
    source.cache_set(0, IcEntry::Megamorphic);
    let clone = source.clone();
    assert!(Arc::ptr_eq(&source.runtime.caches, &clone.runtime.caches));
    source.cache_set(0, IcEntry::Global { index: 2, namespace: 7 });
    assert_eq!(clone.cache_get(0), Some(IcEntry::Global { index: 2, namespace: 7 }));
    let caches = Arc::clone(&source.runtime.caches);
    assert!(std::thread::spawn(move || {
        let _guard = caches.lock().unwrap();
        panic!("intentional shared-cache poison");
    }).join().is_err());
    assert!(std::panic::catch_unwind(AssertUnwindSafe(|| source.cache_get(0))).is_err());
}

#[test]
fn owning_entries_share_only_immutable_programs_across_host_threads() {
    let source = Arc::new(compile("answer=input; for(i=1;i<=5;i++){ answer+=i; } return answer;"));
    let before = source.cache_slice(0, source.code().len());
    let workers: Vec<_> = (0..2).map(|input| {
        let source = Arc::clone(&source);
        std::thread::spawn(move || {
            let mut vm = VM::new();
            vm.insert_global("input".into(), BxValue::new_number(input as f64));
            assert_eq!(vm.interpret((*source).clone()).unwrap().as_number(), input as f64 + 15.0);
        })
    }).collect();
    for worker in workers { worker.join().unwrap(); }
    assert_eq!(source.cache_slice(0, source.code().len()), before);
}
