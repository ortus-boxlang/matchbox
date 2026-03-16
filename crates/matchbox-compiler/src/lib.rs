pub mod ast;
pub mod parser;
pub mod compiler;

pub const PRELUDE_SOURCE: &str = include_str!("prelude.bxs");

use std::collections::{HashSet, HashMap};
use std::path::PathBuf;
use crate::ast::{Statement, StatementKind};

/// Compile `ast` to bytecode, optionally tree-shaking the built-in prelude and any module BIFs.
///
/// # Parameters
/// - `module_mappings` — `(module_name, physical_path)` pairs collected from `matchbox.toml` or
///   `--module` flags.  Each entry creates a `bxmodules.{name}` → `PathBuf` mapping used by the
///   compiler's import resolver so that `import bxModules.foo.models.Person` resolves to
///   `{foo_path}/models/Person.bxs`.
/// - `extra_preludes` — raw source text of each `bifs/*.bxs` file found in loaded modules.
///   These are tree-shaken together with the built-in prelude; module functions shadow built-ins
///   on name collision.
pub fn compile_with_treeshaking(
    filename: &str,
    ast: &[Statement],
    source: &str,
    extra_keep_symbols: Vec<String>,
    no_shaking: bool,
    no_std_lib: bool,
    module_mappings: &[(String, PathBuf)],
    extra_preludes: &[String],
) -> anyhow::Result<matchbox_vm::Chunk> {
    // Parse all module bif sources up front.
    let mut module_prelude_stmts: Vec<Statement> = Vec::new();
    for src in extra_preludes {
        let stmts = parser::parse(src)?;
        module_prelude_stmts.extend(stmts);
    }

    let final_prelude_stmts = if no_std_lib {
        // Built-in prelude excluded, but module bifs are still injected.
        if no_shaking {
            module_prelude_stmts
        } else {
            tree_shake_stmts(&module_prelude_stmts, ast, extra_keep_symbols)
        }
    } else {
        // Parse the built-in prelude.
        let prelude_ast = parser::parse(PRELUDE_SOURCE)?;

        if no_shaking {
            // All built-in + all module bifs, no shaking.
            let mut all = prelude_ast;
            all.extend(module_prelude_stmts);
            all
        } else {
            // Merge: module bifs are appended after built-ins so that on name collision the
            // module function's entry overwrites the built-in one in the pool map.
            let mut merged = prelude_ast;
            merged.extend(module_prelude_stmts);
            tree_shake_stmts(&merged, ast, extra_keep_symbols)
        }
    };

    // Combine prelude and user code.
    let mut combined_ast = final_prelude_stmts;
    let prelude_len = combined_ast.len();
    combined_ast.extend_from_slice(ast);

    if std::env::var("MATCHBOX_DEBUG").is_ok() {
        println!("Tree-shaking: included {} prelude functions", prelude_len);
    }

    // Build the module_paths map: "bxmodules.{name}" -> PathBuf (all lowercase for lookup).
    let module_paths: HashMap<String, PathBuf> = module_mappings
        .iter()
        .map(|(name, path)| (format!("bxmodules.{}", name.to_lowercase()), path.clone()))
        .collect();

    let mut compiler = compiler::Compiler::new(filename);
    compiler.module_paths = module_paths;
    let mut chunk = compiler.compile(&combined_ast, source)?;
    chunk.reconstruct_functions();
    Ok(chunk)
}

/// Tree-shake a pool of prelude/module statements against the user AST.
///
/// Builds a name → statement map from `pool` (last entry wins on collision), then transitively
/// pulls in every function reachable from symbols used in `user_ast` or forced via
/// `extra_keep_symbols` / `@matchbox-keep` attributes.
fn tree_shake_stmts(
    pool: &[Statement],
    user_ast: &[Statement],
    extra_keep_symbols: Vec<String>,
) -> Vec<Statement> {
    // Last definition for a given name wins (module bifs shadow built-ins because they're
    // appended after built-ins in the merged pool).
    let mut pool_map: HashMap<String, Statement> = HashMap::new();
    let mut forced: Vec<String> = extra_keep_symbols
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect();

    for stmt in pool {
        if let StatementKind::FunctionDecl { name, attributes, .. } = &stmt.kind {
            pool_map.insert(name.to_lowercase(), stmt.clone());
            for attr in attributes {
                if attr.name.to_lowercase() == "matchbox-keep" {
                    forced.push(name.to_lowercase());
                }
            }
        }
    }

    // Also honour @matchbox-keep on user-defined functions.
    for stmt in user_ast {
        if let StatementKind::FunctionDecl { name, attributes, .. } = &stmt.kind {
            for attr in attributes {
                if attr.name.to_lowercase() == "matchbox-keep" {
                    forced.push(name.to_lowercase());
                }
            }
        }
    }

    let mut tracker = compiler::DependencyTracker::new();
    tracker.track_statements(user_ast);

    let mut result: Vec<Statement> = Vec::new();
    let mut processed: HashSet<String> = HashSet::new();
    let mut to_process: Vec<String> = tracker.used_symbols.into_iter().collect();
    to_process.extend(forced);

    while let Some(symbol) = to_process.pop() {
        if processed.contains(&symbol) {
            continue;
        }
        processed.insert(symbol.clone());
        if let Some(stmt) = pool_map.get(&symbol) {
            result.push(stmt.clone());
            let mut sub = compiler::DependencyTracker::new();
            sub.track_statements(&[stmt.clone()]);
            for sub_sym in sub.used_symbols {
                if !processed.contains(&sub_sym) {
                    to_process.push(sub_sym);
                }
            }
        }
    }

    result
}
