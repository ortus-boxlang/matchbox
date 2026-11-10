//! Shared function scopes. Closures retain the declaring structs, not stack snapshots.
use super::{BxFiber, CallFrame, VM, gc::{GcObject, Heap}, intern::StringInterner, shape::ShapeRegistry};
use crate::types::{BxVM, BxValue};

#[derive(Clone, Copy)]
pub(super) struct ScopeBinding {
    pub(super) key: u32,
    pub(super) local: bool,
    shape: Option<u32>,
    index: Option<u32>,
    write_ready: bool,
}

impl ScopeBinding {
    fn new(key: u32, local: bool) -> Self {
        Self {
            key,
            local,
            shape: None,
            index: None,
            write_ready: local,
        }
    }
}

// ponytail: index the first 32 slots; wider frames keep the map to cap sparse memory.
pub(super) enum SlotBindings {
    Dense(Vec<Option<ScopeBinding>>),
    Sparse(std::collections::HashMap<usize, ScopeBinding>),
}

impl SlotBindings {
    pub(super) fn new() -> Self { Self::Dense(Vec::new()) }
    pub(super) fn get(&self, slot: &usize) -> Option<&ScopeBinding> {
        match self { Self::Dense(v) => v.get(*slot)?.as_ref(), Self::Sparse(m) => m.get(slot) }
    }
    fn get_mut(&mut self, slot: &usize) -> Option<&mut ScopeBinding> {
        match self { Self::Dense(v) => v.get_mut(*slot)?.as_mut(), Self::Sparse(m) => m.get_mut(slot) }
    }
    fn insert(&mut self, slot: usize, binding: ScopeBinding) -> Option<ScopeBinding> {
        if slot >= 32 && let Self::Dense(v) = self {
            *self = Self::Sparse(v.iter().enumerate().filter_map(|(i, b)| b.map(|b| (i, b))).collect());
        }
        match self {
            Self::Dense(v) => { if v.len() <= slot { v.resize(slot + 1, None); } v[slot].replace(binding) }
            Self::Sparse(m) => m.insert(slot, binding),
        }
    }
    pub(super) fn remove(&mut self, slot: &usize) -> Option<ScopeBinding> {
        match self { Self::Dense(v) => v.get_mut(*slot)?.take(), Self::Sparse(m) => m.remove(slot) }
    }
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &ScopeBinding)> {
        let (dense, sparse) = match self { Self::Dense(v) => (Some(v), None), Self::Sparse(m) => (None, Some(m)) };
        dense.into_iter().flat_map(|v| v.iter().enumerate().filter_map(|(i, b)| b.as_ref().map(|b| (i, b))))
            .chain(sparse.into_iter().flat_map(|m| m.iter().map(|(&i, b)| (i, b))))
    }
    pub(super) fn values(&self) -> impl Iterator<Item = &ScopeBinding> { self.iter().map(|(_, b)| b) }
    fn retain(&mut self, mut keep: impl FnMut(&usize, &mut ScopeBinding) -> bool) {
        match self {
            Self::Dense(v) => { for (i, b) in v.iter_mut().enumerate() { if b.as_mut().is_some_and(|b| !keep(&i, b)) { *b = None; } } }
            Self::Sparse(m) => m.retain(keep),
        }
    }
}

