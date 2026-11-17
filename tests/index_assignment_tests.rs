use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::{
    Chunk,
    types::{BxNativeObject, BxVM, BxValue, NativeCallback, Tracer},
    vm::{VM, gc::GcObject},
};
use std::{cell::{Cell, RefCell}, fmt, panic::AssertUnwindSafe, rc::Rc};

fn compile(source: &str) -> Chunk {
    let ast = parser::parse(source, Some("index_assignment.bxs")).unwrap();
    Compiler::new("index_assignment.bxs").compile(&ast, source).unwrap()
}

fn execute(vm: &mut VM, source: &str) {
    vm.interpret(compile(source)).unwrap();
}

fn global(vm: &VM, name: &str) -> BxValue {
    vm.get_global(name).unwrap()
}

#[test]
fn numeric_arrays_keep_cast_growth_alias_result_and_value_contracts() {
    for index in [BxValue::new_int(4), BxValue::new_number(4.0), BxValue::new_number(4.75)] {
        let mut vm = VM::new();
        let array = vm.array_new();
        let text = BxValue::new_ptr(vm.string_new("payload".into()));
        let nested = BxValue::new_ptr(vm.array_new());
        let _text_root = vm.retain_value(text).unwrap();
        let _nested_root = vm.retain_value(nested).unwrap();
        vm.insert_global("values".into(), BxValue::new_ptr(array));
        vm.insert_global("alias".into(), BxValue::new_ptr(array));
        vm.insert_global("index".into(), index);
        for payload in [BxValue::new_bool(false), BxValue::new_bool(true), BxValue::new_null(),
            BxValue::new_int(31), BxValue::new_number(-2.5), text, nested]
        {
            vm.insert_global("payload".into(), payload);
            execute(&mut vm, "result=alias[index]=payload; first=values[1]=payload;");
            assert_eq!(global(&vm, "result"), payload);
            assert_eq!(global(&vm, "first"), payload);
            assert_eq!(vm.array_len(array), 4);
            assert_eq!(vm.array_get(array, 0), payload);
            assert!(vm.array_get(array, 1).is_null());
            assert!(vm.array_get(array, 2).is_null());
            assert_eq!(vm.array_get(array, 3), payload);
        }
        execute(&mut vm, "left=[]; right=[]; chained=left[2]=right[3]=17;");
        assert_eq!(global(&vm, "chained").as_number(), 17.0);
        assert_eq!(vm.array_get(global(&vm, "left").as_gc_id().unwrap(), 1).as_number(), 17.0);
        assert_eq!(vm.array_get(global(&vm, "right").as_gc_id().unwrap(), 2).as_number(), 17.0);
    }
}

#[test]
fn array_names_and_scalar_receivers_keep_existing_errors() {
    let mut vm = VM::new();
    execute(&mut vm, "values=[7];");
    let string_index = BxValue::new_ptr(vm.string_new("1".into()));
    let _string_index_root = vm.retain_value(string_index).unwrap();
    for index in [BxValue::new_int(0), BxValue::new_number(0.0), BxValue::new_number(-0.0),
        BxValue::new_number(-1.25), BxValue::new_number(0.75), BxValue::new_number(f64::NAN),
        BxValue::new_number(f64::NEG_INFINITY), BxValue::new_bool(true), BxValue::new_null(), string_index]
    {
        vm.insert_global("index".into(), index);
        execute(&mut vm, "message=''; try { values[index]=19; } catch(any e) { message=e.message; } after=42;");
        let expected = if index.is_number() { "Array index out of bounds: 0" } else { "Array index must be a number" };
        assert!(vm.to_string(global(&vm, "message")).contains(expected));
        assert_eq!(vm.array_get(global(&vm, "values").as_gc_id().unwrap(), 0).as_number(), 7.0);
        assert_eq!(global(&vm, "after").as_number(), 42.0);
    }
    for receiver in [BxValue::new_null(), BxValue::new_int(3), BxValue::new_bool(false)] {
        vm.insert_global("receiver".into(), receiver);
        execute(&mut vm, "message=''; try { receiver[2]=7; } catch(any e) { message=e.message; }");
        assert!(vm.to_string(global(&vm, "message")).contains("Invalid indexed assignment"));
    }
}

