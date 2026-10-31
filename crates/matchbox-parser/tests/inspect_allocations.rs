//! Public retained-parse export must not repeatedly copy complete block subtrees.
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell};
use matchbox_parser::{inspect::inspect_parsed, parser};

thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}
struct CountAllocations;
#[global_allocator]
static ALLOCATOR: CountAllocations = CountAllocations;
fn count() {
    let _ = ALLOCATIONS.try_with(|n| { if let Some(value) = n.get() { n.set(Some(value + 1)); } });
}
unsafe impl GlobalAlloc for CountAllocations {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { count(); unsafe { System.alloc(layout) } }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 { count(); unsafe { System.realloc(ptr, layout, size) } }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout) } }
}
fn export(depth: usize, loop_body: bool) -> usize {
    let source = (if loop_body { "while(true){" } else { "if(true){" }).repeat(depth) + "return 1;" + &"}".repeat(depth);
    let statements = parser::parse(&source, None).unwrap();
    ALLOCATIONS.with(|n| n.set(Some(0)));
    let tree = inspect_parsed(&source, &statements, &[], false, false).unwrap();
    let allocations = ALLOCATIONS.with(|n| n.replace(None).unwrap());
    assert_eq!(tree["sourceText"], source);
    let mut node = &tree["statements"][0];
    for _ in 0..depth {
        assert_eq!(node["ASTType"], if loop_body { "BoxWhile" } else { "BoxIfElse" });
        assert_eq!(node["condition"]["value"], true);
        node = &node[if loop_body { "body" } else { "thenBody" }]["statements"][0];
    }
    assert_eq!(node["ASTType"], "BoxReturn");
    assert_eq!(node["expression"]["value"], "1");
    allocations
}
#[test]
fn nested_block_exports_have_linear_allocation_growth() {
    for loop_body in [false, true] {
        let small = export(16, loop_body);
        let large = export(32, loop_body);
        println!("Retained-parse export allocations (while={loop_body}): depth16={small}, depth32={large}");
        assert!(large < small * 5 / 2, "Nested block export recopies subtrees: {small} -> {large}");
    }
}
