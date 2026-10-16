//! Request namespaces restore host globals while retaining live application objects.
use super::{VM, gc::GcObject};
use crate::types::{BxVM, BxValue, FutureStatus};
use anyhow::{Result, bail, ensure};
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

impl VM {
    /// Heap values returned from the closure must use retain_value() to survive cleanup.
    pub fn with_isolated_globals<T>(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.drain_native_completions();
        ensure!(self.fibers.is_empty() && self.pending_native_futures.keys().all(|id| matches!(self.heap.get(*id), GcObject::Future(future) if !matches!(future.status, FutureStatus::Pending))), "Cannot isolate globals while background work is active");
        let pending: std::collections::HashSet<_> =
            self.pending_native_futures.keys().copied().collect();
        let names = self.global_names.clone();
        let values = self.global_values.clone();
        let bifs = self.global_bifs.clone();
        let assignment_hooks = self.struct_assignment_hooks.clone();
        let variables = self.script_variables.clone();
        let retained = values
            .iter()
            .copied()
            .chain(variables.borrow().values().copied())
            .chain(pending.iter().copied().map(BxValue::new_ptr))
            .map(|value| self.heap.retain_value(value).map_err(anyhow::Error::msg))
            .collect::<Result<Vec<_>>>()?;
        self.script_variables = Self::new_variables_scope();
        let output = self.output_buffer.take();
        let captures = std::mem::take(&mut self.output_capture_stack);
        let native_exception = self.native_callback_exception.take();
        let native_depth = self.native_exception_depth;
        let event_depth = self.automatic_event_depth;
        let gc_suspended = self.gc_suspended;
        let result = catch_unwind(AssertUnwindSafe(|| body(self)));
        self.current_fiber_idx = None;
        self.drain_native_completions();
        let new_pending: Vec<_> = self
            .pending_native_futures
            .keys()
            .filter(|id| !pending.contains(*id))
            .copied()
            .collect();
        let detached = !self.fibers.is_empty() || !new_pending.is_empty();
        if detached {
            let error = BxValue::new_ptr(
                self.string_new("Detached work cannot outlive an isolated request".into()),
            );
            let mut futures: Vec<_> = self.fibers.iter().map(|fiber| fiber.future_id).collect();
            futures.extend(new_pending);
            for id in futures {
                if let GcObject::Future(future) = self.heap.get_mut(id)
                    && matches!(future.status, FutureStatus::Pending)
                {
                    future.status = FutureStatus::Failed(error);
                }
                self.thread_futures.remove(&id);
            }
            self.fibers.clear();
            // Native senders must keep cancelled futures rooted until their terminal message arrives.
        }
        self.current_fiber_idx = None;
        self.global_names = names;
        self.global_values = values;
        self.global_bifs = bifs;
        self.struct_assignment_hooks = assignment_hooks;
        self.script_variables = variables;
        self.output_buffer = output;
        self.output_capture_stack = captures;
        self.native_callback_exception = native_exception;
        self.native_exception_depth = native_depth;
        self.automatic_event_depth = event_depth;
        self.gc_suspended = gc_suspended;
        // shortcut: namespace restoration scans function caches, use namespace epochs if profiling shows this cost matters.
        self.heap.clear_global_caches();
        drop(retained);
        // Release request callback captures (including large HTTP bodies) at the boundary.
        self.collect_garbage_now();
        match result {
            Ok(Ok(_)) if detached => bail!("Detached work cannot outlive an isolated request"),
            Ok(result) => result,
            Err(panic) => resume_unwind(panic),
        }
    }
}
