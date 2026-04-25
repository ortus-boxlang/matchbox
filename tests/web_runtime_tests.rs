use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::types::{BxNativeFunction, BxVM, BxValue, NativeFutureValue};
use matchbox_vm::vm::VM;
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
    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
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

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
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

    let ast = parser::parse(&transpiled, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, &transpiled).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "2 is 2 and # is literal");
}

#[test]
fn test_quoted_struct_literal_keys_preserve_original_case() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        options = {
            "acceptAllDevices": true,
            "optionalServices": ["service-a", "service-b"]
        };
        keys = structKeyArray(options);
        writeOutput(keys[1] & "|" & keys[2]);
    "#;

    let ast = parser::parse(source, None).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "acceptAllDevices|optionalServices");
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
    bifs.insert(
        "rejectedfuture".to_string(),
        rejected_future as BxNativeFunction,
    );

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

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "ok");
}

#[test]
fn test_native_future_resolution_returns_value_from_get() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "resolvedfuture".to_string(),
        resolved_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(resolvedFuture().get());
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "done");
}

#[test]
fn test_queued_future_resolution_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "queuedresolvedfuture".to_string(),
        queued_resolved_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(queuedResolvedFuture().get());
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "queued");
}

#[test]
fn test_queued_future_rejection_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "queuedrejectedfuture".to_string(),
        queued_rejected_future as BxNativeFunction,
    );

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

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "queued-boom");
}

#[test]
fn test_threaded_future_resolution_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "threadedresolvedfuture".to_string(),
        threaded_resolved_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(threadedResolvedFuture().get());
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "threaded");
}

#[test]
fn test_threaded_future_rejection_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "threadedrejectedfuture".to_string(),
        threaded_rejected_future as BxNativeFunction,
    );

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

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "threaded-boom");
}

#[test]
fn test_js_import_binds_to_global() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    // Set up a mock js global so the import has something to resolve.
    let setup = r#"
        class MockConsole {
            function log(msg) {
                writeOutput(msg);
            }
        }

        js = {
            console: new MockConsole(),
            window: {
                document: {
                    title: "Mock Title"
                }
            }
        };
    "#;
    let setup_ast = parser::parse(setup, Some("setup")).unwrap();
    let mut setup_compiler = Compiler::new("setup");
    let setup_chunk = setup_compiler.compile(&setup_ast, setup).unwrap();
    vm.interpret(setup_chunk).unwrap();

    let source = r#"
        import js:console;
        console.log("hello from console");

        import js:window.document as doc;
        writeOutput("|");
        writeOutput(doc.title);
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "hello from console|Mock Title");
}

#[test]
fn test_js_import_constructor_native_mock() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    // Set up a mock js global with a simple value.
    let setup = r#"
        js = {
            MyMockCtor: "hello"
        };
    "#;

    let source = r#"
        import js:MyMockCtor;
        writeOutput(MyMockCtor);
    "#;
    let setup_ast = parser::parse(setup, Some("setup")).unwrap();
    let mut setup_compiler = Compiler::new("setup");
    let setup_chunk = setup_compiler.compile(&setup_ast, setup).unwrap();
    vm.interpret(setup_chunk).unwrap();

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "hello");
}

#[test]
fn test_js_import_constructor_inside_class_native() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    // Mock the js global with a BoxLang class (simulates a JS constructor).
    // `this.encoding` is set in the class body so it runs in the auto-generated
    // constructor, matching how JS constructors work without a separate init().
    let setup = r#"
        class MockTextEncoder {
            this.encoding = "utf-8";
        }
        js = {
            TextEncoder: MockTextEncoder
        };
    "#;
    let setup_ast = parser::parse(setup, Some("setup")).unwrap();
    let mut setup_compiler = Compiler::new("setup");
    let setup_chunk = setup_compiler.compile(&setup_ast, setup).unwrap();
    vm.interpret(setup_chunk).unwrap();

    let source = r#"
        import js:TextEncoder;

        class Writer {
            function init() {
                variables.encoder = new TextEncoder();
                return this;
            }

            function getEncoding() {
                return variables.encoder.encoding;
            }
        }

        w = new Writer();
        writeOutput(w.getEncoding());
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "utf-8");
}

#[test]
fn test_cross_file_js_import_propagation() {
    use std::fs;
    use std::path::Path;
    use std::env;
    
    let tmp_dir = Path::new("/tmp/cross_file_test_dir");
    fs::remove_dir_all(tmp_dir).ok();
    fs::create_dir_all(tmp_dir).ok();
    fs::create_dir_all(tmp_dir.join("modules/tspl/models")).ok();

    fs::write(tmp_dir.join("modules/tspl/models/Writer.bx"), r#"
import js:TextEncoder;
class Writer {
    function init() {
        variables.encoder = new TextEncoder();
        return this;
    }
}
"#).unwrap();

    fs::write(tmp_dir.join("test.bxs"), r#"
class MockTextEncoder {
    this.encoding = "utf-8";
}
js = { TextEncoder: MockTextEncoder };
import modules.tspl.models.Writer;
writer = new Writer();
writeOutput("PASS");
"#).unwrap();

    let orig_dir = env::current_dir().unwrap();
    env::set_current_dir(tmp_dir).unwrap();

    let source = fs::read_to_string(tmp_dir.join("test.bxs")).unwrap();
    let ast = matchbox_compiler::parser::parse(&source, Some("test")).unwrap();
    let mut compiler = matchbox_compiler::compiler::Compiler::new("test");
    let chunk = compiler.compile(&ast, &source).unwrap();

    env::set_current_dir(orig_dir).unwrap();

    let mut vm = matchbox_vm::vm::VM::new();
    vm.output_buffer = Some(String::new());
    vm.interpret(chunk).unwrap();
    assert_eq!(vm.output_buffer.unwrap(), "PASS");
}
