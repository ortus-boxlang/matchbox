#[cfg(all(test, target_arch = "wasm32", feature = "js"))]
mod tests {
    use crate::vm::VM;
    use crate::vm::resolve_js_property;
    use crate::types::{BxValue, BxVM};
    use crate::vm::gc::GcObject;
    use wasm_bindgen::JsValue;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_test::*;
    use js_sys::Reflect;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_bx_to_js_int() {
        let vm = VM::new();
        let val = BxValue::new_int(42);
        let js = vm.bx_to_js(&val);
        assert!(js.as_f64().is_some());
        assert_eq!(js.as_f64().unwrap(), 42.0);
    }

    #[wasm_bindgen_test]
    fn test_js_to_bx_int() {
        let mut vm = VM::new();
        let js = JsValue::from_f64(42.0);
        let bx = vm.js_to_bx(js);
        assert!(bx.is_int());
        assert_eq!(bx.as_int(), 42);
    }

    #[wasm_bindgen_test]
    fn test_js_to_bx_float() {
        let mut vm = VM::new();
        let js = JsValue::from_f64(42.5);
        let bx = vm.js_to_bx(js);
        assert!(bx.is_float());
        assert_eq!(bx.as_number(), 42.5);
    }

    #[wasm_bindgen_test]
    fn test_js_to_bx_large_int() {
        let mut vm = VM::new();
        // i32::MAX + 1
        let val = (i32::MAX as f64) + 1.0;
        let js = JsValue::from_f64(val);
        let bx = vm.js_to_bx(js);
        assert!(bx.is_float());
        assert_eq!(bx.as_number(), val);
    }

    #[wasm_bindgen_test]
    fn test_js_to_bx_negative_int() {
        let mut vm = VM::new();
        let js = JsValue::from_f64(-42.0);
        let bx = vm.js_to_bx(js);
        assert!(bx.is_int());
        assert_eq!(bx.as_int(), -42);
    }

    #[wasm_bindgen_test]
    fn test_roundtrip_bool() {
        let mut vm = VM::new();
        let bx = BxValue::new_bool(true);
        let js = vm.bx_to_js(&bx);
        assert!(js.is_truthy());
        let bx2 = vm.js_to_bx(js);
        assert!(bx2.is_bool());
        assert_eq!(bx2.as_bool(), true);
    }

    #[wasm_bindgen_test]
    fn test_roundtrip_null() {
        let mut vm = VM::new();
        let bx = BxValue::new_null();
        let js = vm.bx_to_js(&bx);
        assert!(js.is_null());
        let bx2 = vm.js_to_bx(js);
        assert!(bx2.is_null());
    }

    #[wasm_bindgen_test]
    fn test_bx_to_js_array() {
        let mut vm = VM::new();
        let id = vm.array_new();
        vm.array_push(id, BxValue::new_int(1));
        vm.array_push(id, BxValue::new_int(2));
        let val = BxValue::new_ptr(id);
        let js = vm.bx_to_js(&val);
        assert!(js_sys::Array::is_array(&js));
        let arr = js_sys::Array::from(&js);
        assert_eq!(arr.length(), 2);
        assert_eq!(arr.get(0).as_f64().unwrap(), 1.0);
        assert_eq!(arr.get(1).as_f64().unwrap(), 2.0);
    }

    #[wasm_bindgen_test]
    fn test_bx_to_js_struct() {
        let mut vm = VM::new();
        let id = vm.struct_new();
        vm.struct_set(id, "key1", BxValue::new_int(10));
        vm.struct_set(id, "key2", BxValue::new_bool(true));
        let val = BxValue::new_ptr(id);
        let js = vm.bx_to_js(&val);
        assert!(js.is_object());
        // Verify key access
        assert_eq!(js_sys::Reflect::get(&js, &"key1".into()).unwrap().as_f64().unwrap(), 10.0);
        assert_eq!(js_sys::Reflect::get(&js, &"key2".into()).unwrap().as_bool().unwrap(), true);
    }

    #[wasm_bindgen_test]
    fn test_bx_to_js_nested() {
        let mut vm = VM::new();
        let outer_id = vm.struct_new();
        let inner_id = vm.array_new();
        vm.array_push(inner_id, BxValue::new_int(100));
        vm.struct_set(outer_id, "list", BxValue::new_ptr(inner_id));
        
        let val = BxValue::new_ptr(outer_id);
        let js = vm.bx_to_js(&val);
        
        let list = js_sys::Reflect::get(&js, &"list".into()).unwrap();
        assert!(js_sys::Array::is_array(&list));
        let arr = js_sys::Array::from(&list);
        assert_eq!(arr.get(0).as_f64().unwrap(), 100.0);
    }

