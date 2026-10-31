//! Module-owned class resolution. Providers use ordinary VM callables and objects.
use super::{VM, gc::GcObject};
use crate::types::{BxNativeObject, BxVM, BxValue, NativeCallback, Tracer};
use anyhow::{Context, Result, bail};
use std::{cell::RefCell, rc::Rc};

const PROVIDERS: &str = "__mb_class_providers";

#[derive(Debug)]
struct ClassReference {
    name: String,
    target: BxValue,
    instance: Rc<RefCell<Option<BxValue>>>,
}
impl BxNativeObject for ClassReference {
    fn class_reference_target(&self) -> Option<BxValue> {
        Some(self.target)
    }
    fn class_reference_instance(&self) -> Option<BxValue> {
        *self.instance.borrow()
    }
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn delegated_callback(&self, method: &str) -> Option<NativeCallback> {
        let target = self.target;
        let class = self.name.clone();
        let method = method.to_owned();
        let instance = self.instance.clone();
        Some(NativeCallback {
            parameters: vec!["receiver".into()],
            callback: Rc::new(move |vm, args| {
                let chunk = vm
                    .current_chunk()
                    .unwrap_or_else(|| Rc::new(RefCell::new(crate::Chunk::default())));
                let is_init = method.eq_ignore_ascii_case("init");
                let initialized = *instance.borrow();
                let receiver = if !is_init {
                    initialized
                        .filter(|value| vm.has_method_by_value(*value, &method))
                        .unwrap_or(target)
                } else {
                    target
                };
                let result =
                    vm.call_method_by_value(receiver, &method, args[1..].to_vec(), chunk)?;
                if is_init {
                    if result.is_null() {
                        return Err(format!("Class provider init() returned null: {class}"));
                    }
                    *instance.borrow_mut() = Some(result);
                }
                Ok(result)
            }),
        })
    }
    fn call_method(
        &mut self,
        _vm: &mut dyn BxVM,
        _id: usize,
        _name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        Err("Class reference dispatch unavailable".into())
    }
    fn trace(&self, tracer: &mut dyn Tracer) {
        tracer.mark(&self.target);
        if let Some(value) = *self.instance.borrow() {
            tracer.mark(&value);
        }
    }
}

impl VM {
    pub(super) fn class_reference_target(&self, value: BxValue) -> Option<BxValue> {
        let GcObject::NativeObject(object) = self.heap.get_opt(value.as_gc_id()?)? else {
            return None;
        };
        object.borrow().class_reference_target()
    }

    fn class_reference_instance(&self, value: BxValue) -> Option<BxValue> {
        let GcObject::NativeObject(object) = self.heap.get_opt(value.as_gc_id()?)? else {
            return None;
        };
        object.borrow().class_reference_instance()
    }

    fn wrap_class_reference(&mut self, name: &str, target: BxValue) -> BxValue {
        BxValue::new_ptr(self.native_object_new(Rc::new(RefCell::new(ClassReference {
            name: name.into(),
            target,
            instance: Rc::new(RefCell::new(None)),
        }))))
    }

    pub(super) fn resolve_class_reference(&mut self, name: &str) -> Result<BxValue> {
        if let Some((prefix, class)) = name.split_once(':') {
            if prefix.eq_ignore_ascii_case("java") {
                if let Some(value) = self.provided_class(&format!("java:{class}"))? {
                    return Ok(value);
                }
                // A module-owned Java namespace must not silently fall back to a JVM.
                if self
                    .get_global(PROVIDERS)
                    .and_then(|value| value.as_gc_id())
                    .is_some_and(|id| self.struct_len(id) > 0)
                {
                    bail!("Java class not provided: {class}");
                }
                let target =
                    crate::bifs::jni_class_reference(self, class).map_err(anyhow::Error::msg)?;
                return Ok(self.wrap_class_reference(name, target));
            }
            if prefix.eq_ignore_ascii_case("bx") {
                return self
                    .get_global(class)
                    .filter(|value| {
                        matches!(
                            value.as_gc_id().and_then(|id| self.heap.get_opt(id)),
                            Some(GcObject::Class(_))
                        )
                    })
                    .with_context(|| format!("BoxLang class not found: {class}"));
            }
            bail!("Unsupported class resolver prefix: {prefix}");
        }
        if let Some(value) = self.get_global(name).filter(|value| {
            matches!(
                value.as_gc_id().and_then(|id| self.heap.get_opt(id)),
                Some(GcObject::Class(_))
            )
        }) {
            return Ok(value);
        }
        self.provided_class(&format!("java:{name}"))?
            .with_context(|| format!("Class not found: {name}"))
    }

