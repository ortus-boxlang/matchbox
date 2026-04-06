use matchbox_vm::types::{BxValue, BxVM, BxNativeFunction, BxNativeObject};
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;

#[derive(Debug)]
pub struct Vault {
    secret_key: String,
}

impl BxNativeObject for Vault {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        let name_lower = name.to_lowercase();
        match name_lower.as_str() {
            "encrypt" => {
                if args.len() != 1 {
                    return Err("encrypt requires 1 argument".to_string());
                }
                let input = vm.to_string(args[0]);
                // Dummy encryption for test: "ENCRYPTED(input)_WITH(key)"
                let result = format!("ENCRYPTED({})_WITH({})", input, self.secret_key);
                let id = vm.string_new(result);
                Ok(BxValue::new_ptr(id))
            }
            _ => Err(format!("Method {} not found on Vault", name)),
        }
    }
}

pub fn create_vault(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("Vault constructor requires 1 argument (secret_key)".to_string());
    }
    let secret_key = vm.to_string(args[0]);
    let vault = Vault { secret_key };
    let id = vm.native_object_new(Rc::new(RefCell::new(vault)));
    Ok(BxValue::new_ptr(id))
}

pub fn register_classes() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("crypto.Vault".to_string(), create_vault as BxNativeFunction);
    map
}
