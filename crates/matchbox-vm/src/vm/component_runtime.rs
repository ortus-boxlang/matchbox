//! Compiler component callbacks and body results shared by native and WASM hosts.
use super::VM;
use crate::types::{BxNativeObject, BxVM, BxValue, NativeCallback, Tracer};
use anyhow::Result;
use std::{cell::RefCell, rc::Rc};

#[derive(Debug)]
struct BodyResult {
    kind: u8,
    value: BxValue,
}
impl BxNativeObject for BodyResult {
    fn trace(&self, tracer: &mut dyn Tracer) {
        tracer.mark(&self.value);
    }
    fn native_data(&self, kind: &str) -> Option<*mut std::ffi::c_void> {
        (kind == "matchbox.component.body_result")
            .then_some(self as *const Self as *mut std::ffi::c_void)
    }
    fn get_property(&self, name: &str) -> BxValue {
        match name.to_ascii_lowercase().as_str() {
            "resulttype" => BxValue::new_number(self.kind as f64),
            "returnvalue" => self.value,
            _ => BxValue::new_null(),
        }
    }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn call_method(
        &mut self,
        _vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        let name = name.to_ascii_lowercase();
        let has_label = matches!(name.as_str(), "isbreak" | "iscontinue") && args.len() == 1;
        if !args.is_empty() && !has_label {
            return Err("Invalid component body result arguments".into());
        }
        match name.as_str() {
            "isearlyexit" => Ok(BxValue::new_bool(self.kind != 0)),
            "isreturn" => Ok(BxValue::new_bool(self.kind == 1)),
            "isbreak" => Ok(BxValue::new_bool(self.kind == 2)),
            "iscontinue" => Ok(BxValue::new_bool(self.kind == 3)),
            "returnvalue" => Ok(self.value),
            "label" => Ok(BxValue::new_null()),
            "resulttype" => Ok(BxValue::new_number(self.kind as f64)),
            _ => Err(format!("Unsupported component body result method: {name}")),
        }
    }
}
#[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
pub(super) fn default_body_result(vm: &mut dyn BxVM) -> BxValue {
    BxValue::new_ptr(vm.native_object_new(Rc::new(RefCell::new(BodyResult {
        kind: 0,
        value: BxValue::new_null(),
    }))))
}

fn body_result(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 || !(args[0].is_number() || args[0].is_int()) {
        return Err("Body result requires a result type and value".into());
    }
    let kind = args[0].as_number();
    if !kind.is_finite()
        || kind.fract() != 0.0
        || !(0.0..=3.0).contains(&kind)
        || (kind != 1.0 && !args[1].is_null())
    {
        return Err("Invalid component body result".into());
    }
    Ok(BxValue::new_ptr(vm.native_object_new(Rc::new(
        RefCell::new(BodyResult {
            kind: kind as u8,
            value: args[1],
        }),
    ))))
}

impl VM {
    /// Install the callbacks emitted by the compiler for component bodies and dispatch.
    pub fn register_component_runtime(&mut self) -> Result<()> {
        self.register_callback(
            "__mb_body_result",
            NativeCallback {
                parameters: vec!["type".into(), "value".into()],
                callback: Rc::new(body_result),
            },
        )
        .map_err(anyhow::Error::msg)?;
        self.register_callback(
            "__mb_component",
            NativeCallback {
                parameters: vec!["name".into(), "attributes".into(), "body".into()],
                callback: Rc::new(|vm, args| {
                    if args.len() != 3 {
                        return Err(
                            "Component invocation requires name, attributes and body".into()
                        );
                    }
                    let name = vm.to_string(args[0]);
                    vm.invoke_component(&name, args[1], args[2])
                }),
            },
        )
        .map_err(anyhow::Error::msg)?;
        Ok(())
    }
}
