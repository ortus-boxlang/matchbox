use matchbox_vm::types::{BxValue, BxVM, BxNativeFunction};
use std::collections::HashMap;

pub fn multiply(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("multiply requires 2 arguments".to_string());
    }
    let a = args[0].as_number();
    let b = args[1].as_number();
    Ok(BxValue::new_number(a * b))
}

pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("multiply".to_string(), multiply as BxNativeFunction);
    map
}