//! Shared compiled-function argument binding, including undeclared arguments.
use super::{VM, gc::GcObject};
use crate::types::{BxCompiledFunction, BxVM, BxValue};
use std::rc::Rc;

impl VM {
    pub(super) fn function_arguments(
        &mut self,
        args: Vec<BxValue>,
        names: Option<&[String]>,
        function: &BxCompiledFunction,
    ) -> Vec<BxValue> {
        // Synthetic script/constructor/accessor functions have no implicit arguments local.
        if function.arity as usize != function.params.len() + 1 {
            let mut args = args;
            args.resize(function.arity as usize, BxValue::new_null());
            return args;
        }
        let mut values = vec![BxValue::new_null(); function.params.len() + 1];
        let scope = self.struct_new();
        let mut positional = 0;
        for (index, value) in args.into_iter().enumerate() {
            let name = names
                .and_then(|names| names.get(index))
                .filter(|name| !name.is_empty());
            let (key, slot) = if let Some(name) = name {
                (
                    name.clone(),
                    function
                        .params
                        .iter()
                        .position(|parameter| parameter.eq_ignore_ascii_case(name)),
                )
            } else {
                let slot = positional;
                positional += 1;
                (
                    function
                        .params
                        .get(slot)
                        .cloned()
                        .unwrap_or_else(|| positional.to_string()),
                    Some(slot).filter(|slot| *slot < function.params.len()),
                )
            };
            if let Some(slot) = slot {
                values[slot] = value;
            }
            self.struct_set(scope, &key, value);
        }
        if let Some((name, member)) = &function.bif {
            let name = BxValue::new_ptr(self.string_new(name.clone()));
            self.struct_set(scope, "__functionName", name);
            self.struct_set(scope, "__isMemberExecution", BxValue::new_bool(*member));
        }
        values[function.params.len()] = BxValue::new_ptr(scope);
        values
    }

    pub(crate) fn bif_function(&mut self, value: BxValue, name: &str, member: bool) -> BxValue {
        let Some(GcObject::CompiledFunction(function)) =
            value.as_gc_id().and_then(|id| self.heap.get_opt(id))
        else {
            return value;
        };
        let mut function = (**function).clone();
        function.bif = Some((name.into(), member));
        BxValue::new_ptr(
            self.heap
                .alloc(GcObject::CompiledFunction(Rc::new(function))),
        )
    }
}
