mod ast;
mod bifs;
mod env;
mod evaluator;
mod parser;
mod types;

use std::env as std_env;
use std::fs;
use anyhow::{Result, bail};

fn main() -> Result<()> {
    let args: Vec<String> = std_env::args().collect();
    if args.len() < 2 {
        bail!("Usage: bx-rust <file.bxs>");
    }

    let filename = &args[1];
    let source = fs::read_to_string(filename)?;

    match parser::parse(&source) {
        Ok(ast) => {
            // println!("AST: {:#?}", ast);
            let mut evaluator = evaluator::Evaluator::new();
            bifs::register_bifs(&mut evaluator.env.borrow_mut());
            match evaluator.eval_program(&ast) {
                Ok(_) => {}
                Err(e) => eprintln!("Runtime Error: {}", e),
            }
        }
        Err(e) => {
            eprintln!("Parse Error: {}", e);
        }
    }

    Ok(())
}
