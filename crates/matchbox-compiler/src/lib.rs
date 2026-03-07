pub mod ast;
pub mod parser;
pub mod compiler;

pub const PRELUDE_SOURCE: &str = include_str!("prelude.bxs");

use std::collections::{HashSet, HashMap};
use crate::ast::{Statement, StatementKind};

pub fn compile_with_treeshaking(filename: &str, ast: &[Statement], source: &str, extra_keep_symbols: Vec<String>, no_shaking: bool, no_std_lib: bool) -> anyhow::Result<matchbox_vm::Chunk> {
    let final_prelude_stmts = if no_std_lib {
        Vec::new()
    } else {
        // Parse prelude
        let prelude_ast = parser::parse(PRELUDE_SOURCE)?;
        
        if no_shaking {
            prelude_ast
        } else {
            let mut tracker = compiler::DependencyTracker::new();
            tracker.track_statements(ast);

            let mut prelude_map = HashMap::new();
            let mut forced_symbols = extra_keep_symbols.into_iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();

            for stmt in &prelude_ast {
                if let StatementKind::FunctionDecl { name, attributes, .. } = &stmt.kind {
                    prelude_map.insert(name.to_lowercase(), stmt.clone());
                    // Support @matchbox-keep attribute
                    for attr in attributes {
                        if attr.name.to_lowercase() == "matchbox-keep" {
                            forced_symbols.push(name.to_lowercase());
                        }
                    }
                }
            }

            // Also check user AST for @matchbox-keep to ensure they are tracked as roots
            for stmt in ast {
                if let StatementKind::FunctionDecl { name, attributes, .. } = &stmt.kind {
                    for attr in attributes {
                        if attr.name.to_lowercase() == "matchbox-keep" {
                            forced_symbols.push(name.to_lowercase());
                        }
                    }
                }
            }

            // Recursively find all dependencies in the prelude
            let mut prelude_stmts = Vec::new();
            let mut processed_symbols = HashSet::new();
            let mut to_process: Vec<String> = tracker.used_symbols.into_iter().collect();
            to_process.extend(forced_symbols);

            while let Some(symbol) = to_process.pop() {
                if processed_symbols.contains(&symbol) { continue; }
                if let Some(stmt) = prelude_map.get(&symbol) {
                    prelude_stmts.push(stmt.clone());
                    processed_symbols.insert(symbol.clone());
                    
                    // Track dependencies of this prelude function
                    let mut sub_tracker = compiler::DependencyTracker::new();
                    sub_tracker.track_statements(&[stmt.clone()]);
                    for sub_symbol in sub_tracker.used_symbols {
                        if !processed_symbols.contains(&sub_symbol) {
                            to_process.push(sub_symbol);
                        }
                    }
                }
            }
            prelude_stmts
        }
    };

    // Combine prelude and user code
    let mut combined_ast = final_prelude_stmts;
    let prelude_len = combined_ast.len();
    combined_ast.extend_from_slice(ast);

    if std::env::var("MATCHBOX_DEBUG").is_ok() {
        println!("Tree-shaking: included {} prelude functions", prelude_len);
    }

    let compiler = compiler::Compiler::new(filename);
    compiler.compile(&combined_ast, source)
}