#[test]
fn old_arrays_remember_young_values_after_direct_stores() {
    let mut vm = VM::new();
    let array = vm.array_new();
    vm.insert_global("values".into(), BxValue::new_ptr(array));
    vm.collect_garbage_now(); // Promote the array, before each freshly allocated payload.
    let serial = Rc::new(Cell::new(0));
    vm.register_callback("makePayload", NativeCallback { parameters: vec![], callback: Rc::new({
        let serial = Rc::clone(&serial);
        move |vm, _| {
            serial.set(serial.get() + 1);
            Ok(BxValue::new_ptr(vm.string_new(format!("young-{}", serial.get()))))
        }
    }) }).unwrap();
    for pass in 1..=4 {
        execute(&mut vm, "values[1]=makePayload(); return null;");
        vm.collect_garbage_now(); // The array slot, not a payload global/result, must retain it.
        assert_eq!(vm.to_string(vm.array_get(array, 0)), format!("young-{pass}"));
    }
}

#[test]
fn struct_hooks_transform_storage_but_not_assignment_result() {
    let mut vm = VM::new();
    let id = vm.struct_new();
    let names = Rc::new(RefCell::new(Vec::new()));
    vm.insert_global("record".into(), BxValue::new_ptr(id));
    vm.watch_struct_assignments(id, NativeCallback { parameters: vec!["name".into(), "value".into()], callback: Rc::new({
        let names = Rc::clone(&names);
        move |vm, args| {
            names.borrow_mut().push(vm.to_string(args[0]));
            assert_eq!(args[1].as_number(), 5.0);
            Ok(BxValue::new_number(99.0))
        }
    }) }).unwrap();
    execute(&mut vm, "result=record['MiXeD']=5;");
    assert_eq!(&*names.borrow(), &["MiXeD"]);
    assert_eq!(global(&vm, "result").as_number(), 5.0);
    assert_eq!(vm.struct_get(id, "mixed").as_number(), 99.0);
    execute(&mut vm, "plain={}; plain['Other']=17;");
    assert_eq!(vm.struct_get(global(&vm, "plain").as_gc_id().unwrap(), "other").as_number(), 17.0);
}

#[test]
fn watched_struct_retagged_as_array_still_uses_observer_path() {
    let mut vm = VM::new();
    let id = vm.struct_new();
    let names = Rc::new(RefCell::new(Vec::new()));
    vm.watch_struct_assignments(id, NativeCallback { parameters: vec!["name".into(), "value".into()], callback: Rc::new({
        let names = Rc::clone(&names);
        move |vm, args| {
            names.borrow_mut().push(vm.to_string(args[0]));
            assert_eq!(args[1].as_number(), 23.0);
            Ok(BxValue::new_number(99.0))
        }
    }) }).unwrap();
    *vm.heap.get_mut(id) = GcObject::Array(vec![BxValue::new_number(7.0)]);
    vm.insert_global("watched".into(), BxValue::new_ptr(id));
    execute(&mut vm, "result=watched[4]=23;");
    assert_eq!(&*names.borrow(), &["4"]);
    assert_eq!(global(&vm, "result").as_number(), 23.0);
    // Existing assign_struct on a retagged Array calls the observer, then does no array write.
    assert_eq!(vm.array_len(id), 1);
    assert_eq!(vm.array_get(id, 0).as_number(), 7.0);
}

#[test]
fn retagged_array_hook_errors_are_catchable_and_resume_correctly() {
    let mut vm = VM::new();
    let id = vm.struct_new();
    let calls = Rc::new(Cell::new(0));
    vm.watch_struct_assignments(id, NativeCallback { parameters: vec!["name".into(), "value".into()], callback: Rc::new({
        let calls = Rc::clone(&calls);
        move |vm, args| {
            calls.set(calls.get() + 1);
            assert_eq!(vm.to_string(args[0]), "1");
            Err("observer blocked assignment".into())
        }
    }) }).unwrap();
    *vm.heap.get_mut(id) = GcObject::Array(vec![BxValue::new_number(7.0)]);
    vm.insert_global("watched".into(), BxValue::new_ptr(id));
    execute(&mut vm, "message=''; try { watched[1]=23; } catch(any e) { message=e.message; } plain=[]; after=plain[2]=42;");
    assert_eq!(calls.get(), 1);
    assert!(vm.to_string(global(&vm, "message")).contains("observer blocked assignment"));
    assert_eq!(global(&vm, "after").as_number(), 42.0);
    assert_eq!(vm.array_get(id, 0).as_number(), 7.0);
}

