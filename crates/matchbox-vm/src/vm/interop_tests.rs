#[cfg(all(test, target_arch = "wasm32", feature = "js"))]
mod tests {
    use crate::vm::VM;
    use crate::types::{BxValue, BxVM};
    use crate::vm::gc::GcObject;
    use wasm_bindgen::JsValue;
    use wasm_bindgen_test::*;

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
}
