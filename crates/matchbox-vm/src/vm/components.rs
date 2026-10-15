//! Module components use ordinary, GC-traced instances and registration records.
use super::{VM, gc::GcObject, component_runtime::default_body_result};
use crate::{
    bundle::Component,
    types::{BxVM, BxValue, NativeCallback},
};
use anyhow::{Context, Result, bail};
use std::rc::Rc;

const COMPONENTS: &str = "__mb_components";
const COMPONENT_STACK: &str = "__mb_component_stack";

pub(crate) fn inject_helpers(vm: &mut VM, target: BxValue, name: &str) -> Result<()> {
    let name = name.to_owned();
    let callback = NativeCallback {
        parameters: vec![],
        callback: Rc::new(move |vm, args| {
            if !args.is_empty() {
                return Err("getName() takes no arguments".into());
            }
            Ok(BxValue::new_ptr(vm.string_new(name.clone())))
        }),
    };
    let value = BxValue::new_ptr(vm.heap.alloc(GcObject::NativeCallback(callback)));
    vm.set_instance_variable(target, "getName", value)?;
    for (name, callback) in [
        (
            "newBuffer",
            NativeCallback {
                parameters: vec![],
                callback: Rc::new(|vm, args| {
                    if !args.is_empty() {
                        return Err("newBuffer() takes no arguments".into());
                    }
                    Ok(crate::bifs::string_buffer_new(vm))
                }),
            },
        ),
        (
            "newBuilder",
            NativeCallback {
                parameters: vec![],
                callback: Rc::new(|vm, args| {
                    if !args.is_empty() {
                        return Err("newBuilder() takes no arguments".into());
                    }
                    crate::bifs::string_builder_new_bif(vm, &[])
                }),
            },
        ),
        (
            "processBody",
            NativeCallback {
                parameters: vec!["context".into(), "body".into(), "buffer".into()],
                callback: Rc::new(process_body),
            },
        ),
    ] {
        let value = BxValue::new_ptr(vm.heap.alloc(GcObject::NativeCallback(callback)));
        vm.set_instance_variable(target, name, value)?;
    }
    Ok(())
}

fn process_body(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 3 {
        return Err("processBody() requires context, body and buffer".into());
    }
    if vm
        .native_object_data(args[0], "matchbox.context.v1")
        .is_none()
    {
        return Err("processBody() requires an execution context".into());
    }
    if args[1].is_null() {
        return Ok(default_body_result(vm));
    }
    if !vm.value_matches_type_name(args[1], "function") {
        return Err("processBody() body must be callable".into());
    }
    let chunk = vm
        .current_chunk()
        .ok_or("Component body context unavailable")?;
    let capture = vm
        .native_object_data(args[2], "matchbox.component.buffer")
        .is_some();
    if capture {
        vm.begin_output_capture();
    }
    let result = vm.call_function_by_value(&args[1], vec![args[0]], chunk);
    if capture {
        let output = vm.end_output_capture().unwrap_or_default();
        let text = BxValue::new_ptr(vm.string_new(output.clone()));
        vm.native_object_call_method(args[2].as_gc_id().unwrap(), "append", &[text])?;
        if result.is_err() {
            vm.write_output(&output);
        }
    }
    let result = result?;
    if result.is_null() {
        return Ok(default_body_result(vm));
    }
    if vm
        .native_object_data(result, "matchbox.component.body_result")
        .is_none()
    {
        return Err("Component body returned an unsupported early-exit result".into());
    }
    Ok(result)
}

