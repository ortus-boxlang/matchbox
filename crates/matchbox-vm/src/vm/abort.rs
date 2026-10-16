//! Non-catchable language control flow, retaining finally unwinding and native callback identity.
use super::{VM, gc::GcObject};
use crate::types::{BxNativeObject, BxVM, BxValue};
use std::{cell::RefCell, fmt, rc::Rc};

#[derive(Clone, Debug)]
pub struct AbortSignal {
    pub kind: String,
    pub cause: Option<String>,
}
impl fmt::Display for AbortSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Execution aborted ({})", self.kind)
    }
}
impl std::error::Error for AbortSignal {}
impl BxNativeObject for AbortSignal {
    fn abort_signal(&self) -> Option<AbortSignal> {
        Some(self.clone())
    }
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn call_method(
        &mut self,
        _vm: &mut dyn BxVM,
        _id: usize,
        _name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        Err("Abort signal is opaque".into())
    }
}
impl VM {
    pub fn is_native_abort(&self, message: &str) -> bool {
        self.native_callback_exception
            .as_ref()
            .filter(|(original, _)| original == message)
            .and_then(|(_, root)| self.heap.read_retained_value(root).ok())
            .and_then(|value| self.abort_signal(value))
            .is_some()
    }
    pub fn take_native_abort(&mut self, message: &str) -> Option<AbortSignal> {
        if !self.is_native_abort(message) {
            return None;
        }
        let (_, root) = self.native_callback_exception.take()?;
        self.abort_signal(self.heap.read_retained_value(&root).ok()?)
    }
    pub(super) fn abort_signal(&self, value: BxValue) -> Option<AbortSignal> {
        match self.heap.get_opt(value.as_gc_id()?)? {
            GcObject::NativeObject(object) => object.borrow().abort_signal(),
            _ => None,
        }
    }
    /// Return this message as the callback error; the rooted payload survives nested native bridges.
    pub fn raise_abort(&mut self, kind: &str, cause: Option<String>) -> String {
        let signal = AbortSignal {
            kind: kind.to_ascii_lowercase(),
            cause,
        };
        let message = signal.to_string();
        let id = self.native_object_new(Rc::new(RefCell::new(signal)));
        let root = self
            .heap
            .retain_value(BxValue::new_ptr(id))
            .expect("new abort signal belongs to this heap");
        self.native_callback_exception = Some((message.clone(), root));
        message
    }
}
