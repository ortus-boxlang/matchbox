//! Lazy class initialization and VM-owned static scopes.
use super::{VM, gc::GcObject};
use crate::types::{BxClass, BxVM, BxValue};
use anyhow::Result;
use std::{cell::RefCell, rc::Rc};

/// Repeated constant promotions can produce different heap objects for one class definition.
fn scope_key(class: &BxClass) -> (String, String) {
    (
        class.constructor.chunk.filename.clone(),
        class.constructor.name.to_lowercase(),
    )
}

impl VM {
    /// Runs class initialization before construction or static dispatch. Returns the original
    /// failure on subsequent uses instead of rerunning a block with possible side effects.
    pub(super) fn initialize_class(
        &mut self,
        receiver: BxValue,
        class: &Rc<RefCell<BxClass>>,
        host_args: &[BxValue],
    ) -> Result<()> {
        if class.borrow().static_initializer().is_none() {
            return Ok(());
        }
        let key = scope_key(&class.borrow());
        if let Some(state) = self.class_scopes.get(&key) {
            return state
                .as_ref()
                .map(|_| ())
                .map_err(|message| anyhow::anyhow!("{message}"));
        }
        // Host arguments need to survive arbitrary allocation in the initializer.
        let _arguments = if host_args.is_empty() {
            None
        } else {
            Some(
                self.heap
                    .retain_values(host_args.to_vec())
                    .map_err(anyhow::Error::msg)?,
            )
        };
        let scope = BxValue::new_ptr(self.struct_new());
        // Make the scope visible to reentrant calls made by this initializer.
        self.class_scopes.insert(key.clone(), Ok(scope));
        let mut initializer = class.borrow().static_initializer().unwrap().clone();
        initializer.chunk = initializer.chunk.clone_without_runtime_caches();
        initializer.captured_receiver = Some(receiver);
        let function = BxValue::new_ptr(
            self.heap
                .alloc(GcObject::CompiledFunction(Rc::new(initializer))),
        );
        if let Err(error) = self.call_function_value(function, Vec::new(), None) {
            self.class_scopes.insert(key, Err(error.to_string()));
            return Err(error);
        }
        Ok(())
    }

    /// Initializes the instance's class, then its parents in JVM construction order.
    /// Propagates an initializer failure before running the instance constructor.
    pub(super) fn initialize_instance_classes(
        &mut self,
        receiver: BxValue,
        class: &Rc<RefCell<BxClass>>,
        host_args: &[BxValue],
    ) -> Result<()> {
        if class.borrow().modifiers.is_abstract { anyhow::bail!("Cannot instantiate abstract class {}", class.borrow().name); }
        if class.borrow().extends.is_some() { self.validate_abstract_contracts(class)?; }
        self.initialize_class(receiver, class, host_args)?;
        if class.borrow().extends.is_some() {
            for pair in self.class_chain(class).windows(2) {
                let parent_name = pair[0].borrow().extends.clone().unwrap();
                if let Some(parent) = self.get_global(&parent_name) {
                    self.initialize_class(parent, &pair[1], host_args)?;
                }
            }
        }
        Ok(())
    }

    /// Run ancestor pseudo-constructors on the new receiver, oldest ancestor first.
    /// This initializes inherited fields; it never calls an ancestor's `init` method.
    pub(super) fn initialize_inherited_fields(&mut self, instance: BxValue, class: &Rc<RefCell<BxClass>>, args: &[BxValue]) -> Result<()> {
        if class.borrow().extends.is_none() { return Ok(()); }
        let _roots = self.heap.retain_values(std::iter::once(instance).chain(args.iter().copied()).collect()).map_err(anyhow::Error::msg)?;
        for parent in self.class_chain(class).into_iter().skip(1).rev() {
            let mut constructor = parent.borrow().constructor.clone();
            constructor.captured_receiver = Some(instance);
            let function = BxValue::new_ptr(self.heap.alloc(GcObject::CompiledFunction(Rc::new(constructor))));
            self.call_function_value(function, vec![], None)?;
        }
        Ok(())
    }

    /// Validate the concrete receiver against the nearest inherited declaration of each
    /// abstract method. A public onMissingMethod supplies dynamic dispatch on BoxLang.
    fn validate_abstract_contracts(&self, class: &Rc<RefCell<BxClass>>) -> Result<()> {
        let chain = self.class_chain(class);
        if !chain.iter().any(|c| { let c = c.borrow(); c.modifiers.is_abstract || !c.inherited_contracts.is_empty() }) { return Ok(()); }
        if self.resolve_method(class.clone(), "onmissingmethod").is_some_and(|f| !f.modifiers.is_static && !f.modifiers.access.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("private"))) { return Ok(()); }
        let mut checked = std::collections::HashSet::new();
        for owner in &chain {
            let owner = owner.borrow();
            for contract in &owner.inherited_contracts {
                let implementation = chain.iter().find_map(|c| c.borrow().method_signatures().iter()
                    .find(|s| s.name == contract.name).map(|s| s.implements(contract)));
                if implementation != Some(true) {
                    anyhow::bail!("Class {} must implement interface method {} with a compatible signature", class.borrow().name, contract.name);
                }
            }
            for contract in owner.method_signatures().iter().filter(|s| s.is_abstract) {
                if !checked.insert(contract.name.clone()) { continue; }
                let compatible = chain.iter().find_map(|c| c.borrow().method_signatures().iter()
                    .find(|s| s.name == contract.name).map(|s| s.implements(contract)));
                if compatible != Some(true) {
                    anyhow::bail!("Class {} does not implement abstract method {} from {} with a compatible signature", class.borrow().name, contract.name, owner.name);
                }
            }
        }
        Ok(())
    }

    /// Returns the named lexical scope. Methods inherited by a child retain the parent's scope.
    pub(super) fn lexical_static_scope(
        &mut self,
        filename: String,
        name: String,
    ) -> Result<BxValue> {
        let key = (filename, name);
        if let Some(state) = self.class_scopes.get(&key) {
            return state.clone().map_err(anyhow::Error::msg);
        }
        let scope = BxValue::new_ptr(self.struct_new());
        self.class_scopes.insert(key, Ok(scope));
        Ok(scope)
    }

    /// Reads a static field or returns a bound method. Propagates initializer and lookup errors.
    pub(super) fn class_member_value(
        &mut self,
        receiver: BxValue,
        class: Rc<RefCell<BxClass>>,
        name: &str,
    ) -> Result<BxValue> {
        self.initialize_class(receiver, &class, &[])?;
        if let Some(Ok(scope)) = self.class_scopes.get(&scope_key(&class.borrow())) {
            let scope = scope.as_gc_id().unwrap();
            if self.struct_key_exists(scope, name) {
                return Ok(self.struct_get(scope, name));
            }
        }
        self.method_reference(receiver, name, &[])
    }

    /// Returns the current receiver's static scope, or none when there is no class receiver.
    pub(super) fn frame_class_scope(&mut self, fiber: usize) -> Option<BxValue> {
        let receiver = self.fibers[fiber].frames.last()?.receiver?.as_gc_id()?;
        let key = match self.heap.get_opt(receiver)? {
            GcObject::Class(class) => scope_key(&class.borrow()),
            GcObject::Instance(instance) => scope_key(&instance.class.borrow()),
            _ => return None,
        };
        self.class_scopes.get(&key)?.as_ref().ok().copied()
    }
}