#[test]
fn retagged_array_hook_keeps_name_value_roots_across_gc_and_reentry() {
    let mut vm = VM::new();
    let id = vm.struct_new();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let nested = compile("for(gcIndex=0;gcIndex<1100;gcIndex++){ nestedValue=gcIndex; } return 42;");
    vm.watch_struct_assignments(id, NativeCallback { parameters: vec!["name".into(), "value".into()], callback: Rc::new({
        let observed = Rc::clone(&observed);
        move |vm, args| {
            // Create collection pressure with no persistent roots for this garbage.
            for _ in 0..2048 { vm.string_new("garbage".into()); }
            let parent = vm.current_chunk().unwrap();
            assert_eq!(vm.interpret_chunk_with_receiver(nested.clone(), None)?.as_number(), 42.0);
            assert!(Rc::ptr_eq(&parent, &vm.current_chunk().unwrap()));
            observed.borrow_mut().push((vm.to_string(args[0]), vm.to_string(args[1])));
            Ok(BxValue::new_number(99.0))
        }
    }) }).unwrap();
    *vm.heap.get_mut(id) = GcObject::Array(vec![BxValue::new_number(7.0)]);
    vm.insert_global("watched".into(), BxValue::new_ptr(id));
    vm.register_callback("makePayload", NativeCallback { parameters: vec![], callback: Rc::new(|vm, _| {
        Ok(BxValue::new_ptr(vm.string_new("original-payload".into())))
    }) }).unwrap();
    execute(&mut vm, "assigned=watched[1]=makePayload(); after=43;");
    assert_eq!(&*observed.borrow(), &[("1".into(), "original-payload".into())]);
    assert_eq!(vm.to_string(global(&vm, "assigned")), "original-payload");
    assert_eq!(global(&vm, "after").as_number(), 43.0);
    assert_eq!(vm.array_get(id, 0).as_number(), 7.0);
}

#[derive(Debug)]
struct Receiver {
    writes: Rc<RefCell<Vec<(String, BxValue)>>>,
    probes: Rc<Cell<usize>>,
    readonly: bool,
    target: Option<BxValue>,
}
impl BxNativeObject for Receiver {
    fn class_reference_target(&self) -> Option<BxValue> {
        self.probes.set(self.probes.get() + 1);
        self.target
    }
    fn get_property(&self, _name: &str) -> BxValue { BxValue::new_null() }
    fn is_readonly(&self) -> bool { self.readonly }
    fn set_property(&mut self, name: &str, value: BxValue) { self.writes.borrow_mut().push((name.into(), value)); }
    fn call_method(&mut self, _vm: &mut dyn BxVM, _id: usize, _name: &str, _args: &[BxValue]) -> Result<BxValue, String> { Err("No methods".into()) }
    fn trace(&self, tracer: &mut dyn Tracer) { if let Some(target) = self.target { tracer.mark(&target); } }
}

fn receiver(vm: &mut VM, readonly: bool, target: Option<BxValue>) -> (Rc<RefCell<Vec<(String, BxValue)>>>, Rc<Cell<usize>>) {
    let writes = Rc::new(RefCell::new(Vec::new()));
    let probes = Rc::new(Cell::new(0));
    let id = vm.native_object_new(Rc::new(RefCell::new(Receiver {
        writes: Rc::clone(&writes), probes: Rc::clone(&probes), readonly, target,
    })));
    vm.insert_global("receiver".into(), BxValue::new_ptr(id));
    (writes, probes)
}

