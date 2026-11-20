//! Checked script assignment for host-backed struct scopes; host population remains side-effect-free.
use super::{VM, gc::GcObject};
use crate::types::{BxVM, BxValue, NativeCallback};
impl VM {
    pub fn watch_struct_assignments(
        &mut self,
        id: usize,
        callback: NativeCallback,
    ) -> Result<(), String> {
        if !matches!(self.heap.get_opt(id), Some(GcObject::Struct(_))) {
            return Err("Assignment hooks require a struct".into());
        }
        let root = self.heap.retain_value(BxValue::new_ptr(id))?;
        self.struct_assignment_hooks.insert(id, (root, callback));
        Ok(())
    }
    /// Returns true for receivers whose bytecode stores must not use a shape-only fast path.
    pub(super) fn requires_checked_assignment(&self, id: usize) -> bool {
        self.struct_assignment_hooks.contains_key(&id)
            || matches!(self.heap.get_opt(id), Some(GcObject::Struct(scope)) if scope.final_keys.is_some())
    }

    /// Assigns a script-visible value, returning an error on a final key or failed observer.
    pub fn assign_struct(&mut self, id: usize, key: &str, value: BxValue) -> Result<(), String> {
        if self.struct_key_is_final(id, key) { return Err(format!("Cannot reassign final key [{key}] in scope [static]")); }
        let value = if let Some((_, callback)) = self.struct_assignment_hooks.get(&id).cloned() {
            let _value = self.heap.retain_value(value)?;
            let name = BxValue::new_ptr(self.string_new(key.into()));
            let _name = self.heap.retain_value(name)?;
            self.invoke_callback(&callback, &[name, value])?
        } else {
            value
        };
        self.struct_set(id, key, value);
        Ok(())
    }
}
