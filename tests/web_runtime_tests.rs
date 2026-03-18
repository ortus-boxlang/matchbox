use matchbox_compiler::{parser, compiler::Compiler};
use matchbox_vm::vm::VM;
use matchbox_vm::types::{BxVM, BxValue};

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
