//! Frozen research harness: normal compiler/prelude and VM, never enables JIT.
use anyhow::{Context, Result};
use matchbox_vm::vm::VM;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).context("expected a BoxLang script path")?;
    let source = std::fs::read_to_string(path)?;
    let ast = matchbox_compiler::parser::parse(&source, Some(path))?;
    let chunk = matchbox_compiler::compile_with_treeshaking(
        path, &ast, &source, Vec::new(), false, false, false, &[],
    )?;
    let mut vm = VM::new();
    // Retain default features, including JIT capability, but require it inactive.
    assert!(vm.jit.is_none(), "interpreter-only campaign must not enable JIT");
    vm.set_cli_args(args);
    vm.interpret(chunk)?;
    assert!(vm.jit.is_none(), "JIT activated during interpreter-only measurement");
    Ok(())
}
