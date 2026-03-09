use matchbox_vm::{matchbox_fn, matchbox_class, matchbox_methods, types::{BxValue, BxVM, BxNativeFunction, BxNativeObject}};
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

#[matchbox_fn]
pub fn macro_add(a: f64, b: f64) -> f64 {
    a + b
}

#[matchbox_class]
#[derive(Debug)]
pub struct NativeCalc {
    pub base: f64,
}

#[matchbox_methods]
impl NativeCalc {
    pub fn add(&self, value: f64) -> f64 {
        self.base + value
    }

    pub fn multiply(&self, value: f64) -> f64 {
        self.base * value
    }
}

pub fn create_calc(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("NativeCalc requires 1 argument (base)".to_string());
    }
    let base = args[0].as_number();
    let calc = NativeCalc { base };
    let id = vm.native_object_new(Rc::new(RefCell::new(calc)));
    Ok(BxValue::new_ptr(id))
}

pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("macro_add".to_string(), macro_add_wrapper as BxNativeFunction);
    map
}

pub fn register_classes() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("macro_module.NativeCalc".to_string(), create_calc as BxNativeFunction);
    map
}