    #[wasm_bindgen_test]
    fn test_js_to_bx_array() {
        let mut vm = VM::new();
        let js_arr = js_sys::Array::new();
        js_arr.push(&JsValue::from_f64(10.0));
        let bx = vm.js_to_bx(js_arr.into());
        assert!(bx.is_ptr());
        let id = bx.as_gc_id().unwrap();
        assert!(matches!(vm.heap.get(id), GcObject::Array(_)));
        assert_eq!(vm.array_len(id), 1);
        assert_eq!(vm.array_get(id, 0).as_int(), 10);
    }

    #[wasm_bindgen_test]
    fn test_js_to_bx_struct() {
        let mut vm = VM::new();
        let js_obj = js_sys::Object::new();
        js_sys::Reflect::set(&js_obj, &"a".into(), &JsValue::from_f64(1.0)).unwrap();
        let bx = vm.js_to_bx(js_obj.into());
        assert!(bx.is_ptr());
        let id = bx.as_gc_id().unwrap();
        assert!(matches!(vm.heap.get(id), GcObject::Struct(_)));
        assert_eq!(vm.struct_get(id, "a").as_int(), 1);
    }

    #[wasm_bindgen_test]
    fn test_js_to_bx_nested_plain_values() {
        let mut vm = VM::new();
        let items = js_sys::Array::new();
        items.push(&JsValue::from_f64(1.0));
        items.push(&JsValue::from_f64(2.0));

        let meta = js_sys::Object::new();
        js_sys::Reflect::set(&meta, &"enabled".into(), &JsValue::from_bool(true)).unwrap();

        let js_obj = js_sys::Object::new();
        js_sys::Reflect::set(&js_obj, &"items".into(), &items).unwrap();
        js_sys::Reflect::set(&js_obj, &"meta".into(), &meta).unwrap();

        let bx = vm.js_to_bx(js_obj.into());
        let root_id = bx.as_gc_id().unwrap();
        let items_val = vm.struct_get(root_id, "items");
        let items_id = items_val.as_gc_id().unwrap();
        assert!(matches!(vm.heap.get(items_id), GcObject::Array(_)));
        assert_eq!(vm.array_len(items_id), 2);

        let meta_val = vm.struct_get(root_id, "meta");
        let meta_id = meta_val.as_gc_id().unwrap();
        assert!(matches!(vm.heap.get(meta_id), GcObject::Struct(_)));
        assert!(vm.struct_get(meta_id, "enabled").as_bool());
    }

    #[wasm_bindgen_test]
    fn test_js_to_bx_preserves_dom_node_as_host_handle() {
        let mut vm = VM::new();
        let document = web_sys::window().unwrap().document().unwrap();
        let node: JsValue = document.create_element("div").unwrap().into();
        let bx = vm.js_to_bx(node);
        let id = bx.as_gc_id().unwrap();
        assert!(matches!(vm.heap.get(id), GcObject::JsValue(_)));
    }

    #[wasm_bindgen_test]
    fn test_vm_js_global_is_browser_bridge_namespace() {
        let vm = VM::new();
        let js_global = vm.get_global("js").unwrap();
        let js_id = js_global.as_gc_id().unwrap();
        let root = match vm.heap.get(js_id) {
            GcObject::JsValue(value) => value.clone(),
            other => panic!("expected JsValue root, got {:?}", other),
        };

        let document = js_sys::Reflect::get(&root, &"document".into()).unwrap();
        assert!(document.is_object());

        let window_prop = js_sys::Reflect::get(&root, &"window".into()).unwrap();
        assert!(window_prop.is_object());

        let matchbox_prop = js_sys::Reflect::get(&root, &"MatchBox".into()).unwrap();
        assert!(matchbox_prop.is_undefined() || matchbox_prop.is_object());
    }

    #[wasm_bindgen_test]
    fn test_call_js_function_from_bx() {
        let mut vm = VM::new();
        // Create a JS function that adds 1
        let js_func = js_sys::Function::new_with_args("n", "return n + 1;");
        let bx_func = vm.js_to_bx(js_func.into());
        
        // Call it from BoxLang VM
        let args = vec![BxValue::new_int(41)];
        let result = vm.call_function_value(bx_func, args, None).expect("call_function_value failed");
        
        assert!(result.is_int());
        assert_eq!(result.as_int(), 42);
    }

