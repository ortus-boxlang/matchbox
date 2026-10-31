//! Synchronous, VM-local interceptor chains. Registries are ordinary traced VM values.
use super::{VM, chunk::Chunk, gc::GcObject};
use crate::types::{BxVM, BxValue};
use std::{cell::RefCell, rc::Rc};

const POINTS: &str = "__mb_interception_points";
const STATES: &str = "__mb_interceptor_states";
const BUILTIN_POINTS: &[&str] = &[
    "preModuleLoad",
    "postModuleLoad",
    "preModuleUnload",
    "postModuleUnload",
    "preFunctionInvoke",
    "postFunctionInvoke",
    "onFunctionException",
];

impl VM {
    fn interception_registry(&mut self, name: &str) -> usize {
        self.get_global(name)
            .unwrap_or_else(|| self.insert_empty_struct_global(name))
            .as_gc_id()
            .unwrap()
    }

    pub(crate) fn append_interception_points(&mut self, names: Vec<String>) {
        let points = self.interception_registry(POINTS);
        for name in names {
            self.struct_set(points, &name, BxValue::new_bool(true));
        }
    }

    pub(crate) fn interception_points(&self) -> Vec<String> {
        let mut points: Vec<String> = BUILTIN_POINTS.iter().map(|name| (*name).into()).collect();
        if let Some(id) = self.get_global(POINTS).and_then(|value| value.as_gc_id()) {
            for name in self.struct_key_array(id) {
                if !points.iter().any(|point| point.eq_ignore_ascii_case(&name)) {
                    points.push(name);
                }
            }
        }
        points
    }

    pub(crate) fn has_interception_state(&self, name: &str) -> bool {
        self.get_global(STATES)
            .and_then(|value| value.as_gc_id())
            .and_then(|id| self.struct_get(id, name).as_gc_id())
            .is_some_and(|id| self.array_len(id) != 0)
    }

    pub(crate) fn announce_event(
        &mut self,
        name: &str,
        fields: &[(&str, BxValue)],
    ) -> Result<(), String> {
        if !self.has_interception_state(name) {
            return Ok(());
        }
        let data = self.struct_new();
        for (key, value) in fields {
            self.struct_set(data, key, *value);
        }
        self.announce(name, BxValue::new_ptr(data))
    }

    pub(crate) fn register_interceptor(&mut self, target: BxValue) -> Result<(), String> {
        if !self.value_matches_type_name(target, "class") {
            return Err("Interceptor must be a BoxLang class instance".into());
        }
        let points = self.interception_points();
        let GcObject::Instance(instance) = self.heap.get(target.as_gc_id().unwrap()) else {
            unreachable!()
        };
        // JVM class metadata discovers declared functions, not inherited handlers.
        let names: Vec<_> = instance
            .class
            .borrow()
            .methods
            .iter()
            .filter(|(name, method)| {
                method.modifiers.interception_point
                    || points.iter().any(|point| point.eq_ignore_ascii_case(name))
            })
            .map(|(name, _)| name.clone())
            .collect();
        self.append_interception_points(names.clone());
        let states = self.interception_registry(STATES);
        for name in names {
            if !self.instance_has_method(target, &name) {
                continue;
            }
            let observers = self.struct_get(states, &name);
            let observers = if let Some(id) = observers.as_gc_id() {
                id
            } else {
                let id = self.array_new();
                self.struct_set(states, &name, BxValue::new_ptr(id));
                id
            };
            self.array_push(observers, target);
        }
        Ok(())
    }

    pub(crate) fn unregister_interceptor(&mut self, target: BxValue) -> Result<(), String> {
        let states = self.interception_registry(STATES);
        for name in self.struct_key_array(states) {
            let observers = self.struct_get(states, &name).as_gc_id().unwrap();
            for index in 0..self.array_len(observers) {
                if self.array_get(observers, index) == target {
                    self.array_delete_at(observers, index)?;
                    break;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn announce(&mut self, name: &str, data: BxValue) -> Result<(), String> {
        if !self.is_struct_value(data) {
            return Err("Interception data must be a struct".into());
        }
        let states = self.interception_registry(STATES);
        let Some(observers) = self.struct_get(states, name).as_gc_id() else {
            return Ok(());
        };
        // Like the JVM copy-on-write list, registrations during dispatch affect the next announcement.
        let observers: Vec<_> = (0..self.array_len(observers))
            .map(|index| self.array_get(observers, index))
            .collect();
        let observers: Vec<_> = observers
            .into_iter()
            .map(|value| self.heap.retain_value(value))
            .collect::<Result<Vec<_>, _>>()?;
        let _data = self.heap.retain_value(data)?;
        let context = self.execution_context()?;
        let _context = self.heap.retain_value(context)?;
        for target in observers {
            let target = self.heap.read_retained_value(&target)?;
            let result = self.call_method_by_value(
                target,
                name,
                vec![data, context],
                Rc::new(RefCell::new(Chunk::default())),
            )?;
            if result.is_bool() && result.as_bool() {
                break;
            }
        }
        Ok(())
    }
}
