//! Live application-isolated native sessions; values stay in their owning VM heap.
use crate::cookies::{Cookie, CookieJar};
use chrono::{Months, Utc};
use matchbox_vm::{
    types::{BxNativeObject, BxVM, BxValue, NativeCallback},
    vm::{VM, gc::RootedValue},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

pub(crate) struct SessionState {
    scope: Rc<RootedValue>,
    listener: Rc<RootedValue>,
    end_hook: bool,
    started: bool,
    touched: Instant,
    timeout: Option<Duration>,
}
pub(crate) type Store = Rc<RefCell<HashMap<String, SessionState>>>;
#[derive(Clone)]
pub(crate) struct Settings {
    pub enabled: bool,
    timeout: Option<Duration>,
    send_cookie: bool,
    cookie: Cookie,
}
fn flag(vm: &mut VM, listener: BxValue, name: &str, default: bool) -> anyhow::Result<bool> {
    vm.instance_property(listener, name)
        .filter(|v| !v.is_null())
        .map(|v| {
            vm.cast_value_to_type(v, "boolean")
                .map(|v| v.as_bool())
                .map_err(anyhow::Error::msg)
        })
        .unwrap_or(Ok(default))
}
impl Settings {
    pub fn from_listener(vm: &mut VM, listener: Option<BxValue>) -> anyhow::Result<Self> {
        let mut result = Self {
            enabled: false,
            timeout: Some(Duration::from_secs(1800)),
            send_cookie: true,
            cookie: Cookie::new("jsessionid".into(), String::new()),
        };
        result.cookie.http_only = true;
        result.cookie.same_site = Some("Lax".into());
        result.cookie.expires = Some(
            (Utc::now() + Months::new(360))
                .format("%a, %d %b %Y %H:%M:%S GMT")
                .to_string(),
        );
        let Some(listener) = listener else {
            return Ok(result);
        };
        result.enabled = flag(vm, listener, "sessionManagement", false)?;
        if !result.enabled {
            return Ok(result);
        }
        result.send_cookie = flag(vm, listener, "setClientCookies", true)?;
        for (name, expected) in [("sessionStorage", "memory"), ("sessionType", "native")] {
            if let Some(value) = vm
                .instance_property(listener, name)
                .filter(|v| !v.is_null())
            {
                anyhow::ensure!(
                    vm.to_string(value).eq_ignore_ascii_case(expected),
                    "Only native in-memory sessions are supported; {name} must be {expected}"
                );
            }
        }
        if let Some(value) = vm
            .instance_property(listener, "sessionTimeout")
            .filter(|v| !v.is_null())
        {
            let seconds = value.as_number();
            anyhow::ensure!(
                (value.is_number() || value.is_int()) && seconds.is_finite() && seconds >= 0.0,
                "sessionTimeout must be a nonnegative createTimeSpan duration"
            );
            result.timeout = if seconds == 0.0 {
                None
            } else {
                Some(Duration::try_from_secs_f64(seconds)?)
            };
        }
        if let Some(value) = vm
            .instance_property(listener, "sessionCookie")
            .filter(|v| !v.is_null())
        {
            anyhow::ensure!(vm.is_struct_value(value), "sessionCookie must be a struct");
            let id = value.as_gc_id().unwrap();
            // Session-cookie names/path are fixed by MiniServer; its disableUpdate setting is currently unused.
            let normalized = vm.struct_new();
            for key in ["secure", "httpOnly", "domain", "sameSite"] {
                let item = vm.struct_get(id, key);
                if !item.is_null() {
                    vm.struct_set(normalized, key, item);
                }
            }
            if vm.struct_get(normalized, "httpOnly").is_null() {
                vm.struct_set(normalized, "httpOnly", BxValue::new_bool(true));
            }
            if vm.struct_get(normalized, "sameSite").is_null() {
                let mode = BxValue::new_ptr(vm.string_new("Lax".into()));
                vm.struct_set(normalized, "sameSite", mode);
            }
            let mut cookie =
                Cookie::from_value(vm, "jsessionid".into(), BxValue::new_ptr(normalized))
                    .map_err(anyhow::Error::msg)?;
            cookie.expires = result.cookie.expires.clone();
            let timeout = vm.struct_get(id, "timeout");
            if !timeout.is_null() {
                if timeout.is_number() || timeout.is_int() {
                    let seconds = timeout.as_number();
                    anyhow::ensure!(
                        seconds.is_finite() && seconds >= 0.0,
                        "Invalid sessionCookie timeout"
                    );
                    let duration =
                        chrono::Duration::from_std(Duration::try_from_secs_f64(seconds)?)?;
                    cookie.expires = Some(
                        Utc::now()
                            .checked_add_signed(duration)
                            .ok_or_else(|| {
                                anyhow::anyhow!("Session cookie timeout is out of range")
                            })?
                            .format("%a, %d %b %Y %H:%M:%S GMT")
                            .to_string(),
                    );
                } else if let Ok(date) =
                    chrono::DateTime::parse_from_rfc3339(&vm.to_string(timeout))
                {
                    cookie.expires = Some(
                        date.with_timezone(&Utc)
                            .format("%a, %d %b %Y %H:%M:%S GMT")
                            .to_string(),
                    );
                }
            }
            cookie.header().map_err(anyhow::Error::msg)?;
            result.cookie = cookie;
        }
        Ok(result)
    }
    pub fn id(&self, incoming: &HashMap<String, String>) -> String {
        incoming
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("jsessionid"))
            .map(|(_, value)| value)
            .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("null"))
            .cloned()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }
    pub fn emit(&self, jar: &Rc<RefCell<CookieJar>>, id: &str) -> Result<(), String> {
        if self.send_cookie {
            let mut cookie = self.cookie.clone();
            cookie.value = id.into();
            jar.borrow_mut().set(cookie)?;
        }
        Ok(())
    }
}
struct SessionRequest {
    store: Store,
    app: Rc<RootedValue>,
    listener: Rc<RootedValue>,
    end_listener: Rc<RootedValue>,
    start_hook: bool,
    end_hook: bool,
    name: String,
    id: RefCell<String>,
    settings: Settings,
    jar: Rc<RefCell<CookieJar>>,
}
impl std::fmt::Debug for SessionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<active session request>")
    }
}
#[derive(Debug)]
struct Context(Rc<SessionRequest>);
impl BxNativeObject for Context {
    fn native_data(&self, kind: &str) -> Option<*mut std::ffi::c_void> {
        (kind == "matchbox.session.v1").then_some(self as *const Self as *mut std::ffi::c_void)
    }
    fn get_property(&self, _: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _: &str, _: BxValue) {}
    fn call_method(
        &mut self,
        _: &mut dyn BxVM,
        _: usize,
        _: &str,
        _: &[BxValue],
    ) -> Result<BxValue, String> {
        Err("Session context is opaque".into())
    }
}
fn current(vm: &dyn BxVM) -> Result<Rc<SessionRequest>, String> {
    let value = vm
        .get_global_value("__mb_session_context")
        .ok_or("Session management is disabled")?;
    let pointer = vm
        .native_object_data(value, "matchbox.session.v1")
        .ok_or("Session management is disabled")?;
    // Private brand; clone owned data before VM callbacks/allocations.
    Ok(unsafe { (&*(pointer as *const Context)).0.clone() })
}
fn call(
    vm: &mut dyn BxVM,
    listener: BxValue,
    name: &str,
    args: Vec<BxValue>,
) -> Result<BxValue, String> {
    let chunk = vm
        .current_chunk()
        .unwrap_or_else(|| Rc::new(RefCell::new(matchbox_vm::Chunk::default())));
    vm.call_method_by_value(listener, name, args, chunk)
}
fn open(vm: &mut dyn BxVM, ctx: &SessionRequest) -> Result<(), String> {
    let id = ctx.id.borrow().clone();
    if !ctx.store.borrow().contains_key(&id) {
        let scope = vm.struct_new();
        let root = vm.retain_value(BxValue::new_ptr(scope))?;
        for (key, value) in [
            ("jsessionid", id.clone()),
            ("sessionid", format!("{}:{id}", ctx.name)),
        ] {
            let value = BxValue::new_ptr(vm.string_new(value));
            vm.struct_set(scope, key, value);
        }
        let date = BxValue::new_ptr(vm.datetime_new(Utc::now()));
        vm.struct_set(scope, "timeCreated", date);
        vm.struct_set(scope, "lastVisit", date);
        ctx.store.borrow_mut().insert(
            id.clone(),
            SessionState {
                scope: root,
                listener: ctx.end_listener.clone(),
                end_hook: ctx.end_hook,
                started: false,
                touched: Instant::now(),
                timeout: ctx.settings.timeout,
            },
        );
    }
    let (scope, first) = {
        let mut store = ctx.store.borrow_mut();
        let session = store.get_mut(&id).unwrap();
        session.touched = Instant::now();
        let first = !session.started;
        session.started = true;
        (session.scope.clone(), first)
    };
    let value = vm.read_retained_value(&scope)?;
    vm.insert_global("session".into(), value);
    let now = BxValue::new_ptr(vm.datetime_new(Utc::now()));
    vm.struct_set(value.as_gc_id().unwrap(), "lastVisit", now);
    if first && ctx.start_hook {
        let listener = vm.read_retained_value(&ctx.listener)?;
        let id_value = BxValue::new_ptr(vm.string_new(id.clone()));
        if let Err(error) = call(vm, listener, "onSessionStart", vec![id_value]) {
            if !vm.is_native_abort(&error)
                && let Some(state) = ctx.store.borrow_mut().get_mut(&id)
            {
                state.started = false;
            }
            return Err(error);
        }
    }
    let scope = vm.get_global_value("cookie").unwrap().as_gc_id().unwrap();
    let value = BxValue::new_ptr(vm.string_new(ctx.id.borrow().clone()));
    vm.struct_set(scope, "jsessionid", value);
    ctx.settings.emit(&ctx.jar, &ctx.id.borrow())?;
    Ok(())
}
fn end(vm: &mut dyn BxVM, app: &RootedValue, state: SessionState) {
    let result = (|| -> Result<(), String> {
        let scope = vm.read_retained_value(&state.scope)?;
        if state.end_hook {
            let listener = vm.read_retained_value(&state.listener)?;
            let app = vm.read_retained_value(app)?;
            vm.begin_output_capture();
            let result = call(vm, listener, "onSessionEnd", vec![scope, app]);
            vm.end_output_capture();
            result?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("onSessionEnd failed: {error}");
    }
    if let Ok(scope) = vm.read_retained_value(&state.scope) {
        vm.struct_clear(scope.as_gc_id().unwrap());
    }
}
pub(crate) fn expire(vm: &mut VM, app: &RootedValue, store: &Store, all: bool) {
    let expired: Vec<_> = store
        .borrow()
        .iter()
        .filter(|(_, state)| {
            all || state
                .timeout
                .is_some_and(|timeout| state.touched.elapsed() >= timeout)
        })
        .map(|(id, _)| id.clone())
        .collect();
    for id in expired {
        let state = store.borrow_mut().remove(&id).unwrap();
        let _ = vm.with_isolated_globals(|vm| {
            vm.insert_global(
                "application".into(),
                vm.read_retained_value(app).map_err(anyhow::Error::msg)?,
            );
            end(vm, app, state);
            Ok(())
        });
    }
}
pub(crate) fn attach(
    vm: &mut VM,
    store: Store,
    app: Rc<RootedValue>,
    listener: BxValue,
    end_listener: Rc<RootedValue>,
    name: String,
    id: String,
    settings: Settings,
    jar: Rc<RefCell<CookieJar>>,
) -> anyhow::Result<()> {
    let end_value = vm
        .read_retained_value(&end_listener)
        .map_err(anyhow::Error::msg)?;
    let ctx = Rc::new(SessionRequest {
        store,
        app,
        listener: vm.retain_value(listener).map_err(anyhow::Error::msg)?,
        end_listener,
        start_hook: vm.instance_has_method(listener, "onSessionStart"),
        end_hook: vm.instance_has_method(end_value, "onSessionEnd"),
        name,
        id: RefCell::new(id),
        settings,
        jar,
    });
    let context = vm.native_object_new(Rc::new(RefCell::new(Context(ctx.clone()))));
    vm.insert_global("__mb_session_context".into(), BxValue::new_ptr(context));
    for name in ["sessionInvalidate", "sessionRotate"] {
        let rotate = name == "sessionRotate";
        vm.register_callback(
            name,
            NativeCallback {
                parameters: vec![],
                callback: Rc::new(move |vm, args| {
                    if !args.is_empty() {
                        return Err("Session control functions take no arguments".into());
                    }
                    let ctx = current(vm)?;
                    let id = ctx.id.borrow().clone();
                    let old = ctx.store.borrow_mut().remove(&id);
                    let saved = if rotate {
                        old.as_ref()
                            .map(|state| {
                                vm.read_retained_value(&state.scope).map(|value| {
                                    vm.struct_key_array(value.as_gc_id().unwrap())
                                        .into_iter()
                                        .map(|key| {
                                            let val =
                                                vm.struct_get(value.as_gc_id().unwrap(), &key);
                                            (key, val)
                                        })
                                        .collect::<Vec<_>>()
                                })
                            })
                            .transpose()?
                            .unwrap_or_default()
                    } else {
                        vec![]
                    };
                    let roots = saved
                        .iter()
                        .map(|(_, value)| vm.retain_value(*value))
                        .collect::<Result<Vec<_>, _>>()?;
                    if let Some(old) = old {
                        end(vm, &ctx.app, old);
                    }
                    *ctx.id.borrow_mut() = uuid::Uuid::new_v4().to_string();
                    open(vm, &ctx)?;
                    let scope = vm.get_global_value("session").unwrap().as_gc_id().unwrap();
                    for (key, value) in saved {
                        if !vm.struct_key_exists(scope, &key) {
                            vm.struct_set(scope, &key, value);
                        }
                    }
                    drop(roots);
                    Ok(BxValue::new_null())
                }),
            },
        )
        .map_err(anyhow::Error::msg)?;
    }
    vm.register_callback(
        "sessionStartTime",
        NativeCallback {
            parameters: vec![],
            callback: Rc::new(|vm, args| {
                if !args.is_empty() {
                    return Err("sessionStartTime takes no arguments".into());
                }
                current(vm)?;
                let scope = vm.get_global_value("session").unwrap().as_gc_id().unwrap();
                Ok(vm.struct_get(scope, "timeCreated"))
            }),
        },
    )
    .map_err(anyhow::Error::msg)?;
    if let Err(error) = open(vm, &ctx) {
        if let Some(abort) = vm.take_native_abort(&error) {
            return Err(abort.into());
        }
        anyhow::bail!("{error}");
    }
    Ok(())
}
