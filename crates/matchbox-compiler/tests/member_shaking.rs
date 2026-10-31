//! Every prelude function reachable through a member call must survive tree-shaking.

use matchbox_compiler::compiler::member_function_candidates;
use matchbox_compiler::{ast::StatementKind, parser, PRELUDE_SOURCE};
use std::collections::HashSet;

const VM_SOURCE: &str = include_str!("../../matchbox-vm/src/vm/mod.rs");

fn prelude_functions() -> HashSet<String> {
    parser::parse(PRELUDE_SOURCE, Some("prelude"))
        .expect("prelude parses")
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StatementKind::FunctionDecl { name, .. } => Some(name.to_lowercase()),
            _ => None,
        })
        .collect()
}

/// (member name, BIF) pairs from the VM's `resolve_member_method` table.
fn member_table() -> Vec<(String, String)> {
    let start = VM_SOURCE.find("fn resolve_member_method").expect("member table");
    let end = VM_SOURCE[start..].find("fn resolve_method(").expect("end of member table") + start;
    let mut pairs = Vec::new();
    for line in VM_SOURCE[start..end].lines() {
        let Some((names, bif)) = line.split_once("=> Some(\"") else { continue };
        let Some(bif) = bif.split('"').next() else { continue };
        for name in names.split('|') {
            let name = name.trim().trim_matches('"');
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                pairs.push((name.to_string(), bif.to_string()));
            }
        }
    }
    pairs
}

#[test]
fn member_table_is_parsed() {
    let table = member_table();
    assert!(table.len() > 300, "expected the full member table, got {}", table.len());
    assert!(table.contains(&("sort".to_string(), "arraysort".to_string())));
}

#[test]
fn shaking_keeps_every_prelude_bif_reachable_through_a_member_call() {
    let prelude = prelude_functions();
    let reachable = member_table().into_iter().filter(|(_, bif)| prelude.contains(bif)).count();
    assert!(reachable > 30, "expected many prelude BIFs behind members, got {reachable}");
    let missing: Vec<String> = member_table()
        .into_iter()
        .filter(|(_, bif)| prelude.contains(bif))
        .filter(|(member, bif)| !member_function_candidates(member).contains(bif))
        .map(|(member, bif)| format!(".{member}() -> {bif}"))
        .collect();
    assert!(missing.is_empty(), "members whose prelude BIF would be shaken away: {missing:?}");
}
