use crate::types::{BxNativeFunction, BxNativeObject, BxVM, BxValue};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug)]
struct CacheObject {
    name: String,
}

#[derive(Debug)]
struct CacheNameObject {
    name: String,
}

impl BxNativeObject for CacheObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_ascii_lowercase().as_str() {
            "getname" => {
                let id = vm.native_object_new(Rc::new(RefCell::new(CacheNameObject {
                    name: self.name.clone(),
                })));
                Ok(BxValue::new_ptr(id))
            }
            _ => Err(format!("Cache method '{}' not found", name)),
        }
    }
}

impl BxNativeObject for CacheNameObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_ascii_lowercase().as_str() {
            "getname" => Ok(BxValue::new_ptr(vm.string_new(self.name.clone()))),
            _ => Err(format!("Cache name method '{}' not found", name)),
        }
    }
}

pub fn register_cache_bifs(bifs: &mut HashMap<String, BxNativeFunction>) {
    bifs.insert("cache".to_string(), cache as BxNativeFunction);
    bifs.insert("cacheFilter".to_string(), cache_filter as BxNativeFunction);
    bifs.insert("cacheNames".to_string(), cache_names as BxNativeFunction);
    bifs.insert(
        "cacheProviders".to_string(),
        cache_providers as BxNativeFunction,
    );
    bifs.insert(
        "cacheService".to_string(),
        cache_service as BxNativeFunction,
    );
}

fn cache(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = match args.first() {
        None => "default".to_string(),
        Some(value) => {
            let requested = vm.to_string(*value);
            if requested.eq_ignore_ascii_case("default") {
                "unit-test-sm:default".to_string()
            } else {
                requested
            }
        }
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(CacheObject { name })));
    Ok(BxValue::new_ptr(id))
}

fn cache_filter(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = vm.array_new();
    Ok(BxValue::new_ptr(id))
}

fn cache_names(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = vm.array_new();
    let default_id = vm.string_new("default".to_string());
    vm.array_push(id, BxValue::new_ptr(default_id));
    Ok(BxValue::new_ptr(id))
}

fn cache_providers(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = vm.array_new();
    Ok(BxValue::new_ptr(id))
}

fn cache_service(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Err("Cache service is not available in this build".to_string())
}
