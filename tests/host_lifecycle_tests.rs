use matchbox_compiler::{compile_with_treeshaking, parser};
use matchbox_vm::vm::VM;
use std::sync::Arc;

fn compile_source(source: &str) -> matchbox_vm::Chunk {
    let ast = parser::parse(source, Some("host_lifecycle_test.bx")).unwrap();
    let mut chunk = compile_with_treeshaking(
        "host_lifecycle_test.bx",
        &ast,
        source,
        Vec::new(),
        false,
        false,
        false,
        &[],
    )
    .unwrap();
    chunk.reconstruct_functions();
    chunk
}

#[test]
fn host_method_call_preserves_fibers_spawned_by_run_async() {
    let source = r#"
        class Application {
            function onApplicationStart() {
                runAsync( function() {
                    sleep( 1000 );
                    return "done";
                } );
            }
        }
    "#;

    let mut vm = VM::new();
    let chunk = compile_source(source);
    vm.interpret_chunk_borrowed(&chunk).unwrap();
    let application = vm
        .construct_global_class("Application", Vec::new())
        .unwrap();

    vm.call_method_value(application, "onApplicationStart", Vec::new())
        .unwrap();

    vm.pump_until_blocked().unwrap();

    assert_eq!(
        vm.fibers.len(),
        1,
        "runAsync fiber should remain queued for the host scheduler after the completed lifecycle fiber is cleaned up"
    );
}

#[test]
fn pump_until_blocked_does_not_resume_sleeping_fiber_early() {
    let source = r#"
        class Application {
            function onApplicationStart() {
                runAsync( function() {
                    sleep( 1000 );
                    variables.done = true;
                } );
            }
        }
    "#;

    let mut vm = VM::new();
    let chunk = compile_source(source);
    vm.interpret_chunk_borrowed(&chunk).unwrap();
    let application = vm
        .construct_global_class("Application", Vec::new())
        .unwrap();

    vm.call_method_value(application, "onApplicationStart", Vec::new())
        .unwrap();

    vm.pump_until_blocked().unwrap();
    vm.pump_until_blocked().unwrap();

    assert_eq!(
        vm.get_global("done").map(|value| value.as_bool()),
        None,
        "sleeping runAsync fiber should not resume until its wait time has elapsed"
    );
    assert_eq!(vm.fibers.len(), 1);
}

#[test]
fn application_scope_is_mutable_from_application_start() {
    let source = r#"
        class Application {
            function onApplicationStart() {
                application.automaticJiggleEnabled = true;
            }
        }
    "#;

    let mut vm = VM::new();
    let chunk = compile_source(source);
    vm.interpret_chunk_borrowed(&chunk).unwrap();
    let application = vm
        .construct_global_class("Application", Vec::new())
        .unwrap();
    vm.insert_empty_struct_global("application");

    vm.call_method_value(application, "onApplicationStart", Vec::new())
        .unwrap();

    let enabled = vm
        .get_global_struct_member("application", "automaticJiggleEnabled")
        .unwrap();

    assert_eq!(enabled.as_bool(), true);
}

#[test]
fn current_task_chunk_completes_with_background_fiber_alive() {
    let application_source = r#"
        class Application {
            function onApplicationStart() {
                runAsync( function() {
                    while ( true ) {
                        sleep( 5000 );
                    }
                } );
            }
        }
    "#;
    let route_source = r#"
        routeCompleted = true;
        "ok";
    "#;

    let mut vm = VM::new();
    let application_chunk = compile_source(application_source);
    vm.interpret_chunk_borrowed(&application_chunk).unwrap();
    let application = vm
        .construct_global_class("Application", Vec::new())
        .unwrap();
    vm.insert_empty_struct_global("application");
    vm.call_method_value(application, "onApplicationStart", Vec::new())
        .unwrap();
    vm.pump_until_blocked().unwrap();

    assert_eq!(vm.fibers.len(), 1);

    let route_chunk = compile_source(route_source);
    vm.interpret_chunk_borrowed_current_task(&route_chunk)
        .unwrap();

    assert_eq!(
        vm.get_global("routeCompleted").map(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        vm.fibers.len(),
        1,
        "background fiber should remain alive after route execution"
    );
}

#[test]
fn repeated_route_execution_shares_program_data_without_recloning_constants() {
    // Regression test for ESP32 route execution: routes with large constant
    // tables used to be cloned in full on every request, which exhausted the
    // ESP32-S3 heap. After the split-program refactor, the immutable program
    // data is shared via Arc<ChunkProgram> and only a small per-request
    // ChunkRuntime is created, so many executions succeed with stable heap.
    //
    // Build a route with many literal numbers so that the program data is
    // meaningfully large; a loop of 1..2000 produces 2000 numeric literals
    // in the constant table.
    // Use sequential `result = result + N;` statements to ensure a large
    // constant table (each numeric literal is a constant). This avoids
    // relying on compound assignment `+=` or array literals, which are not
    // needed for the clone-path regression.
    let additions: Vec<String> = (1..=2000)
        .map(|n| format!("result = result + {};", n))
        .collect();
    let additions_src = additions.join("\n");
    let route_source = format!(
        r#"
        result = 0;
        {}
        routeRunCount = ( isNull( routeRunCount ) ? 0 : routeRunCount ) + 1;
    "#,
        additions_src
    );

    let route_chunk = compile_source(&route_source);
    let program_ptr = Arc::as_ptr(&route_chunk.program) as usize;
    let first_constants_len = route_chunk.constants().len();
    let first_runtime_cache_len = route_chunk.runtime.caches.lock().unwrap().len();
    assert!(
        first_constants_len >= 2000,
        "route should have a large constant table to exercise the clone path (got {})",
        first_constants_len
    );

    let mut vm = VM::new();
    for run in 0..50 {
        let value = vm
            .interpret_chunk_borrowed_current_task(&route_chunk)
            .expect(&format!("route execution should succeed on run {}", run));
        assert_eq!(
            value.is_null(),
            true,
            "route execution returns null when the script has no explicit return"
        );
        assert_eq!(
            vm.get_global("result").map(|value| value.as_number()),
            Some(2_001_000.0),
            "route should compute the sum of 1..2000"
        );
        // The original chunk's runtime must not have been mutated by a
        // borrowed execution (each request uses its own runtime caches).
        assert_eq!(
            Arc::as_ptr(&route_chunk.program) as usize,
            program_ptr,
            "program data must remain shared across executions"
        );
        assert_eq!(
            route_chunk.runtime.caches.lock().unwrap().len(),
            first_runtime_cache_len,
            "source chunk runtime caches must not be populated by route execution"
        );
    }

    // The source chunk's constants should still be the original program.
    assert_eq!(
        route_chunk.constants().len(),
        first_constants_len,
        "source chunk constants must not grow across executions"
    );
    assert_eq!(
        vm.get_global("routeRunCount")
            .map(|value| value.as_number()),
        Some(50.0),
        "route should execute once per request"
    );
}
