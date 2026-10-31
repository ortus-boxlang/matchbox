//! Thread-affine execution contexts retain the live scopes of their declaring call.
use super::VM;
use crate::types::{BxNativeObject, BxVM, BxValue, FunctionKind, NativeCallback, Tracer};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Debug)]
struct ExecutionContext {
    scopes: HashMap<String, BxValue>,
}

impl BxNativeObject for ExecutionContext {
    fn trace(&self, tracer: &mut dyn Tracer) {
        for value in self.scopes.values() {
            tracer.mark(value);
        }
    }
    fn native_data(&self, kind: &str) -> Option<*mut std::ffi::c_void> {
        (kind == "matchbox.context.v1").then_some(self as *const Self as *mut std::ffi::c_void)
    }
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn delegated_callback(&self, name: &str) -> Option<NativeCallback> {
        name.eq_ignore_ascii_case("invokeFunction").then(|| NativeCallback {
            parameters: vec!["__receiver".into(), "function".into(), "arguments".into()],
            callback: Rc::new(|vm, args| {
                let args = args.get(1..).ok_or("Execution context receiver missing")?;
                if args.is_empty() || args.len() > 2 || !vm.value_matches_type_name(args[0], "function") {
                    return Err("Context invokeFunction() requires a callable and optional array or struct arguments".into());
                }
                let (values, names) = if let Some(arguments) = args.get(1) {
                    let id = arguments.as_gc_id().ok_or("Context function arguments must be an array or struct")?;
                    if vm.is_array_value(*arguments) {
                        ((0..vm.array_len(id)).map(|index| vm.array_get(id, index)).collect(), None)
                    } else if vm.is_struct_value(*arguments) {
                        let names = vm.struct_key_array(id);
                        let values = names.iter().map(|name| vm.struct_get(id, name)).collect();
                        (values, Some(names))
                    } else { return Err("Context function arguments must be an array or struct".into()); }
                } else { (Vec::new(), None) };
                vm.call_function_by_value_named(args[0], values, names)
            }),
        })
    }
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_ascii_lowercase().as_str() {
            "getvisiblescopes" if args.is_empty() => {
                let contextual = vm.struct_new();
                for (name, value) in &self.scopes {
                    vm.struct_set(contextual, name, *value);
                }
                let lexical = BxValue::new_ptr(vm.struct_new());
                let scopes = vm.struct_new();
                vm.struct_set(scopes, "contextual", BxValue::new_ptr(contextual));
                vm.struct_set(scopes, "lexical", lexical);
                Ok(BxValue::new_ptr(scopes))
            }
            "getscopenearby" if args.len() == 1 && vm.is_string_value(args[0]) => {
                let name = vm.to_string(args[0]).to_ascii_lowercase();
                self.scopes
                    .get(&name)
                    .copied()
                    .ok_or_else(|| format!("Scope [{name}] is not available in this context"))
            }
            "getcomponents" if args.is_empty() => {
                let states = vm.execution_components()?;
                let array = vm.array_new();
                for state in states {
                    vm.array_push(array, state);
                }
                Ok(BxValue::new_ptr(array))
            }
            "findclosestcomponent"
                if (1..=2).contains(&args.len()) && vm.is_string_value(args[0]) =>
            {
                let offset = if let Some(offset) = args.get(1) {
                    if !(offset.is_number() || offset.is_int())
                        || !offset.as_number().is_finite()
                        || offset.as_number() < 0.0
                        || offset.as_number().fract() != 0.0
                    {
                        return Err("Component offset must be a nonnegative integer".into());
                    }
                    offset.as_number() as usize
                } else {
                    0
                };
                let name = vm.to_string(args[0]);
                Ok(vm
                    .execution_components()?
                    .into_iter()
                    .skip(offset)
                    .find(|state| {
                        let value = vm.struct_get(state.as_gc_id().unwrap(), "name");
                        vm.to_string(value).eq_ignore_ascii_case(&name)
                    })
                    .unwrap_or(BxValue::new_null()))
            }
            "writetobuffer" if args.len() == 1 || args.len() == 2 && args[1].is_bool() => {
                let text = vm.to_string(args[0]);
                vm.write_output(&text);
                Ok(BxValue::new_ptr(id))
            }
            _ => Err(format!(
                "Unsupported MatchBox execution context method or arguments: {name}"
            )),
        }
    }
}

impl VM {
    pub(crate) fn execution_context(&mut self) -> Result<BxValue, String> {
        let mut scopes = HashMap::new();
        if let Some(fiber) = self.current_fiber_idx {
            self.expose_frame_scopes(fiber);
            let frame = self.fibers[fiber]
                .frames
                .last()
                .ok_or("Execution context unavailable")?;
            if let Some(context) = frame.context {
                return Ok(context);
            }
            if frame.function.kind == FunctionKind::ComponentBody {
                let arguments =
                    self.fibers[fiber].stack[frame.stack_base + frame.function.params.len()];
                let context = self.struct_get(
                    arguments
                        .as_gc_id()
                        .ok_or("Component body context unavailable")?,
                    "1",
                );
                if !context.is_null() {
                    return Ok(context);
                }
                return Err("Component body context unavailable".into());
            }
            if let Some(arguments) = self.arguments_scope(fiber) {
                scopes.insert("arguments".into(), arguments);
                scopes.insert("local".into(), self.local_scope(fiber));
            }
        } else if let Some(context) = self.get_global("__mb_runtime_context") {
            return Ok(context);
        }
        let context = BxValue::new_ptr(
            self.native_object_new(Rc::new(RefCell::new(ExecutionContext { scopes }))),
        );
        if let Some(fiber) = self.current_fiber_idx {
            self.fibers[fiber].frames.last_mut().unwrap().context = Some(context);
        } else {
            self.insert_global("__mb_runtime_context".into(), context);
        }
        Ok(context)
    }
}
