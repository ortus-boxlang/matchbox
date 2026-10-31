use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::{
    Chunk,
    types::{BxVM, BxValue},
    vm::VM,
};
use std::sync::Arc;

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