    #[wasm_bindgen_test]
    fn test_bx_function_to_js_callback() {
        let mut vm = VM::new();
        // Create a BoxLang callable (NativeFunction is easiest to set up in this test)
        let id = vm.heap.alloc(GcObject::NativeFunction(|_vm, args| {
            if args.is_empty() { return Ok(BxValue::new_int(0)); }
            Ok(BxValue::new_int(args[0].as_int() * 2))
        }));
        let bx_func = BxValue::new_ptr(id);
        
        let js_val = vm.bx_to_js(&bx_func);
        assert!(js_val.is_function(), "Expected a JS function, got {:?}", js_val);
        
        let func = js_val.dyn_into::<js_sys::Function>().unwrap();
        let args = js_sys::Array::new();
        args.push(&JsValue::from_f64(21.0));
        
        let result: JsValue = js_sys::Reflect::apply(&func, &JsValue::UNDEFINED, &args).expect("JS call failed");
        assert_eq!(result.as_f64().unwrap(), 42.0);
    }

    #[wasm_bindgen_test]
    fn test_js_promise_is_returned_as_js_value() {
        let mut vm = VM::new();
        // Create a JS function that returns a Promise
        let js_func = js_sys::Function::new_no_args("return Promise.resolve(42);");
        let bx_func = vm.js_to_bx(js_func.into());
        
        let result = vm.call_function_value(bx_func, vec![], None).expect("call_function_value failed");
        let id = result.as_gc_id().expect("Expected a GC pointer");
        
        match vm.heap.get(id) {
            GcObject::JsValue(js) => {
                assert!(js.is_instance_of::<js_sys::Promise>());
            }
            other => panic!("Expected JsValue, got {:?}", other),
        }
    }

    #[wasm_bindgen_test]
    fn test_call_non_function_js_value_fails() {
        let mut vm = VM::new();
        // Create a non-function JS object
        let js_obj = js_sys::Object::new();
        let bx_val = vm.js_to_bx(js_obj.into());
        
        let result = vm.call_function_value(bx_val, vec![], None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a callable JS function"));
    }

    #[wasm_bindgen_test]
    fn test_host_object_property_access() {
        let mut vm = VM::new();
        let document = web_sys::window().unwrap().document().unwrap();
        let div = document.create_element("div").unwrap();
        div.set_id("test-div");
        let bx_div = vm.js_to_bx(div.into());

        let id_val = if let Some(id) = bx_div.as_gc_id() {
             if let GcObject::JsValue(js) = vm.heap.get(id) {
                 // Simulate MEMBER opcode logic
                 let prop = resolve_js_property(js, "id");
                 let val = Reflect::get(js, &prop).unwrap();
                 vm.js_to_bx(val)
             } else { panic!("Not a JsValue"); }
        } else { panic!("Not a pointer"); };

        assert_eq!(vm.to_string(id_val), "test-div");
    }

    #[wasm_bindgen_test]
    fn test_host_object_classList_behavior() {
        let mut vm = VM::new();
        let document = web_sys::window().unwrap().document().unwrap();
        let div = document.create_element("div").unwrap();
        
        let js_div: JsValue = div.into();
        let cl_prop = resolve_js_property(&js_div, "classList");
        let cl_js = Reflect::get(&js_div, &cl_prop).unwrap();
        
        // Add a class via JS
        let add_fn = Reflect::get(&cl_js, &"add".into()).unwrap().dyn_into::<js_sys::Function>().unwrap();
        Reflect::apply(&add_fn, &cl_js, &js_sys::Array::of1(&"foo".into())).unwrap();

        let bx_div = vm.js_to_bx(js_div);

        if let Some(id) = bx_div.as_gc_id() {
            if let GcObject::JsValue(js) = vm.heap.get(id) {
                let cl_prop = resolve_js_property(js, "classList");
                let cl_js = Reflect::get(js, &cl_prop).unwrap();
                let bx_cl = vm.js_to_bx(cl_js);
                
                // classList should remain a handle
                let cl_id = bx_cl.as_gc_id().unwrap();
                assert!(matches!(vm.heap.get(cl_id), GcObject::JsValue(_)));
                
                if let GcObject::JsValue(cl_js_val) = vm.heap.get(cl_id) {
                    assert!(Reflect::has(cl_js_val, &"contains".into()).unwrap());
                }
            }
        }
    }

    #[wasm_bindgen_test]
    fn test_host_object_event_behavior() {
        let mut vm = VM::new();
        // Create event via JS
        let event_ctor = Reflect::get(&js_sys::global(), &"Event".into()).unwrap().dyn_into::<js_sys::Function>().unwrap();
        let event = Reflect::construct(&event_ctor, &js_sys::Array::of1(&"click".into())).unwrap();
        
        let bx_event = vm.js_to_bx(event);
        
        let id = bx_event.as_gc_id().unwrap();
        assert!(matches!(vm.heap.get(id), GcObject::JsValue(_)));
        
        if let GcObject::JsValue(js) = vm.heap.get(id) {
            let type_prop = resolve_js_property(js, "type");
            assert_eq!(Reflect::get(js, &type_prop).unwrap().as_string().unwrap(), "click");
        }
    }
}
