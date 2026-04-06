#[cfg(test)]
mod tests {
    use crate::types::*;
    use matchbox_macros::{BxObject, bx_methods};
    use std::rc::Rc;
    use std::cell::RefCell;

    #[derive(Debug, BxObject)]
    struct TestObject {
        value: f64,
    }

    #[bx_methods]
    impl TestObject {
        pub fn new(value: f64) -> Self {
            Self { value }
        }

        pub fn get_value(&self) -> f64 {
            self.value
        }

        pub fn set_value(&mut self, new_val: f64) {
            self.value = new_val;
        }

        pub fn add(&mut self, other: f64) -> &mut Self {
            self.value += other;
            self
        }

        pub fn set_int(&mut self, val: i32) {
            self.value = val as f64;
        }

        pub fn set_bool(&mut self, val: bool) {
            self.value = if val { 1.0 } else { 0.0 };
        }

        pub fn describe(&self, prefix: String) -> String {
            format!("{}: {}", prefix, self.value)
        }
    }

    struct MockVM;
    impl BxVM for MockVM {
        fn current_chunk(&self) -> Option<Rc<RefCell<crate::vm::chunk::Chunk>>> { None }
        fn spawn(&mut self, _: Rc<BxCompiledFunction>, _: Vec<BxValue>, _: u8, _: Rc<RefCell<crate::vm::chunk::Chunk>>) -> BxValue { BxValue::new_null() }
        fn spawn_by_value(&mut self, _: &BxValue, _: Vec<BxValue>, _: u8, _: Rc<RefCell<crate::vm::chunk::Chunk>>) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn call_function_by_value(&mut self, _: &BxValue, _: Vec<BxValue>, _: Rc<RefCell<crate::vm::chunk::Chunk>>) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn yield_fiber(&mut self) {}
        fn sleep(&mut self, _: u64) {}
        fn get_root_shape(&self) -> u32 { 0 }
        fn get_shape_index(&self, _: u32, _: &str) -> Option<u32> { None }
        fn get_len(&self, _: usize) -> usize { 0 }
        fn array_len(&self, _: usize) -> usize { 0 }
        fn array_push(&mut self, _: usize, _: BxValue) {}
        fn array_pop(&mut self, _: usize) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn array_get(&self, _: usize, _: usize) -> BxValue { BxValue::new_null() }
        fn array_set(&mut self, _: usize, _: usize, _: BxValue) -> Result<(), String> { Ok(()) }
        fn array_delete_at(&mut self, _: usize, _: usize) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn array_insert_at(&mut self, _: usize, _: usize, _: BxValue) -> Result<(), String> { Ok(()) }
        fn array_clear(&mut self, _: usize) -> Result<(), String> { Ok(()) }
        fn array_new(&mut self) -> usize { 0 }
        fn struct_len(&self, _: usize) -> usize { 0 }
        fn struct_new(&mut self) -> usize { 0 }
        fn struct_set(&mut self, _: usize, _: &str, _: BxValue) {}
        fn struct_get(&self, _: usize, _: &str) -> BxValue { BxValue::new_null() }
        fn struct_delete(&mut self, _: usize, _: &str) -> bool { false }
        fn struct_key_exists(&self, _: usize, _: &str) -> bool { false }
        fn struct_key_array(&self, _: usize) -> Vec<String> { vec![] }
        fn struct_clear(&mut self, _: usize) {}
        fn struct_get_shape(&self, _: usize) -> u32 { 0 }
        fn future_on_error(&mut self, _: usize, _: BxValue) {}
        fn native_object_new(&mut self, _: Rc<RefCell<dyn BxNativeObject>>) -> usize { 0 }
        fn native_object_call_method(&mut self, _: usize, _: &str, _: &[BxValue]) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn construct_native_class(&mut self, _: &str, _: &[BxValue]) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn string_new(&mut self, s: String) -> usize { 1234 } // Mock string ID
        fn to_string(&self, _: BxValue) -> String { "prefix".to_string() }
        fn to_box_string(&self, _: BxValue) -> box_string::BoxString { box_string::BoxString::new("") }
        fn get_cli_args(&self) -> Vec<String> { vec![] }
        fn write_output(&mut self, _: &str) {}
    }

    #[test]
    fn test_bx_object_derive() {
        let mut obj = TestObject::new(10.0);
        let mut vm = MockVM;
        
        // Test getter
        let val = obj.call_method(&mut vm, "get_value", &[]).unwrap();
        assert_eq!(val.as_number(), 10.0);

        // Test setter
        obj.call_method(&mut vm, "set_value", &[BxValue::new_number(20.0)]).unwrap();
        assert_eq!(obj.value, 20.0);

        // Test fluent chaining
        let result = obj.call_method(&mut vm, "add", &[BxValue::new_number(5.0)]).unwrap();
        assert!(result.is_ptr());
        assert_eq!(obj.value, 25.0);

        // Test i32 conversion
        obj.call_method(&mut vm, "set_int", &[BxValue::new_int(42)]).unwrap();
        assert_eq!(obj.value, 42.0);

        // Test bool conversion
        obj.call_method(&mut vm, "set_bool", &[BxValue::new_bool(true)]).unwrap();
        assert_eq!(obj.value, 1.0);

        // Test String conversion and return
        let result = obj.call_method(&mut vm, "describe", &[BxValue::new_ptr(0)]).unwrap();
        assert!(result.is_ptr()); // Should be a pointer to the new string
    }
}