#[cfg(test)]
mod slot_binding_tests {
    use super::{ScopeBinding, SlotBindings};
    #[test]
    fn indexed_and_sparse_binding_operations_match_map() {
        let mut bindings = SlotBindings::new(); let mut oracle = std::collections::HashMap::new();
        for slot in [3, 0, 31, 3, 65_535, usize::MAX, 32, 2] {
            let b = ScopeBinding::new(slot as u32, slot % 2 == 0);
            assert_eq!(bindings.insert(slot, b).map(|b| b.key), oracle.insert(slot, b).map(|b| b.key));
            for i in [0, 1, 2, 3, 31, 32, 65_535, usize::MAX] { assert_eq!(bindings.get(&i).map(|b| b.key), oracle.get(&i).map(|b| b.key)); }
        }
        bindings.get_mut(&3).unwrap().shape = Some(42); oracle.get_mut(&3).unwrap().shape = Some(42);
        assert_eq!(bindings.get(&3).unwrap().shape, oracle[&3].shape);
        for slot in [0, 65_535, 1, usize::MAX] { assert_eq!(bindings.remove(&slot).map(|b| b.key), oracle.remove(&slot).map(|b| b.key)); }
        bindings.retain(|i, b| { b.index = Some(7); *i >= 3 }); oracle.retain(|i, b| { b.index = Some(7); *i >= 3 });
        let mut actual: Vec<_> = bindings.iter().map(|(i, b)| (i, b.key, b.local, b.shape, b.index, b.write_ready)).collect(); actual.sort_unstable();
        let mut expected: Vec<_> = oracle.iter().map(|(&i, b)| (i, b.key, b.local, b.shape, b.index, b.write_ready)).collect(); expected.sort_unstable();
        assert_eq!(actual, expected);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScopeMode {
    Binding,
    Stack,
    Shared,
    // No slot bindings, parameters or implicit arguments. Invalidated before binding/exposure.
    Unbound,
}

impl VM {
    pub(super) fn prepare_unbound_frame(&mut self, fiber: usize) {
        let frame = self.fibers[fiber].frames.last_mut().unwrap();
        if frame.scope_mode == ScopeMode::Binding
            && frame.function.params.is_empty()
            && frame.function.arity == 0
            && frame.function.kind != crate::types::FunctionKind::ComponentBody
            && frame.local_names.iter().next().is_none()
        {
            frame.scope_mode = ScopeMode::Unbound;
        }
    }

    pub(super) fn enter_frame_scopes(&mut self, fiber: usize) {
        let frame = self.fibers[fiber].frames.last_mut().unwrap();
        if frame.scope_mode == ScopeMode::Binding
            && frame.function.kind == crate::types::FunctionKind::Udf
            && frame.function.arity as usize == frame.function.params.len() + 1
            && frame.function.params.len() < 32
            && frame.function.params.iter().enumerate().all(|(slot, name)|
                !name.chars().any(char::is_uppercase) && !frame.function.params[..slot].contains(name))
            && frame.function.captured_scopes.is_empty()
            && frame.function.captured_receiver.is_none()
            && frame.receiver.is_none()
            && frame.context.is_none()
            && !cfg!(feature = "debugger")
        {
            frame.scope_mode = ScopeMode::Stack;
        }
    }

    pub(super) fn expose_frame_scopes(&mut self, fiber: usize) {
        self.fibers[fiber].expose_frame_scopes();
    }

    pub(super) fn arguments_scope(&self, fiber: usize) -> Option<BxValue> {
        self.fibers[fiber].arguments_scope()
    }

    pub(super) fn local_scope(&mut self, fiber: usize) -> BxValue {
        if let Some(scope) = self.fibers[fiber].frames.last().unwrap().local_scope {
            return scope;
        }
        let frame = self.fibers[fiber].frames.last().unwrap();
        let captured = (frame.function.kind == crate::types::FunctionKind::ComponentBody)
            .then(|| frame.function.captured_scopes.first().copied())
            .flatten();
        let scope = captured.unwrap_or_else(|| BxValue::new_ptr(self.struct_new()));
        self.fibers[fiber].frames.last_mut().unwrap().local_scope = Some(scope);
        scope
    }

    pub(super) fn bind_local(&mut self, fiber: usize, slot: usize, name: &str) {
        let frame = self.fibers[fiber].frames.last_mut().unwrap();
        if frame.scope_mode == ScopeMode::Unbound {
            frame.scope_mode = ScopeMode::Binding;
        }
        let base = frame.stack_base;
        let value = self.fibers[fiber].stack[base + slot];
        let scope = self.local_scope(fiber).as_gc_id().unwrap();
        let key = self.struct_set_and_key(scope, name, value);
        let frame = self.fibers[fiber].frames.last_mut().unwrap();
        // ponytail: bounded scan of at most 32 slots; shared/sparse frames keep ordinary guards.
        // Different block slots may alias one function-local name without scope exposure.
        if slot >= 32 || (frame.scope_mode == ScopeMode::Stack
            && frame.local_names.get(&slot).is_none_or(|old| !old.local || old.key != key)
            && frame.local_names.values().any(|old| old.local && old.key == key))
        {
            frame.scope_mode = ScopeMode::Shared;
        }
        self.fibers[fiber]
            .frames
            .last_mut()
            .unwrap()
            .local_names
            .insert(slot, ScopeBinding::new(key, true));
    }

    #[inline(always)]
    pub(super) fn local_value(&mut self, fiber: usize, slot: usize) -> BxValue {
        self.fibers[fiber].local_value(slot, &self.heap, &self.shapes, &self.interner)
    }

    #[inline(always)]
    pub(super) fn set_local_value(&mut self, fiber: usize, slot: usize, value: BxValue) {
        self.fibers[fiber].set_local_value(
            slot, value, &mut self.heap, &mut self.shapes, &mut self.interner,
        );
    }

    #[cfg(feature = "jit")]
    pub(super) fn sync_jit_locals(&mut self, fiber: usize, from_stack: bool) {
        let frame = self.fibers[fiber].frames.last().unwrap();
        let base = frame.stack_base;
        let mut slots: Vec<_> = frame
            .local_names
            .iter()
            .filter_map(|(slot, binding)| binding.local.then_some(slot))
            .chain(0..frame.function.params.len())
            .filter(|slot| base + slot < self.fibers[fiber].stack.len())
            .collect();
        slots.sort_unstable();
        for slot in slots {
            if from_stack {
                let value = self.fibers[fiber].stack[base + slot];
                self.set_local_value(fiber, slot, value);
            } else {
                let value = self.local_value(fiber, slot);
                self.fibers[fiber].stack[base + slot] = value;
            }
        }
    }

    pub(super) fn truncate_scope_stack(&mut self, fiber: usize, length: usize) {
        let frame = self.fibers[fiber].frames.last_mut().unwrap();
        frame
            .local_names
            .retain(|slot, _| frame.stack_base + slot < length);
        self.fibers[fiber].stack.truncate(length);
    }

    fn scope_value(&self, scope: BxValue, name: &str) -> Option<BxValue> {
        let id = scope.as_gc_id()?;
        let GcObject::Struct(scope) = self.heap.get(id) else {
            return None;
        };
        let key = self.struct_key_lookup_id(id, name)?;
        self.shapes
            .get_index(scope.shape_id, key)
            .map(|slot| scope.properties[slot as usize])
    }

    pub(super) fn function_scope_value(&mut self, fiber: usize, name: &str) -> Option<BxValue> {
        if name == "local" && self.arguments_scope(fiber).is_some() {
            self.expose_frame_scopes(fiber);
            return Some(self.local_scope(fiber));
        }
        if name == "arguments" {
            self.expose_frame_scopes(fiber);
            return self.arguments_scope(fiber);
        }
        self.function_scope_entry(fiber, name)
            .map(|(_, value)| value)
    }

    fn function_scope_entry(&self, fiber: usize, name: &str) -> Option<(BxValue, BxValue)> {
        let frame = self.fibers[fiber].frames.last().unwrap();
        frame
            .local_scope
            .into_iter()
            .chain(self.arguments_scope(fiber))
            .chain(frame.function.captured_scopes.iter().copied())
            .find_map(|scope| self.scope_value(scope, name).map(|value| (scope, value)))
    }

    pub(super) fn assign_function_scope(
        &mut self,
        fiber: usize,
        name: &str,
        value: BxValue,
    ) -> bool {
        if let Some((scope, _)) = self.function_scope_entry(fiber, name) {
            self.expose_frame_scopes(fiber);
            self.struct_set(scope.as_gc_id().unwrap(), name, value);
            return true;
        }
        false
    }

    pub(super) fn capture_scopes(&mut self, fiber: usize) -> Vec<BxValue> {
        self.expose_frame_scopes(fiber);
        let function = &self.fibers[fiber].frames.last().unwrap().function;
        if function.kind == crate::types::FunctionKind::ComponentBody {
            return function.captured_scopes.clone();
        }
        let Some(arguments) = self.arguments_scope(fiber) else {
            return Vec::new();
        };
        let local = self.local_scope(fiber);
        let mut scopes = vec![local, arguments];
        scopes.extend(
            self.fibers[fiber]
                .frames
                .last()
                .unwrap()
                .function
                .captured_scopes
                .iter()
                .copied(),
        );
        scopes
    }
}

impl CallFrame {
    #[inline(always)]
    fn arguments_scope(&self, stack: &[BxValue]) -> Option<BxValue> {
        if self.function.kind == crate::types::FunctionKind::ComponentBody {
            return self.function.captured_scopes.get(1).copied();
        }
        (self.function.arity as usize == self.function.params.len() + 1)
            .then(|| stack[self.stack_base + self.function.params.len()])
    }

    // Only a current, short frame/stack borrow: metadata locates the live value,
    // never a cached value, scope identity or pointer surviving this read.
    #[inline(always)]
    fn cached_binding_value(&self, slot: usize, stack: &[BxValue], heap: &Heap) -> Option<BxValue> {
        let binding = self.local_names.get(&slot)?;
        let shape = binding.shape?;
        let index = binding.index?;
        let scope = if binding.local {
            self.local_scope?
        } else {
            self.arguments_scope(stack)?
        };
        let GcObject::Struct(record) = heap.get_opt(scope.as_gc_id()?)? else {
            return None;
        };
        if record.shape_id != shape {
            return None;
        }
        record.properties.get(index as usize).copied()
    }
}

// One authoritative local resolver for both whole-VM callers and a short-lived
// dispatch borrow. Explicit fields prevent a local access from borrowing the VM
// again; they do not detach the fiber or move its GC roots out of VM::fibers.
impl BxFiber {
    fn expose_frame_scopes(&mut self) {
        // Permanent transition: keep live dictionaries and the deletion fallback.
        // Do not copy aliased dictionary values back into stack fallback slots.
        if let Some(frame) = self.frames.last_mut() {
            frame.scope_mode = ScopeMode::Shared;
        }
    }

    fn arguments_scope(&self) -> Option<BxValue> {
        self.frames.last()?.arguments_scope(&self.stack)
    }

    fn slot_binding(
        &mut self, slot: usize, heap: &Heap, interner: &StringInterner,
    ) -> Option<(usize, ScopeBinding)> {
        let frame = self.frames.last().unwrap();
        if let Some(binding) = frame.local_names.get(&slot).copied() {
            let scope = if binding.local {
                frame.local_scope?
            } else {
                self.arguments_scope()?
            };
            return Some((scope.as_gc_id()?, binding));
        }
        let name = frame.function.params.get(slot)?;
        let scope = self.arguments_scope()?.as_gc_id()?;
        // Read misses must not intern a name or upgrade its visible spelling.
        let key = VM::struct_key_lookup_id_in(heap, interner, scope, name)?;
        let binding = ScopeBinding::new(key, false);
        self.frames.last_mut().unwrap().local_names.insert(slot, binding);
        Some((scope, binding))
    }

    fn binding_index(
        &mut self, slot: usize, scope: usize, binding: ScopeBinding,
        heap: &Heap, shapes: &ShapeRegistry,
    ) -> Option<u32> {
        let GcObject::Struct(record) = heap.get(scope) else {
            return None;
        };
        if binding.shape == Some(record.shape_id) {
            return binding.index;
        }
        let shape = record.shape_id;
        let index = shapes.get_index(shape, binding.key);
        let cached = self.frames.last_mut().unwrap().local_names.get_mut(&slot).unwrap();
        cached.shape = Some(shape);
        cached.index = index;
        index
    }

    #[inline(always)]
    pub(super) fn local_value(
        &mut self, slot: usize, heap: &Heap, shapes: &ShapeRegistry, interner: &StringInterner,
    ) -> BxValue {
        let frame = self.frames.last().unwrap();
        if frame.scope_mode == ScopeMode::Unbound {
            return self.stack[frame.stack_base + slot];
        }
        if frame.scope_mode == ScopeMode::Stack && slot == frame.function.params.len() {
            // A body read of implicit arguments is a real scope exposure. The
            // compiler prologue stays dictionary-authoritative in Binding mode.
            self.expose_frame_scopes();
        }
        let frame = self.frames.last().unwrap();
        if frame.scope_mode == ScopeMode::Stack {
            return self.stack[frame.stack_base + slot];
        }
        if frame.function.kind == crate::types::FunctionKind::ComponentBody
            && slot == frame.function.params.len()
            && let Some(arguments) = self.arguments_scope()
        {
            return arguments;
        }
        if let Some(value) = frame.cached_binding_value(slot, &self.stack, heap) {
            return value;
        }
        let value = self.slot_binding(slot, heap, interner).and_then(|(scope, binding)| {
            let index = self.binding_index(slot, scope, binding, heap, shapes)?;
            let GcObject::Struct(record) = heap.get(scope) else {
                return None;
            };
            record.properties.get(index as usize).copied()
        });
        if let Some(value) = value {
            return value;
        }
        let frame = self.frames.last().unwrap();
        self.stack[frame.stack_base + slot]
    }

    #[inline(always)]
    pub(super) fn set_local_value(
        &mut self, slot: usize, value: BxValue, heap: &mut Heap,
        shapes: &mut ShapeRegistry, interner: &mut StringInterner,
    ) {
        let frame = self.frames.last().unwrap();
        if frame.scope_mode == ScopeMode::Unbound {
            let base = frame.stack_base;
            self.stack[base + slot] = value;
            return;
        }
        let binding = self.slot_binding(slot, heap, interner).or_else(|| {
            let name = self.frames.last()?.function.params.get(slot)?.clone();
            let scope = self.arguments_scope()?.as_gc_id()?;
            let key = VM::struct_key_id_in(heap, interner, scope, &name);
            let mut binding = ScopeBinding::new(key, false);
            binding.write_ready = true;
            self.frames.last_mut()?.local_names.insert(slot, binding);
            Some((scope, binding))
        });
        if let Some((scope, mut binding)) = binding {
            if !binding.write_ready {
                let name = self.frames.last().unwrap().function.params[slot].clone();
                binding.key = VM::struct_key_id_in(heap, interner, scope, &name);
                binding.write_ready = true;
                *self.frames.last_mut().unwrap().local_names.get_mut(&slot).unwrap() = binding;
            }
            if let Some(index) = self.binding_index(slot, scope, binding, heap, shapes) {
                // get_mut retains the ordinary GC write barrier; never cache values.
                if let GcObject::Struct(record) = heap.get_mut(scope) {
                    record.properties[index as usize] = value;
                }
            } else {
                let name = interner.resolve(binding.key).to_owned();
                VM::struct_set_and_key_in(heap, shapes, interner, scope, &name, value);
                self.frames.last_mut().unwrap().local_names.get_mut(&slot).unwrap().shape = None;
            }
        }
        let base = self.frames.last().unwrap().stack_base;
        self.stack[base + slot] = value;
    }
}

#[cfg(test)]
mod unbound_tests;
#[cfg(test)]
mod binding_read_tests;