#[test]
fn native_receivers_keep_numeric_and_string_property_dispatch_and_readonly_error() {
    let mut vm = VM::new();
    let (writes, probes) = receiver(&mut vm, false, None);
    execute(&mut vm, "numeric=receiver[2]=17; named=receiver['field']=18;");
    assert_eq!(probes.get(), 2);
    assert_eq!(&*writes.borrow(), &[("2".into(), BxValue::new_number(17.0)), ("field".into(), BxValue::new_number(18.0))]);
    assert_eq!(global(&vm, "numeric").as_number(), 17.0);
    assert_eq!(global(&vm, "named").as_number(), 18.0);
    let (writes, _) = receiver(&mut vm, true, None);
    execute(&mut vm, "message=''; try { receiver[2]=19; } catch(any e) { message=e.message; }");
    assert!(writes.borrow().is_empty());
    assert!(vm.to_string(global(&vm, "message")).contains("Native object is read-only"));
}

#[test]
fn native_class_reference_to_array_is_not_a_direct_array() {
    let mut vm = VM::new();
    let array = vm.array_new();
    vm.array_push(array, BxValue::new_number(7.0));
    let (writes, probes) = receiver(&mut vm, false, Some(BxValue::new_ptr(array)));
    execute(&mut vm, "message=''; try { receiver[1]=19; } catch(any e) { message=e.message; }");
    assert_eq!(probes.get(), 1);
    assert!(writes.borrow().is_empty());
    assert_eq!(vm.array_get(array, 0).as_number(), 7.0);
    assert!(vm.to_string(global(&vm, "message")).contains("Provided class field is missing or read-only: 1"));
}

struct ChangingKey(Rc<Cell<usize>>);
impl fmt::Debug for ChangingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.set(self.0.get() + 1);
        write!(f, "key{}", self.0.get())
    }
}
impl BxNativeObject for ChangingKey {
    fn get_property(&self, _name: &str) -> BxValue { BxValue::new_null() }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn call_method(&mut self, _vm: &mut dyn BxVM, _id: usize, _name: &str, _args: &[BxValue]) -> Result<BxValue, String> { Err("No methods".into()) }
}

#[test]
fn non_numeric_key_conversion_keeps_order_and_second_property_name() {
    let mut vm = VM::new();
    let (writes, probes) = receiver(&mut vm, false, None);
    let conversions = Rc::new(Cell::new(0));
    let key = vm.native_object_new(Rc::new(RefCell::new(ChangingKey(Rc::clone(&conversions)))));
    vm.insert_global("key".into(), BxValue::new_ptr(key));
    execute(&mut vm, "receiver[key]=17;");
    assert_eq!(conversions.get(), 2);
    assert_eq!(probes.get(), 1);
    assert_eq!(&*writes.borrow(), &[("<native object key2>".into(), BxValue::new_number(17.0))]);
}

#[test]
fn bound_method_arrays_and_captures_are_unchanged() {
    let mut vm = VM::new();
    execute(&mut vm, r#"
        class Matrix {
            function fill() {
                var values=[];
                var reader=()=>values;
                for(var index=1;index<=12;index++){ values[index]=index+0.5; }
                return reader();
            }
        }
        matrix=new Matrix(); values=matrix.fill();
    "#);
    let array = global(&vm, "values").as_gc_id().unwrap();
    assert_eq!(vm.array_len(array), 12);
    for index in 0..12 { assert_eq!(vm.array_get(array, index).as_number(), index as f64 + 1.5); }
}

#[test]
fn invalid_heap_receiver_keeps_existing_panic_path() {
    let mut vm = VM::new();
    vm.insert_global("receiver".into(), BxValue::new_ptr(1 << 30));
    let chunk = compile("receiver[1]=17;");
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| vm.interpret(chunk)));
    assert!(panic.is_err());
    // This deliberately malformed host value's VM is discarded, never resumed.
}

#[test]
fn existing_integer_and_float_cast_distinction_is_not_normalized() {
    // Avoid attempting enormous allocations while documenting the unchanged setter expressions.
    assert_eq!(BxValue::new_int(-1).as_int() as usize, usize::MAX);
    assert_eq!(BxValue::new_number(-1.0).as_number() as usize, 0);
    assert_eq!(BxValue::new_number(f64::NAN).as_number() as usize, 0);
    assert_eq!(BxValue::new_number(f64::INFINITY).as_number() as usize, usize::MAX);
}