impl VM {
    pub(crate) fn register_component(
        &mut self,
        target: BxValue,
        component: &Component,
    ) -> Result<()> {
        if !self.instance_has_method(target, "invoke") {
            bail!("Module component {} needs invoke()", component.class);
        }
        let registry = self
            .get_global(COMPONENTS)
            .unwrap_or_else(|| self.insert_empty_struct_global(COMPONENTS))
            .as_gc_id()
            .unwrap();
        let record = self.struct_new();
        let name = BxValue::new_ptr(
            self.string_new(component.class.rsplit('.').next().unwrap().to_string()),
        );
        self.struct_set(record, "name", name);
        self.struct_set(record, "target", target);
        self.struct_set(
            record,
            "allowsBody",
            BxValue::new_bool(component.allows_body || component.requires_body),
        );
        self.struct_set(
            record,
            "requiresBody",
            BxValue::new_bool(component.requires_body),
        );
        for alias in &component.aliases {
            self.struct_set(registry, alias, BxValue::new_ptr(record));
        }
        Ok(())
    }

    pub(crate) fn execution_components(&self) -> Vec<BxValue> {
        self.get_global(COMPONENT_STACK)
            .and_then(|value| value.as_gc_id())
            .map(|id| {
                (0..self.array_len(id))
                    .rev()
                    .map(|index| self.array_get(id, index))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn component_names(&self) -> Vec<String> {
        self.get_global(COMPONENTS)
            .and_then(|value| value.as_gc_id())
            .map(|id| self.struct_key_array(id))
            .unwrap_or_default()
    }

    pub(crate) fn invoke_component(
        &mut self,
        name: &str,
        attributes: BxValue,
        body: BxValue,
    ) -> Result<BxValue> {
        if !self.is_struct_value(attributes) {
            bail!("Component attributes must be a struct");
        }
        let registry = self
            .get_global(COMPONENTS)
            .and_then(|value| value.as_gc_id())
            .with_context(|| format!("Component not registered: {name}"))?;
        let record = self
            .struct_get(registry, name)
            .as_gc_id()
            .with_context(|| format!("Component not registered: {name}"))?;
        if body.is_null() && self.struct_get(record, "requiresBody").as_bool() {
            bail!("Component {name} requires a body");
        }
        if !body.is_null() && !self.struct_get(record, "allowsBody").as_bool() {
            bail!("Component {name} does not allow a body");
        }
        let body = if body.is_null() {
            let callback = NativeCallback {
                parameters: vec!["context".into()],
                callback: Rc::new(|vm, _| Ok(default_body_result(vm))),
            };
            BxValue::new_ptr(self.heap.alloc(GcObject::NativeCallback(callback)))
        } else {
            body
        };
        let target = self.struct_get(record, "target");
        let context = self.execution_context().map_err(anyhow::Error::msg)?;
        let state = self.struct_new();
        let registered_name = self.struct_get(record, "name");
        self.struct_set(state, "name", registered_name);
        self.struct_set(state, "attributes", attributes);
        let collection = BxValue::new_ptr(self.struct_new());
        self.struct_set(state, "dataCollection", collection);
        let stack = if let Some(stack) = self.get_global(COMPONENT_STACK) {
            stack.as_gc_id().unwrap()
        } else {
            let stack = BxValue::new_ptr(self.array_new());
            self.insert_global(COMPONENT_STACK.into(), stack);
            stack.as_gc_id().unwrap()
        };
        self.array_push(stack, BxValue::new_ptr(state));
        let result = self.call_method_value_named(
            target,
            "invoke",
            vec![context, attributes, body, BxValue::new_ptr(state)],
            vec![
                "context".into(),
                "attributes".into(),
                "body".into(),
                "executionState".into(),
            ],
        );
        self.array_pop(stack).expect("balanced component stack");
        let result = result?;
        if !result.is_null()
            && !result.as_gc_id().is_some_and(|id| match self.heap.get(id) {
                GcObject::NativeObject(object) => object
                    .borrow()
                    .native_data("matchbox.component.body_result")
                    .is_some(),
                _ => false,
            })
        {
            bail!("Module component invoke() must return a body result or null");
        }
        Ok(if result.is_null() {
            default_body_result(self)
        } else {
            result
        })
    }
}
