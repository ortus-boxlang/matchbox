use base64::{engine::general_purpose::STANDARD, Engine};
use matchbox_vm::types::{BxNativeFunction, BxVM, BxValue};
use std::collections::HashMap;

pub fn base64encode_native(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let text = args.first().map(|v| vm.to_string(*v)).ok_or("base64encode_native requires 1 argument")?;
    Ok(BxValue::new_ptr(vm.string_new(STANDARD.encode(text))))
}

pub fn register_bifs() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("base64encode_native".to_string(), base64encode_native as BxNativeFunction);
    map
}
