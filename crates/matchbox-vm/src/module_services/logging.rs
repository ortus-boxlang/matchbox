//! VM handles for the existing host log service. File ownership, configuration and
//! filtering stay in matchbox-utility; the VM owns only the BoxLang object identities.
use crate::types::{BxNativeObject, BxVM, BxValue, Tracer};
use matchbox_utility::logger::{self, Level, Logger};
use std::{cell::RefCell, collections::HashMap, fmt, rc::Rc, sync::Arc};

#[derive(Debug, Default)]
pub(super) struct LoggingService {
    // LogService interns Logger instances by destination. Key by that same identity so
    // casing, .log suffixes, and absolute paths use its normalization rules exactly.
    handles: HashMap<*const Logger, BxValue>,
}

impl BxNativeObject for LoggingService {
    fn get_property(&self, _: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _: &str, _: BxValue) {}
    fn is_readonly(&self) -> bool {
        true
    }
    fn has_method(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case("getLogger")
    }
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        if !self.has_method(name) || args.len() != 1 || !vm.is_string_value(args[0]) {
            return Err(format!(
                "Unsupported LoggingService method or arguments: {name}; expected getLogger(name)"
            ));
        }
        let logger = logger::service()
            .ok_or("Host logging service is not initialized")?
            .logger(&vm.to_string(args[0]))
            .map_err(|error| error.to_string())?;
        Ok(
            *self.handles.entry(Arc::as_ptr(&logger)).or_insert_with(|| {
                BxValue::new_ptr(vm.native_object_new(Rc::new(RefCell::new(NamedLogger(logger)))))
            }),
        )
    }
    fn trace(&self, tracer: &mut dyn Tracer) {
        for handle in self.handles.values() {
            tracer.mark(handle);
        }
    }
}

struct NamedLogger(Arc<Logger>);
impl fmt::Debug for NamedLogger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BoxLangLogger")
    }
}

impl NamedLogger {
    fn operation(name: &str) -> Option<(Level, bool)> {
        let name = name.to_ascii_lowercase();
        let query = name
            .strip_prefix("is")
            .and_then(|s| s.strip_suffix("enabled"));
        Level::parse(query.unwrap_or(&name))
            .filter(|level| *level != Level::Off)
            .map(|level| (level, query.is_some()))
    }
}
impl BxNativeObject for NamedLogger {
    fn get_property(&self, _: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _: &str, _: BxValue) {}
    fn is_readonly(&self) -> bool {
        true
    }
    fn has_method(&self, name: &str) -> bool {
        Self::operation(name).is_some()
    }
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match (Self::operation(name), args) {
            (Some((level, true)), []) => Ok(BxValue::new_bool(self.0.enabled(level))),
            (Some((level, false)), [message]) if vm.is_string_value(*message) => {
                self.0.log(level, &vm.to_string(*message));
                Ok(BxValue::new_null())
            }
            _ => Err(format!(
                "Unsupported BoxLangLogger method or arguments: {name}"
            )),
        }
    }
}
