use matchbox_compiler::{parser, compiler::Compiler};
use matchbox_vm::vm::VM;
use matchbox_vm::types::{BxNativeFunction, BxVM, BxValue, NativeFutureValue};
use std::collections::HashMap;
use std::time::Duration;

#[test]
fn test_bxm_transpilation() {
    let bxm_source = r#"
        <bx:set x = 10>
        <bx:if condition="x == 10">
            <bx:output>Value is #x#</bx:output>
        </bx:if>
    "#;
    
    let transpiled = matchbox_compiler::parser::bxm::transpile_bxm(bxm_source);
    
    // Check if it contains expected BoxLang script
    assert!(transpiled.contains("x = 10;"));
    assert!(transpiled.contains("if (x == 10) {"));
    assert!(transpiled.contains("writeOutput(\"Value is \");"));
    assert!(transpiled.contains("writeOutput(x);"));
}

#[test]
fn test_vm_output_buffering() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());
    
    let source = "writeOutput('Hello ', 'World'); println('!'); print('MatchBox');";
    let ast = parser::parse(source).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();
    
    vm.interpret(chunk).unwrap();
    
    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "Hello World!\nMatchBox");
}

#[test]
fn test_weak_typing_addition() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());
    
    // Test: string "10" + number 5 = 15
    // Test: string "1.5" + string "2.5" = 4.0
    // Test: string "Hello" + 5 = "Hello5" (fallback to concat)
    let source = r#"
        writeOutput("10" + 5);
        writeOutput("|");
        writeOutput("1.5" + "2.5");
        writeOutput("|");
        writeOutput("Hello" + 5);
    "#;
    
    let ast = parser::parse(source).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();
    
    vm.interpret(chunk).unwrap();
    
    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "15|4|Hello5");
}

#[test]
fn test_nested_bxm_interpolation() {
    let bxm_source = r#"<bx:output>#1 + 1# is #2# and ## is literal</bx:output>"#;
    let transpiled = matchbox_compiler::parser::bxm::transpile_bxm(bxm_source);
    println!("Transpiled: {}", transpiled);
    
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());
    
    let ast = parser::parse(&transpiled).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, &transpiled).unwrap();
    
    vm.interpret(chunk).unwrap();
    
    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "2 is 2 and # is literal");
}

fn rejected_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let future = vm.future_new();
    let err_id = vm.struct_new();
    let msg_id = vm.string_new("boom".to_string());
    vm.struct_set(err_id, "message", BxValue::new_ptr(msg_id));
    vm.future_reject(future, BxValue::new_ptr(err_id))?;
    Ok(future)
}

fn resolved_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let future = vm.future_new();
    let value_id = vm.string_new("done".to_string());
    vm.future_resolve(future, BxValue::new_ptr(value_id))?;
    Ok(future)
}

fn queued_resolved_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let future = vm.future_new();
    let value_id = vm.string_new("queued".to_string());
    vm.future_schedule_resolve(future, BxValue::new_ptr(value_id))?;
    Ok(future)
}

fn queued_rejected_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let future = vm.future_new();
    let err_id = vm.struct_new();
    let msg_id = vm.string_new("queued-boom".to_string());
    vm.struct_set(err_id, "message", BxValue::new_ptr(msg_id));
    vm.future_schedule_reject(future, BxValue::new_ptr(err_id))?;
    Ok(future)
}

fn threaded_resolved_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let handle = vm.native_future_new();
    let future = handle.future();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        let _ = handle.resolve(NativeFutureValue::String("threaded".to_string()));
    });
    Ok(future)
}

fn threaded_rejected_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let handle = vm.native_future_new();
    let future = handle.future();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        let _ = handle.reject(NativeFutureValue::Error {
            message: "threaded-boom".to_string(),
        });
    });
    Ok(future)
}

#[test]
fn test_native_future_rejection_propagates_value_to_catch() {
    let mut bifs = HashMap::new();
    bifs.insert("rejectedfuture".to_string(), rejected_future as BxNativeFunction);

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        try {
            rejectedFuture().get();
            throw "expected rejection";
        } catch (err) {
            if (err.message != "boom") {
                throw "unexpected rejection payload: " & err.message;
            }
            writeOutput("ok");
        }
    "#;

    let ast = parser::parse(source).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "ok");
}

#[test]
fn test_native_future_resolution_returns_value_from_get() {
    let mut bifs = HashMap::new();
    bifs.insert("resolvedfuture".to_string(), resolved_future as BxNativeFunction);

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(resolvedFuture().get());
    "#;

    let ast = parser::parse(source).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "done");
}

#[test]
fn test_queued_future_resolution_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert("queuedresolvedfuture".to_string(), queued_resolved_future as BxNativeFunction);

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(queuedResolvedFuture().get());
    "#;

    let ast = parser::parse(source).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "queued");
}

#[test]
fn test_queued_future_rejection_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert("queuedrejectedfuture".to_string(), queued_rejected_future as BxNativeFunction);

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        try {
            queuedRejectedFuture().get();
            throw "expected queued rejection";
        } catch (err) {
            writeOutput(err.message);
        }
    "#;

    let ast = parser::parse(source).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "queued-boom");
}

#[test]
fn test_threaded_future_resolution_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert("threadedresolvedfuture".to_string(), threaded_resolved_future as BxNativeFunction);

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(threadedResolvedFuture().get());
    "#;

    let ast = parser::parse(source).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "threaded");
}

#[test]
fn test_threaded_future_rejection_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert("threadedrejectedfuture".to_string(), threaded_rejected_future as BxNativeFunction);

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        try {
            threadedRejectedFuture().get();
            throw "expected threaded rejection";
        } catch (err) {
            writeOutput(err.message);
        }
    "#;

    let ast = parser::parse(source).unwrap();
    let compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "threaded-boom");
}