    pub(super) fn class_reference_member(
        &mut self,
        receiver: BxValue,
        name: &str,
    ) -> Result<BxValue> {
        let target = self
            .class_reference_target(receiver)
            .context("Not a provided class reference")?;
        if let Some(instance) = self.class_reference_instance(receiver) {
            if let Some(value) = self.instance_property(instance, name) {
                return Ok(value);
            }
            if let Some(GcObject::NativeObject(object)) =
                instance.as_gc_id().and_then(|id| self.heap.get_opt(id))
            {
                let value = object.borrow().get_property(name);
                if !value.is_null() {
                    return Ok(value);
                }
            }
        }
        if let Some(value) = self.instance_property(target, name) {
            return Ok(value);
        }
        let has_instance_method = self
            .class_reference_instance(receiver)
            .is_some_and(|value| self.has_method_by_value(value, name));
        let has_method = has_instance_method
            || match self.heap.get(
                target
                    .as_gc_id()
                    .context("Invalid class reference target")?,
            ) {
                GcObject::Instance(instance) => {
                    self.resolve_method(instance.class.clone(), name).is_some()
                }
                GcObject::NativeObject(object) => {
                    let object = object.borrow();
                    let value = object.get_property(name);
                    if !value.is_null() {
                        return Ok(value);
                    }
                    object.delegated_callback(name).is_some() || name.eq_ignore_ascii_case("init")
                }
                _ => false,
            };
        if !has_method {
            bail!("Provided class member not found: {name}");
        }
        let root = self
            .heap
            .retain_value(receiver)
            .map_err(anyhow::Error::msg)?;
        let name = name.to_owned();
        let callback = NativeCallback {
            // Java methods/constructors do not accept named arguments.
            parameters: vec![],
            callback: Rc::new(move |vm, args| {
                let receiver = vm.read_retained_value(&root)?;
                let chunk = vm
                    .current_chunk()
                    .unwrap_or_else(|| Rc::new(RefCell::new(crate::Chunk::default())));
                vm.call_method_by_value(receiver, &name, args.to_vec(), chunk)
            }),
        };
        Ok(BxValue::new_ptr(
            self.heap.alloc(GcObject::NativeCallback(callback)),
        ))
    }

    pub(super) fn class_reference_assignment_target(
        &self,
        value: BxValue,
        name: &str,
    ) -> Result<BxValue> {
        if let Some(target) = self.class_reference_target(value) {
            if let Some(instance) = self
                .class_reference_instance(value)
                .filter(|instance| self.instance_property(*instance, name).is_some())
            {
                return Ok(instance);
            }
            if self.instance_property(target, name).is_none() {
                bail!("Provided class field is missing or read-only: {name}");
            }
            Ok(target)
        } else {
            Ok(value)
        }
    }

    pub(crate) fn register_class_providers(
        &mut self,
        owner: BxValue,
        providers: BxValue,
    ) -> Result<()> {
        if !self.is_array_value(providers) {
            bail!("Module classProviders must be an array");
        }
        let registry = match self.get_global(PROVIDERS) {
            Some(value) => value
                .as_gc_id()
                .context("Invalid class provider registry")?,
            None => {
                let id = self.struct_new_with_options(&["casesensitive".into()]);
                self.insert_global(PROVIDERS.into(), BxValue::new_ptr(id));
                id
            }
        };
        let providers = providers.as_gc_id().unwrap();
        for index in 0..self.array_len(providers) {
            let entry = self.array_get(providers, index);
            if !self.is_struct_value(entry) {
                bail!("Class provider must be a struct");
            }
            let entry = entry.as_gc_id().unwrap();
            let name = self.struct_get(entry, "name");
            if !self.is_string_value(name) {
                bail!("Class provider name must be a string");
            }
            let name = self.to_string(name);
            let Some((prefix, class)) = name.split_once(':') else {
                bail!("Class provider name requires a resolver prefix");
            };
            if !prefix.eq_ignore_ascii_case("java")
                || class.trim().is_empty()
                || class != class.trim()
                || class.contains([':', '*', '@'])
            {
                bail!("Class providers currently require an exact java: class name");
            }
            let name = format!("java:{class}");
            if self.struct_key_exists(registry, &name) {
                bail!("Class provider already registered: {name}");
            }
            let provider = self.struct_get(entry, "provider");
            if !matches!(
                provider.as_gc_id().and_then(|id| self.heap.get_opt(id)),
                Some(
                    GcObject::CompiledFunction(_)
                        | GcObject::NativeFunction(_)
                        | GcObject::NativeCallback(_)
                )
            ) {
                bail!("Class provider must be callable: {name}");
            }
            let registration = self.struct_new();
            self.struct_set(registration, "owner", owner);
            self.struct_set(registration, "provider", provider);
            self.struct_set(registry, &name, BxValue::new_ptr(registration));
        }
        Ok(())
    }

    pub(crate) fn unregister_class_providers(&mut self, owner: BxValue) {
        let Some(registry) = self
            .get_global(PROVIDERS)
            .and_then(|value| value.as_gc_id())
        else {
            return;
        };
        for name in self.struct_key_array(registry) {
            let registration = self.struct_get(registry, &name).as_gc_id().unwrap();
            if self.struct_get(registration, "owner") == owner {
                self.struct_delete(registry, &name);
            }
        }
    }

    fn provided_class(&mut self, name: &str) -> Result<Option<BxValue>> {
        let Some(registry) = self
            .get_global(PROVIDERS)
            .and_then(|value| value.as_gc_id())
        else {
            return Ok(None);
        };
        if !self.struct_key_exists(registry, name) {
            return Ok(None);
        }
        let registration = self.struct_get(registry, name).as_gc_id().unwrap();
        let provider = self.struct_get(registration, "provider");
        let _root = self
            .heap
            .retain_value(provider)
            .map_err(anyhow::Error::msg)?;
        let value = self.call_function_value(provider, vec![], None)?;
        if !matches!(
            value.as_gc_id().and_then(|id| self.heap.get_opt(id)),
            Some(GcObject::Class(_) | GcObject::Instance(_) | GcObject::NativeObject(_))
        ) {
            bail!("Class provider did not return a class reference: {name}");
        }
        // A BoxLang façade needs its pseudo-constructor, but init is the provided class constructor.
        let _value_root = self.heap.retain_value(value).map_err(anyhow::Error::msg)?;
        let value = if matches!(self.heap.get(value.as_gc_id().unwrap()), GcObject::Class(_)) {
            self.construct_class_value(value, vec![])?
        } else {
            self.class_reference_target(value).unwrap_or(value)
        };
        Ok(Some(self.wrap_class_reference(name, value)))
    }
}
