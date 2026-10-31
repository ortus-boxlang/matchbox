//! Public VM-boundary regressions; allocation counts are deterministic, not stopwatch gates.
use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::{
    types::{BxVM, BxValue, NativeCallback},
    vm::VM,
};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, rc::Rc};

struct CountingAllocator;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
fn count() {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
    }
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn empty_request_snapshot_has_bounded_allocation_calls() {
    let mut vm = VM::new();
    for _ in 0..3 { vm.with_isolated_globals(|_| Ok(())).unwrap(); }
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|enabled| enabled.set(true));
    let result = vm.with_isolated_globals(|_| Ok(()));
    COUNTING.with(|enabled| enabled.set(false));
    result.unwrap();
    let allocations = ALLOCATIONS.with(Cell::get);
    println!("empty isolated request allocation calls: {allocations}");
    assert!(allocations <= 128, "snapshot allocated {allocations} times; bound is 128");
}

fn execute(vm: &mut VM, source: &str) {
    let ast = parser::parse(source, Some("snapshot.bxs")).unwrap();
    let chunk = Compiler::new("snapshot.bxs").compile(&ast, source).unwrap();
    vm.interpret_chunk_borrowed_current_task(&chunk).unwrap();
}
fn register(vm: &mut VM, name: &str) {
    vm.register_callback(name, NativeCallback {
        parameters: vec![], callback: Rc::new(|_, _| Ok(BxValue::new_number(17.0))),
    }).unwrap();
}
fn full_collections(vm: &mut VM) {
    for _ in 0..9 { vm.collect_garbage_now(); }
}

#[test]
fn nested_snapshots_keep_hidden_roots_and_restore_bif_registration() {
    let mut vm = VM::new();
    execute(&mut vm, "hostValue={nested: ['host']};");
    register(&mut vm, "hostBif");
    let host = vm.get_global("hostValue").unwrap();
    let variables = vm.script_variables.clone();
    let saved = BxValue::new_ptr(vm.string_new("variable".into()));
    variables.borrow_mut().insert("saved".into(), saved);
    vm.with_isolated_globals(|vm| {
        execute(vm, "hostValue={nested: ['outer']}; variables.saved={nested: ['outer-variable']};");
        register(vm, "outerBif");
        let outer = vm.get_global("hostValue").unwrap();
        vm.with_isolated_globals(|vm| {
            execute(vm, "hostValue='inner'; variables.saved='inner-variable';");
            register(vm, "innerBif");
            full_collections(vm);
            assert!(vm.has_global_function("hostBif") && vm.has_global_function("outerBif") && vm.has_global_function("innerBif"));
            Ok(())
        })?;
        assert_eq!(vm.get_global("hostValue"), Some(outer));
        assert_eq!(vm.to_string(outer), r#"{"nested":["outer"]}"#);
        assert!(!vm.has_global_function("innerBif"));
        assert!(vm.has_global_function("outerBif"));
        full_collections(vm);
        Ok(())
    }).unwrap();
    assert_eq!(vm.get_global("hostValue"), Some(host));
    assert_eq!(vm.to_string(host), r#"{"nested":["host"]}"#);
    assert!(Rc::ptr_eq(&variables, &vm.script_variables));
    assert_eq!(vm.to_string(saved), "variable");
    assert!(vm.has_global_function("hostBif"));
    assert!(!vm.has_global_function("outerBif") && !vm.has_global_function("innerBif"));

    let error = vm.with_isolated_globals::<()>(|vm| {
        register(vm, "errorBif");
        execute(vm, "hostValue='error';");
        full_collections(vm);
        anyhow::bail!("expected request error")
    });
    assert!(error.is_err());
    assert!(!vm.has_global_function("errorBif"));
    assert_eq!(vm.to_string(host), r#"{"nested":["host"]}"#);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = vm.with_isolated_globals::<()>(|vm| {
            register(vm, "panicBif");
            execute(vm, "hostValue='panic';");
            full_collections(vm);
            panic!("expected request panic");
        });
    }));
    assert!(panic.is_err());
    assert!(!vm.has_global_function("panicBif"));
    assert_eq!(vm.to_string(host), r#"{"nested":["host"]}"#);

    let value = BxValue::new_ptr(vm.string_new("independent".into()));
    let root = vm.retain_value(value).unwrap();
    vm.with_isolated_globals(|vm| { full_collections(vm); Ok(()) }).unwrap();
    assert_eq!(vm.to_string(vm.read_retained_value(&root).unwrap()), "independent");
    drop(root);
    full_collections(&mut vm);
    assert!(vm.heap.get_opt(value.as_gc_id().unwrap()).is_none());

    let dead = BxValue::new_ptr(vm.string_new("collected".into()));
    full_collections(&mut vm);
    vm.insert_global("invalid".into(), dead);
    let mut entered = false;
    assert!(vm.with_isolated_globals(|_| { entered = true; Ok(()) }).is_err());
    assert!(!entered);
}
