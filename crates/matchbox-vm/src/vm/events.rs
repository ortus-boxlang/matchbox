//! Automatic function events share the VM's ordinary synchronous interception path.
use super::{VM, gc::GcObject};
use crate::types::{BxVM, BxValue};

impl VM {
    fn announce_function_event(&mut self, name: &str, data: BxValue) -> Result<(), String> {
        // Observer calls are dispatch machinery, not new user invocations. Explicit
        // nested announcements still run; automatic events must not observe themselves.
        self.automatic_event_depth += 1;
        let result = self.announce(name, data);
        self.automatic_event_depth -= 1;
        result
    }

    pub(super) fn enter_function_events(&mut self, fiber: usize) -> Result<(), String> {
        if self.automatic_event_depth != 0
            || ![
                "preFunctionInvoke",
                "postFunctionInvoke",
                "onFunctionException",
            ]
            .iter()
            .any(|name| self.has_interception_state(name))
        {
            return Ok(());
        }
        let context = self.execution_context()?;
        let arguments = self
            .arguments_scope(fiber)
            .ok_or("Function event arguments unavailable")?;
        let function = self.fibers[fiber].frames.last().unwrap().function.clone();
        let name = BxValue::new_ptr(self.string_new(function.name.clone()));
        let function = BxValue::new_ptr(self.heap.alloc(GcObject::CompiledFunction(function)));
        let data = self.struct_new();
        for (key, value) in [
            ("context", context),
            ("arguments", arguments),
            ("function", function),
            ("name", name),
        ] {
            self.struct_set(data, key, value);
        }
        let data = BxValue::new_ptr(data);
        self.function_events
            .insert(context.as_gc_id().unwrap(), data);
        self.announce_function_event("preFunctionInvoke", data)
    }

    pub(super) fn finish_function_events(
        &mut self,
        fiber: usize,
        result: BxValue,
    ) -> Result<BxValue, String> {
        let Some(context) = self.fibers[fiber]
            .frames
            .last()
            .unwrap()
            .context
            .and_then(|value| value.as_gc_id())
        else {
            return Ok(result);
        };
        let Some(data) = self.function_events.get(&context).copied() else {
            return Ok(result);
        };
        let id = data.as_gc_id().unwrap();
        if !result.is_null() {
            self.struct_set(id, "result", result);
        }
        self.announce_function_event("postFunctionInvoke", data)?;
        let result = if self.struct_key_exists(id, "result") {
            self.struct_get(id, "result")
        } else {
            result
        };
        self.function_events.remove(&context);
        Ok(result)
    }

    pub(super) fn fail_function_events(
        &mut self,
        fiber: usize,
        error: BxValue,
    ) -> Result<(), String> {
        let Some(context) = self.fibers[fiber]
            .frames
            .last()
            .unwrap()
            .context
            .and_then(|value| value.as_gc_id())
        else {
            return Ok(());
        };
        let Some(data) = self.function_events.remove(&context) else {
            return Ok(());
        };
        self.struct_set(data.as_gc_id().unwrap(), "exception", error);
        self.announce_function_event("onFunctionException", data)
    }
}
