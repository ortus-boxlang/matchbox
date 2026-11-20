//! Shared compiled-function argument binding, including undeclared arguments.
use super::{VM, gc::GcObject};
use crate::types::{BxCompiledFunction, BxVM, BxValue};
use std::rc::Rc;

impl VM {
    /// Builds the parameter metadata available from a compiled callable's `$bx.meta`.
    /// This is declaration data, not the most recent invocation's arguments.
    pub(super) fn callable_parameter_metadata(&mut self, function: &BxCompiledFunction) -> Result<BxValue, String> {
        let Some(crate::types::Constant::MethodSignatures(signatures)) = function.chunk.constants().last() else {
            return Err("Callable parameter metadata is unavailable; recompile the defining source".into());
        };
        let signature = signatures.first().ok_or("Missing callable declaration")?;
        let parameters = self.array_new();
        for (name, ty, required) in &signature.parameters {
            let row = self.struct_new();
            let name = BxValue::new_ptr(self.string_new(name.clone()));
            let ty = BxValue::new_ptr(self.string_new(ty.clone()));
            self.struct_set(row, "name", name);
            self.struct_set(row, "type", ty);
            self.struct_set(row, "required", BxValue::new_bool(*required));
            self.array_push(parameters, BxValue::new_ptr(row));
        }
        let metadata = self.struct_new();
        self.struct_set(metadata, "parameters", BxValue::new_ptr(parameters));
        let wrapper = self.struct_new();
        self.struct_set(wrapper, "meta", BxValue::new_ptr(metadata));
        Ok(BxValue::new_ptr(wrapper))
    }

    pub(super) fn stack_function_arguments(
        &mut self,
        fiber: usize,
        start: usize,
        function: &BxCompiledFunction,
    ) {
        let scope = self.struct_new();
        let count = self.fibers[fiber].stack.len() - start;
        for index in 0..count {
            let value = self.fibers[fiber].stack[start + index];
            if let Some(name) = function.params.get(index) {
                self.struct_set(scope, name, value);
            } else {
                self.struct_set(scope, &(index + 1).to_string(), value);
            }
        }
        let stack = &mut self.fibers[fiber].stack;
        stack.resize(start + function.params.len(), BxValue::new_null());
        stack.push(BxValue::new_ptr(scope));
    }

    /// Copies collection-supplied arguments into the new frame. Only arrays and structs
    /// expand; other values remain ordinary arguments named `argumentCollection` on the JVM.
    fn bind_argument_collection(&mut self, source: BxValue, params: &[String], values: &mut [BxValue], scope: usize) -> bool {
        let Some(id) = source.as_gc_id() else { return false };
        if self.is_array_value(source) {
            for index in 0..self.array_len(id) {
                let value = self.array_get(id, index);
                if let Some(key) = params.get(index) {
                    values[index] = value;
                    self.struct_set(scope, key, value);
                } else {
                    self.struct_set(scope, &(index + 1).to_string(), value);
                }
            }
        } else if self.is_struct_value(source) {
            for key in self.struct_key_array(id) {
                let value = self.struct_get(id, &key);
                let slot = params.iter().position(|p| p.eq_ignore_ascii_case(&key)).or_else(|| {
                    key.parse::<usize>().ok()?.checked_sub(1).filter(|&index| index < params.len())
                });
                if let Some(index) = slot {
                    values[index] = value;
                    self.struct_set(scope, &params[index], value);
                } else {
                    self.struct_set(scope, &key, value);
                }
            }
        } else { return false; }
        true
    }

    /// Binds supplied arguments to the declared slots and the script-visible arguments scope.
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
        if names.is_none() && function.bif.is_none() {
            let scope = self.struct_new();
            for (index, &value) in args.iter().enumerate() {
                if let Some(name) = function.params.get(index) {
                    self.struct_set(scope, name, value);
                } else {
                    self.struct_set(scope, &(index + 1).to_string(), value);
                }
            }
            let mut values = args;
            values.resize(function.params.len() + 1, BxValue::new_null());
            values[function.params.len()] = BxValue::new_ptr(scope);
            return values;
        }
        let mut values = vec![BxValue::new_null(); function.params.len() + 1];
        let scope = self.struct_new();
        // Explicit arguments override the collection regardless of their textual order.
        let expanded = names.and_then(|names| names.iter().position(|name| name.eq_ignore_ascii_case("argumentCollection")))
            .filter(|&index| args.get(index).is_some_and(|&source| self.bind_argument_collection(source, &function.params, &mut values, scope)));
        let mut positional = 0;
        for (index, value) in args.into_iter().enumerate() {
            if expanded == Some(index) { continue; }
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
