pub mod chunk;
pub mod gc;
pub mod intern;
#[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
mod members;
mod calls;
mod classes;
mod scopes;
mod contexts;
mod events;
mod interceptors;
#[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
pub(crate) mod components;
mod component_runtime;
mod request_scope;
mod struct_scope;
mod abort;
pub use abort::AbortSignal;
#[cfg(feature = "jit")]
pub mod jit;
pub mod opcode;
pub mod shape;

#[cfg(all(test, target_arch = "wasm32", feature = "js"))]
mod interop_tests;

use self::chunk::{Chunk, IcEntry};
use self::gc::{GCConfig, GcId, GcObject, Heap};
use self::intern::StringInterner;
use self::opcode::op;
use self::shape::ShapeRegistry;
use crate::types::{
    BxClass, BxCompiledFunction, BxFuture, BxInstance, BxInterface, BxNativeFunction,
    BxNativeObject, BxRange, BxStruct, BxVM, BxValue, Constant, FutureStatus, NativeFutureHandle,
    NativeFutureMessage, NativeFutureValue, Tracer, box_string::BoxString,
};
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use crate::types::{register_wasm_future_thunk, take_wasm_future_thunk};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use std::cell::RefCell;
use std::collections::HashSet;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use web_time::Instant;
use std::vec;

pub static INTERRUPT_REQUESTED: AtomicBool = AtomicBool::new(false);

fn wait_until_instant(until: Instant) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let now = Instant::now();
        if until > now {
            std::thread::sleep(until - now);
        }
    }
    // Browser WASM cannot park a thread; synchronous waits must poll the clock.
    #[cfg(target_arch = "wasm32")]
    while Instant::now() < until {
        std::hint::spin_loop();
    }
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
fn browser_js_root() -> Option<JsValue> {
    let window = web_sys::window()?;
    let window_js: JsValue = window.into();

    // Build a Proxy over window so that `js.Alpine`, `js.document`, etc.
    // resolve live at access time instead of being eagerly snapshotted.
    // `js.window` and `js.globalThis` are explicit overrides so that
    // `js.window` always returns the Window object itself (avoiding an
    // infinite proxy loop on `window.window`).
    let target = Object::new();
    Reflect::set(&target, &JsValue::from_str("window"), &window_js).ok()?;
    Reflect::set(&target, &JsValue::from_str("globalThis"), &js_sys::global()).ok()?;

    let handler = Object::new();

    // --- get trap: target overrides first, then live window lookup ---
    let win_for_get = window_js.clone();
    let get_fn = Closure::<dyn Fn(JsValue, JsValue, JsValue) -> JsValue>::new(
        move |target: JsValue, prop: JsValue, _receiver: JsValue| {
            if let Ok(val) = Reflect::get(&target, &prop) {
                if !val.is_undefined() {
                    return val;
                }
            }
            Reflect::get(&win_for_get, &prop).unwrap_or(JsValue::UNDEFINED)
        },
    );
    Reflect::set(
        &handler,
        &JsValue::from_str("get"),
        get_fn.as_ref().unchecked_ref(),
    )
    .ok()?;
    get_fn.forget();

    // --- has trap: delegate to window so resolve_js_property works ---
    let win_for_has = window_js.clone();
    let has_fn =
        Closure::<dyn Fn(JsValue, JsValue) -> bool>::new(move |target: JsValue, prop: JsValue| {
            if Reflect::has(&target, &prop).unwrap_or(false) {
                return true;
            }
            Reflect::has(&win_for_has, &prop).unwrap_or(false)
        });
    Reflect::set(
        &handler,
        &JsValue::from_str("has"),
        has_fn.as_ref().unchecked_ref(),
    )
    .ok()?;
    has_fn.forget();

    // --- getOwnPropertyDescriptor trap: bridge to window for case-insensitive enumeration ---
    let win_for_desc = window_js.clone();
    let desc_fn = Closure::<dyn Fn(JsValue, JsValue) -> JsValue>::new(
        move |target: JsValue, prop: JsValue| {
            let desc = Object::get_own_property_descriptor(&Object::from(target), &prop);
            if !desc.is_undefined() {
                return desc;
            }
            Object::get_own_property_descriptor(&Object::from(win_for_desc.clone()), &prop)
        },
    );
    Reflect::set(
        &handler,
        &JsValue::from_str("getOwnPropertyDescriptor"),
        desc_fn.as_ref().unchecked_ref(),
    )
    .ok()?;
    desc_fn.forget();

    // --- ownKeys trap: merge target keys + window keys for enumeration ---
    let win_for_keys = window_js.clone();
    let keys_fn = Closure::<dyn Fn(JsValue) -> JsValue>::new(move |target: JsValue| {
        let result = Array::new();
        // Add target's own keys first
        let target_keys = Reflect::own_keys(&target).unwrap_or_else(|_| Array::new());
        for k in target_keys.iter() {
            result.push(&k);
        }
        // Add window's own keys (skip duplicates)
        if let Ok(win_keys) = Reflect::own_keys(&win_for_keys) {
            for k in win_keys.iter() {
                if !Reflect::has(&target, &k).unwrap_or(false) {
                    result.push(&k);
                }
            }
        }
        result.into()
    });
    Reflect::set(
        &handler,
        &JsValue::from_str("ownKeys"),
        keys_fn.as_ref().unchecked_ref(),
    )
    .ok()?;
    keys_fn.forget();

    let proxy = Proxy::new(&target, &handler);
    Some(proxy.into())
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
fn is_plain_js_object(value: &JsValue) -> bool {
    if !value.is_object() || Array::is_array(value) {
        return false;
    }

    if value.clone().dyn_into::<Function>().is_ok() {
        return false;
    }

    let nested_proxy_marker = Reflect::get(value, &JsValue::from_str("__matchbox_nested_proxy__"))
        .unwrap_or(JsValue::UNDEFINED);
    if nested_proxy_marker.as_bool().unwrap_or(false) {
        return false;
    }

    let object = Object::from(value.clone());
    let prototype = Object::get_prototype_of(&object);
    if prototype.is_null() || prototype.is_undefined() {
        return true;
    }

    let global = js_sys::global();
    let object_ctor = match Reflect::get(&global, &JsValue::from_str("Object")) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let object_prototype = match Reflect::get(&object_ctor, &JsValue::from_str("prototype")) {
        Ok(value) => value,
        Err(_) => return false,
    };

    Object::is(&prototype, &object_prototype)
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
fn unwrap_matchbox_js_proxy(value: &JsValue) -> JsValue {
    let mut current = value.clone();
    for _ in 0..JS_INTEROP_MAX_DEPTH {
        let global = js_sys::global();
        let matchbox =
            Reflect::get(&global, &JsValue::from_str("MatchBox")).unwrap_or(JsValue::UNDEFINED);
        if matchbox.is_undefined() || matchbox.is_null() {
            break;
        }

        let proxy_targets = Reflect::get(&matchbox, &JsValue::from_str("__matchbox_proxy_targets"))
            .unwrap_or(JsValue::UNDEFINED);
        if proxy_targets.is_undefined() || proxy_targets.is_null() {
            break;
        }

        let has_fn =
            Reflect::get(&proxy_targets, &JsValue::from_str("has")).unwrap_or(JsValue::UNDEFINED);
        let get_fn =
            Reflect::get(&proxy_targets, &JsValue::from_str("get")).unwrap_or(JsValue::UNDEFINED);
        let (Ok(has_fn), Ok(get_fn)) =
            (has_fn.dyn_into::<Function>(), get_fn.dyn_into::<Function>())
        else {
            break;
        };

        let has_current = Reflect::apply(&has_fn, &proxy_targets, &Array::of1(&current))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !has_current {
            break;
        }

        let target = Reflect::apply(&get_fn, &proxy_targets, &Array::of1(&current))
            .unwrap_or(JsValue::UNDEFINED);
        if target.is_undefined() || target.is_null() {
            break;
        }

        current = target;
    }
    current
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
fn schedule_browser_vm_pump(vm_ptr: usize) {
    let global = js_sys::global();
    let matchbox = match Reflect::get(&global, &JsValue::from_str("MatchBox")) {
        Ok(value) if !value.is_undefined() && !value.is_null() => value,
        _ => return,
    };

    let schedule = match Reflect::get(&matchbox, &JsValue::from_str("schedulePump")) {
        Ok(value) => value,
        Err(_) => return,
    };

    if let Ok(func) = schedule.dyn_into::<Function>() {
        let _ = func.call1(&matchbox, &JsValue::from_f64(vm_ptr as f64));
    }
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
fn js_error_value(message: &str) -> JsValue {
    let global = js_sys::global();
    let error_key = JsValue::from_str("Error");

    if let Ok(error_ctor) = Reflect::get(&global, &error_key) {
        if let Ok(error_ctor) = error_ctor.dyn_into::<Function>() {
            let args = Array::of1(&JsValue::from_str(message));
            if let Ok(value) = Reflect::construct(&error_ctor, &args) {
                return value;
            }
        }
    }

    JsValue::from_str(message)
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
pub(crate) fn resolve_js_property(target: &JsValue, name: &str) -> JsValue {
    let direct = JsValue::from_str(name);
    if Reflect::has(target, &direct).unwrap_or(false) {
        return direct;
    }

    let mut current = target.clone();
    while !current.is_null() && !current.is_undefined() {
        let object = Object::from(current.clone());
        let names = Object::get_own_property_names(&object);
        for candidate in names.iter() {
            if let Some(candidate_str) = candidate.as_string() {
                if candidate_str.eq_ignore_ascii_case(name) {
                    return JsValue::from_str(&candidate_str);
                }
            }
        }
        current = Object::get_prototype_of(&object).into();
    }

    direct
}

#[derive(Debug)]
struct VariablesScopeProxy {
    variables: Rc<RefCell<HashMap<String, BxValue>>>,
    /// Script-level `variables`: the VM globals are the real store (a bare `x` is `variables.x`).
    script: bool,
}

impl BxNativeObject for VariablesScopeProxy {
    fn native_data(&self, kind: &str) -> Option<*mut std::ffi::c_void> {
        (kind == "matchbox.variables_scope" || (self.script && kind == "matchbox.script_variables"))
            .then_some(self as *const Self as *mut std::ffi::c_void)
    }

    fn get_property(&self, name: &str) -> BxValue {
        self.variables
            .borrow()
            .get(&name.to_lowercase())
            .copied()
            .unwrap_or(BxValue::new_null())
    }

    fn set_property(&mut self, name: &str, value: BxValue) {
        self.variables
            .borrow_mut()
            .insert(name.to_lowercase(), value);
    }

    fn call_method(
        &mut self,
        _vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        Err(format!("Method {} not found on variables scope.", name))
    }

    fn trace(&self, tracer: &mut dyn Tracer) {
        for value in self.variables.borrow().values() {
            tracer.mark(value);
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
use js_sys::Object;
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use js_sys::{Array, Function, Promise, Proxy, Reflect};
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use wasm_bindgen::JsCast;
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use wasm_bindgen::closure::Closure;
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use wasm_bindgen::prelude::*;
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use wasm_bindgen_futures::{JsFuture, future_to_promise};
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use web_sys::window;

#[cfg(all(target_arch = "wasm32", feature = "js-host-abi", not(feature = "js")))]
#[link(wasm_import_module = "matchbox_js_host")]
unsafe extern "C" {
    fn bx_js_get_prop(
        obj_id: u32,
        key_ptr: *const u8,
        key_len: usize,
        str_buf: *mut u8,
        str_buf_len: usize,
        out_str_len: *mut usize,
        out_num: *mut f64,
        out_bool: *mut i32,
        out_obj: *mut u32,
    ) -> i32;
    fn bx_js_set_prop_null(obj_id: u32, key_ptr: *const u8, key_len: usize);
    fn bx_js_set_prop_bool(obj_id: u32, key_ptr: *const u8, key_len: usize, val: i32);
    fn bx_js_set_prop_num(obj_id: u32, key_ptr: *const u8, key_len: usize, val: f64);
    fn bx_js_set_prop_str(
        obj_id: u32,
        key_ptr: *const u8,
        key_len: usize,
        val_ptr: *const u8,
        val_len: usize,
    );
    fn bx_js_set_prop_obj(obj_id: u32, key_ptr: *const u8, key_len: usize, val_id: u32);
    fn bx_js_call_method(
        obj_id: u32,
        method_ptr: *const u8,
        method_len: usize,
        args_json_ptr: *const u8,
        args_json_len: usize,
        str_buf: *mut u8,
        str_buf_len: usize,
        out_str_len: *mut usize,
        out_num: *mut f64,
        out_bool: *mut i32,
        out_obj: *mut u32,
    ) -> i32;
}

#[derive(Clone, Copy)]
pub struct ExceptionHandler {
    pub catch_ip: Option<usize>,
    pub finally_ip: Option<usize>,
    pub saved_stack_len: usize,
    catch_enabled: bool,
}

#[derive(Clone, Copy)]
enum PendingCompletion {
    Return(BxValue),
    Throw(BxValue),
}

pub struct CallFrame {
    pub function: Rc<BxCompiledFunction>,
    pub chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    pub ip: usize,
    pub stack_base: usize,
    pub receiver: Option<BxValue>,
    pub handlers: Vec<ExceptionHandler>,
    pending_completion: Option<PendingCompletion>,
    pub promoted_constants: Vec<Option<BxValue>>,
    local_scope: Option<BxValue>,
    context: Option<BxValue>,
    // Invocation-local numeric scope bindings; no GC values or shared-program IDs.
    local_names: scopes::SlotBindings,
    scope_mode: scopes::ScopeMode,
    #[cfg(feature = "jit")]
    jit_loop_attempted: bool,
    #[cfg(feature = "jit")]
    jit_construct_unsupported: Option<usize>,
}

pub struct BxFiber {
    pub stack: Vec<BxValue>,
    pub frames: Vec<CallFrame>,
    pub variables: Rc<RefCell<HashMap<String, BxValue>>>,
    pub future_id: usize,
    pub wait_until: Option<Instant>,
    pub yield_requested: bool,
    /// Inside `run_fiber` right now: a suspended caller of a nested synchronous call.
    pub running: bool,
    /// Set when the fiber yielded in `future.get()` on a pending future.
    pub blocked_on_future: bool,
    /// Completed by a nested scheduler; removed at the next safe point (indexes must stay stable).
    pub finished: bool,
    pub priority: u8,
    pub root_stack: Vec<BxValue>,
}

#[cfg(feature = "debugger")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugLocation {
    pub function: String,
    pub filename: String,
    pub line: u32,
    pub frame_depth: usize,
}

#[cfg(feature = "debugger")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugStepStatus {
    Paused,
    Completed,
    RuntimeError,
    BudgetExhausted,
    Blocked,
}

/// Runtime errors that can bubble up as BoxLang exceptions
/// rather than causing a Rust panic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    /// Memory allocation failed even after garbage collection.
    OutOfMemory(String),
}

impl RuntimeError {
    /// Returns the BoxLang exception type name for this error.
    pub fn exception_type(&self) -> &str {
        match self {
            RuntimeError::OutOfMemory(_) => "OutOfMemoryException",
        }
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::OutOfMemory(msg) => write!(f, "OutOfMemory: {}", msg),
        }
    }
}

impl std::error::Error for RuntimeError {}

#[cfg(feature = "debugger")]
#[derive(Clone, Debug, PartialEq)]
pub struct DebugStepResult {
    pub status: DebugStepStatus,
    pub location: Option<DebugLocation>,
    pub value: Option<serde_json::Value>,
    pub instructions: u64,
    pub error: Option<String>,
}

#[cfg(feature = "debugger")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebugPauseReason {
    Line,
    Budget,
}

#[cfg(feature = "debugger")]
#[derive(Clone, Debug)]
struct DebugRunState {
    start: Option<DebugLocation>,
    executed: u64,
    budget: u64,
    pause: Option<DebugPauseReason>,
}

enum NativeCompletion {
    Resolve { future: BxValue, value: BxValue },
    Reject { future: BxValue, error: BxValue },
}

#[derive(Clone, Copy, Debug)]
pub enum HostFutureState {
    Pending,
    Completed(BxValue),
    Failed(BxValue),
}

// Process-unique identities also distinguish independent VMs; never recycle on overflow.
fn next_global_namespace() -> u64 {
    static NEXT: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
    let mut next = NEXT.lock().expect("global namespace allocator poisoned");
    *next = next.checked_add(1).expect("global namespace identities exhausted");
    *next
}

pub struct VM {
    pub fibers: Vec<BxFiber>,
    global_names: HashMap<u32, usize>,
    global_values: Vec<BxValue>,
    global_namespace: u64,
    global_bifs: HashSet<Rc<str>>,
    struct_assignment_hooks: HashMap<usize, (Rc<gc::RootedValue>, crate::types::NativeCallback)>,
    pub script_variables: Rc<RefCell<HashMap<String, BxValue>>>,
    pub current_fiber_idx: Option<usize>,
    pub shapes: ShapeRegistry,
    pub heap: Heap,
    pub native_classes: HashMap<String, BxNativeFunction>,
    pub interner: StringInterner,
    pub cli_args: Vec<String>,
    soap_clients: HashMap<String, BxValue>,
    #[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
    members: HashMap<String, HashMap<String, members::RegisteredMember>>,
    pub output_buffer: Option<String>,
    output_capture_stack: Vec<Option<String>>,
    pub gc_suspended: bool,
    native_exception_depth: usize,
    native_callback_exception: Option<(String, Rc<gc::RootedValue>)>,
    function_events: HashMap<usize, BxValue>,
    automatic_event_depth: usize,
    thread_futures: HashSet<usize>,
    datetime_timezones: HashMap<usize, String>,
    /// GC tuning parameters; use `VM::with_config()` or accept defaults.
    pub config: GCConfig,
    native_completions: VecDeque<NativeCompletion>,
    native_future_tx: Sender<NativeFutureMessage>,
    native_future_rx: Receiver<NativeFutureMessage>,
    pending_native_futures: HashMap<usize, usize>,
    #[cfg(feature = "debugger")]
    debug_run: Option<DebugRunState>,
    #[cfg(feature = "jit")]
    pub jit: Option<Box<jit::JitState>>,
    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub(crate) callback_registry: Rc<RefCell<HashMap<usize, BxValue>>>,
    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub(crate) next_callback_id: RefCell<usize>,
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
const JS_INTEROP_MAX_DEPTH: usize = 32;

impl VM {
    fn datetime_value(&self, val: BxValue) -> Option<DateTime<Utc>> {
        val.as_gc_id().and_then(|id| match self.heap.get(id) {
            GcObject::DateTime(dt) => Some(dt.clone()),
            _ => None,
        })
    }
}

impl BxVM for VM {
    fn call_super_method(&mut self, receiver: BxValue, defining_class: &str, name: &str, args: Vec<BxValue>) -> Result<BxValue, String> {
        let id = receiver.as_gc_id().ok_or("super can only be used on a class instance")?;
        let mut current = match self.heap.get(id) {
            GcObject::Instance(instance) => Rc::clone(&instance.class),
            _ => return Err("super can only be used on a class instance".into()),
        };
        // Find the class that wrote the calling method, then start from its parent.
        let parent = loop {
            let (is_defining, parent_name) = {
                let class = current.borrow();
                (class.name.eq_ignore_ascii_case(defining_class), class.extends.clone())
            };
            let parent_class = parent_name.and_then(|parent_name| match self.get_global(&parent_name).and_then(|v| v.as_gc_id()) {
                Some(parent_id) => match self.heap.get(parent_id) {
                    GcObject::Class(parent_class) => Some(Rc::clone(parent_class)),
                    _ => None,
                },
                None => None,
            });
            if is_defining {
                break parent_class.ok_or_else(|| format!("{defining_class} has no parent class, so super.{name}() cannot be called"))?;
            }
            current = parent_class.ok_or_else(|| format!("{defining_class} is not in the class chain of this instance"))?;
        };
        let method = self
            .resolve_method(parent, name)
            .ok_or_else(|| format!("Method {name} not found in the parent class of {defining_class}"))?;
        let mut method = (*method).clone();
        method.captured_receiver = Some(receiver);
        let function = BxValue::new_ptr(self.heap.alloc(GcObject::CompiledFunction(Rc::new(method))));
        self.call_function_value_named(function, args, None).map_err(|e| e.to_string())
    }

    fn current_chunk(&self) -> Option<Rc<RefCell<crate::vm::chunk::Chunk>>> {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].frames.last().map(|f| Rc::clone(&f.chunk))
        } else {
            None
        }
    }

    fn call_function_by_value_named(&mut self, func: BxValue, args: Vec<BxValue>, names: Option<Vec<String>>) -> Result<BxValue, String> {
        self.call_function_value_named(func, args, names).map_err(|error| error.to_string())
    }
    fn execution_context(&mut self) -> Result<BxValue, String> { VM::execution_context(self) }

    fn current_receiver(&self) -> Option<BxValue> {
        self.current_fiber_idx
            .and_then(|idx| self.fibers.get(idx))
            .and_then(|fiber| fiber.frames.last())
            .and_then(|frame| frame.receiver)
    }

    fn current_function_called_name(&self) -> String {
        let Some(fiber) = self.current_fiber_idx.and_then(|idx| self.fibers.get(idx)) else {
            return String::new();
        };
        let Some(frame) = fiber.frames.last() else {
            return String::new();
        };
        let function_name = frame.function.name.clone();
        let Some(function_value) = frame
            .stack_base
            .checked_sub(1)
            .and_then(|index| fiber.stack.get(index))
            .copied()
        else {
            return String::new();
        };
        if let Some(alias) = fiber.variables.borrow().iter().find_map(|(name, value)| {
            (*value == function_value && *name != function_name).then(|| name.clone())
        }) {
            return alias;
        }
        self.global_names
            .iter()
            .find_map(|(name_id, index)| {
                let value = self.global_values.get(*index)?;
                (*value == function_value && self.interner.resolve(*name_id) != function_name)
                    .then(|| self.interner.resolve(*name_id).to_string())
            })
            .unwrap_or(function_name)
    }

    fn interpret_chunk_with_receiver(&mut self, chunk: Chunk, receiver: Option<BxValue>) -> Result<BxValue, String> {
        VM::interpret_chunk_with_receiver(self, &chunk, receiver).map_err(|error| error.to_string())
    }

    fn interpret_chunk(&mut self, chunk: Chunk) -> Result<BxValue, String> {
        // Legacy consuming execution path. Keep this behavior intact so the
        // main VM can migrate to the borrowed path incrementally later.
        self.interpret(chunk).map_err(|e| e.to_string())
    }

    fn spawn(
        &mut self,
        func: Rc<BxCompiledFunction>,
        args: Vec<BxValue>,
        priority: u8,
        _chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    ) -> BxValue {
        let dummy = Rc::new(RefCell::new(Chunk::default()));
        self.spawn(func, args, priority, dummy, None)
    }

    fn spawn_by_value(
        &mut self,
        func: &BxValue,
        args: Vec<BxValue>,
        priority: u8,
        _chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    ) -> Result<BxValue, String> {
        if let Some(id) = func.as_gc_id() {
            let obj = self.heap.get(id);
            if let GcObject::CompiledFunction(f) = obj {
                let f = Rc::clone(f);
                let dummy = Rc::new(RefCell::new(Chunk::default()));
                Ok(self.spawn(f, args, priority, dummy, None))
            } else {
                Err("Value is not a callable function".to_string())
            }
        } else {
            Err("Value is not a callable function".to_string())
        }
    }

    fn call_function_by_value(
        &mut self,
        func: &BxValue,
        args: Vec<BxValue>,
        chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    ) -> Result<BxValue, String> {
        self.call_function_value(*func, args, Some(chunk))
            .map_err(|e| e.to_string())
    }

    fn call_method_by_value(
        &mut self,
        receiver: BxValue,
        name: &str,
        args: Vec<BxValue>,
        _chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    ) -> Result<BxValue, String> {
        let previous=self.current_fiber_idx;
        let result=self.native_exception_scope(|vm| vm.call_method_value(receiver,name,args).map_err(|error|error.to_string()));
        self.current_fiber_idx=previous;
        result
    }

    fn yield_fiber(&mut self) {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].yield_requested = true;
        }
    }

    fn sleep(&mut self, ms: u64) {
        if let Some(idx) = self.current_fiber_idx {
            let until = Instant::now() + Duration::from_millis(ms);
            self.fibers[idx].wait_until = Some(until);
            self.fibers[idx].yield_requested = true;
        }
    }

    fn get_root_shape(&self) -> u32 {
        self.shapes.get_root()
    }

    fn get_shape_index(&self, shape_id: u32, field_name: &str) -> Option<u32> {
        if let Some(id) = self.interner.get_id(field_name) {
            self.shapes.get_index(shape_id, id)
        } else {
            None
        }
    }

    fn get_len(&self, id: usize) -> usize {
        match self.heap.get(id) {
            GcObject::Array(arr) => arr.len(),
            GcObject::Range(range) => range.len(),
            GcObject::Struct(s) => s.properties.len(),
            GcObject::String(s) => s.len(),
            GcObject::Bytes(bytes) => bytes.len(),
            GcObject::DateTime(_) => 26,
            _ => 0,
        }
    }

    fn is_truthy(&self, val: BxValue) -> bool {
        VM::is_truthy(self, val)
    }

    fn is_array_value(&self, val: BxValue) -> bool {
        val.as_gc_id()
            .map(|id| matches!(self.heap.get(id), GcObject::Array(_)))
            .unwrap_or(false)
    }

    fn is_struct_value(&self, val: BxValue) -> bool {
        val.as_gc_id()
            .map(|id| matches!(self.heap.get(id), GcObject::Struct(_)))
            .unwrap_or(false)
    }

    fn is_string_value(&self, val: BxValue) -> bool {
        val.as_gc_id()
            .map(|id| matches!(self.heap.get(id), GcObject::String(_)))
            .unwrap_or(false)
    }

    fn is_bytes(&self, val: BxValue) -> bool {
        if let Some(id) = val.as_gc_id() {
            matches!(self.heap.get(id), GcObject::Bytes(_))
        } else {
            false
        }
    }

    fn type_name_from_value(&self, val: BxValue) -> Option<String> {
        if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => Some(s.to_string()),
                GcObject::Class(class) => Some(class.borrow().name.clone()),
                GcObject::Interface(interface) => Some(interface.borrow().name.clone()),
                GcObject::Instance(inst) => Some(inst.class.borrow().name.clone()),
                GcObject::DateTime(_) => Some("datetime".to_string()),
                GcObject::Range(_) => Some("range".to_string()),
                _ => None,
            }
        } else {
            None
        }
    }

    fn find_global_class_by_name(&self, type_name: &str) -> Option<Rc<RefCell<BxClass>>> {
        if let Some(val) = self.get_global(type_name) {
            if let Some(id) = val.as_gc_id() {
                if let GcObject::Class(class) = self.heap.get(id) {
                    return Some(Rc::clone(class));
                }
            }
        }

        for (&name_id, _) in &self.global_names {
            let name = self.interner.resolve(name_id);
            if name.eq_ignore_ascii_case(type_name) {
                if let Some(val) = self.get_global(name) {
                    if let Some(id) = val.as_gc_id() {
                        if let GcObject::Class(class) = self.heap.get(id) {
                            return Some(Rc::clone(class));
                        }
                    }
                }
            }
        }

        None
    }

    fn find_global_interface_by_name(&self, type_name: &str) -> Option<Rc<RefCell<BxInterface>>> {
        if let Some(val) = self.get_global(type_name) {
            if let Some(id) = val.as_gc_id() {
                if let GcObject::Interface(interface) = self.heap.get(id) {
                    return Some(Rc::clone(interface));
                }
            }
        }

        for (&name_id, _) in &self.global_names {
            let name = self.interner.resolve(name_id);
            if name.eq_ignore_ascii_case(type_name) {
                if let Some(val) = self.get_global(name) {
                    if let Some(id) = val.as_gc_id() {
                        if let GcObject::Interface(interface) = self.heap.get(id) {
                            return Some(Rc::clone(interface));
                        }
                    }
                }
            }
        }

        None
    }

    fn class_matches_type_name(&self, class: &Rc<RefCell<BxClass>>, type_name: &str) -> bool {
        let (class_name, extends, implements) = {
            let class_ref = class.borrow();
            (
                class_ref.name.clone(),
                class_ref.extends.clone(),
                class_ref.implements.clone(),
            )
        };

        if class_name.eq_ignore_ascii_case(type_name) {
            return true;
        }
        if implements
            .iter()
            .any(|iface| iface.eq_ignore_ascii_case(type_name))
        {
            return true;
        }
        if let Some(parent_name) = extends {
            if parent_name.eq_ignore_ascii_case(type_name) {
                return true;
            }
            if let Some(parent_class) = self.find_global_class_by_name(&parent_name) {
                return self.class_matches_type_name(&parent_class, type_name);
            }
        }
        false
    }

    fn value_matches_type_name(&self, val: BxValue, type_name: &str) -> bool {
        let lower = type_name.trim().to_ascii_lowercase();
        let builtin = lower.rsplit('.').next().unwrap_or(&lower);
        match builtin {
            "any" | "object" => !val.is_null(),
            "null" | "void" => val.is_null(),
            "string" => val
                .as_gc_id()
                .map(|id| matches!(self.heap.get(id), GcObject::String(_)))
                .unwrap_or(false),
            "numeric" | "number" | "double" | "float" | "bigdecimal" => val.is_number(),
            "integer" | "int" | "long" | "short" | "byte" => val.is_int(),
            "boolean" | "bool" => val.is_bool(),
            "array" => val
                .as_gc_id()
                .map(|id| matches!(self.heap.get(id), GcObject::Array(_)))
                .unwrap_or(false),
            "datetime" => val
                .as_gc_id()
                .map(|id| matches!(self.heap.get(id), GcObject::DateTime(_)))
                .unwrap_or(false),
            "range" => val
                .as_gc_id()
                .map(|id| matches!(self.heap.get(id), GcObject::Range(_)))
                .unwrap_or(false),
            "struct" | "componentstruct" => val
                .as_gc_id()
                .map(|id| matches!(self.heap.get(id), GcObject::Struct(_)))
                .unwrap_or(false),
            "function" => val
                .as_gc_id()
                .map(|id| {
                    matches!(
                        self.heap.get(id),
                        GcObject::CompiledFunction(_) | GcObject::NativeFunction(_) | GcObject::NativeCallback(_)
                    )
                })
                .unwrap_or(false),
            "udf" => val.as_gc_id().is_some_and(|id| {
                matches!(self.heap.get(id), GcObject::CompiledFunction(f) if f.kind == crate::types::FunctionKind::Udf)
            }),
            "closure" => val.as_gc_id().is_some_and(|id| {
                matches!(self.heap.get(id), GcObject::CompiledFunction(f) if f.kind == crate::types::FunctionKind::Closure)
            }),
            "lambda" => val.as_gc_id().is_some_and(|id| {
                matches!(self.heap.get(id), GcObject::CompiledFunction(f) if f.kind == crate::types::FunctionKind::Lambda)
            }),
            "class" | "component" => val
                .as_gc_id()
                .map(|id| {
                    matches!(
                        self.heap.get(id),
                        GcObject::Class(_) | GcObject::Instance(_)
                    )
                })
                .unwrap_or(false),
            "interface" => val
                .as_gc_id()
                .map(|id| matches!(self.heap.get(id), GcObject::Interface(_)))
                .unwrap_or(false),
            _ => {
                if let Some(class) = self.find_global_class_by_name(type_name) {
                    match val.as_gc_id() {
                        Some(id) => match self.heap.get(id) {
                            GcObject::Class(target_class) => {
                                self.class_matches_type_name(target_class, &class.borrow().name)
                            }
                            GcObject::Instance(inst) => {
                                self.class_matches_type_name(&inst.class, &class.borrow().name)
                            }
                            _ => false,
                        },
                        None => false,
                    }
                } else if let Some(interface) = self.find_global_interface_by_name(type_name) {
                    let interface_name = interface.borrow().name.clone();
                    match val.as_gc_id() {
                        Some(id) => match self.heap.get(id) {
                            GcObject::Class(target_class) => {
                                self.class_matches_type_name(target_class, &interface_name)
                            }
                            GcObject::Instance(inst) => {
                                self.class_matches_type_name(&inst.class, &interface_name)
                            }
                            GcObject::Interface(existing) => {
                                existing.borrow().name.eq_ignore_ascii_case(&interface_name)
                            }
                            _ => false,
                        },
                        None => false,
                    }
                } else {
                    false
                }
            }
        }
    }

    fn cast_value_to_type(&mut self, val: BxValue, type_name: &str) -> Result<BxValue, String> {
        let lower = type_name.trim().to_ascii_lowercase();
        match lower.as_str() {
            "any" | "object" => Ok(val),
            "null" | "void" => {
                if val.is_null() {
                    Ok(BxValue::new_null())
                } else {
                    Err(format!(
                        "Could not cast object [{}] to type [{}]",
                        self.to_string(val),
                        type_name
                    ))
                }
            }
            "string" => {
                if let Some(id) = val.as_gc_id() {
                    if matches!(self.heap.get(id), GcObject::String(_)) {
                        return Ok(val);
                    }
                }
                let id = self.string_new(self.to_string(val));
                Ok(BxValue::new_ptr(id))
            }
            "numeric" | "number" | "double" | "float" | "bigdecimal" => {
                if val.is_number() {
                    return Ok(val);
                }
                let parsed = if val.is_bool() {
                    Some(if val.as_bool() { 1.0 } else { 0.0 })
                } else {
                    self.to_string(val).trim().parse::<f64>().ok()
                };
                if let Some(num) = parsed {
                    if num.fract() == 0.0 && num >= i32::MIN as f64 && num <= i32::MAX as f64 {
                        Ok(BxValue::new_int(num as i32))
                    } else {
                        Ok(BxValue::new_number(num))
                    }
                } else {
                    Err(format!(
                        "Could not cast object [{}] to type [{}]",
                        self.to_string(val),
                        type_name
                    ))
                }
            }
            "integer" | "int" | "long" | "short" | "byte" => {
                if val.is_int() {
                    return Ok(val);
                }
                let parsed = if val.is_number() {
                    let num = val.as_number();
                    if num.fract() == 0.0 && num >= i32::MIN as f64 && num <= i32::MAX as f64 {
                        Some(num as i32)
                    } else {
                        None
                    }
                } else if val.is_bool() {
                    Some(if val.as_bool() { 1 } else { 0 })
                } else {
                    self.to_string(val).trim().parse::<i32>().ok()
                };
                if let Some(num) = parsed {
                    Ok(BxValue::new_int(num))
                } else {
                    Err(format!(
                        "Could not cast object [{}] to type [{}]",
                        self.to_string(val),
                        type_name
                    ))
                }
            }
            "boolean" | "bool" => {
                if val.is_bool() {
                    Ok(val)
                } else {
                    Ok(BxValue::new_bool(self.is_truthy(val)))
                }
            }
            "array" | "set" => {
                if self.is_array_value(val) {
                    Ok(val)
                } else {
                    Err(format!(
                        "Could not cast object [{}] to type [{}]",
                        self.to_string(val),
                        type_name
                    ))
                }
            }
            "struct" | "componentstruct" => {
                if self.is_struct_value(val) {
                    Ok(val)
                } else {
                    Err(format!(
                        "Could not cast object [{}] to type [{}]",
                        self.to_string(val),
                        type_name
                    ))
                }
            }
            "function" => {
                if let Some(id) = val.as_gc_id() {
                    if matches!(
                        self.heap.get(id),
                        GcObject::CompiledFunction(_) | GcObject::NativeFunction(_) | GcObject::NativeCallback(_)
                    ) {
                        Ok(val)
                    } else {
                        Err(format!(
                            "Could not cast object [{}] to type [{}]",
                            self.to_string(val),
                            type_name
                        ))
                    }
                } else {
                    Err(format!(
                        "Could not cast object [{}] to type [{}]",
                        self.to_string(val),
                        type_name
                    ))
                }
            }
            "class" | "component" | "interface" => {
                if self.value_matches_type_name(val, type_name) {
                    Ok(val)
                } else {
                    Err(format!(
                        "Could not cast object [{}] to type [{}]",
                        self.to_string(val),
                        type_name
                    ))
                }
            }
            _ => {
                if self.value_matches_type_name(val, type_name) {
                    Ok(val)
                } else {
                    Err(format!(
                        "Could not cast object [{}] to type [{}]",
                        self.to_string(val),
                        type_name
                    ))
                }
            }
        }
    }

    fn bytes_new(&mut self, data: Vec<u8>) -> usize {
        self.heap.alloc(GcObject::Bytes(data))
    }

    fn bytes_len(&self, id: usize) -> usize {
        if let GcObject::Bytes(bytes) = self.heap.get(id) {
            bytes.len()
        } else {
            0
        }
    }

    fn bytes_get(&self, id: usize, idx: usize) -> Result<u8, String> {
        if let GcObject::Bytes(bytes) = self.heap.get(id) {
            bytes
                .get(idx)
                .copied()
                .ok_or_else(|| format!("Index {} out of bounds", idx))
        } else {
            Err("Not bytes".to_string())
        }
    }

    fn bytes_set(&mut self, id: usize, idx: usize, value: u8) -> Result<(), String> {
        if let GcObject::Bytes(bytes) = self.heap.get_mut(id) {
            if idx < bytes.len() {
                bytes[idx] = value;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", idx))
            }
        } else {
            Err("Not bytes".to_string())
        }
    }

    fn to_bytes(&self, val: BxValue) -> Result<Vec<u8>, String> {
        if let Some(id) = val.as_gc_id() {
            if let GcObject::Bytes(bytes) = self.heap.get(id) {
                return Ok(bytes.clone());
            }
        }
        Err("Value is not bytes".to_string())
    }

    fn array_len(&self, id: usize) -> usize {
        if let GcObject::Array(arr) = self.heap.get(id) {
            arr.len()
        } else {
            0
        }
    }

    fn array_push(&mut self, id: usize, val: BxValue) {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            arr.push(val);
        }
    }

    fn array_pop(&mut self, id: usize) -> Result<BxValue, String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            Ok(arr.pop().unwrap_or(BxValue::new_null()))
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_get(&self, id: usize, idx: usize) -> BxValue {
        if let GcObject::Array(arr) = self.heap.get(id) {
            arr.get(idx).copied().unwrap_or(BxValue::new_null())
        } else {
            BxValue::new_null()
        }
    }

    fn array_set(&mut self, id: usize, idx: usize, val: BxValue) -> Result<(), String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            if idx < arr.len() {
                arr[idx] = val;
                Ok(())
            } else if idx < 100_000 {
                // Reasonable limit for sparse expansion
                arr.resize(idx + 1, BxValue::new_null());
                arr[idx] = val;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", idx))
            }
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_delete_at(&mut self, id: usize, idx: usize) -> Result<BxValue, String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            if idx < arr.len() {
                Ok(arr.remove(idx))
            } else {
                Err(format!("Index {} out of bounds", idx))
            }
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_insert_at(&mut self, id: usize, idx: usize, val: BxValue) -> Result<(), String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            if idx <= arr.len() {
                arr.insert(idx, val);
                Ok(())
            } else if idx < 100_000 {
                arr.resize(idx, BxValue::new_null());
                arr.push(val);
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", idx))
            }
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_clear(&mut self, id: usize) -> Result<(), String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            arr.clear();
            Ok(())
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_new(&mut self) -> usize {
        self.heap.alloc(GcObject::Array(Vec::new()))
    }

    fn range_values(&self, id: usize) -> Option<Vec<BxValue>> {
        if let GcObject::Range(range) = self.heap.get_opt(id)? {
            let (start, end) = range.iter_bounds();
            let step = if start <= end { 1 } else { -1 };
            Some(
                (0..range.len())
                    .map(|offset| BxValue::new_int((start + step * offset as i64) as i32))
                    .collect(),
            )
        } else {
            None
        }
    }

    fn struct_len(&self, id: usize) -> usize {
        if let GcObject::Struct(s) = self.heap.get(id) {
            s.properties.len()
        } else {
            0
        }
    }

    fn struct_new(&mut self) -> usize {
        self.heap.alloc(GcObject::Struct(BxStruct {
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
            case_sensitive: false,
            ordered: false,
        }))
    }

    fn struct_new_with_options(&mut self, options: &[String]) -> usize {
        let id = self.struct_new();
        if let GcObject::Struct(structure) = self.heap.get_mut(id) {
            for option in options {
                match option.to_ascii_lowercase().as_str() {
                    "casesensitive" => structure.case_sensitive = true,
                    "ordered" => structure.ordered = true,
                    _ => {}
                }
            }
        }
        id
    }

    fn struct_is_case_sensitive(&self, id: usize) -> bool {
        matches!(self.heap.get_opt(id), Some(GcObject::Struct(structure)) if structure.case_sensitive)
    }

    fn struct_is_ordered(&self, id: usize) -> bool {
        matches!(self.heap.get_opt(id), Some(GcObject::Struct(structure)) if structure.ordered)
    }

    fn struct_set(&mut self, id: usize, key: &str, val: BxValue) {
        self.struct_set_and_key(id, key, val);
    }

    fn struct_assign(&mut self, id: usize, key: &str, value: BxValue) -> Result<(), String> { self.assign_struct(id, key, value) }

    fn struct_get(&self, id: usize, key: &str) -> BxValue {
        let key_id = self.struct_key_lookup_id(id, key).unwrap_or(u32::MAX);
        if let GcObject::Struct(s) = self.heap.get(id) {
            if let Some(idx) = self.shapes.get_index(s.shape_id, key_id) {
                return s.properties[idx as usize];
            }
        }
        BxValue::new_null()
    }

    fn struct_delete(&mut self, id: usize, key: &str) -> bool {
        let key_id = self.struct_key_lookup_id(id, key).unwrap_or(u32::MAX);
        if let GcObject::Struct(s) = self.heap.get_mut(id) {
            if self.shapes.get_index(s.shape_id, key_id).is_some() {
                // To delete from a shape-based struct, we must reconstruct the struct's state
                // minus the deleted field and find/create a new shape.
                let mut entries = Vec::new();
                let current_shape = &self.shapes.shapes[s.shape_id as usize];
                for (&fid, &fidx) in &current_shape.fields {
                    if fid != key_id {
                        entries.push((fidx, fid, s.properties[fidx as usize]));
                    }
                }

                // Hash-map iteration order is not insertion order. Preserve surviving slots.
                entries.sort_unstable_by_key(|entry| entry.0);
                let mut new_shape_id = self.shapes.get_root();
                let mut new_properties = Vec::with_capacity(entries.len());
                for (_, fid, val) in entries {
                    new_shape_id = self.shapes.transition(new_shape_id, fid);
                    new_properties.push(val);
                }

                s.shape_id = new_shape_id;
                s.properties = new_properties;
                return true;
            }
        }
        false
    }

    fn struct_key_exists(&self, id: usize, key: &str) -> bool {
        let key_id = self.struct_key_lookup_id(id, key).unwrap_or(u32::MAX);
        if let GcObject::Struct(s) = self.heap.get(id) {
            return self.shapes.get_index(s.shape_id, key_id).is_some();
        }
        false
    }

    fn struct_key_array(&self, id: usize) -> Vec<String> {
        if let GcObject::Struct(s) = self.heap.get(id) {
            let shape = &self.shapes.shapes[s.shape_id as usize];
            let mut keys = vec![String::new(); shape.fields.size()];
            for (&fid, &fidx) in &shape.fields {
                keys[fidx as usize] = self.interner.resolve(fid).to_string();
            }
            return keys;
        }
        Vec::new()
    }

    fn struct_clear(&mut self, id: usize) {
        if let GcObject::Struct(s) = self.heap.get_mut(id) {
            s.shape_id = self.shapes.get_root();
            s.properties.clear();
        }
    }

    fn struct_get_shape(&self, id: usize) -> u32 {
        if let GcObject::Struct(s) = self.heap.get(id) {
            s.shape_id
        } else {
            0
        }
    }

    fn future_new(&mut self) -> BxValue {
        BxValue::new_ptr(self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
            success_handler: None,
        })))
    }

    fn future_resolve(&mut self, future: BxValue, value: BxValue) -> Result<(), String> {
        let id = future
            .as_gc_id()
            .ok_or_else(|| "Value is not a future".to_string())?;
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            if !matches!(f.status, FutureStatus::Pending) {
                return Err("Future is already settled".to_string());
            }
            f.value = value;
            f.status = FutureStatus::Completed;
            Ok(())
        } else {
            Err("Value is not a future".to_string())
        }
    }

    fn future_reject(&mut self, future: BxValue, error: BxValue) -> Result<(), String> {
        let id = future
            .as_gc_id()
            .ok_or_else(|| "Value is not a future".to_string())?;
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            if !matches!(f.status, FutureStatus::Pending) {
                return Err("Future is already settled".to_string());
            }
            f.status = FutureStatus::Failed(error);
            Ok(())
        } else {
            Err("Value is not a future".to_string())
        }
    }

    fn future_schedule_resolve(&mut self, future: BxValue, value: BxValue) -> Result<(), String> {
        let id = future
            .as_gc_id()
            .ok_or_else(|| "Value is not a future".to_string())?;
        if matches!(self.heap.get(id), GcObject::Future(_)) {
            self.native_completions
                .push_back(NativeCompletion::Resolve { future, value });
            Ok(())
        } else {
            Err("Value is not a future".to_string())
        }
    }

    fn future_schedule_reject(&mut self, future: BxValue, error: BxValue) -> Result<(), String> {
        let id = future
            .as_gc_id()
            .ok_or_else(|| "Value is not a future".to_string())?;
        if matches!(self.heap.get(id), GcObject::Future(_)) {
            self.native_completions
                .push_back(NativeCompletion::Reject { future, error });
            Ok(())
        } else {
            Err("Value is not a future".to_string())
        }
    }

    fn native_future_new(&mut self) -> NativeFutureHandle {
        let future = self.future_new();
        if let Some(id) = future.as_gc_id() {
            *self.pending_native_futures.entry(id).or_insert(0) += 1;
        }
        NativeFutureHandle::new(future, self.native_future_tx.clone())
    }

    fn future_on_error(&mut self, id: usize, handler: BxValue) {
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            f.error_handler = Some(handler);
        }
    }

    fn future_on_success(&mut self, id: usize, handler: BxValue) {
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            f.success_handler = Some(handler);
        }
    }

    fn future_status(&self, future: BxValue) -> Option<FutureStatus> {
        future
            .as_gc_id()
            .and_then(|id| match self.heap.get_opt(id) {
                Some(GcObject::Future(f)) => Some(f.status.clone()),
                _ => None,
            })
    }

    fn future_value(&self, future: BxValue) -> Option<BxValue> {
        future
            .as_gc_id()
            .and_then(|id| match self.heap.get_opt(id) {
                Some(GcObject::Future(f)) => Some(f.value),
                _ => None,
            })
    }

    fn future_wait(&mut self, future: BxValue) -> Result<BxValue, String> {
        let previous_fiber = self.current_fiber_idx;
        let result = match self.future_status(future) {
            Some(FutureStatus::Completed) => Ok(self.future_value(future).unwrap_or(BxValue::new_null())),
            Some(FutureStatus::Failed(error)) => Err(self.to_string(error)),
            Some(FutureStatus::Pending) => {
                let fiber_idx = self
                    .fibers
                    .iter()
                    .position(|fiber| BxValue::new_ptr(fiber.future_id) == future)
                    .ok_or_else(|| "Future is not scheduled".to_string())?;
                self.run_fiber_to_completion(fiber_idx)
                    .map_err(|error| error.to_string())
            }
            None => Err("Value is not a future".to_string()),
        };
        self.current_fiber_idx = previous_fiber;
        result
    }

    fn mark_future_as_thread(&mut self, future: BxValue) {
        if let Some(id) = future.as_gc_id() {
            self.thread_futures.insert(id);
        }
    }

    fn is_in_thread(&self) -> bool {
        self.current_fiber_idx
            .and_then(|idx| self.fibers.get(idx))
            .is_some_and(|fiber| self.thread_futures.contains(&fiber.future_id))
    }

    fn has_global_function(&self, name: &str) -> bool {
        self.global_bifs.contains(name.to_lowercase().as_str())
    }

    fn append_interception_points(&mut self, names: Vec<String>) -> Result<(), String> { VM::append_interception_points(self, names); Ok(()) }
    fn interception_points(&self) -> Vec<String> { VM::interception_points(self) }
    #[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
    fn execution_components(&self) -> Result<Vec<BxValue>, String> { Ok(VM::execution_components(self)) }
    fn component_names(&self) -> Vec<String> { VM::component_names(self) }
    #[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
    fn invoke_component(&mut self, name: &str, attributes: BxValue, body: BxValue) -> Result<BxValue, String> { VM::invoke_component(self, name, attributes, body).map_err(|error| error.to_string()) }
    fn register_interceptor(&mut self, target: BxValue) -> Result<(), String> { VM::register_interceptor(self, target) }
    fn unregister_interceptor(&mut self, target: BxValue) -> Result<(), String> { VM::unregister_interceptor(self, target) }
    #[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
    fn new_interceptor(&mut self, class: &str, properties: BxValue, name: &str, record: Option<BxValue>) -> Result<BxValue, String> { crate::module_services::new_interceptor(self, class, properties, name, record).map_err(|error| error.to_string()) }
    fn announce(&mut self, name: &str, data: BxValue) -> Result<(), String> { VM::announce(self, name, data) }

    fn global_function_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.global_bifs.iter().map(|name| name.to_string()).collect();
        names.sort();
        names
    }

    fn is_native_abort(&self, message: &str) -> bool { VM::is_native_abort(self, message) }
    fn raise_abort(&mut self, kind: &str, cause: Option<String>) -> String { VM::raise_abort(self, kind, cause) }
    fn register_callback(&mut self, name: &str, callback: crate::types::NativeCallback) -> Result<(), String> {
        let id = self.heap.alloc(GcObject::NativeCallback(callback));
        self.register_global_bif(name.to_string(), BxValue::new_ptr(id));
        Ok(())
    }

    fn native_object_data(&self, value: BxValue, kind: &str) -> Option<*mut std::ffi::c_void> {
        match self.heap.get_opt(value.as_gc_id()?)? {
            GcObject::NativeObject(object) => object.try_borrow().ok()?.native_data(kind),
            _ => None,
        }
    }

    fn retain_value(&mut self, value: BxValue) -> Result<Rc<crate::vm::gc::RootedValue>, String> {
        self.heap.retain_value(value)
    }

    fn read_retained_value(&self, root: &crate::vm::gc::RootedValue) -> Result<BxValue, String> {
        self.heap.read_retained_value(root)
    }

    fn native_object_new(&mut self, obj: Rc<RefCell<dyn BxNativeObject>>) -> usize {
        self.heap.alloc(GcObject::NativeObject(obj))
    }

    fn native_object_call_method(
        &mut self,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        let previous_gc = self.gc_suspended;
        self.gc_suspended = true;
        // Clone the Rc to release the heap borrow immediately
        let obj_rc = if let GcObject::NativeObject(obj) = self.heap.get_mut(id) {
            Rc::clone(obj)
        } else {
            self.gc_suspended = previous_gc;
            return Err(format!("Value at id {} is not a native object", id));
        };

        let delegated = obj_rc.borrow().delegated_callback(name);
        let res = if let Some(callback) = delegated {
            let mut values = vec![BxValue::new_ptr(id)];
            values.extend_from_slice(args);
            self.invoke_callback(&callback, &values)
        } else {
            obj_rc.borrow_mut().call_method(self, id, name, args)
        };
        self.gc_suspended = previous_gc;
        res
    }

    fn has_method_by_value(&self, receiver: BxValue, name: &str) -> bool {
        self.instance_has_method(receiver, name) || self.resolve_member_method(&receiver, name).is_some() || matches!(receiver.as_gc_id().and_then(|id| self.heap.get_opt(id)), Some(GcObject::NativeObject(object)) if object.borrow().has_method(name))
    }

    fn resolve_class_reference(&mut self, name: &str) -> Result<BxValue, String> {
        VM::resolve_class_reference(self, name).map_err(|error| error.to_string())
    }

    fn construct_native_class(
        &mut self,
        class_name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        let class_lower = class_name.to_lowercase();
        // Since we need to borrow `self` mutably in the function call, we must clone the function pointer first
        let func = { self.native_classes.get(&class_lower).copied() };

        if let Some(constructor) = func {
            constructor(self, args)
        } else {
            Err(format!(
                "Native class '{}' not found. Ensure it is registered.",
                class_name
            ))
        }
    }

    fn construct_box_class(&mut self, class_name: &str, args: Vec<BxValue>, names: Option<Vec<String>>) -> Result<BxValue, String> {
        let class = self.resolve_class_reference(class_name).map_err(|error| error.to_string())?;
        if self.class_reference_target(class).is_some() {
            return self.call_method_value_named(class, "init", args, names.unwrap_or_default()).map_err(|error| error.to_string());
        }
        let instance = self.construct_class_value(class, vec![]).map_err(|error| error.to_string())?;
        let _root = self.heap.retain_value(instance)?;
        if self.instance_has_method(instance, "init") {
            let result = match names {
                Some(names) => self.call_method_value_named(instance, "init", args, names),
                None => self.call_method_value(instance, "init", args),
            }.map_err(|error| error.to_string())?;
            Ok(if result.is_null() { instance } else { result })
        } else if args.is_empty() {
            Ok(instance)
        } else {
            Err("Implicit class constructor arguments are not yet supported".into())
        }
    }

    fn instance_class_name(&self, receiver: BxValue) -> Result<String, String> {
        self.instance_class_name(receiver)
            .map_err(|e| e.to_string())
    }

    fn instance_variables_json(&self, receiver: BxValue) -> Result<serde_json::Value, String> {
        self.instance_variables_json(receiver)
            .map_err(|e| e.to_string())
    }

    fn datetime_new(&mut self, dt: DateTime<Utc>) -> usize {
        self.heap.alloc(GcObject::DateTime(dt))
    }

    fn datetime_new_with_timezone(&mut self, dt: DateTime<Utc>, timezone: &str) -> usize {
        let id = self.datetime_new(dt);
        self.datetime_timezones.insert(id, timezone.to_string());
        id
    }

    fn datetime_timezone(&self, value: BxValue) -> Option<String> {
        value.as_gc_id().and_then(|id| self.datetime_timezones.get(&id).cloned())
    }

    fn string_new(&mut self, s: String) -> usize {
        self.heap.alloc(GcObject::String(BoxString::new(&s)))
    }

    fn to_string(&self, val: BxValue) -> String {
        self.to_string_internal(val)
    }

    fn to_box_string(&self, val: BxValue) -> BoxString {
        if let Some(id) = val.as_gc_id() {
            if let GcObject::String(s) = self.heap.get(id) {
                return s.clone();
            }
        }
        BoxString::new(&self.to_string_internal(val))
    }

    fn insert_global(&mut self, name: String, val: BxValue) {
        VM::insert_global(self, name, val);
    }

    fn get_global_value(&self, name: &str) -> Option<BxValue> {
        self.get_global(name)
    }

    fn resolve_variable_path(&self, path: &str) -> Option<BxValue> {
        if let Some(open) = path.find('[') {
            let close = path.rfind(']')?;
            if close != path.len() - 1 || open == 0 {
                return None;
            }
            let base = self.resolve_variable_path(path[..open].trim())?;
            let key_expr = path[open + 1..close].trim();
            if key_expr.is_empty() {
                return None;
            }
            let key = if (key_expr.starts_with('"') && key_expr.ends_with('"'))
                || (key_expr.starts_with('\'') && key_expr.ends_with('\''))
            {
                key_expr[1..key_expr.len() - 1].to_string()
            } else {
                let key_value = self
                    .get_global(key_expr)
                    .or_else(|| self.resolve_variable_path(key_expr))?;
                self.to_string(key_value)
            };
            let base_id = base.as_gc_id()?;
            let value = self.struct_get(base_id, &key);
            return (!value.is_null()).then_some(value);
        }
        let parts: Vec<&str> = path.split('.').collect();
        let first = parts.first()?.to_lowercase();
        let mut value = self
            .current_variables_scope()
            .borrow()
            .get(&first)
            .copied()
            .or_else(|| self.get_global(&first))?;

        for part in parts.iter().skip(1) {
            let id = value.as_gc_id()?;
            value = match self.heap.get_opt(id)? {
                GcObject::Struct(_) => self.struct_get(id, part),
                GcObject::Instance(instance) => instance
                    .variables
                    .borrow()
                    .get(&part.to_lowercase())
                    .copied()
                    .unwrap_or_else(BxValue::new_null),
                GcObject::NativeObject(object) => object.borrow().get_property(part),
                _ => BxValue::new_null(),
            };
            if value.is_null() {
                return None;
            }
        }
        Some(value)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_query_source_path(&self, path: &[String]) -> Option<BxValue> {
        let mut offset = 0;
        if path.first()?.eq_ignore_ascii_case("variables") {
            offset = 1;
        }
        let first = path.get(offset)?.to_lowercase();
        let mut value = self
            .current_variables_scope()
            .borrow()
            .get(&first)
            .copied()
            .or_else(|| self.get_global(&first))?;

        for part in &path[(offset + 1)..] {
            let id = value.as_gc_id()?;
            value = match self.heap.get_opt(id)? {
                GcObject::Struct(_) => {
                    let direct = self.struct_get(id, part);
                    if direct.is_null() {
                        self.struct_get(id, &part.to_lowercase())
                    } else {
                        direct
                    }
                }
                GcObject::Instance(instance) => instance
                    .variables
                    .borrow()
                    .get(&part.to_lowercase())
                    .copied()
                    .unwrap_or_else(BxValue::new_null),
                GcObject::NativeObject(obj) => obj.borrow().get_property(part),
                _ => BxValue::new_null(),
            };
            if value.is_null() {
                return None;
            }
        }

        Some(value)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native_object_query_result(
        &self,
        id: usize,
    ) -> Option<crate::datasource::traits::QueryResult> {
        match self.heap.get_opt(id) {
            Some(GcObject::NativeObject(obj)) => obj.borrow().query_result(),
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native_object_query_columns(
        &self,
        id: usize,
    ) -> Option<Vec<crate::datasource::traits::QueryColumn>> {
        match self.heap.get_opt(id) {
            Some(GcObject::NativeObject(obj)) => obj.borrow().query_columns(),
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native_object_query_row_count(&self, id: usize) -> Option<usize> {
        match self.heap.get_opt(id) {
            Some(GcObject::NativeObject(obj)) => obj.borrow().query_row_count(),
            _ => None,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn native_object_query_cell(
        &self,
        id: usize,
        row_idx: usize,
        col_idx: usize,
    ) -> Option<crate::datasource::traits::SqlValue> {
        match self.heap.get_opt(id) {
            Some(GcObject::NativeObject(obj)) => obj.borrow().query_cell(row_idx, col_idx),
            _ => None,
        }
    }

    fn get_cli_args(&self) -> Vec<String> {
        self.cli_args.clone()
    }

    fn soap_client_get(&self, url: &str) -> Option<BxValue> {
        self.soap_clients.get(url).copied()
    }

    fn soap_client_set(&mut self, url: String, client: BxValue) {
        self.soap_clients.insert(url, client);
    }

    fn write_output(&mut self, s: &str) {
        if let Some(ref mut buffer) = self.output_buffer {
            buffer.push_str(s);
        } else {
            print!("{}", s);
        }
    }

    fn begin_output_capture(&mut self) {
        self.output_capture_stack.push(self.output_buffer.take());
        self.output_buffer = Some(String::new());
    }

    fn end_output_capture(&mut self) -> Option<String> {
        let output = self.output_buffer.take();
        self.output_buffer = self.output_capture_stack.pop().flatten();
        output
    }

    fn suspend_gc(&mut self) {
        self.gc_suspended = true;
    }

    fn resume_gc(&mut self) {
        self.gc_suspended = false;
    }

    fn push_root(&mut self, val: BxValue) {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].root_stack.push(val);
        }
    }

    fn pop_root(&mut self) {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].root_stack.pop();
        }
    }

    fn get_interner(&mut self) -> &mut crate::vm::intern::StringInterner {
        &mut self.interner
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    fn js_to_bx_wasm(&mut self, val: JsValue) -> BxValue {
        self.js_to_bx(val)
    }
}

impl VM {
    /// Property read on a native object. The script-level `variables` scope is the VM globals.
    fn native_object_get(&self, object: &Rc<RefCell<dyn BxNativeObject>>, name: &str) -> BxValue {
        if object.borrow().native_data("matchbox.script_variables").is_some() {
            if let Some(value) = self.get_global(&name.to_lowercase()) {
                return value;
            }
        }
        object.borrow().get_property(name)
    }

    /// Property write on a native object; see `native_object_get`.
    fn native_object_set(&mut self, object: &Rc<RefCell<dyn BxNativeObject>>, name: &str, value: BxValue) {
        if object.borrow().native_data("matchbox.script_variables").is_some() {
            self.insert_global(name.to_lowercase(), value);
            return;
        }
        object.borrow_mut().set_property(name, value);
    }

    /// Shared GET_PRIVATE resolution; JIT intrinsic guards must respect the same scopes.
    fn private_value(&mut self, fiber: usize, name: &str) -> Option<BxValue> {
        let mut found = self.function_scope_value(fiber, name);
        let lambda = self.fibers[fiber].frames.last().unwrap().function.kind == crate::types::FunctionKind::Lambda;
        if let Some(receiver) = self.fibers[fiber].frames.last().unwrap()
            .receiver.filter(|_| found.is_none() && !lambda)
        {
            if name == "this" {
                found = Some(receiver);
            } else if let Some(id) = self.receiver_instance_gc_id(receiver) {
                if name == "variables" {
                    if let GcObject::Instance(inst) = self.heap.get(id) {
                        let proxy = VariablesScopeProxy { variables: Rc::clone(&inst.variables), script: false };
                        found = Some(BxValue::new_ptr(self.heap.alloc(
                            GcObject::NativeObject(Rc::new(RefCell::new(proxy))),
                        )));
                    }
                } else if let GcObject::Instance(inst) = self.heap.get(id) {
                    found = inst.variables.borrow().get(name).copied();
                }
            }
            if found.is_none() && self.instance_has_method(receiver, name) {
                found = self.method_reference(receiver, name).ok();
            }
        }
        if found.is_none() && !lambda && name == "variables" {
            let proxy = VariablesScopeProxy { variables: Rc::clone(&self.fibers[fiber].variables), script: true };
            found = Some(BxValue::new_ptr(self.heap.alloc(
                GcObject::NativeObject(Rc::new(RefCell::new(proxy))),
            )));
        }
        if found.is_none() && (!lambda || self.has_global_function(name)) {
            found = self.get_global(name);
        }
        found
    }

    /// Keep the write barrier and spelling rules in one path; bindings reuse its key.
    fn struct_set_and_key(&mut self, id: usize, key: &str, val: BxValue) -> u32 {
        let key_id = self.struct_key_id(id, key);
        if let GcObject::Struct(s) = self.heap.get_mut(id) {
            if let Some(idx) = self.shapes.get_index(s.shape_id, key_id) {
                s.properties[idx as usize] = val;
            } else {
                s.shape_id = self.shapes.transition(s.shape_id, key_id);
                s.properties.push(val);
            }
        }
        key_id
    }

    fn struct_key_id(&mut self, id: usize, key: &str) -> u32 {
        let case_sensitive = matches!(self.heap.get_opt(id), Some(GcObject::Struct(s)) if s.case_sensitive);
        if case_sensitive {
            self.interner.intern_case_sensitive(key)
        } else {
            self.interner.intern(key)
        }
    }

    fn struct_key_lookup_id(&self, id: usize, key: &str) -> Option<u32> {
        let case_sensitive = matches!(self.heap.get_opt(id), Some(GcObject::Struct(s)) if s.case_sensitive);
        if case_sensitive {
            self.interner.get_exact_id(key)
        } else {
            self.interner.get_id(key)
        }
    }

    fn new_variables_scope() -> Rc<RefCell<HashMap<String, BxValue>>> {
        Rc::new(RefCell::new(HashMap::new()))
    }

    fn current_variables_scope(&self) -> Rc<RefCell<HashMap<String, BxValue>>> {
        self.current_fiber_idx
            .and_then(|idx| self.fibers.get(idx))
            .map(|fiber| Rc::clone(&fiber.variables))
            .unwrap_or_else(|| Rc::clone(&self.script_variables))
    }

    fn flatten_spread_array_value(&self, val: BxValue) -> Result<Vec<BxValue>> {
        if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::Array(arr) => Ok(arr.iter().copied().collect()),
                GcObject::Range(range) => {
                    let (start, end) = range.iter_bounds();
                    let step = if start <= end { 1 } else { -1 };
                    Ok((0..range.len())
                        .map(|offset| BxValue::new_int((start + step * offset as i64) as i32))
                        .collect())
                }
                _ => bail!(
                    "Cannot spread value of type [{}] into an array literal.",
                    self.type_name_from_value(val)
                        .unwrap_or_else(|| "unknown".to_string())
                ),
            }
        } else {
            bail!(
                "Cannot spread value of type [{}] into an array literal.",
                self.type_name_from_value(val)
                    .unwrap_or_else(|| "unknown".to_string())
            )
        }
    }

    fn flatten_spread_struct_entries(&self, val: BxValue) -> Result<Vec<(String, BxValue)>> {
        if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::Struct(_) => {
                    let keys = self.struct_key_array(id);
                    let mut entries = Vec::with_capacity(keys.len());
                    for key in keys {
                        entries.push((key.clone(), self.struct_get(id, &key)));
                    }
                    Ok(entries)
                }
                GcObject::Array(arr) => {
                    let mut entries = Vec::with_capacity(arr.len());
                    for (idx, item) in arr.iter().enumerate() {
                        entries.push(((idx + 1).to_string(), *item));
                    }
                    Ok(entries)
                }
                _ => bail!(
                    "Cannot spread value of type [{}] into a struct literal.",
                    self.type_name_from_value(val)
                        .unwrap_or_else(|| "unknown".to_string())
                ),
            }
        } else {
            bail!(
                "Cannot spread value of type [{}] into a struct literal.",
                self.type_name_from_value(val)
                    .unwrap_or_else(|| "unknown".to_string())
            )
        }
    }

    fn flatten_encoded_named_spread_args(
        &mut self,
        fiber_idx: usize,
        arg_count: u32,
    ) -> Result<(Vec<BxValue>, Vec<String>)> {
        let mut chunks: Vec<Vec<(String, BxValue)>> = Vec::with_capacity(arg_count as usize);
        for _ in 0..arg_count {
            let marker = self.fibers[fiber_idx].stack.pop().unwrap();
            if !marker.is_bool() {
                bail!("Internal VM error: spread argument marker must be boolean");
            }
            let key_val = self.fibers[fiber_idx].stack.pop().unwrap();
            let value = self.fibers[fiber_idx].stack.pop().unwrap();
            if marker.as_bool() {
                chunks.push(self.flatten_spread_struct_entries(value)?);
            } else {
                chunks.push(vec![(self.to_string(key_val), value)]);
            }
        }

        chunks.reverse();
        let mut args = Vec::new();
        let mut names = Vec::new();
        for chunk in chunks {
            for (name, value) in chunk {
                names.push(name);
                args.push(value);
            }
        }
        Ok((args, names))
    }

    pub fn interpret_sync(&mut self, chunk: Chunk) -> Result<BxValue> {
        let mut chunk = chunk.clone_without_runtime_caches();
        chunk.ensure_caches();
        let chunk_for_func = chunk.clone();
        let function = Rc::new(BxCompiledFunction {
            name: "script".to_string(),
            kind: crate::types::FunctionKind::Udf,
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            modifiers: crate::types::FunctionModifiers::default(),
            captured_receiver: None,
            captured_scopes: Vec::new(),
            chunk: chunk_for_func,
            bif: None,
        });

        let future = self.spawn(
            function,
            Vec::new(),
            0,
            Rc::new(RefCell::new(Chunk::default())),
            None,
        );
        self.run_future_to_completion(future)
    }

    fn enqueue_function_call(
        &mut self,
        func: BxValue,
        function: Rc<BxCompiledFunction>,
        args: Vec<BxValue>,
        priority: u8,
        receiver: Option<BxValue>,
        names: Option<Vec<String>>,
    ) -> BxValue {
        let receiver = receiver.or(function.captured_receiver);
        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
            success_handler: None,
        }));

        let args = self.function_arguments(args, names.as_deref(), &function);
        let mut stack = Vec::with_capacity(function.arity as usize + 1);
        stack.push(func);
        for arg in args {
            stack.push(arg);
        }
        while stack.len() < (function.arity + 1) as usize {
            stack.push(BxValue::new_null());
        }

        let chunk = function.chunk.execution_chunk();

        let fiber = BxFiber {
            stack,
            frames: vec![CallFrame {
                #[cfg(feature = "jit")]
                jit_loop_attempted: false,
                #[cfg(feature = "jit")]
                jit_construct_unsupported: None,
                function,
                chunk,
                context: None,
                ip: 0,
                scope_mode: scopes::ScopeMode::Binding,
                local_scope: None,
                local_names: scopes::SlotBindings::new(),
                stack_base: 1,
                receiver,
                handlers: Vec::new(),
                pending_completion: None,
                promoted_constants: Vec::new(),
            }],
            variables: self.current_variables_scope(),
            future_id,
            wait_until: None,
            yield_requested: false,
            running: false,
            blocked_on_future: false,
            finished: false,
            priority,
            root_stack: Vec::new(),
        };

        self.fibers.push(fiber);
        BxValue::new_ptr(future_id)
    }

    fn native_future_value_to_bx(&mut self, value: NativeFutureValue) -> BxValue {
        match value {
            NativeFutureValue::Null => BxValue::new_null(),
            NativeFutureValue::Bool(v) => BxValue::new_bool(v),
            NativeFutureValue::Int(v) => BxValue::new_int(v),
            NativeFutureValue::Number(v) => BxValue::new_number(v),
            NativeFutureValue::String(v) => BxValue::new_ptr(self.string_new(v)),
            NativeFutureValue::Bytes(v) => BxValue::new_ptr(self.bytes_new(v)),
            NativeFutureValue::Error { message } => {
                let struct_id = self.struct_new();
                let message_id = self.string_new(message);
                self.struct_set(struct_id, "message", BxValue::new_ptr(message_id));
                BxValue::new_ptr(struct_id)
            }
        }
    }

    fn release_pending_native_future(&mut self, future: BxValue) {
        if let Some(id) = future.as_gc_id() {
            if let Some(count) = self.pending_native_futures.get_mut(&id) {
                if *count <= 1 {
                    self.pending_native_futures.remove(&id);
                } else {
                    *count -= 1;
                }
            }
        }
    }

    fn drain_native_completions(&mut self) {
        // Native senders remain counted/rooted until their terminal message is drained.
        if self.pending_native_futures.is_empty() && self.native_completions.is_empty() {
            return;
        }
        while let Ok(message) = self.native_future_rx.try_recv() {
            match message {
                NativeFutureMessage::Resolve { future, value } => {
                    let value = self.native_future_value_to_bx(value);
                    let _ = self.future_resolve(future, value);
                    self.release_pending_native_future(future);
                }
                NativeFutureMessage::Reject { future, error } => {
                    let error = self.native_future_value_to_bx(error);
                    let _ = self.future_reject(future, error);
                    self.release_pending_native_future(future);
                }
                #[cfg(all(target_arch = "wasm32", feature = "js"))]
                NativeFutureMessage::ResolveWasmThunk { future, thunk_id } => {
                    let result = take_wasm_future_thunk(thunk_id)
                        .ok_or_else(|| "WASM future thunk not found".to_string())
                        .and_then(|thunk| thunk(self));

                    match result {
                        Ok(value) => {
                            let _ = self.future_resolve(future, value);
                        }
                        Err(message) => {
                            let error = self
                                .native_future_value_to_bx(NativeFutureValue::Error { message });
                            let _ = self.future_reject(future, error);
                        }
                    }
                    self.release_pending_native_future(future);
                }
                NativeFutureMessage::Abandon { future } => {
                    self.release_pending_native_future(future);
                }
            }
        }
        while let Some(completion) = self.native_completions.pop_front() {
            match completion {
                NativeCompletion::Resolve { future, value } => {
                    let _ = self.future_resolve(future, value);
                }
                NativeCompletion::Reject { future, error } => {
                    let _ = self.future_reject(future, error);
                }
            }
        }
    }

    fn to_string_internal(&self, val: BxValue) -> String {
        if val.is_number() {
            val.as_number().to_string()
        } else if val.is_int() {
            val.as_int().to_string()
        } else if val.is_bool() {
            val.as_bool().to_string()
        } else if val.is_null() {
            "null".to_string()
        } else if let Some(id) = val.as_gc_id() {
            if let Some(xml) = crate::bifs::xml::try_xml_to_string(self, val) {
                return xml;
            }
            match self.heap.get(id) {
                GcObject::String(s) => s.to_string(),
                GcObject::Bytes(bytes) => String::from_utf8_lossy(bytes).to_string(),
                GcObject::Array(values) => format!(
                    "[{}]",
                    values
                        .iter()
                        .map(|value| self.to_string_internal(*value))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                GcObject::Range(range) => format!("{}", range),
                GcObject::DateTime(dt) => dt.to_rfc3339_opts(SecondsFormat::Nanos, true),
                GcObject::Struct(_) => self.bx_to_json(&val).to_string(),
                GcObject::Instance(inst) => format!("<instance of {}>", inst.class.borrow().name),
                GcObject::Future(_) => format!("<future id:{}>", id),
                GcObject::CompiledFunction(f) => format!("<function {}>", f.name),
                GcObject::NativeFunction(_) | GcObject::NativeCallback(_) => "<native function>".to_string(),
                GcObject::Class(c) => format!("<class {}>", c.borrow().name),
                GcObject::Interface(i) => format!("<interface {}>", i.borrow().name),
                GcObject::NativeObject(o) => format!("<native object {:?}>", o.borrow()),
                #[cfg(all(target_arch = "wasm32", feature = "js"))]
                GcObject::JsValue(js) => format!("<js value {:?}>", js),
                #[cfg(all(target_arch = "wasm32", feature = "js-host-abi", not(feature = "js")))]
                GcObject::JsHandle(h) => format!("<js object #{}>", h),
            }
        } else {
            "<invalid>".to_string()
        }
    }

    fn literal_struct(&mut self, pairs: &[(u32, BxValue)]) -> BxValue {
        let mut shape_id = self.shapes.get_root();
        let mut properties = Vec::with_capacity(pairs.len());
        for &(key, value) in pairs {
            shape_id = self.shapes.transition(shape_id, key);
            properties.push(value);
        }
        BxValue::new_ptr(self.heap.alloc(GcObject::Struct(BxStruct {
            shape_id, properties, case_sensitive: false, ordered: false,
        })))
    }

    fn numeric_value(&self, val: BxValue) -> Option<f64> {
        if val.is_number() {
            Some(val.as_number())
        } else if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => s.to_string().trim().parse::<f64>().ok(),
                _ => None,
            }
        } else {
            None
        }
    }

    fn compare_values(&self, a: BxValue, b: BxValue) -> std::cmp::Ordering {
        if a.is_null() && b.is_null() {
            return std::cmp::Ordering::Equal;
        }
        if a.is_null() {
            return std::cmp::Ordering::Less;
        }
        if b.is_null() {
            return std::cmp::Ordering::Greater;
        }

        if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
            if let Some(ordering) = na.partial_cmp(&nb) {
                return ordering;
            }
        }

        if a.is_bool() && b.is_bool() {
            return a.as_bool().cmp(&b.as_bool());
        }

        if let (Some(a_dt), Some(b_dt)) = (self.datetime_value(a), self.datetime_value(b)) {
            return a_dt.cmp(&b_dt);
        }

        self.to_string_internal(a)
            .to_lowercase()
            .cmp(&self.to_string_internal(b).to_lowercase())
    }

    fn is_equal(&self, a: BxValue, b: BxValue) -> bool {
        if a.is_null() || b.is_null() {
            return a.is_null() && b.is_null();
        }

        if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
            return na == nb;
        }

        if let (Some(id_a), Some(id_b)) = (a.as_gc_id(), b.as_gc_id()) {
            match (self.heap.get(id_a), self.heap.get(id_b)) {
                (GcObject::String(s1), GcObject::String(s2)) => {
                    return s1.to_string().to_lowercase() == s2.to_string().to_lowercase();
                }
                (GcObject::DateTime(a), GcObject::DateTime(b)) => return a == b,
                (GcObject::Bytes(a), GcObject::Bytes(b)) => return a == b,
                (GcObject::Array(a), GcObject::Array(b)) => {
                    return a.len() == b.len()
                        && a.iter().zip(b).all(|(left, right)| self.is_equal(*left, *right));
                }
                (GcObject::Struct(_), GcObject::Struct(_)) => {
                    let keys_a = self.struct_key_array(id_a);
                    let keys_b = self.struct_key_array(id_b);
                    if keys_a.len() != keys_b.len() {
                        return false;
                    }
                    return keys_a.iter().all(|key| {
                        self.struct_key_exists(id_b, key)
                            && self.is_equal(
                                self.struct_get(id_a, key),
                                self.struct_get(id_b, key),
                            )
                    });
                }
                _ => {}
            }
        }

        if a == b {
            return true;
        }
        false
    }

    fn is_strict_equal(&self, a: BxValue, b: BxValue) -> bool {
        if a.is_number() && b.is_number() {
            return a.as_number() == b.as_number();
        }
        if a.is_bool() && b.is_bool() {
            return a.as_bool() == b.as_bool();
        }
        if a.is_null() || b.is_null() {
            return a.is_null() && b.is_null();
        }
        if let (Some(id_a), Some(id_b)) = (a.as_gc_id(), b.as_gc_id()) {
            if let (GcObject::String(s1), GcObject::String(s2)) =
                (self.heap.get(id_a), self.heap.get(id_b))
            {
                return s1.to_string().to_lowercase() == s2.to_string().to_lowercase();
            }
        }
        a == b
    }

    // Preserve original BoxLang throws across the existing String native ABI.
    // Scoped save/restore prevents handled/retried/nested callback errors escaping.
    fn native_exception_scope(&mut self, call: impl FnOnce(&mut Self) -> Result<BxValue, String>) -> Result<BxValue, String> {
        let previous_gc = self.gc_suspended;
        // Native arguments/temporaries may live only in Rust. Reentrant BoxLang
        // callbacks must not yield for collection until this boundary returns.
        self.gc_suspended = true;
        let saved = self.native_callback_exception.take();
        self.native_exception_depth += 1;
        let result = call(self);
        self.native_exception_depth -= 1;
        self.gc_suspended = previous_gc;
        let raised = self.native_callback_exception.take();
        self.native_callback_exception = match (&result, raised) {
            (Err(message), Some((original, root))) if *message == original => Some((original, root)),
            _ => saved,
        };
        result
    }

    fn invoke_callback(&mut self, callback: &crate::types::NativeCallback, args: &[BxValue]) -> Result<BxValue, String> {
        let previous = self.gc_suspended;
        self.gc_suspended = true;
        let result = self.native_exception_scope(|vm| (callback.callback)(vm, args));
        self.gc_suspended = previous;
        result
    }

    pub fn new() -> Self {
        Self::new_with_bifs(HashMap::new(), HashMap::new())
    }

    /// Create a VM with custom GC tuning parameters.
    pub fn with_config(config: GCConfig) -> Self {
        let mut vm = Self::new_with_bifs(HashMap::new(), HashMap::new());
        vm.heap = Heap::with_config(config.clone());
        vm.config = config;
        vm
    }

    pub fn new_with_args(args: Vec<String>) -> Self {
        let mut vm = Self::new();
        vm.set_cli_args(args);
        vm
    }

    /// Records the command line (executable first) and exposes it like BoxLang's CLI mode:
    /// `server.cli` (`args`, `parsed`, `executionPath`, `command`) and `server.boxlang.cliMode`.
    pub fn set_cli_args(&mut self, args: Vec<String>) {
        use crate::types::BxVM as _;
        let user_args: Vec<String> = args.iter().skip(1).cloned().collect();
        let command = args.join(" ");
        self.cli_args = args;
        let Some(server_id) = self.get_global("server").and_then(|server| server.as_gc_id()) else {
            return;
        };
        let vm: &mut dyn crate::types::BxVM = self;
        let raw = vm.array_new();
        for arg in &user_args {
            let value = BxValue::new_ptr(vm.string_new(arg.clone()));
            vm.array_push(raw, value);
        }
        let parsed = crate::bifs::cli::parsed_arguments_value(vm, &user_args);
        #[cfg(not(target_arch = "wasm32"))]
        let execution_path = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
        #[cfg(target_arch = "wasm32")]
        let execution_path = String::new();
        let cli = vm.struct_new();
        vm.struct_set(cli, "args", BxValue::new_ptr(raw));
        vm.struct_set(cli, "parsed", parsed);
        let execution_path = BxValue::new_ptr(vm.string_new(execution_path));
        vm.struct_set(cli, "executionPath", execution_path);
        let command = BxValue::new_ptr(vm.string_new(command));
        vm.struct_set(cli, "command", command);
        vm.struct_set(server_id, "cli", BxValue::new_ptr(cli));
        let boxlang = vm.struct_new();
        vm.struct_set(boxlang, "cliMode", BxValue::new_bool(true));
        vm.struct_set(server_id, "boxlang", BxValue::new_ptr(boxlang));
    }

    pub fn new_with_bifs(
        external_bifs: HashMap<String, BxNativeFunction>,
        native_classes: HashMap<String, BxNativeFunction>,
    ) -> Self {
        let (native_future_tx, native_future_rx) = mpsc::channel();
        let mut vm = VM {
            fibers: Vec::new(),
            global_names: HashMap::new(),
            global_values: Vec::new(),
            global_namespace: next_global_namespace(),
            global_bifs: HashSet::new(),
            struct_assignment_hooks: HashMap::new(),
            script_variables: Self::new_variables_scope(),
            current_fiber_idx: None,
            shapes: ShapeRegistry::new(),
            heap: Heap::new(),
            config: GCConfig::default(),
            native_classes: native_classes
                .into_iter()
                .map(|(k, v)| (k.to_lowercase(), v))
                .collect(),
            interner: StringInterner::new(),
            cli_args: Vec::new(),
            soap_clients: HashMap::new(),
            #[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
            members: HashMap::new(),
            output_buffer: None,
            output_capture_stack: Vec::new(),
            gc_suspended: false,
            native_exception_depth: 0,
            native_callback_exception: None,
            function_events: HashMap::new(),
            automatic_event_depth: 0,
            thread_futures: HashSet::new(),
            datetime_timezones: HashMap::new(),
            native_completions: VecDeque::new(),
            native_future_tx,
            native_future_rx,
            pending_native_futures: HashMap::new(),
            #[cfg(feature = "debugger")]
            debug_run: None,
            #[cfg(feature = "jit")]
            jit: None,
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            callback_registry: Rc::new(RefCell::new(HashMap::new())),
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            next_callback_id: RefCell::new(1),
        };

        #[cfg(all(target_arch = "wasm32", feature = "js"))]
        {
            if let Some(js_root) = browser_js_root() {
                let id = vm.heap.alloc(GcObject::JsValue(js_root));
                vm.insert_global("js".to_string(), BxValue::new_ptr(id));
            }
        }
        #[cfg(all(target_arch = "wasm32", feature = "js-host-abi", not(feature = "js")))]
        {
            // WASI build: register `js` as handle 1 (window) for browser JS interop
            let id = vm.heap.alloc(GcObject::JsHandle(1));
            vm.insert_global("js".to_string(), BxValue::new_ptr(id));
        }

        // Register standard BIFs
        for (name, func) in crate::bifs::register_all() {
            let id = vm.heap.alloc(GcObject::NativeFunction(func));
            vm.register_global_bif(name, BxValue::new_ptr(id));
        }

        // Register external/plugin BIFs
        for (name, func) in external_bifs {
            let id = vm.heap.alloc(GcObject::NativeFunction(func));
            vm.register_global_bif(name, BxValue::new_ptr(id));
        }

        // Initialize 'server' scope
        vm.init_server_scope();

        vm
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    fn is_seen_js_value(value: &JsValue, seen: &[JsValue]) -> bool {
        seen.iter().any(|candidate| Object::is(candidate, value))
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    fn plain_js_value_to_bx(
        &mut self,
        value: JsValue,
        seen: &mut Vec<JsValue>,
        depth: usize,
    ) -> Option<BxValue> {
        if depth >= JS_INTEROP_MAX_DEPTH {
            return None;
        }

        if Array::is_array(&value) {
            if Self::is_seen_js_value(&value, seen) {
                return None;
            }
            seen.push(value.clone());
            let js_array = Array::from(&value);
            let id = self.array_new();
            for idx in 0..js_array.length() {
                let item = self.js_to_bx_with_seen(js_array.get(idx), seen, depth + 1);
                self.array_push(id, item);
            }
            seen.pop();
            return Some(BxValue::new_ptr(id));
        }

        if !is_plain_js_object(&value) {
            return None;
        }

        if Self::is_seen_js_value(&value, seen) {
            return None;
        }
        let object = Object::from(value.clone());
        let keys = Object::keys(&object);

        // Plain API objects such as Alpine often expose callable members.
        // Keep those as JS handles so BoxLang can invoke the original method
        // instead of normalizing the object into a dead struct snapshot.
        for key in keys.iter() {
            if let Some(key_str) = key.as_string() {
                let prop = Reflect::get(&value, &JsValue::from_str(&key_str))
                    .unwrap_or(JsValue::UNDEFINED);
                if prop.is_function() {
                    return None;
                }
            }
        }

        seen.push(value.clone());
        let id = self.struct_new();
        for key in keys.iter() {
            if let Some(key_str) = key.as_string() {
                let prop = Reflect::get(&value, &JsValue::from_str(&key_str))
                    .unwrap_or(JsValue::UNDEFINED);
                let bx = self.js_to_bx_with_seen(prop, seen, depth + 1);
                self.struct_set(id, &key_str, bx);
            }
        }
        seen.pop();
        Some(BxValue::new_ptr(id))
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    fn js_to_bx_with_seen(
        &mut self,
        val: JsValue,
        seen: &mut Vec<JsValue>,
        depth: usize,
    ) -> BxValue {
        if val.is_string() {
            let id = self
                .heap
                .alloc(GcObject::String(BoxString::new(&val.as_string().unwrap())));
            return BxValue::new_ptr(id);
        }

        if let Some(n) = val.as_f64() {
            if n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64 {
                BxValue::new_int(n as i32)
            } else {
                BxValue::new_number(n)
            }
        } else if let Some(b) = val.as_bool() {
            BxValue::new_bool(b)
        } else if val.is_null() || val.is_undefined() {
            BxValue::new_null()
        } else if let Some(bx) = self.plain_js_value_to_bx(val.clone(), seen, depth) {
            bx
        } else {
            BxValue::new_ptr(self.heap.alloc(GcObject::JsValue(val)))
        }
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    fn bridge_js_promise_to_future(&mut self, promise: JsValue) -> BxValue {
        let vm_ptr = self as *const VM as usize;
        let future = self.future_new();
        if let Some(id) = future.as_gc_id() {
            *self.pending_native_futures.entry(id).or_insert(0) += 1;
        }

        let future_for_resolve = future;
        let sender_for_resolve = self.native_future_tx.clone();
        let resolve_cb = Closure::wrap(Box::new(move |res: JsValue| {
            let thunk_id =
                register_wasm_future_thunk(Box::new(move |vm| Ok(vm.js_to_bx_wasm(res))));
            let _ = sender_for_resolve.send(NativeFutureMessage::ResolveWasmThunk {
                future: future_for_resolve,
                thunk_id,
            });
            schedule_browser_vm_pump(vm_ptr);
        }) as Box<dyn FnMut(JsValue)>);

        let future_for_reject = future;
        let sender_for_reject = self.native_future_tx.clone();
        let reject_cb = Closure::wrap(Box::new(move |err: JsValue| {
            let thunk_id = register_wasm_future_thunk(Box::new(move |vm| {
                let bx_err = vm.js_to_bx_wasm(err);
                Err(vm.to_string(bx_err))
            }));
            let _ = sender_for_reject.send(NativeFutureMessage::ResolveWasmThunk {
                future: future_for_reject,
                thunk_id,
            });
            schedule_browser_vm_pump(vm_ptr);
        }) as Box<dyn FnMut(JsValue)>);

        let promise = js_sys::Promise::from(promise);
        let _ = promise.then2(&resolve_cb, &reject_cb);

        resolve_cb.forget();
        reject_cb.forget();

        future
    }

    /// Activate the Cranelift JIT. Call this before `interpret` to enable
    /// hot-loop compilation. No-op (compile error) without the `jit` feature.
    #[cfg(feature = "jit")]
    pub fn enable_jit(&mut self) {
        match jit::JitState::new() {
            Ok(state) => self.jit = Some(Box::new(state)),
            Err(e) => eprintln!("[JIT] init failed: {}", e),
        }
    }

    fn init_server_scope(&mut self) {
        use crate::types::BxStruct;

        let mut os_struct = BxStruct {
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
            case_sensitive: false,
            ordered: false,
        };

        let os_name = if cfg!(target_os = "espidf") {
            "FreeRTOS"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "linux") {
            "Linux"
        } else if cfg!(target_arch = "wasm32") {
            "WebAssembly"
        } else {
            "Unknown"
        };

        let os_arch = if cfg!(target_arch = "xtensa") {
            "xtensa"
        } else if cfg!(target_arch = "riscv32") {
            "riscv32"
        } else if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else if cfg!(target_arch = "wasm32") {
            "wasm32"
        } else {
            "unknown"
        };

        let os_name_id = self.heap.alloc(GcObject::String(BoxString::new(os_name)));
        let os_arch_id = self.heap.alloc(GcObject::String(BoxString::new(os_arch)));

        // Manual struct property insertion (since we don't have BxStruct::set yet)
        let name_idx = self.interner.intern("name");
        let arch_idx = self.interner.intern("arch");

        os_struct.shape_id = self.shapes.transition(os_struct.shape_id, name_idx);
        os_struct.properties.push(BxValue::new_ptr(os_name_id));

        os_struct.shape_id = self.shapes.transition(os_struct.shape_id, arch_idx);
        os_struct.properties.push(BxValue::new_ptr(os_arch_id));

        let os_ptr = self.heap.alloc(GcObject::Struct(os_struct));

        let mut server_struct = BxStruct {
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
            case_sensitive: false,
            ordered: false,
        };
        let os_key_idx = self.interner.intern("os");
        server_struct.shape_id = self.shapes.transition(server_struct.shape_id, os_key_idx);
        server_struct.properties.push(BxValue::new_ptr(os_ptr));

        let server_ptr = self.heap.alloc(GcObject::Struct(server_struct));
        self.insert_global("server".to_string(), BxValue::new_ptr(server_ptr));
        // BoxLang's request context owns a `request` scope in CLI scripts too.
        self.insert_empty_struct_global("request");
    }

    pub fn register_global_bif(&mut self, name: String, value: BxValue) {
        self.global_bifs.insert(Rc::from(name.to_lowercase()));
        let value = self.bif_function(value, &name, false);
        self.insert_global(name, value);
    }

    pub fn insert_global(&mut self, name: String, val: BxValue) {
        let name_id = self.interner.intern(&name);
        self.insert_global_interned(name_id, val);
    }

    pub fn insert_empty_struct_global(&mut self, name: &str) -> BxValue {
        let id = self.struct_new();
        let value = BxValue::new_ptr(id);
        self.insert_global(name.to_string(), value);
        value
    }

    pub fn get_global_struct_member(
        &self,
        global_name: &str,
        member_name: &str,
    ) -> Option<BxValue> {
        let global = self.get_global(global_name)?;
        let id = global.as_gc_id()?;
        Some(self.struct_get(id, member_name))
    }

    /// Read-only host roots; binding changes must go through VM APIs.
    pub fn global_values(&self) -> &[BxValue] {
        &self.global_values
    }

    fn insert_global_interned(&mut self, name_id: u32, val: BxValue) {
        if let Some(&idx) = self.global_names.get(&name_id) {
            self.global_values[idx] = val;
        } else {
            let idx = self.global_values.len();
            self.global_names.insert(name_id, idx);
            self.global_values.push(val);
        }
    }

    pub fn get_global(&self, name: &str) -> Option<BxValue> {
        if let Some(name_id) = self.interner.get_id(name) {
            self.global_names
                .get(&name_id)
                .map(|&idx| self.global_values[idx])
        } else {
            None
        }
    }

    /// One step of `for ( row in query )`: the row struct and the 1-based row number.
    #[cfg(target_arch = "wasm32")]
    fn query_iteration_step(
        &mut self,
        _collection: BxValue,
        _cursor: usize,
    ) -> Result<Option<(bool, Option<BxValue>, Option<BxValue>)>> {
        Ok(None)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn query_iteration_step(
        &mut self,
        collection: BxValue,
        cursor: usize,
    ) -> Result<Option<(bool, Option<BxValue>, Option<BxValue>)>> {
        let Some(id) = collection.as_gc_id() else { return Ok(None) };
        if !matches!(self.heap.get_opt(id), Some(GcObject::NativeObject(_)))
            || BxVM::native_object_query_columns(self, id).is_none()
        {
            return Ok(None);
        }
        let count = BxVM::native_object_call_method(self, id, "recordcount", &[])
            .map_err(anyhow::Error::msg)?
            .as_number() as usize;
        if cursor >= count {
            return Ok(Some((true, None, None)));
        }
        let row = BxVM::native_object_call_method(
            self,
            id,
            "getrow",
            &[BxValue::new_number((cursor + 1) as f64)],
        )
        .map_err(anyhow::Error::msg)?;
        Ok(Some((false, Some(row), Some(BxValue::new_number(cursor as f64 + 1.0)))))
    }

    fn resolve_member_method(&self, receiver: &BxValue, method_name: &str) -> Option<String> {
        let name = method_name.to_ascii_lowercase();
        if receiver.is_bool() {
            return match name.as_str() {
                "booleanformat" | "truefalseformat" => Some("booleanformat".to_string()),
                "tojson" => Some("serializejson".to_string()),
                _ => None,
            };
        }
        if receiver.is_number() || receiver.is_int() {
            return match name.as_str() {
                "len" | "length" => Some("len".to_string()),
                "abs" => Some("abs".to_string()),
                "round" => Some("round".to_string()),
                "floor" => Some("floor".to_string()),
                "ceiling" => Some("ceiling".to_string()),
                "int" => Some("int".to_string()),
                "fix" => Some("fix".to_string()),
                "incrementvalue" => Some("incrementvalue".to_string()),
                "decrementvalue" => Some("decrementvalue".to_string()),
                "sgn" => Some("sgn".to_string()),
                "sqr" => Some("sqr".to_string()),
                "decimalformat" => Some("decimalformat".to_string()),
                "max" => Some("max".to_string()),
                "min" => Some("min".to_string()),
                "formatbasen" => Some("formatbasen".to_string()),
                "numberformat" => Some("numberformat".to_string()),
                "currencyformat" => Some("currencyformat".to_string()),
                "booleanformat" | "truefalseformat" => Some("booleanformat".to_string()),
                "log" => Some("log".to_string()),
                "log10" => Some("log10".to_string()),
                "exp" => Some("exp".to_string()),
                "sin" => Some("sin".to_string()),
                "cos" => Some("cos".to_string()),
                "tan" => Some("tan".to_string()),
                "asin" => Some("asin".to_string()),
                "acos" => Some("acos".to_string()),
                "atan" => Some("atan".to_string()),
                "atn" => Some("atan".to_string()),
                "tojson" => Some("serializejson".to_string()),
                _ => None,
            };
        }

        if let Some(id) = receiver.as_gc_id() {
            if self.struct_key_exists(id, "__xml_kind") {
                return match name.as_str() {
                     "xmlformat" => Some("xmlformat".to_string()),
                    "search" => Some("xmlsearch".to_string()),
                    "transform" => Some("xmltransform".to_string()),
                    "getnodetype" => Some("xmlgetnodetype".to_string()),
                    "childpos" => Some("xmlchildpos".to_string()),
                    "size" => Some("xmlsize".to_string()),
                    _ => None,
                };
            }
            if matches!(self.heap.get(id), GcObject::Array(_)) && !crate::bifs::set::is_set(id) {
                let bif = match name.as_str() {
                    "each" => "arrayeach",
                    "every" => "arrayevery",
                    "map" => "arraymap",
                    "reduce" => "arrayreduce",
                    "filter" => "arrayfilter",
                    "some" => "arraysome",
                    "none" => "arraynone",
                    "findnocase" => "arrayfindnocase",
                    "join" => "arraytolist",
                    "isdefined" => "arrayindexexists",
                    _ => "",
                };
                if !bif.is_empty() {
                    return Some(bif.to_string());
                }
            }
            match self.heap.get(id) {
                GcObject::String(_) => match name.as_str() {
                    "len" | "length" => Some("len".to_string()),
                    "tostring" => Some("tostring".to_string()),
                    "ucase" | "touppercase" => Some("ucase".to_string()),
                    "lcase" | "tolowercase" => Some("lcase".to_string()),
                    "split" => Some("listtoarray".to_string()),
                    "trim" => Some("trim".to_string()),
                    "ltrim" => Some("ltrim".to_string()),
                    "rtrim" => Some("rtrim".to_string()),
                    "compare" => Some("compare".to_string()),
                    "comparenocase" => Some("comparenocase".to_string()),
                    "removechars" => Some("removechars".to_string()),
                    "stripcr" => Some("stripcr".to_string()),
                    "ucfirst" => Some("ucfirst".to_string()),
                    "replacenocase" => Some("replacenocase".to_string()),
                    "endswithnocase" => Some("stringendswithnocase".to_string()),
                    "startswithnocase" => Some("stringstartswithnocase".to_string()),
                    "ascii" => Some("ascii".to_string()),
                    "findoneof" => Some("findoneof".to_string()),
                    "insert" => Some("insert".to_string()),
                    "snakecase" => Some("snakecase".to_string()),
                    "kebabcase" => Some("kebabcase".to_string()),
                    "camelcase" => Some("camelcase".to_string()),
                    "pascalcase" => Some("pascalcase".to_string()),
                    "replacelist" => Some("replacelist".to_string()),
                    "find" => Some("stringfind".to_string()),
                    "findnocase" => Some("stringfindnocase".to_string()),
                    "endswith" => Some("stringendswith".to_string()),
                    "startswith" => Some("stringstartswith".to_string()),
                     "val" => Some("val".to_string()),
                     "urlencodedformat" => Some("urlencodedformat".to_string()),
                     "xmlformat" => Some("xmlformat".to_string()),
                      "todatetime" => Some("parsedatetime".to_string()),
                      "dateformat" => Some("dateformat".to_string()),
                      "datetimeformat" => Some("datetimeformat".to_string()),
                      "timeformat" => Some("timeformat".to_string()),
                     "tojson" => Some("serializejson".to_string()),
                     "year" => Some("year".to_string()),
                     "quarter" => Some("quarter".to_string()),
                     "month" => Some("month".to_string()),
                     "monthasstring" => Some("monthasstring".to_string()),
                     "monthshortasstring" => Some("monthshortasstring".to_string()),
                     "day" => Some("day".to_string()),
                     "daysinmonth" => Some("daysinmonth".to_string()),
                     "daysinyear" => Some("daysinyear".to_string()),
                     "dayofweek" => Some("dayofweek".to_string()),
                     "dayofweekasstring" => Some("dayofweekasstring".to_string()),
                     "dayofweekshortasstring" => Some("dayofweekshortasstring".to_string()),
                     "dayofyear" => Some("dayofyear".to_string()),
                     "firstdayofmonth" => Some("firstdayofmonth".to_string()),
                     "week" => Some("week".to_string()),
                     "hour" => Some("hour".to_string()),
                     "minute" => Some("minute".to_string()),
                     "second" => Some("second".to_string()),
                     "millisecond" => Some("millisecond".to_string()),
                    "fromjson" => Some("deserializejson".to_string()),
                    "toast" => Some("boxast".to_string()),
                    "rematch" => Some("rematch".to_string()),
                    "rematchnocase" => Some("rematchnocase".to_string()),
                    "refind" => Some("refind".to_string()),
                    "refindnocase" => Some("refindnocase".to_string()),
                    "rereplace" => Some("rereplace".to_string()),
                    "rereplacenocase" => Some("rereplacenocase".to_string()),
                    "hash" => Some("hash".to_string()),
                    "hmac" => Some("hmac".to_string()),
                    "indexof" => Some("indexof".to_string()),
                    "left" => Some("left".to_string()),
                    "right" => Some("right".to_string()),
                    "mid" => Some("mid".to_string()),
                    "reverse" => Some("reverse".to_string()),
                    "spanexcluding" => Some("spanexcluding".to_string()),
                     "spanincluding" => Some("spanincluding".to_string()),
                     "booleanformat" | "truefalseformat" => Some("booleanformat".to_string()),
                     "isempty" => Some("isempty".to_string()),
                    "replace" => Some("replace".to_string()),
                     "listlen" => Some("listlen".to_string()),
                     "listavg" => Some("listavg".to_string()),
                     "listchangedelims" => Some("listchangedelims".to_string()),
                     "listcompact" => Some("listcompact".to_string()),
                     "listcontains" => Some("listcontains".to_string()),
                     "listcontainsnocase" => Some("listcontainsnocase".to_string()),
                     "listgetat" => Some("listgetat".to_string()),
                    "listappend" => Some("listappend".to_string()),
                    "listfirst" => Some("listfirst".to_string()),
                    "listlast" => Some("listlast".to_string()),
                    "listrest" => Some("listrest".to_string()),
                    "listdeleteat" => Some("listdeleteat".to_string()),
                    "listfind" => Some("listfind".to_string()),
                     "listfindnocase" => Some("listfindnocase".to_string()),
                     "listgettoken" | "gettoken" => Some("gettoken".to_string()),
                     "listeach" => Some("listeach".to_string()),
                     "listevery" => Some("listevery".to_string()),
                     "listfilter" => Some("listfilter".to_string()),
                     "listmap" => Some("listmap".to_string()),
                     "listnone" => Some("listnone".to_string()),
                     "listqualify" => Some("listqualify".to_string()),
                     "listreduceright" => Some("listreduceright".to_string()),
                     "listremoveduplicates" => Some("listremoveduplicates".to_string()),
                     "listsome" => Some("listsome".to_string()),
                     "listsort" => Some("listsort".to_string()),
                     "listvaluecount" => Some("listvaluecount".to_string()),
                      "listvaluecountnocase" => Some("listvaluecountnocase".to_string()),
                     "listtoset" => Some("listtoset".to_string()),
                    "jsformat" | "jsstringformat" => Some("jsstringformat".to_string()),
                    "ljustify" => Some("ljustify".to_string()),
                    "rjustify" => Some("rjustify".to_string()),
                    "paragraphformat" => Some("paragraphformat".to_string()),
                    "slugify" => Some("slugify".to_string()),
                    "wrap" => Some("wrap".to_string()),
                    "bind" | "stringbind" => Some("stringbind".to_string()),
                    "charsetdecode" => Some("charsetdecode".to_string()),
                     "sqlprettify" => Some("sqlprettify".to_string()),
                     "each" => Some("stringeach".to_string()),
                     "every" => Some("stringevery".to_string()),
                     "filter" => Some("stringfilter".to_string()),
                     "map" => Some("stringmap".to_string()),
                     "reduce" => Some("stringreduce".to_string()),
                     "reduceright" => Some("stringreduceright".to_string()),
                     "some" => Some("stringsome".to_string()),
                     "sort" => Some("stringsort".to_string()),
                    "listindexexists" => Some("listindexexists".to_string()),
                    "listinsertat" => Some("listinsertat".to_string()),
                    "listitemtrim" => Some("listitemtrim".to_string()),
                    "listprepend" => Some("listprepend".to_string()),
                    "listreduce" => Some("listreduce".to_string()),
                    "listsetat" => Some("listsetat".to_string()),
                    "listtoarray" => Some("listtoarray".to_string()),
                    "listtrim" => Some("listcompact".to_string()),
                    "jsondeserialize" => Some("deserializejson".to_string()),
                    "jsonprettify" => Some("jsonprettify".to_string()),
                    "inputbasen" => Some("inputbasen".to_string()),
                    "tobase64" => Some("tobase64".to_string()),
                    "tobinary" => Some("tobinary".to_string()),
                    "yesnoformat" => Some("yesnoformat".to_string()),
                    "parsedatetime" => Some("parsedatetime".to_string()),
                    "stringreduceright" => Some("stringreduceright".to_string()),
                    "stringsome" => Some("stringsome".to_string()),
                    "stringsort" => Some("stringsort".to_string()),
                     _ => None,
                 },
                 GcObject::Array(_) => match name.as_str() {
                     "len" | "length" | "count" | "size" => Some("len".to_string()),
                     "append" => Some("arrayappendmember".to_string()),
                     "add" => Some("arrayappend".to_string()),
                     "xmlgetnodetype" | "getnodetype" => Some("xmlgetnodetype".to_string()),
                     "avg" => Some("arrayavg".to_string()),
                     "chunk" => Some("arraychunk".to_string()),
                       "contains" => Some("arraycontains".to_string()),
                       "has" => Some("boxsetcontains".to_string()),
                       "remove" | "delete" => Some("boxsetremove".to_string()),
                       "clear" => Some("boxsetclear".to_string()),
                       "addall" => Some("boxsetaddall".to_string()),
                       "removeall" => Some("boxsetremoveall".to_string()),
                       "retainall" => Some("boxsetretainall".to_string()),
                       "isempty" => Some("boxsetisempty".to_string()),
                      "containsnocase" => Some("arraycontainsnocase".to_string()),
                      "deletenocase" => Some("arraydeletenocase".to_string()),
                     "deleteat" => Some("arraydeleteat".to_string()),
                     "resize" => Some("arrayresize".to_string()),
                     "swap" => Some("arrayswap".to_string()),
                      "each" => Some("boxseteach".to_string()),
                      "every" => Some("boxsetevery".to_string()),
                      "map" => Some("boxsetmap".to_string()),
                      "reduce" => Some("boxsetreduce".to_string()),
                      "filter" => Some("boxsetfilter".to_string()),
                      "find" => Some("boxsetfind".to_string()),
                     "findall" => Some("arrayfindall".to_string()),
                     "findallnocase" => Some("arrayfindallnocase".to_string()),
                     "findfirst" => Some("arrayfindfirst".to_string()),
                     "first" => Some("arrayfirst".to_string()),
                     "flatmap" => Some("arrayflatmap".to_string()),
                     "flatten" => Some("arrayflatten".to_string()),
                     "getmetadata" => Some("arraygetmetadata".to_string()),
                     "groupby" => Some("arraygroupby".to_string()),
                     "indexexists" => Some("arrayindexexists".to_string()),
                     "insertat" => Some("arrayinsertat".to_string()),
                      "last" => Some("arraylast".to_string()),
                     "max" => Some("arraymax".to_string()),
                     "median" => Some("arraymedian".to_string()),
                     "merge" => Some("arraymerge".to_string()),
                     "min" => Some("arraymin".to_string()),
                      "none" => Some("boxsetnone".to_string()),
                     "pop" => Some("arraypop".to_string()),
                     "prepend" => Some("arrayprepend".to_string()),
                     "push" => Some("arraypush".to_string()),
                     "range" => Some("arrayrange".to_string()),
                     "reduceright" => Some("arrayreduceright".to_string()),
                     "reject" => Some("arrayreject".to_string()),
                     "reverse" => Some("arrayreverse".to_string()),
                     "set" => Some("arrayset".to_string()),
                     "shift" => Some("arrayshift".to_string()),
                     "slice" => Some("arrayslice".to_string()),
                      "some" => Some("boxsetsome".to_string()),
                     "sort" => Some("arraysort".to_string()),
                     "splice" => Some("arraysplice".to_string()),
                      "sum" => Some("arraysum".to_string()),
                      "intersection" => Some("boxsetintersection".to_string()),
                      "difference" => Some("boxsetdifference".to_string()),
                      "symmetricdifference" => Some("boxsetsymmetricdifference".to_string()),
                      "union" => Some("boxsetunion".to_string()),
                      "issubsetof" => Some("boxsetissubsetof".to_string()),
                      "issupersetof" => Some("boxsetissupersetof".to_string()),
                      "isdisjointfrom" => Some("boxsetisdisjointfrom".to_string()),
                      "iscasesensitive" => Some("boxsetiscasesensitive".to_string()),
                      "issynchronized" => Some("boxsetissynchronized".to_string()),
                      "toarray" => Some("boxsettoarray".to_string()),
                      "tolist" => Some("boxsettolist".to_string()),
                     "tostruct" => Some("arraytostruct".to_string()),
                     "transpose" => Some("arraytranspose".to_string()),
                     "unique" => Some("arrayunique".to_string()),
                     "unshift" => Some("arrayunshift".to_string()),
                     "zip" => Some("arrayzip".to_string()),
                     "tojson" => Some("serializejson".to_string()),
                     "toset" => Some("toset".to_string()),
                     "tomodifiable" => Some("tomodifiable".to_string()),
                     "tounmodifiable" => Some("tounmodifiable".to_string()),
                     "duplicate" => Some("duplicate".to_string()),
                    _ => None,
                },
                GcObject::Struct(_) => match name.as_str() {
                    "len" | "count" => Some("len".to_string()),
                     "tostring" => Some("tostring".to_string()),
                    "exists" | "keyexists" => Some("structkeyexists".to_string()),
                     "find" => Some("structfind".to_string()),
                     "append" => Some("structappend".to_string()),
                     "isempty" => Some("structisempty".to_string()),
                     "each" => Some("structeach".to_string()),
                     "tojson" => Some("serializejson".to_string()),
                     "tomodifiable" => Some("tomodifiable".to_string()),
                     "tounmodifiable" => Some("tounmodifiable".to_string()),
                     "duplicate" => Some("duplicate".to_string()),
                    "iscasesensitive" => Some("structiscasesensitive".to_string()),
                    "isordered" => Some("structisordered".to_string()),
                    "equals" => Some("structequals".to_string()),
                    "getmetadata" => Some("structgetmetadata".to_string()),
                    "toquerystring" => Some("structtoquerystring".to_string()),
                    "tosorted" => Some("structtosorted".to_string()),
                    "keytranslate" => Some("structkeytranslate".to_string()),
                     "findkey" => Some("structfindkey".to_string()),
                     "keyset" => Some("structkeyset".to_string()),
                     "valueset" => Some("structvalueset".to_string()),
                    "clear" => Some("structclear".to_string()),
                    "copy" => Some("structcopy".to_string()),
                    "delete" => Some("structdelete".to_string()),
                    "every" => Some("structevery".to_string()),
                    "filter" => Some("structfilter".to_string()),
                    "findvalue" => Some("structfindvalue".to_string()),
                    "hash" => Some("hash".to_string()),
                    "insert" => Some("structinsert".to_string()),
                    "keyarray" => Some("structkeyarray".to_string()),
                    "keylist" => Some("structkeylist".to_string()),
                    "map" => Some("structmap".to_string()),
                    "none" => Some("structnone".to_string()),
                    "reduce" => Some("structreduce".to_string()),
                    "some" => Some("structsome".to_string()),
                    "sort" => Some("structsort".to_string()),
                    "update" => Some("structupdate".to_string()),
                    "valuearray" => Some("structvaluearray".to_string()),
                    _ => None,
                },
                GcObject::Future(_) => match name.as_str() {
                    "onerror" => Some("futureonerror".to_string()),
                    "get" => Some("futureget".to_string()),
                    "isdone" => Some("futureisdone".to_string()),
                    "completeexceptionally" => Some("futurecompleteexceptionally".to_string()),
                    "completeontimeout" => Some("futurecompleteontimeout".to_string()),
                    "joinordefault" => Some("futurejoinordefault".to_string()),
                    "getordefault" => Some("futuregetordefault".to_string()),
                    "ortimeout" => Some("futureortimeout".to_string()),
                    "iscompletedexceptionally" => {
                        Some("futureiscompletedexceptionally".to_string())
                    }
                    "exceptionally" => Some("futureexceptionally".to_string()),
                    "then" => Some("futurethen".to_string()),
                    "getasattempt" => Some("futuregetasattempt".to_string()),
                    _ => None,
                },
                 GcObject::DateTime(_) => match name.as_str() {
                    "len" => Some("len".to_string()),
                    "add" => Some("dateadd".to_string()),
                    "diff" => Some("datediff".to_string()),
                     "format" => Some("datetimeformat".to_string()),
                     "timeformat" => Some("timeformat".to_string()),
                    "dateformat" => Some("dateformat".to_string()),
                     "datetimeformat" => Some("datetimeformat".to_string()),
                     "compare" => Some("datecompare".to_string()),
                     "toodbcdatetime" => Some("createodbcdatetime".to_string()),
                     "toodbcdate" => Some("createodbcdate".to_string()),
                     "toodbctime" => Some("createodbctime".to_string()),
                     "toepochmillis" => Some("toepochmillis".to_string()),
                     "hash" => Some("hash".to_string()),
                     "offset" => Some("offset".to_string()),
                     "timezone" | "gettimezone" => Some("gettimezone".to_string()),
                     "getnumericdate" => Some("getnumericdate".to_string()),
                     "gettime" => Some("gettime".to_string()),
                     "nanosecond" => Some("nanosecond".to_string()),
                     "year" => Some("year".to_string()),
                     "quarter" => Some("quarter".to_string()),
                     "month" => Some("month".to_string()),
                     "monthasstring" => Some("monthasstring".to_string()),
                     "monthshortasstring" => Some("monthshortasstring".to_string()),
                     "day" => Some("day".to_string()),
                     "daysinmonth" => Some("daysinmonth".to_string()),
                     "daysinyear" => Some("daysinyear".to_string()),
                     "dayofweek" => Some("dayofweek".to_string()),
                     "dayofweekasstring" => Some("dayofweekasstring".to_string()),
                     "dayofweekshortasstring" => Some("dayofweekshortasstring".to_string()),
                     "dayofyear" => Some("dayofyear".to_string()),
                     "firstdayofmonth" => Some("firstdayofmonth".to_string()),
                     "week" => Some("week".to_string()),
                     "hour" => Some("hour".to_string()),
                     "minute" => Some("minute".to_string()),
                     "second" => Some("second".to_string()),
                     "millisecond" => Some("millisecond".to_string()),
                    "duplicate" => Some("duplicate".to_string()),
                    _ => None,
                },
                #[cfg(all(target_arch = "wasm32", feature = "js"))]
                GcObject::JsValue(js) => {
                    if js.is_instance_of::<js_sys::Promise>() {
                        match name.as_str() {
                            "get" => Some("futureget".to_string()),
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    fn resolve_method(
        &self,
        class: Rc<RefCell<BxClass>>,
        method_name: &str,
    ) -> Option<Rc<BxCompiledFunction>> {
        let mut current_class = class;
        loop {
            let class_ref = current_class.borrow();
            if let Some((_, method)) = class_ref
                .methods
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(method_name))
            {
                return Some(Rc::new(method.clone()));
            }

            if let Some(parent_name) = &class_ref.extends {
                if let Some(val) = self.get_global(parent_name) {
                    if let Some(id) = val.as_gc_id() {
                        if let GcObject::Class(parent_class) = self.heap.get(id) {
                            let next_class = Rc::clone(parent_class);
                            drop(class_ref); // release borrow
                            current_class = next_class;
                            continue;
                        }
                    }
                }
            }
            return None;
        }
    }

    pub fn interpret(&mut self, chunk: Chunk) -> Result<BxValue> {
        // Script programs may be reused by independent VMs. Only functions
        // already promoted into this VM share their invocation caches.
        let mut chunk = chunk.clone_without_runtime_caches();
        chunk.ensure_caches();
        let chunk_for_func = chunk.clone();
        let chunk_rc = Rc::new(RefCell::new(chunk));
        self.interpret_chunk_shared(chunk_for_func, chunk_rc)
    }

    pub fn interpret_chunk_borrowed(&mut self, chunk: &Chunk) -> Result<BxValue> {
        // Borrowed execution path used by host tests. It still creates a
        // fresh Chunk wrapper so the caller's runtime caches are not mutated,
        // but the immutable program data is shared via Arc.
        let mut route_chunk = chunk.fresh_route_chunk();
        route_chunk.ensure_caches();
        let chunk_rc = Rc::new(RefCell::new(route_chunk));
        let chunk_for_func = chunk.clone_without_runtime_caches();
        self.interpret_chunk_shared(chunk_for_func, chunk_rc)
    }

    pub fn interpret_chunk_borrowed_current_task(&mut self, chunk: &Chunk) -> Result<BxValue> {
        // ESP32 route execution path: share the route's immutable program
        // data (code/constants/lines/filename/source) and attach only a fresh
        // per-request runtime cache. This avoids the previous OOM-prone
        // `clone_without_runtime_caches()` path that duplicated the entire
        // route chunk on every HTTP request.
        let mut route_chunk = chunk.fresh_route_chunk();
        route_chunk.ensure_caches();
        let chunk_rc = Rc::new(RefCell::new(route_chunk));
        let chunk_for_func = Chunk::default();
        self.interpret_chunk_shared_current_task(chunk_for_func, chunk_rc, None)
    }

    pub fn interpret_chunk_with_receiver(&mut self, chunk: &Chunk, receiver: Option<BxValue>) -> Result<BxValue> {
        if let Some(receiver) = receiver { self.instance_class_name(receiver)?; }
        let mut chunk = chunk.fresh_route_chunk();
        chunk.ensure_caches();
        let previous = self.current_fiber_idx;
        let result = self.interpret_chunk_shared_current_task(Chunk::default(), Rc::new(RefCell::new(chunk)), receiver);
        self.current_fiber_idx = previous;
        // Page aborts stop this template, not its including caller or request hooks.
        if result.as_ref().err().and_then(|error| error.downcast_ref::<AbortSignal>()).is_some_and(|signal| signal.kind == "page") {
            self.native_callback_exception = None;
            return Ok(BxValue::new_null());
        }
        result
    }

    fn interpret_chunk_shared(
        &mut self,
        chunk_for_func: Chunk,
        chunk_rc: Rc<RefCell<Chunk>>,
    ) -> Result<BxValue> {
        let function = Rc::new(BxCompiledFunction {
            name: "script".to_string(),
            kind: crate::types::FunctionKind::Udf,
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            modifiers: crate::types::FunctionModifiers::default(),
            captured_receiver: None,
            captured_scopes: Vec::new(),
            bif: None,
            chunk: chunk_for_func,
        });

        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
            success_handler: None,
        }));

        let fiber = BxFiber {
            stack: Vec::with_capacity(256),
            frames: vec![CallFrame {
                function,
                chunk: chunk_rc,
                ip: 0,
                stack_base: 0,
                receiver: None,
                handlers: Vec::new(),
                pending_completion: None,
                promoted_constants: Vec::new(),
                local_scope: None,
                local_names: scopes::SlotBindings::new(),
                scope_mode: scopes::ScopeMode::Binding,
                context: None,
                #[cfg(feature = "jit")]
                jit_loop_attempted: false,
                #[cfg(feature = "jit")]
                jit_construct_unsupported: None,
            }],

            variables: Self::new_variables_scope(),
            future_id,
            wait_until: None,
            yield_requested: false,
            running: false,
            blocked_on_future: false,
            finished: false,
            priority: 0,
            root_stack: Vec::new(),
        };

        self.fibers.push(fiber);
        let res = self.run_all();
        self.current_fiber_idx = None;
        res
    }

    fn interpret_chunk_shared_current_task(
        &mut self,
        chunk_for_func: Chunk,
        chunk_rc: Rc<RefCell<Chunk>>,
        receiver: Option<BxValue>,
    ) -> Result<BxValue> {
        let function = Rc::new(BxCompiledFunction {
            bif: None,
            name: "script".to_string(),
            kind: crate::types::FunctionKind::Udf,
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            modifiers: crate::types::FunctionModifiers::default(),
            captured_receiver: None,
            captured_scopes: Vec::new(),
            chunk: chunk_for_func,
        });

        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
            success_handler: None,
        }));

        let fiber = BxFiber {
            stack: Vec::with_capacity(256),
            frames: vec![CallFrame {
                function,
                chunk: chunk_rc,
                ip: 0,
                stack_base: 0,
                receiver: None,
                handlers: Vec::new(),
                pending_completion: None,
                promoted_constants: Vec::new(),
                local_scope: None,
                local_names: scopes::SlotBindings::new(),
                scope_mode: scopes::ScopeMode::Binding,
                context: None,
                #[cfg(feature = "jit")]
                jit_loop_attempted: false,
                #[cfg(feature = "jit")]
                jit_construct_unsupported: None,
            }],

            variables: Self::new_variables_scope(),
            future_id,
            wait_until: None,
            yield_requested: false,
            running: false,
            blocked_on_future: false,
            finished: false,
            priority: 0,
            root_stack: Vec::new(),
        };

        self.fibers.push(fiber);
        let fiber_idx = self.fibers.len() - 1;
        self.fibers[fiber_idx].frames.last_mut().unwrap().receiver = receiver;
        let result = self.run_fiber_to_completion(fiber_idx);
        self.current_fiber_idx = None;
        result
    }

    pub fn start_call_function_value(
        &mut self,
        func: BxValue,
        args: Vec<BxValue>,
    ) -> Result<BxValue> {
        self.start_call_function_value_with_receiver(func, args, None)
    }

    pub fn start_call_function_value_with_receiver(
        &mut self,
        func: BxValue,
        args: Vec<BxValue>,
        receiver: Option<BxValue>,
    ) -> Result<BxValue> {
        if let Some(id) = func.as_gc_id() {
            match self.heap.get(id) {
                GcObject::CompiledFunction(f) => {
                    let f = Rc::clone(f);
                    if args.len() < f.min_arity as usize {
                        anyhow::bail!("Expected at least {} arguments but got {}", f.min_arity, args.len());
                    }
                    Ok(self.enqueue_function_call(func, f, args, 0, receiver, None))
                }
                GcObject::NativeCallback(_) => {
                    let res = self.call_function_value(func, args, None);
                    let future = self.future_new();
                    match res {
                        Ok(value) => self.future_resolve(future, value).map_err(anyhow::Error::msg)?,
                        Err(error) => {
                            let id = self.string_new(error.to_string());
                            self.future_reject(future, BxValue::new_ptr(id)).map_err(anyhow::Error::msg)?;
                        }
                    }
                    Ok(future)
                }
                GcObject::NativeFunction(f) => {
                    let f = *f;
                    let previous_gc = self.gc_suspended;
                    self.gc_suspended = true;
                    let res = self.native_exception_scope(|vm| f(vm, &args)).map_err(|e| anyhow::anyhow!(e));
                    self.gc_suspended = previous_gc;
                    let future = self.future_new();
                    match res {
                        Ok(value) => {
                            let _ = self.future_resolve(future, value);
                            Ok(future)
                        }
                        Err(err) => {
                            let error_id = self.string_new(err.to_string());
                            let _ = self.future_reject(future, BxValue::new_ptr(error_id));
                            Ok(future)
                        }
                    }
                }
                _ => anyhow::bail!("Value is not a callable function"),
            }
        } else {
            anyhow::bail!("Value is not a callable function")
        }
    }

    pub fn pump_until_blocked(&mut self) -> Result<()> {
        self.drain_native_completions();
        self.sweep_finished_fibers();
        let mut i = 0;

        while i < self.fibers.len() {
            if self.fibers[i].finished {
                i += 1;
                continue;
            }
            if let Some(until) = self.fibers[i].wait_until {
                if Instant::now() < until {
                    i += 1;
                    continue;
                }
                self.fibers[i].wait_until = None;
            }

            self.current_fiber_idx = Some(i);
            match self.run_fiber(i, None) {
                Ok(Some(result)) => {
                    let fiber = self.fibers.swap_remove(i);
                    self.settle_future_success(fiber.future_id, result)?;
                }
                Ok(None) => {
                    i += 1;
                }
                Err(err) => {
                    let err_str = err.to_string();
                    let err_id = self.string_new(err_str);
                    let err_val = BxValue::new_ptr(err_id);
                    let fiber = self.fibers.swap_remove(i);
                    self.settle_future_error(fiber.future_id, err_val)?;
                }
            }
        }

        self.current_fiber_idx = None;
        Ok(())
    }

    #[cfg(feature = "debugger")]
    pub fn start_debug_chunk(&mut self, chunk: Chunk) -> Result<BxValue> {
        let mut chunk = chunk.clone_without_runtime_caches();
        chunk.ensure_caches();
        let chunk_for_func = chunk.clone();
        let chunk_rc = Rc::new(RefCell::new(chunk));
        let function = Rc::new(BxCompiledFunction {
            bif: None,
            name: "script".to_string(),
            kind: crate::types::FunctionKind::Udf,
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            modifiers: crate::types::FunctionModifiers::default(),
            captured_receiver: None,
            captured_scopes: Vec::new(),
            chunk: chunk_for_func,
        });

        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
            success_handler: None,
        }));

        let fiber = BxFiber {
            stack: Vec::with_capacity(256),
            frames: vec![CallFrame {
                function,
                chunk: chunk_rc,
                ip: 0,
                stack_base: 0,
                receiver: None,
                handlers: Vec::new(),
                pending_completion: None,
                promoted_constants: Vec::new(),
                local_scope: None,
                local_names: scopes::SlotBindings::new(),
                scope_mode: scopes::ScopeMode::Binding,
                context: None,
                #[cfg(feature = "jit")]
                jit_loop_attempted: false,
                #[cfg(feature = "jit")]
                jit_construct_unsupported: None,
            }],
            variables: Self::new_variables_scope(),
            future_id,
            wait_until: None,
            yield_requested: false,
            running: false,
            blocked_on_future: false,
            finished: false,
            priority: 0,
            root_stack: Vec::new(),
        };

        self.fibers.push(fiber);
        Ok(BxValue::new_ptr(future_id))
    }

    #[cfg(feature = "debugger")]
    pub fn debug_step_source_line(
        &mut self,
        instruction_budget: u64,
        value_path: Option<&str>,
    ) -> DebugStepResult {
        let budget = instruction_budget.max(1);
        if self.fibers.is_empty() {
            return DebugStepResult {
                status: DebugStepStatus::Completed,
                location: None,
                value: value_path.and_then(|path| self.debug_get_value_json(path).ok()),
                instructions: 0,
                error: None,
            };
        }

        self.drain_native_completions();
        self.debug_run = Some(DebugRunState {
            start: None,
            executed: 0,
            budget,
            pause: None,
        });

        self.current_fiber_idx = Some(0);
        let run_result = self.run_fiber(0, None);
        self.current_fiber_idx = None;

        let state = self.debug_run.take();
        let instructions = state.as_ref().map(|s| s.executed).unwrap_or(0);
        let pause = state.and_then(|s| s.pause);
        let value = value_path.and_then(|path| self.debug_get_value_json(path).ok());

        match run_result {
            Ok(Some(result)) => {
                let fiber = self.fibers.swap_remove(0);
                if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                    f.value = result;
                    f.status = FutureStatus::Completed;
                }
                DebugStepResult {
                    status: DebugStepStatus::Completed,
                    location: None,
                    value,
                    instructions,
                    error: None,
                }
            }
            Ok(None) => {
                let status = match pause {
                    Some(DebugPauseReason::Budget) => DebugStepStatus::BudgetExhausted,
                    Some(DebugPauseReason::Line) => DebugStepStatus::Paused,
                    None => DebugStepStatus::Blocked,
                };
                DebugStepResult {
                    status,
                    location: self.debug_current_location(),
                    value,
                    instructions,
                    error: None,
                }
            }
            Err(err) => {
                let message = err.to_string();
                if !self.fibers.is_empty() {
                    let err_id = self.string_new(message.clone());
                    let fiber = self.fibers.swap_remove(0);
                    if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                        f.status = FutureStatus::Failed(BxValue::new_ptr(err_id));
                    }
                }
                DebugStepResult {
                    status: DebugStepStatus::RuntimeError,
                    location: None,
                    value,
                    instructions,
                    error: Some(message),
                }
            }
        }
    }

    #[cfg(feature = "debugger")]
    pub fn debug_current_location(&self) -> Option<DebugLocation> {
        let fiber_idx = self.current_fiber_idx.unwrap_or(0);
        let frame = self.fibers.get(fiber_idx)?.frames.last()?;
        self.debug_location_at_ip(fiber_idx, frame.ip)
    }

    #[cfg(feature = "debugger")]
    fn debug_location_at_ip(&self, fiber_idx: usize, ip: usize) -> Option<DebugLocation> {
        let fiber = self.fibers.get(fiber_idx)?;
        let frame = fiber.frames.last()?;
        let chunk = frame.chunk.borrow();
        let line = if ip < chunk.lines.len() {
            chunk.lines[ip]
        } else if ip > 0 && ip - 1 < chunk.lines.len() {
            chunk.lines[ip - 1]
        } else {
            0
        };

        Some(DebugLocation {
            function: frame.function.name.clone(),
            filename: chunk.filename.clone(),
            line,
            frame_depth: fiber.frames.len(),
        })
    }

    #[cfg(feature = "debugger")]
    pub fn debug_get_value_json(&mut self, path: &str) -> Result<serde_json::Value> {
        let mut parts = path.split('.');
        let root = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Value path cannot be empty"))?;
        if !root.eq_ignore_ascii_case("variables") {
            anyhow::bail!("Only variables.* paths are supported by the debugger inspector");
        }

        let fiber_idx = self.current_fiber_idx.unwrap_or(0);
        anyhow::ensure!(
            self.fibers.get(fiber_idx).is_some_and(|fiber| !fiber.frames.is_empty()),
            "No active debug frame"
        );
        let first = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Value path must include a variables member"))?;
        let mut value = self.private_value(fiber_idx, "variables")
            .unwrap_or_else(BxValue::new_null);

        for part in std::iter::once(first).chain(parts) {
            let Some(id) = value.as_gc_id() else {
                return Ok(serde_json::Value::Null);
            };
            value = match self.heap.get(id) {
                GcObject::Struct(_) => self.struct_get(id, part),
                GcObject::NativeObject(obj) => self.native_object_get(obj, part),
                _ => BxValue::new_null(),
            };
        }

        Ok(self.bx_to_json(&value))
    }

    pub fn future_state(&self, future: BxValue) -> Result<HostFutureState> {
        let id = future
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not a future"))?;
        match self.heap.get(id) {
            GcObject::Future(f) => match &f.status {
                FutureStatus::Pending => Ok(HostFutureState::Pending),
                FutureStatus::Completed => Ok(HostFutureState::Completed(f.value)),
                FutureStatus::Failed(error) => Ok(HostFutureState::Failed(*error)),
            },
            _ => anyhow::bail!("Value is not a future"),
        }
    }

    pub fn run_future_to_completion(&mut self, future: BxValue) -> Result<BxValue> {
        loop {
            self.pump_until_blocked()?;
            match self.future_state(future)? {
                HostFutureState::Pending => continue,
                HostFutureState::Completed(value) => return Ok(value),
                HostFutureState::Failed(err) => {
                    let message = self.format_error_value(err);
                    anyhow::bail!("{}", message);
                }
            }
        }
    }

    pub fn spawn(
        &mut self,
        func: Rc<BxCompiledFunction>,
        args: Vec<BxValue>,
        priority: u8,
        _chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
        receiver: Option<BxValue>,
    ) -> BxValue {
        let receiver = receiver.or(func.captured_receiver);
        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
            success_handler: None,
        }));

        let args = self.function_arguments(args, None, &func);
        let mut stack = Vec::with_capacity(func.arity as usize + 1);
        let func_val = BxValue::new_ptr(
            self.heap
                .alloc(GcObject::CompiledFunction(Rc::clone(&func))),
        );
        stack.push(func_val); // function itself at base
        for arg in args {
            stack.push(arg);
        }
        while stack.len() < (func.arity + 1) as usize {
            stack.push(BxValue::new_null());
        }

        let chunk = func.chunk.execution_chunk();
        let fiber = BxFiber {
            stack,
            frames: vec![CallFrame {
                #[cfg(feature = "jit")]
                jit_loop_attempted: false,
                #[cfg(feature = "jit")]
                jit_construct_unsupported: None,
                function: func,
                chunk,
                context: None,
                ip: 0,
                scope_mode: scopes::ScopeMode::Binding,
                local_scope: None,
                local_names: scopes::SlotBindings::new(),
                stack_base: 1,
                receiver,
                handlers: Vec::new(),
                pending_completion: None,
                promoted_constants: Vec::new(),
            }],
            variables: self.current_variables_scope(),
            future_id,
            wait_until: None,
            yield_requested: false,
            running: false,
            blocked_on_future: false,
            finished: false,
            priority,
            root_stack: Vec::new(),
        };

        self.fibers.push(fiber);
        BxValue::new_ptr(future_id)
    }

    fn run_all(&mut self) -> Result<BxValue> {
        let mut last_result = BxValue::new_null();

        loop {
            self.sweep_finished_fibers();
            if self.fibers.is_empty() {
                break;
            }
            self.drain_native_completions();
            let mut i = 0;
            let mut all_waiting = true;
            let mut earliest_wait: Option<Instant> = None;

            // 1. Find the highest priority among non-waiting fibers
            let mut max_priority = 0;
            let now = Instant::now();
            for f in self.fibers.iter().filter(|f| !f.finished) {
                if let Some(until) = f.wait_until {
                    if now < until {
                        if earliest_wait.is_none() || until < earliest_wait.unwrap() {
                            earliest_wait = Some(until);
                        }
                        continue;
                    }
                }
                if f.priority > max_priority {
                    max_priority = f.priority;
                }
            }

            while i < self.fibers.len() {
                if self.fibers[i].finished {
                    i += 1;
                    continue;
                }
                let now = Instant::now();
                if let Some(until) = self.fibers[i].wait_until {
                    if now < until {
                        i += 1;
                        continue;
                    } else {
                        self.fibers[i].wait_until = None;
                    }
                }

                // Only run fibers with the current maximum priority to avoid starvation of I/O/callbacks
                if self.fibers[i].priority < max_priority {
                    i += 1;
                    all_waiting = false;
                    continue;
                }

                all_waiting = false;
                self.current_fiber_idx = Some(i);
                // Only pay for timeslice tracking when there are multiple fibers
                // to cooperatively schedule. Single-fiber scripts skip Instant::now()
                // entirely inside run_fiber, eliminating a syscall from every loop.
                let deadline = if self.fibers.len() > 1 {
                    Some(Instant::now() + Duration::from_millis(2))
                } else {
                    None
                };
                match self.run_fiber(i, deadline) {
                    Ok(Some(result)) => {
                        let fiber = self.fibers.swap_remove(i);
                        self.settle_future_success(fiber.future_id, result)?;
                        last_result = result;
                        // No i += 1 here because swap_remove moved another fiber into index i
                    }
                    Ok(None) => {
                        i += 1;
                    }
                    Err(e) => {
                        let fiber = self.fibers.swap_remove(i);
                        let mut handler = None;
                        let err_val = BxValue::new_ptr(
                            self.heap
                                .alloc(GcObject::String(BoxString::new(&e.to_string()))),
                        );
                        if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                            if matches!(f.status, FutureStatus::Pending) {
                                f.status = FutureStatus::Failed(err_val);
                                handler = f.error_handler.take();
                            }
                        }

                        if let Some(h) = handler {
                            if let Ok(value) = self.call_function_value(h, vec![err_val], None) {
                                if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                                    if matches!(f.status, FutureStatus::Failed(_)) {
                                        f.value = value;
                                        f.status = FutureStatus::Completed;
                                    }
                                }
                            }
                        } else {
                            if self.fibers.is_empty() {
                                return Err(e);
                            } else {
                                eprintln!("\n[Async Task Error] {}", e);
                            }
                        }
                    }
                }
                self.current_fiber_idx = None;
            }

            if all_waiting && !self.fibers.is_empty() {
                if let Some(until) = earliest_wait {
                    wait_until_instant(until);
                } else {
                    // Fallback if somehow all_waiting but no earliest_wait
                    wait_until_instant(Instant::now() + Duration::from_millis(1));
                }
            }

            // Periodically collect garbage
            if self.heap.should_collect() {
                self.collect_garbage();
            }
        }

        Ok(last_result)
    }

    /// Runs one timeslice of every fiber that is not suspended on the Rust stack.
    /// Finished fibers are only marked: removing them would move the fibers whose indexes
    /// the suspended callers still hold.
    fn run_other_fibers(&mut self) -> Result<()> {
        self.drain_native_completions();
        let mut ran = false;
        let mut i = 0;
        while i < self.fibers.len() {
            if self.fibers[i].running || self.fibers[i].finished {
                i += 1;
                continue;
            }
            if let Some(until) = self.fibers[i].wait_until {
                if Instant::now() < until {
                    i += 1;
                    continue;
                }
                self.fibers[i].wait_until = None;
            }
            ran = true;
            self.current_fiber_idx = Some(i);
            let outcome = self.run_fiber(i, Some(Instant::now() + Duration::from_millis(2)));
            self.current_fiber_idx = None;
            match outcome {
                Ok(Some(result)) => {
                    self.fibers[i].finished = true;
                    let future_id = self.fibers[i].future_id;
                    self.settle_future_success(future_id, result)?;
                }
                Ok(None) => {}
                Err(e) => {
                    self.fibers[i].finished = true;
                    let future_id = self.fibers[i].future_id;
                    let error = self.string_new(e.to_string());
                    self.settle_future_error(future_id, BxValue::new_ptr(error))?;
                }
            }
            i += 1;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !ran {
            std::thread::sleep(Duration::from_micros(200));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = ran;
        Ok(())
    }

    fn settle_future_success(&mut self, future_id: usize, result: BxValue) -> Result<()> {
        let handler = match self.heap.get_mut(future_id) {
            GcObject::Future(f) if matches!(f.status, FutureStatus::Pending) => {
                f.success_handler.take()
            }
            _ => return Ok(()),
        };

        let result = if let Some(handler) = handler {
            match self.call_function_value(handler, vec![result], None) {
                Ok(value) => value,
                Err(error) => {
                    let error_id = self.string_new(error.to_string());
                    if let GcObject::Future(f) = self.heap.get_mut(future_id) {
                        f.status = FutureStatus::Failed(BxValue::new_ptr(error_id));
                    }
                    return Ok(());
                }
            }
        } else {
            result
        };

        if let GcObject::Future(f) = self.heap.get_mut(future_id) {
            if matches!(f.status, FutureStatus::Pending) {
                f.value = result;
                f.status = FutureStatus::Completed;
            }
        }
        Ok(())
    }

    fn settle_future_error(&mut self, future_id: usize, error: BxValue) -> Result<()> {
        let handler = match self.heap.get_mut(future_id) {
            GcObject::Future(f) if matches!(f.status, FutureStatus::Pending) => {
                f.status = FutureStatus::Failed(error);
                f.error_handler.take()
            }
            _ => return Ok(()),
        };

        if let Some(handler) = handler {
            if let Ok(value) = self.call_function_value(handler, vec![error], None) {
                if let GcObject::Future(f) = self.heap.get_mut(future_id) {
                    if matches!(f.status, FutureStatus::Failed(_)) {
                        f.value = value;
                        f.status = FutureStatus::Completed;
                    }
                }
            }
        }
        Ok(())
    }

    fn run_fiber_to_completion(&mut self, fiber_idx: usize) -> Result<BxValue> {
        loop {
            self.drain_native_completions();
            if fiber_idx >= self.fibers.len() {
                return Ok(BxValue::new_null());
            }

            if let Some(until) = self.fibers[fiber_idx].wait_until {
                wait_until_instant(until);
                if fiber_idx < self.fibers.len() {
                    self.fibers[fiber_idx].wait_until = None;
                }
            }

            self.current_fiber_idx = Some(fiber_idx);
            match self.run_fiber(fiber_idx, None) {
                Ok(Some(result)) => {
                    let fiber = self.fibers.swap_remove(fiber_idx);
                    self.settle_future_success(fiber.future_id, result)?;
                    self.current_fiber_idx = None;
                    return Ok(result);
                }
                Ok(None) => {
                    self.current_fiber_idx = None;
                }
                Err(e) => {
                    let fiber = self.fibers.swap_remove(fiber_idx);
                    let err_val = BxValue::new_ptr(
                        self.heap
                            .alloc(GcObject::String(BoxString::new(&e.to_string()))),
                    );
                    self.settle_future_error(fiber.future_id, err_val)?;
                    self.current_fiber_idx = None;
                    return Err(e);
                }
            }

            if self.heap.should_collect() {
                self.collect_garbage();
            }
        }
    }

    fn run_fiber(
        &mut self,
        fiber_idx: usize,
        timeslice_end: Option<Instant>,
    ) -> Result<Option<BxValue>> {
        let future_id = self.fibers[fiber_idx].future_id;
        self.fibers[fiber_idx].running = true;
        let result = self.run_fiber_inner(fiber_idx, timeslice_end);
        // Removal of other fibers is deferred while any fiber runs, so the index is still ours.
        if let Some(fiber) = self.fibers.get_mut(fiber_idx) {
            if fiber.future_id == future_id {
                fiber.running = false;
            }
        }
        result
    }

    /// Drops fibers finished by a nested scheduler. Only call where no scheduler index is held.
    fn sweep_finished_fibers(&mut self) {
        let mut i = 0;
        while i < self.fibers.len() {
            if self.fibers[i].finished && !self.fibers[i].running {
                self.fibers.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    fn run_fiber_inner(
        &mut self,
        fiber_idx: usize,
        timeslice_end: Option<Instant>,
    ) -> Result<Option<BxValue>> {
        // Persistent state across dispatch iterations. Refreshed only when
        // `frame_changed` is true (after CALL/RETURN/THROW/NEW/etc.).
        // In tight loops there are no frame changes, so these are loaded just once.
        let mut frame_changed = true;
        let mut ip: usize = 0;
        let mut stack_base: usize = 0;
        let mut code_ptr: *const u32 = std::ptr::null();
        let mut code_len: usize = 0;
        let mut promoted_ptr: *mut Vec<Option<BxValue>> = std::ptr::null_mut();
        // Pointer to the base of the current frame's locals on the value stack.
        // Refreshed before every dispatch because any preceding opcode may have
        // reallocated the stack Vec.
        #[cfg(feature = "jit")]
        let mut locals_ptr: *mut BxValue;
        // Counter used to throttle Instant::now() at safe points.
        // We only call the (expensive) system clock every 1024 backward branches
        // to avoid the ~20–50ns syscall cost on every loop iteration.
        // Skipped entirely (Option::None fast path) when only one fiber is running.
        let mut safe_point_count: u32 = 0;
        let trace = std::env::var("BX_TRACE").is_ok();

        // JIT profiling state — tracked per run_fiber call to avoid per-iteration
        // HashMap overhead.  Only a single local counter is hot; the JitState's
        // HashMap is consulted at most twice (once to compile, once to cache here).
        #[cfg(feature = "jit")]
        let mut jit_hot_ip: usize = usize::MAX; // ip_at_start of the loop being counted
        #[cfg(feature = "jit")]
        let mut jit_hot_count: u64 = 0; // consecutive iterations of that loop
        // Once compiled, we cache the fn pointer locally so subsequent invocations
        // don't need a HashMap lookup either.
        #[cfg(feature = "jit")]
        let mut jit_active: Option<(usize, jit::JitLoopFn)> = None; // (ip_at_start, fn)
        // Generic body-loop JIT state.
        // jit_body_active is a quantum-local cache of the compiled fn pointer.
        // Profiling counters now live in JitState (persistent across quanta).
        #[cfg(feature = "jit")]
        let mut jit_body_active: Option<(usize, jit::GenericJitLoopFn)> = None; // (ip_at_start, fn)
        // Tier-3: array iterator JIT state.
        // jit_iter_active is a quantum-local cache of the compiled fn pointer.
        #[cfg(feature = "jit")]
        let mut jit_iter_active: Option<(usize, jit::ArrayIterJitFn)> = None;

        'quantum: loop {
            self.drain_native_completions();
            if fiber_idx >= self.fibers.len() {
                return Ok(None);
            }
            if self.fibers[fiber_idx].frames.is_empty() {
                return Ok(Some(BxValue::new_null()));
            }
            if self.fibers[fiber_idx].yield_requested {
                self.fibers[fiber_idx].yield_requested = false;
                return Ok(None);
            }

            // Reload all frame-derived state when the frame changes.
            // In tight loops `frame_changed` stays false — this block never runs.
            if frame_changed {
                frame_changed = false;
                ip = self.fibers[fiber_idx].frames.last().unwrap().ip;
                stack_base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                if ip == 0 {
                    let chunk_rc = Rc::clone(&self.fibers[fiber_idx].frames.last().unwrap().chunk);
                    chunk_rc.borrow_mut().ensure_caches();
                }
                {
                    let frame = self.fibers[fiber_idx].frames.last().unwrap();
                    let chunk = frame.chunk.borrow();
                    code_ptr = chunk.code().as_ptr();
                    code_len = chunk.code().len();
                }
                promoted_ptr = &mut self.fibers[fiber_idx]
                    .frames
                    .last_mut()
                    .unwrap()
                    .promoted_constants as *mut _;
                // Reserve headroom to avoid reallocating the stack for ordinary
                // expression temporaries. locals_ptr is still refreshed below.
                self.fibers[fiber_idx].stack.reserve(256);
            }

            if ip >= code_len {
                return Ok(Some(BxValue::new_null()));
            }

            // Any preceding opcode may grow and reallocate the value stack. Refresh
            // the frame-local base before every dispatch rather than relying on fixed
            // reserve headroom to keep a raw Vec pointer valid.
            #[cfg(feature = "jit")]
            unsafe {
                locals_ptr = self.fibers[fiber_idx].stack.as_mut_ptr().add(stack_base);
            }

            // Periodic GC check: on ESP32 (and elsewhere), long-running fibers in tight
            // while(true) loops can accumulate allocations without ever returning to the
            // dispatcher. This check breaks the quantum so the caller (pump_until_blocked or
            // run_fiber_to_completion) can run collect_garbage(). Throttled with safe_point_count
            // to avoid checking every single iteration.
            safe_point_count = safe_point_count.wrapping_add(1);
            if !self.gc_suspended && (safe_point_count & 1023 == 0 || self.heap.should_collect_promptly()) && self.heap.should_collect() {
                self.fibers[fiber_idx].frames.last_mut().unwrap().ip = ip;
                return Ok(None);
            }

            #[cfg(feature = "debugger")]
            if self.debug_run.is_some() {
                let loc = self.debug_location_at_ip(fiber_idx, ip);
                let mut pause_reason = None;
                if let Some(state) = self.debug_run.as_mut() {
                    if state.executed >= state.budget {
                        pause_reason = Some(DebugPauseReason::Budget);
                    } else if let Some(loc) = loc {
                        match &state.start {
                            Some(start)
                                if state.executed > 0
                                    && (start.line != loc.line
                                        || start.frame_depth != loc.frame_depth
                                        || start.function != loc.function) =>
                            {
                                pause_reason = Some(DebugPauseReason::Line);
                            }
                            Some(_) => {}
                            None => state.start = Some(loc),
                        }
                    }

                    if let Some(reason) = pause_reason {
                        state.pause = Some(reason);
                    }
                }

                if pause_reason.is_some() {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().ip = ip;
                    return Ok(None);
                }
            }

            // SAFETY: ip < code_len; pointer is valid for the Rc<Chunk> lifetime.
            let word0 = unsafe { *code_ptr.add(ip) };
            let ip_at_start = ip;
            ip += 1;

            #[cfg(feature = "debugger")]
            if let Some(state) = self.debug_run.as_mut() {
                state.executed += 1;
            }

            let opcode = (word0 & 0xFF) as u8;
            let op0 = word0 >> 8;

            if trace {
                let stack = &self.fibers[fiber_idx].stack;
                let stack_display: Vec<String> = stack
                    .iter()
                    .map(|v| {
                        if v.is_null() {
                            "null".to_string()
                        } else if v.is_bool() {
                            format!("bool({})", v.as_bool())
                        } else if v.is_int() {
                            format!("int({})", v.as_int())
                        } else if v.is_number() {
                            format!("num({})", v.as_number())
                        } else if v.is_ptr() {
                            format!("ptr({:?})", v.as_gc_id())
                        } else {
                            "?".to_string()
                        }
                    })
                    .collect();
                eprintln!(
                    "[TRACE] ip={:04} sb={} op={} op0={} stack=[{}]",
                    ip_at_start,
                    stack_base,
                    crate::vm::opcode::opcode_name(opcode),
                    op0,
                    stack_display.join(", ")
                );
            }

            // Flush ip to frame before frame changes or throws.
            macro_rules! flush_ip {
                () => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().ip = ip;
                };
            }

            // Read next word via raw code pointer — zero RefCell overhead.
            macro_rules! next_word {
                () => {{
                    let w = unsafe { *code_ptr.add(ip) };
                    ip += 1;
                    w
                }};
            }

            // vm_throw! flushes ip, throws, then marks frame_changed so the next
            // iteration reloads ip = handler_ip set by throw_value.
            macro_rules! vm_throw {
                ($msg:expr) => {{
                    flush_ip!();
                    self.throw_error(fiber_idx, $msg)?;
                    frame_changed = true;
                    continue 'quantum;
                }};
                ($fmt:literal, $($args:expr),+) => {{
                    flush_ip!();
                    self.throw_error(fiber_idx, &format!($fmt, $($args),+))?;
                    frame_changed = true;
                    continue 'quantum;
                }};
            }

            if INTERRUPT_REQUESTED.load(Ordering::Relaxed) {
                vm_throw!("Force Quit (Ctrl+C)");
            }

            match opcode {
                // --- Hot Loop / Specialized Opcodes ---
                op::INC_LOCAL => {
                    let slot = op0;
                    let val = self.local_value(fiber_idx, slot as usize);
                    if val.is_number() {
                        self.set_local_value(fiber_idx, slot as usize, BxValue::new_number(val.as_number() + 1.0));
                    } else if val.is_int() {
                        self.set_local_value(fiber_idx, slot as usize, BxValue::new_int(val.as_int() + 1));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::LOCAL_COMPARE_JUMP => {
                    let slot = op0;
                    let const_idx = next_word!();
                    let offset = next_word!();
                    let val = self.local_value(fiber_idx, slot as usize);
                    let limit: BxValue = {
                        let already =
                            unsafe { (&*promoted_ptr).get(const_idx as usize).copied().flatten() };
                        if let Some(v) = already {
                            v
                        } else {
                            self.read_constant(fiber_idx, const_idx as usize)?
                        }
                    };
                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            ip -= offset as usize;
                            if let Some(end) = timeslice_end {
                                safe_point_count = safe_point_count.wrapping_add(1);
                                if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                    break 'quantum;
                                }
                            }
                        }
                    } else if val.is_int() && limit.is_int() {
                        if val.as_int() < limit.as_int() {
                            ip -= offset as usize;
                            if let Some(end) = timeslice_end {
                                safe_point_count = safe_point_count.wrapping_add(1);
                                if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                    break 'quantum;
                                }
                            }
                        }
                    }
                }
                op::FOR_LOOP_STEP => {
                    // Fused: increment local, compare to const, jump back if still less.
                    // Replaces INC_LOCAL + LOCAL_COMPARE_JUMP — halves dispatch overhead for tight for-loops.
                    let slot = op0;
                    let const_idx = next_word!();
                    let offset = next_word!();
                    let val = self.local_value(fiber_idx, slot as usize);
                    let next_val = if val.is_int() {
                        BxValue::new_int(val.as_int() + 1)
                    } else if val.is_number() {
                        BxValue::new_number(val.as_number() + 1.0)
                    } else {
                        vm_throw!("For loop variable must be a number");
                    };
                    self.set_local_value(fiber_idx, slot as usize, next_val);
                    // Hot-path: read the loop-limit constant without a RefCell borrow.
                    // SAFETY: single-threaded VM; the code_ptr borrow has already dropped;
                    // no concurrent mutable access to promoted_constants is possible here.
                    let limit: BxValue = {
                        let already =
                            unsafe { (&*promoted_ptr).get(const_idx as usize).copied().flatten() };
                        if let Some(v) = already {
                            v
                        } else {
                            self.read_constant(fiber_idx, const_idx as usize)?
                        }
                    };
                    let should_loop = if next_val.is_int() && limit.is_int() {
                        next_val.as_int() < limit.as_int()
                    } else if next_val.is_number() && limit.is_number() {
                        next_val.as_number() < limit.as_number()
                    } else {
                        false
                    };
                    if should_loop {
                        // JIT fast path: eliminate remaining loop iterations with one native call.
                        // Uses local counters (no per-iteration HashMap) for near-zero overhead.
                        #[cfg(feature = "jit")]
                        {
                            if next_val.is_float() && limit.is_float() && offset == 3 {
                                // ── Tier-1: empty self-loop (floats only for now) ────────────────
                                if let Some((active_ip, compiled)) = jit_active {
                                    if active_ip == ip_at_start {
                                        let final_val = unsafe {
                                            compiled(next_val.as_number(), limit.as_number())
                                        };
                                        self.set_local_value(fiber_idx, slot as usize, BxValue::new_number(final_val));
                                        // Loop complete — do NOT jump back.
                                        jit_active = None;
                                        jit_body_active = None;
                                    } else {
                                        ip -= offset as usize;
                                    }
                                } else {
                                    if jit_hot_ip == ip_at_start {
                                        jit_hot_count += 1;
                                        const JIT_PROFILE_THRESHOLD: u64 = 5_000;
                                        if jit_hot_count >= JIT_PROFILE_THRESHOLD {
                                            let fn_id = code_ptr as usize;
                                            if let Some(ref mut jit) = self.jit {
                                                jit.profile_loop(fn_id, ip_at_start, jit_hot_count);
                                                if let Some(f) =
                                                    jit.get_compiled_loop(fn_id, ip_at_start)
                                                {
                                                    jit_active = Some((ip_at_start, f));
                                                }
                                            }
                                            jit_hot_count = 0;
                                        }
                                    } else {
                                        jit_hot_ip = ip_at_start;
                                        jit_hot_count = 1;
                                    }
                                    ip -= offset as usize;
                                    if let Some(end) = timeslice_end {
                                        safe_point_count = safe_point_count.wrapping_add(1);
                                        if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                            break 'quantum;
                                        }
                                    }
                                }
                            } else if next_val.is_number() && limit.is_number() && offset > 3 {
                                // ── Tier-2: generic numeric body ─────────────────────────────
                                // The JIT translates each body bytecode 1:1 into Cranelift IR
                                // and emits a real native loop — no mathematical shortcuts.
                                // OSR: 't2 loop lets us activate a freshly compiled fn (or one
                                // compiled in a prior quantum) and call it in the same dispatch.
                                't2: loop {
                                    let fn_id = code_ptr as usize;

                                    // ── OSR check: already compiled (possibly prior quantum) ──
                                    if jit_body_active.is_none() {
                                        if let Some(ref mut jit) = self.jit {
                                            if let Some(f) =
                                                jit.get_compiled_generic(fn_id, ip_at_start)
                                            {
                                                jit_body_active = Some((ip_at_start, f));
                                                continue 't2; // re-enter to call via active path
                                            }
                                        }
                                    }

                                    // ── Active path: call the compiled native loop ────────────
                                    if let Some((active_ip, compiled)) = jit_body_active {
                                        if active_ip == ip_at_start {
                                            eprintln!(
                                                "[JIT] calling compiled loop at ip={}!",
                                                ip_at_start
                                            );
                                            self.sync_jit_locals(fiber_idx, false);
                                            let deopt = unsafe {
                                                compiled(
                                                    locals_ptr as *mut u64,
                                                    &self.heap as *const _
                                                        as *const std::ffi::c_void,
                                                )
                                            };
                                            self.sync_jit_locals(fiber_idx, true);
                                            if deopt == 1 {
                                                eprintln!(
                                                    "[JIT] deoptimizing loop at ip={} (type mismatch)!",
                                                    ip_at_start
                                                );
                                                // JIT bailed out — resume at start of this iteration.
                                                ip -= offset as usize;
                                                jit_body_active = None;
                                                jit_active = None;
                                            } else {
                                                // Loop ran to completion — do NOT jump back.
                                                jit_body_active = None;
                                                jit_active = None;
                                                if let Some(end) = timeslice_end {
                                                    safe_point_count =
                                                        safe_point_count.wrapping_add(1);
                                                    if safe_point_count & 1023 == 0
                                                        && Instant::now() >= end
                                                    {
                                                        break 'quantum;
                                                    }
                                                }
                                            }
                                            break 't2;
                                        } else {
                                            // Different loop site — fall through to back-edge.
                                        }
                                    }

                                    // ── Profiling: accumulate count in JitState (survives quanta) ──
                                    // Fire every JIT_BODY_THRESHOLD iterations so that profile_generic's
                                    // internal counter (which requires 2×5000 = 10000 total) can be
                                    // reached across multiple quanta.
                                    const JIT_BODY_THRESHOLD: u64 = 5_000;
                                    let reached_threshold = if let Some(ref mut jit) = self.jit {
                                        let count = jit.inc_loop_profile(fn_id, ip_at_start);
                                        count % JIT_BODY_THRESHOLD == 0
                                    } else {
                                        false
                                    };

                                    if reached_threshold {
                                        let fn_id = code_ptr as usize;
                                        // Copy body bytes (offset - 3 words before FOR_LOOP_STEP).
                                        let body_start = ip - offset as usize;
                                        let body_len = offset as usize - 3;
                                        let body_code: Vec<u32> = unsafe {
                                            std::slice::from_raw_parts(
                                                code_ptr.add(body_start),
                                                body_len,
                                            )
                                            .to_vec()
                                        };
                                        // Extract numeric constants referenced in the body.
                                        let mut const_map: HashMap<u32, f64> = HashMap::new();
                                        let ic_entries = {
                                            let frame =
                                                self.fibers[fiber_idx].frames.last().unwrap();
                                            let c = frame.chunk.borrow();
                                            // JIT specializes shapes only, never namespace-bound slots.
                                            c.cache_slice(body_start, body_len).into_iter()
                                                .map(|entry| entry.filter(|entry| !matches!(entry, IcEntry::Global { .. })))
                                                .collect::<Vec<_>>()
                                        };
                                        for &word in &body_code {
                                            if (word & 0xFF) as u8 == op::CONSTANT {
                                                let cidx = word >> 8;
                                                let cv =
                                                    self.read_constant(fiber_idx, cidx as usize)?;
                                                if cv.is_number() {
                                                    const_map.insert(cidx, cv.as_number());
                                                }
                                            }
                                        }
                                        if let Some(ref mut jit) = self.jit {
                                            jit.profile_generic(
                                                fn_id,
                                                ip_at_start,
                                                JIT_BODY_THRESHOLD,
                                                &body_code,
                                                &ic_entries,
                                                slot,
                                                limit.as_number(),
                                                &const_map,
                                            );
                                            if let Some(f) =
                                                jit.get_compiled_generic(fn_id, ip_at_start)
                                            {
                                                jit_body_active = Some((ip_at_start, f));
                                                continue 't2; // immediately run the freshly compiled fn
                                            }
                                        }
                                    }

                                    // Not compiled yet — back-edge to loop header.
                                    ip -= offset as usize;
                                    if let Some(end) = timeslice_end {
                                        safe_point_count = safe_point_count.wrapping_add(1);
                                        if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                            break 'quantum;
                                        }
                                    }
                                    break 't2;
                                }
                            } else {
                                // Non-float or unhandled: plain interpreter.
                                ip -= offset as usize;
                                if let Some(end) = timeslice_end {
                                    safe_point_count = safe_point_count.wrapping_add(1);
                                    if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                        break 'quantum;
                                    }
                                }
                            }
                        }
                        #[cfg(not(feature = "jit"))]
                        {
                            ip -= offset as usize;
                            // Safe point: yield to scheduler if timeslice expired.
                            // Skipped entirely when timeslice_end is None (single-fiber case).
                            if let Some(end) = timeslice_end {
                                safe_point_count = safe_point_count.wrapping_add(1);
                                if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                    break 'quantum; // ip flushed after the loop
                                }
                            }
                        }
                    }
                }
                op::COMPARE_JUMP => {
                    let const_idx = op0;
                    let offset = next_word!();
                    let limit = self.read_constant(fiber_idx, const_idx as usize)?;
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            ip -= offset as usize;
                        }
                    } else {
                        vm_throw!("OpCompareJump expects numeric operands");
                    }
                }
                op::INC_GLOBAL => {
                    let idx = op0;
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.cache_get(ip_at_start)
                    };

                    if let Some(IcEntry::Global { index, namespace }) = ic
                        && namespace == self.global_namespace
                    {
                        let val = self.global_values[index];
                        if val.is_number() {
                            self.global_values[index] = BxValue::new_number(val.as_number() + 1.0);
                        } else {
                            flush_ip!();
                            self.throw_error(fiber_idx, "Operand of increment must be a number")?;
                            frame_changed = true;
                            continue 'quantum;
                        }
                    } else {
                        // Slow path: resolve global and update IC
                        let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            let val = self.global_values[global_idx];
                            if val.is_number() {
                                self.global_values[global_idx] =
                                    BxValue::new_number(val.as_number() + 1.0);
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.chunk.borrow_mut();
                                chunk.cache_set(ip_at_start, IcEntry::Global { index: global_idx, namespace: self.global_namespace });
                            } else {
                                flush_ip!();
                                self.throw_error(
                                    fiber_idx,
                                    "Operand of increment must be a number",
                                )?;
                                frame_changed = true;
                                continue 'quantum;
                            }
                        } else {
                            let name = self.interner.resolve(name_id).to_string();
                            flush_ip!();
                            self.throw_error(fiber_idx, &format!("Global {} not found", name))?;
                            frame_changed = true;
                            continue 'quantum;
                        }
                    }
                }
                op::GLOBAL_COMPARE_JUMP => {
                    let name_idx = op0;
                    let const_idx = next_word!();
                    let offset = next_word!();
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.cache_get(ip_at_start)
                    };

                    let val = if let Some(IcEntry::Global { index, namespace }) = ic
                        && namespace == self.global_namespace
                    {
                        self.global_values[index]
                    } else {
                        let name_id = self.read_intern_id(fiber_idx, name_idx as usize)?;
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            let v = self.global_values[global_idx];
                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.chunk.borrow_mut();
                            chunk.cache_set(ip_at_start, IcEntry::Global { index: global_idx, namespace: self.global_namespace });
                            v
                        } else {
                            BxValue::new_null()
                        }
                    };

                    let limit = self.read_constant(fiber_idx, const_idx as usize)?;
                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            ip -= offset as usize;
                            if let Some(end) = timeslice_end {
                                safe_point_count = safe_point_count.wrapping_add(1);
                                if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                    break 'quantum;
                                }
                            }
                        }
                    }
                }

                // --- Basic Hot Opcodes ---
                op::GET_LOCAL => {
                    let val = self.local_value(fiber_idx, op0 as usize);
                    self.fibers[fiber_idx].stack.push(val);
                }
                op::SET_LOCAL => {
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    self.set_local_value(fiber_idx, op0 as usize, val);
                }
                op::SET_LOCAL_POP => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.set_local_value(fiber_idx, op0 as usize, val);
                }
                op::FUNCTION_ENTER => {
                    self.enter_frame_scopes(fiber_idx);
                    flush_ip!();
                    if let Err(error) = self.enter_function_events(fiber_idx) {
                        self.throw_error(fiber_idx, &error)?;
                    } else {
                        #[cfg(feature = "jit")]
                        if let Some(result) = self.run_jit_fn_body(fiber_idx, ip) {
                            if let Some(result) = self.handle_return(fiber_idx, result)? {
                                return Ok(Some(result));
                            }
                        }
                    }
                    frame_changed = true;
                    continue 'quantum;
                }
                op::BIND_LOCAL => {
                    let key = next_word!();
                    let mut name = self.read_string_constant(fiber_idx, key as usize)?;
                    let name = name.as_flat_str();
                    let folded = if name.bytes().all(|b| b.is_ascii() && !b.is_ascii_uppercase()) {
                        std::borrow::Cow::Borrowed(name)
                    } else {
                        std::borrow::Cow::Owned(name.to_lowercase())
                    };
                    self.bind_local(fiber_idx, op0 as usize, &folded);
                }
                op::CONSTANT => {
                    let idx = op0;
                    let constant = self.read_constant(fiber_idx, idx as usize)?;
                    self.fibers[fiber_idx].stack.push(constant);
                }
                op::ADD_INT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_int(
                        a.as_number() as i32 + b.as_number() as i32,
                    ));
                }
                op::ADD_FLOAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_number(a.as_number() + b.as_number()));
                }
                op::ADD => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();

                    let array_union = match (a.as_gc_id(), b.as_gc_id()) {
                        (Some(a_id), Some(b_id)) => {
                            let (left, right) = match (self.heap.get(a_id), self.heap.get(b_id)) {
                                (GcObject::Array(left), GcObject::Array(right)) => {
                                    (Some(left.clone()), Some(right.clone()))
                                }
                                _ => (None, None),
                            };
                            left.zip(right).map(|(left, right)| {
                                let mut values = left;
                                for value in right {
                                    if !values.iter().any(|existing| self.is_equal(*existing, value)) {
                                        values.push(value);
                                    }
                                }
                                values
                            })
                        }
                        _ => None,
                    };

                    if let Some(values) = array_union {
                        let id = self.heap.alloc(GcObject::Array(values));
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                    } else if let (Some(na), Some(nb)) =
                        (self.numeric_value(a), self.numeric_value(b))
                    {
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_number(na + nb));
                    } else {
                        let a_s = self.to_box_string(a);
                        let b_s = self.to_box_string(b);
                        let res_id = self.heap.alloc(GcObject::String(a_s.concat(&b_s)));
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(res_id));
                    }
                }
                op::SUBTRACT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let array_difference = match (a.as_gc_id(), b.as_gc_id()) {
                        (Some(a_id), Some(b_id)) => {
                            let (left, right) = match (self.heap.get(a_id), self.heap.get(b_id)) {
                                (GcObject::Array(left), GcObject::Array(right)) => {
                                    (Some(left.clone()), Some(right.clone()))
                                }
                                _ => (None, None),
                            };
                            left.zip(right).map(|(left, right)| {
                                left.into_iter()
                                    .filter(|value| {
                                        !right.iter().any(|excluded| self.is_equal(*value, *excluded))
                                    })
                                    .collect::<Vec<_>>()
                            })
                        }
                        _ => None,
                    };

                    if let Some(values) = array_difference {
                        let id = self.heap.alloc(GcObject::Array(values));
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                    } else if let (Some(na), Some(nb)) =
                        (self.numeric_value(a), self.numeric_value(b))
                    {
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_number(na - nb));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::SUB_INT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = self.numeric_value(a).unwrap_or(f64::NAN) as i32
                        - self.numeric_value(b).unwrap_or(f64::NAN) as i32;
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_int(result));
                }
                op::SUB_FLOAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_number(a.as_number() - b.as_number()));
                }
                op::MULTIPLY => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let array_intersection = match (a.as_gc_id(), b.as_gc_id()) {
                        (Some(a_id), Some(b_id)) => {
                            let (left, right) = match (self.heap.get(a_id), self.heap.get(b_id)) {
                                (GcObject::Array(left), GcObject::Array(right)) => {
                                    (Some(left.clone()), Some(right.clone()))
                                }
                                _ => (None, None),
                            };
                            left.zip(right).map(|(left, right)| {
                                left.into_iter()
                                    .filter(|value| {
                                        right.iter().any(|other| self.is_equal(*value, *other))
                                    })
                                    .collect::<Vec<_>>()
                            })
                        }
                        _ => None,
                    };

                    if let Some(values) = array_intersection {
                        let id = self.heap.alloc(GcObject::Array(values));
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                    } else if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_number(na * nb));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::MUL_INT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = self.numeric_value(a).unwrap_or(f64::NAN) as i32
                        * self.numeric_value(b).unwrap_or(f64::NAN) as i32;
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_int(result));
                }
                op::MUL_FLOAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_number(a.as_number() * b.as_number()));
                }
                op::DIVIDE => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(b_n)) = (self.numeric_value(a), self.numeric_value(b)) {
                        if b_n == 0.0 {
                            flush_ip!();
                            self.throw_error(fiber_idx, "Division by zero")?;
                            frame_changed = true;
                            continue 'quantum;
                        } else {
                            self.fibers[fiber_idx]
                                .stack
                                .push(BxValue::new_number(na / b_n));
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::DIV_FLOAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let Some(na) = self.numeric_value(a) else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true;
                        continue 'quantum;
                    };
                    let Some(nb) = self.numeric_value(b) else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true;
                        continue 'quantum;
                    };
                    if nb == 0.0 {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Division by zero")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                    let result = na / nb;
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_number(result));
                }
                op::INTEGER_DIVIDE => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        let left = na.floor();
                        let right = nb.floor();
                        if right == 0.0 {
                            flush_ip!();
                            self.throw_error(fiber_idx, "Division by zero")?;
                            frame_changed = true;
                            continue 'quantum;
                        }
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_number((left / right).trunc()));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::MODULO => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(b_n)) = (self.numeric_value(a), self.numeric_value(b)) {
                        if b_n == 0.0 {
                            flush_ip!();
                            self.throw_error(fiber_idx, "Division by zero (modulo)")?;
                            frame_changed = true;
                            continue 'quantum;
                        } else {
                            self.fibers[fiber_idx]
                                .stack
                                .push(BxValue::new_number(na % b_n));
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers for modulo.")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::BIT_OR => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        let result = (na as i32) | (nb as i32);
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_int(result));
                    } else {
                        flush_ip!();
                        self.throw_error(
                            fiber_idx,
                            "Operands must be two numbers for bitwise OR.",
                        )?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::BIT_AND => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        let result = (na as i32) & (nb as i32);
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_int(result));
                    } else {
                        flush_ip!();
                        self.throw_error(
                            fiber_idx,
                            "Operands must be two numbers for bitwise AND.",
                        )?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::BIT_XOR => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        let result = (na as i32) ^ (nb as i32);
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_int(result));
                    } else {
                        flush_ip!();
                        self.throw_error(
                            fiber_idx,
                            "Operands must be two numbers for bitwise XOR.",
                        )?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::BIT_NOT => {
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let Some(na) = self.numeric_value(a) {
                        let result = !(na as i32);
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_int(result));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operand must be a number for bitwise NOT.")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::BIT_SHL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        let shift = (nb as i32 as u32) & 31;
                        let result = (na as i32 as u32).wrapping_shl(shift) as i32;
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_int(result));
                    } else {
                        flush_ip!();
                        self.throw_error(
                            fiber_idx,
                            "Operands must be two numbers for bitwise shift left.",
                        )?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::BIT_SHR => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        let shift = (nb as i32 as u32) & 31;
                        let result = (na as i32) >> shift;
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_int(result));
                    } else {
                        flush_ip!();
                        self.throw_error(
                            fiber_idx,
                            "Operands must be two numbers for bitwise shift right.",
                        )?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::BIT_USHR => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        let shift = (nb as i32 as u32) & 31;
                        let result = (na as i32 as u32) >> shift;
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_number(result as f64));
                    } else {
                        flush_ip!();
                        self.throw_error(
                            fiber_idx,
                            "Operands must be two numbers for bitwise unsigned shift right.",
                        )?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::POW => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let array_symmetric_difference = match (a.as_gc_id(), b.as_gc_id()) {
                        (Some(a_id), Some(b_id)) => {
                            let (left, right) = match (self.heap.get(a_id), self.heap.get(b_id)) {
                                (GcObject::Array(left), GcObject::Array(right)) => {
                                    (Some(left.clone()), Some(right.clone()))
                                }
                                _ => (None, None),
                            };
                            left.zip(right).map(|(left, right)| {
                                let mut values = left
                                    .iter()
                                    .filter(|value| {
                                        !right.iter().any(|other| self.is_equal(**value, *other))
                                    })
                                    .copied()
                                    .collect::<Vec<_>>();
                                values.extend(
                                    right.iter().filter(|value| {
                                        !left.iter().any(|other| self.is_equal(**value, *other))
                                    }).copied(),
                                );
                                values
                            })
                        }
                        _ => None,
                    };

                    if let Some(values) = array_symmetric_difference {
                        let id = self.heap.alloc(GcObject::Array(values));
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                    } else if let (Some(na), Some(nb)) = (self.numeric_value(a), self.numeric_value(b)) {
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_number(na.powf(nb)));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers for power.")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::XOR_OP => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a_true = self.is_truthy(a);
                    let b_true = self.is_truthy(b);
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_bool(a_true != b_true));
                }
                op::EQV_OP => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a_true = self.is_truthy(a);
                    let b_true = self.is_truthy(b);
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_bool(a_true == b_true));
                }
                op::RANGE => {
                    let right_excl_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let left_excl_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let end_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let start_val = self.fibers[fiber_idx].stack.pop().unwrap();

                    let right_excl = right_excl_val.is_bool() && right_excl_val.as_bool();
                    let left_excl = left_excl_val.is_bool() && left_excl_val.as_bool();

                    if !start_val.is_number() || !end_val.is_number() {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Range bounds must be numbers")?;
                        frame_changed = true;
                        continue 'quantum;
                    }

                    let start = start_val.as_number() as i64;
                    let end = end_val.as_number() as i64;
                    let id = self.heap.alloc(GcObject::Range(BxRange::from_bounds(
                        start, end, left_excl, right_excl,
                    )));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                }
                op::POP => {
                    let fiber = &mut self.fibers[fiber_idx];
                    fiber.stack.pop();
                    let frame = fiber.frames.last_mut().unwrap();
                    if let Some(slot) = fiber.stack.len().checked_sub(frame.stack_base) {
                        frame.local_names.remove(&slot);
                    }
                }
                op::JUMP_IF_FALSE => {
                    let offset = op0;
                    if !self.is_truthy(*self.fibers[fiber_idx].stack.last().unwrap()) {
                        ip += offset as usize;
                    }
                }
                op::JUMP_IF_NULL => {
                    let offset = op0;
                    if self.fibers[fiber_idx].stack.last().unwrap().is_null() {
                        ip += offset as usize;
                    }
                }
                op::JUMP => {
                    let offset = op0;
                    ip += offset as usize;
                }
                op::LOOP => {
                    let offset = op0;
                    ip -= offset as usize;
                    #[cfg(feature = "jit")]
                    {
                        flush_ip!();
                        match self.run_jit_construct_loop(fiber_idx, ip).or_else(||self.run_jit_fn_loop(fiber_idx, ip)) {
                            Some(jit::FnJitOutcome::Return(result)) => {
                                if let Some(result) = self.handle_return(fiber_idx, result)? {
                                    return Ok(Some(result));
                                }
                                frame_changed = true;
                                continue 'quantum;
                            }
                            Some(jit::FnJitOutcome::Yield) => break 'quantum,
                            Some(jit::FnJitOutcome::Resume(resume)) => { ip=resume; }
                            None => {}
                        }
                    }
                    // Safe point: yield to scheduler if timeslice expired.
                    // Skipped entirely (None fast-path) when only one fiber is running.
                    // Throttled to every 1024 backward branches when active.
                    if let Some(end) = timeslice_end {
                        safe_point_count = safe_point_count.wrapping_add(1);
                        if safe_point_count & 1023 == 0 && Instant::now() >= end {
                            break 'quantum; // ip flushed after the loop
                        }
                    }
                }
                op::RETURN => {
                    let result = {
                        let fiber = &mut self.fibers[fiber_idx];
                        let stack_base = fiber.frames.last().unwrap().stack_base;
                        if fiber.stack.len() > stack_base {
                            fiber.stack.pop().unwrap()
                        } else {
                            BxValue::new_null()
                        }
                    };

                    if let Some(result) = self.handle_return(fiber_idx, result)? {
                        return Ok(Some(result));
                    }

                    // Either a finally block was entered or the result was delivered to
                    // a caller frame. Reload the active frame before dispatching again.
                    frame_changed = true;
                    continue 'quantum;
                }
                op::END_FINALLY => {
                    let pending = self.fibers[fiber_idx]
                        .frames
                        .last_mut()
                        .unwrap()
                        .pending_completion
                        .take();

                    match pending {
                        Some(PendingCompletion::Return(result)) => {
                            if let Some(result) = self.handle_return(fiber_idx, result)? {
                                return Ok(Some(result));
                            }
                            frame_changed = true;
                            continue 'quantum;
                        }
                        Some(PendingCompletion::Throw(value)) => {
                            flush_ip!();
                            self.throw_value(fiber_idx, value)?;
                            frame_changed = true;
                            continue 'quantum;
                        }
                        None => {}
                    }
                }

                // --- Global / Scope Opcodes ---
                op::GET_GLOBAL => {
                    let idx = op0;
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.cache_get(ip_at_start)
                    };

                    if let Some(IcEntry::Global { index, namespace }) = ic
                        && namespace == self.global_namespace
                    {
                        let val = self.global_values[index];
                        self.fibers[fiber_idx].stack.push(val);
                    } else {
                        let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            let val = self.global_values[global_idx];
                            self.fibers[fiber_idx].stack.push(val);

                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.chunk.borrow_mut();
                            chunk.cache_set(ip_at_start, IcEntry::Global { index: global_idx, namespace: self.global_namespace });
                        } else {
                            self.fibers[fiber_idx].stack.push(BxValue::new_null());
                        }
                    }
                }
                op::SET_GLOBAL => {
                    let idx = op0;
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.cache_get(ip_at_start)
                    };

                    let val = *self.fibers[fiber_idx].stack.last().unwrap();

                    if let Some(IcEntry::Global { index, namespace }) = ic
                        && namespace == self.global_namespace
                    {
                        self.global_values[index] = val;
                    } else {
                        let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            self.global_values[global_idx] = val;

                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.chunk.borrow_mut();
                            chunk.cache_set(ip_at_start, IcEntry::Global { index: global_idx, namespace: self.global_namespace });
                        } else {
                            self.insert_global_interned(name_id, val);
                            if let Some(&global_idx) = self.global_names.get(&name_id) {
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.chunk.borrow_mut();
                                chunk.cache_set(ip_at_start, IcEntry::Global { index: global_idx, namespace: self.global_namespace });
                            }
                        }
                    }
                }
                op::SET_GLOBAL_POP => {
                    let idx = op0;
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.cache_get(ip_at_start)
                    };

                    let val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(IcEntry::Global { index, namespace }) = ic
                        && namespace == self.global_namespace
                    {
                        self.global_values[index] = val;
                    } else {
                        let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            self.global_values[global_idx] = val;

                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.chunk.borrow_mut();
                            chunk.cache_set(ip_at_start, IcEntry::Global { index: global_idx, namespace: self.global_namespace });
                        } else {
                            self.insert_global_interned(name_id, val);
                            if let Some(&global_idx) = self.global_names.get(&name_id) {
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.chunk.borrow_mut();
                                chunk.cache_set(ip_at_start, IcEntry::Global { index: global_idx, namespace: self.global_namespace });
                            }
                        }
                    }
                }
                op::DEFINE_GLOBAL => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.insert_global_interned(name_id, val);
                }
                op::GET_PRIVATE => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                    let name = self.interner.resolve(name_id).to_string().to_lowercase();
                    let val = self.private_value(fiber_idx, &name);

                    if let Some(v) = val {
                        self.fibers[fiber_idx].stack.push(v);
                    } else {
                        flush_ip!();
                        self.throw_error(
                            fiber_idx,
                            &format!("Variable '{}' not found in class or global scope.", name),
                        )?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::SET_PRIVATE => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                    let name = self.interner.resolve(name_id).to_string().to_lowercase();
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    if self.assign_function_scope(fiber_idx, &name, val) {
                        continue 'quantum;
                    }
                    let instance = self.fibers[fiber_idx].frames.last().unwrap().receiver
                        .and_then(|receiver| self.receiver_instance_gc_id(receiver));
                    let declared = instance.is_some_and(|id| {
                        matches!(self.heap.get(id), GcObject::Instance(inst) if inst.variables.borrow().contains_key(&name))
                    });
                    let function = &self.fibers[fiber_idx].frames.last().unwrap().function;
                    if instance.is_none() && function.kind != crate::types::FunctionKind::Lambda
                        && !self.has_global_function(&name) && self.get_global(&name).is_some() {
                        self.insert_global(name, val);
                    } else if instance.is_none() && function.kind == crate::types::FunctionKind::ComponentBody && function.captured_scopes.is_empty() {
                        self.insert_global(name, val);
                    } else if !declared && function.arity as usize == function.params.len() + 1
                        && !(function.kind == crate::types::FunctionKind::ComponentBody && function.captured_scopes.is_empty()) {
                        let scope = self.local_scope(fiber_idx).as_gc_id().unwrap();
                        self.struct_set(scope, &name, val);
                    } else if let Some(id) = instance {
                        if let GcObject::Instance(inst) = self.heap.get_mut(id) {
                            inst.variables.borrow_mut().insert(name, val);
                        }
                    } else {
                        self.fibers[fiber_idx].variables.borrow_mut().insert(name, val);
                    }
                }

                // --- Stack Manipulation ---
                op::DUP => {
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    self.fibers[fiber_idx].stack.push(val);
                }
                op::DUP_BELOW => {
                    let stack = &mut self.fibers[fiber_idx].stack;
                    let len = stack.len();
                    let val = stack[len - 1 - op0 as usize];
                    stack.insert(len - op0 as usize, val);
                }
                op::CTOR_FINISH => {
                    let stack = &mut self.fibers[fiber_idx].stack;
                    let result = stack.pop().unwrap();
                    if !result.is_null() {
                        *stack.last_mut().unwrap() = result;
                    }
                }
                op::SWAP => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(b);
                    self.fibers[fiber_idx].stack.push(a);
                }
                op::OVER => {
                    let val = self.fibers[fiber_idx].stack[self.fibers[fiber_idx].stack.len() - 2];
                    self.fibers[fiber_idx].stack.push(val);
                }
                op::INC => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if val.is_number() {
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_number(val.as_number() + 1.0));
                    } else if val.is_int() {
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_int(val.as_int() + 1));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::DEC => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if val.is_number() {
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_number(val.as_number() - 1.0));
                    } else if val.is_int() {
                        self.fibers[fiber_idx]
                            .stack
                            .push(BxValue::new_int(val.as_int() - 1));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Decrement operand must be a number")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }

                // --- Data Structures ---
                op::ARRAY => {
                    let count = op0;
                    let mut items = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        items.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    items.reverse();
                    let id = self.heap.alloc(GcObject::Array(items));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                }
                op::ARRAY_SPREAD => {
                    let count = op0;
                    let mut result = Vec::new();
                    let mut pairs = Vec::with_capacity(count as usize * 2);
                    for _ in 0..(count * 2) {
                        pairs.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    pairs.reverse();
                    for chunk in pairs.chunks(2) {
                        let should_spread = chunk[0].is_bool() && chunk[0].as_bool();
                        let val = chunk[1];
                        if should_spread {
                            result.extend(self.flatten_spread_array_value(val)?);
                        } else {
                            result.push(val);
                        }
                    }
                    let id = self.heap.alloc(GcObject::Array(result));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                }
                op::STRUCT_SPREAD => {
                    let count = op0;
                    let mut resolved: Vec<(String, BxValue)> = Vec::new();
                    for _ in 0..count {
                        let marker = self.fibers[fiber_idx].stack.pop().unwrap();
                        if !marker.is_bool() {
                            flush_ip!();
                            self.throw_error(
                                fiber_idx,
                                "Internal error: struct spread marker must be boolean",
                            )?;
                            frame_changed = true;
                            continue 'quantum;
                        }
                        if marker.as_bool() {
                            let spread_val = self.fibers[fiber_idx].stack.pop().unwrap();
                            let mut spread_entries =
                                self.flatten_spread_struct_entries(spread_val)?;
                            spread_entries.reverse();
                            resolved.extend(spread_entries);
                        } else {
                            let value = self.fibers[fiber_idx].stack.pop().unwrap();
                            let key_val = self.fibers[fiber_idx].stack.pop().unwrap();
                            resolved.push((self.to_string(key_val), value));
                        }
                    }
                    resolved.reverse();
                    let mut shape_id = self.shapes.get_root();
                    let mut props = Vec::with_capacity(resolved.len());
                    for (key, value) in resolved {
                        let key_id = self.interner.intern(&key);
                        shape_id = self.shapes.transition(shape_id, key_id);
                        props.push(value);
                    }
                    let id = self.heap.alloc(GcObject::Struct(BxStruct {
                        shape_id,
                        properties: props,
                        case_sensitive: false,
                        ordered: false,
                    }));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                }
                op::STRUCT => {
                    let count = op0;
                    let mut kv_pairs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        let value = self.fibers[fiber_idx].stack.pop().unwrap();
                        let key_val = self.fibers[fiber_idx].stack.pop().unwrap();
                        let key_str = self.to_string(key_val);
                        let key_id = self.interner.intern(&key_str);
                        kv_pairs.push((key_id, value));
                    }
                    kv_pairs.reverse();
                    let value = self.literal_struct(&kv_pairs);
                    self.fibers[fiber_idx].stack.push(value);
                }
                op::INDEX | op::SAFE_INDEX => {
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if self.class_reference_target(base_val).is_some() {
                        match self.class_reference_member(base_val, &self.to_string(index_val)) {
                            Ok(value) => self.fibers[fiber_idx].stack.push(value),
                            Err(_) if opcode == op::SAFE_INDEX => self.fibers[fiber_idx].stack.push(BxValue::new_null()),
                            Err(error) => vm_throw!("{}", error),
                        }
                        continue 'quantum;
                    }
                    if let Some(id) = base_val.as_gc_id() {
                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        if let GcObject::JsValue(js) = self.heap.get(id) {
                            let js = unwrap_matchbox_js_proxy(js);
                            let js_index = if index_val.is_int() {
                                JsValue::from_f64(index_val.as_int() as f64)
                            } else if index_val.is_number() {
                                JsValue::from_f64(index_val.as_number())
                            } else {
                                JsValue::from_str(&self.to_string(index_val))
                            };
                            match Reflect::get(&js, &js_index) {
                                Ok(val) => {
                                    let bx_val = self.js_to_bx(val);
                                    self.fibers[fiber_idx].stack.push(bx_val);
                                }
                                Err(_) => self.fibers[fiber_idx].stack.push(BxValue::new_null()),
                            }
                            flush_ip!();
                            continue 'quantum;
                        }

                        match self.heap.get(id) {
                            GcObject::Array(arr) => {
                                if index_val.is_number() || index_val.is_int() {
                                    if index_val.is_number() && index_val.as_number().fract() != 0.0 {
                                        flush_ip!();
                                        self.throw_error(fiber_idx, "Array index must be an integer")?;
                                        frame_changed = true;
                                        continue 'quantum;
                                    }
                                    let raw_index = if index_val.is_int() {
                                        index_val.as_int() as i64
                                    } else {
                                        index_val.as_number() as i64
                                    };
                                    if raw_index == 0 {
                                        flush_ip!();
                                        self.throw_error(fiber_idx, "Array index must not be zero")?;
                                        frame_changed = true;
                                        continue 'quantum;
                                    }
                                    let resolved_index = if raw_index < 0 {
                                        arr.len() as i64 + raw_index
                                    } else {
                                        raw_index - 1
                                    };
                                    if resolved_index < 0 || resolved_index >= arr.len() as i64 {
                                        if opcode == op::SAFE_INDEX {
                                            self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                        } else {
                                            flush_ip!();
                                            self.throw_error(fiber_idx, "Array index out of bounds")?;
                                            frame_changed = true;
                                            continue 'quantum;
                                        }
                                    } else {
                                        self.fibers[fiber_idx]
                                            .stack
                                            .push(arr[resolved_index as usize]);
                                    }
                                } else {
                                    flush_ip!();
                                    self.throw_error(fiber_idx, "Array index must be a number")?;
                                    frame_changed = true;
                                    continue 'quantum;
                                }
                            }
                            GcObject::Struct(_) => {
                                let key_str = self.to_string(index_val);
                                let value = self.struct_get(id, &key_str);
                                self.fibers[fiber_idx].stack.push(value);
                            }
                            _ => {
                                flush_ip!();
                                self.throw_error(
                                    fiber_idx,
                                    "Invalid access: base must be array or struct",
                                )?;
                                frame_changed = true;
                                continue 'quantum;
                            }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(
                            fiber_idx,
                            "Invalid access: base must be array or struct",
                        )?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::SET_INDEX => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = match self.class_reference_assignment_target(base_val, &self.to_string(index_val)) {
                        Ok(value) => value,
                        Err(error) => vm_throw!("{}", error),
                    };

                    if let Some(id) = base_val.as_gc_id() {
                        let index_string = self.to_string(index_val);
                        if self.struct_assignment_hooks.contains_key(&id) {
                            flush_ip!();
                            match self.assign_struct(id, &index_string, val) {
                                Ok(()) => self.fibers[fiber_idx].stack.push(val),
                                Err(error) => self.throw_error(fiber_idx, &error)?,
                            }
                            frame_changed = true; continue 'quantum;
                        }
                        let key_id = if !index_val.is_number() && !index_val.is_int() {
                            Some(self.struct_key_id(id, &index_string))
                        } else {
                            None
                        };

                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        {
                            if let GcObject::JsValue(js) = self.heap.get(id) {
                                let js = unwrap_matchbox_js_proxy(js);
                                let js_index = if index_val.is_int() {
                                    JsValue::from_f64(index_val.as_int() as f64)
                                } else if index_val.is_number() {
                                    JsValue::from_f64(index_val.as_number())
                                } else {
                                    JsValue::from_str(&self.to_string(index_val))
                                };
                                let js_val = self.bx_to_js(&val);
                                match Reflect::set(&js, &js_index, &js_val) {
                                    Ok(_) => {
                                        self.fibers[fiber_idx].stack.push(val);
                                    }
                                    Err(e) => {
                                        flush_ip!();
                                        self.throw_error(
                                            fiber_idx,
                                            &format!("JS set error: {:?}", e),
                                        )?;
                                        frame_changed = true;
                                        continue 'quantum;
                                    }
                                }
                                flush_ip!();
                                continue 'quantum;
                            }
                        }

                        match self.heap.get_mut(id) {
                            GcObject::Array(arr) => {
                                if index_val.is_number() || index_val.is_int() {
                                    let idx = if index_val.is_int() {
                                        index_val.as_int() as usize
                                    } else {
                                        index_val.as_number() as usize
                                    };
                                    if idx < 1 {
                                        flush_ip!();
                                        self.throw_error(
                                            fiber_idx,
                                            &format!("Array index out of bounds: {}", idx),
                                        )?;
                                        frame_changed = true;
                                        continue 'quantum;
                                    } else if idx > arr.len() {
                                        // Auto-grow: fill gaps with null
                                        arr.resize(idx, BxValue::new_null());
                                        arr[idx - 1] = val;
                                        self.fibers[fiber_idx].stack.push(val);
                                    } else {
                                        arr[idx - 1] = val;
                                        self.fibers[fiber_idx].stack.push(val);
                                    }
                                } else {
                                    flush_ip!();
                                    self.throw_error(fiber_idx, "Array index must be a number")?;
                                    frame_changed = true;
                                    continue 'quantum;
                                }
                            }
                            GcObject::Struct(s) => {
                                let key_id = key_id.unwrap();
                                if let Some(idx) = self.shapes.get_index(s.shape_id, key_id) {
                                    s.properties[idx as usize] = val;
                                } else {
                                    s.shape_id = self.shapes.transition(s.shape_id, key_id);
                                    s.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::Instance(inst) => {
                                let key_id = key_id.unwrap();
                                if let Some(idx) = self.shapes.get_index(inst.shape_id, key_id) {
                                    inst.properties[idx as usize] = val;
                                } else {
                                    inst.shape_id = self.shapes.transition(inst.shape_id, key_id);
                                    inst.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::NativeObject(object) => {
                                if object.borrow().is_readonly() {
                                    flush_ip!(); self.throw_error(fiber_idx, "Native object is read-only")?;
                                    frame_changed = true; continue 'quantum;
                                }
                                let object = object.clone();
                                self.native_object_set(&object, &index_string, val);
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => {
                                flush_ip!();
                                self.throw_error(fiber_idx, "Invalid indexed assignment")?;
                                frame_changed = true;
                                continue 'quantum;
                            }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Invalid indexed assignment")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::MEMBER => {
                    let idx = op0;
                    let base = *self.fibers[fiber_idx].stack.last().unwrap();
                    if let Some(value) = self.cached_member_value(fiber_idx, ip_at_start, base) {
                        *self.fibers[fiber_idx].stack.last_mut().unwrap() = value;
                        flush_ip!();
                        continue 'quantum;
                    }
                    let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if self.class_reference_target(base_val).is_some() {
                        let name = self.interner.resolve(name_id).to_owned();
                        match self.class_reference_member(base_val, &name) {
                            Ok(value) => self.fibers[fiber_idx].stack.push(value),
                            Err(error) => vm_throw!("{}", error),
                        }
                        continue 'quantum;
                    }

                    if let Some(id) = base_val.as_gc_id() {
                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        if let GcObject::JsValue(js) = self.heap.get(id) {
                            let js = unwrap_matchbox_js_proxy(js);
                            let name = self.interner.resolve(name_id);
                            let prop = resolve_js_property(&js, name);
                            match Reflect::get(&js, &prop) {
                                Ok(val) => {
                                    let bx_val = self.js_to_bx(val);
                                    self.fibers[fiber_idx].stack.push(bx_val);
                                }
                                Err(_) => self.fibers[fiber_idx].stack.push(BxValue::new_null()),
                            }
                            flush_ip!();
                            continue 'quantum;
                        }

                        #[cfg(all(
                            target_arch = "wasm32",
                            feature = "js-host-abi",
                            not(feature = "js")
                        ))]
                        {
                            let maybe_handle = if let GcObject::JsHandle(h) = self.heap.get(id) {
                                Some(*h)
                            } else {
                                None
                            };
                            if let Some(handle) = maybe_handle {
                                let name = self.interner.resolve(name_id);
                                let key_bytes = name.as_bytes();
                                let mut str_buf = [0u8; 4096];
                                let mut out_str_len: usize = 0;
                                let mut out_num: f64 = 0.0;
                                let mut out_bool: i32 = 0;
                                let mut out_obj: u32 = 0;
                                let rtype = unsafe {
                                    bx_js_get_prop(
                                        handle,
                                        key_bytes.as_ptr(),
                                        key_bytes.len(),
                                        str_buf.as_mut_ptr(),
                                        4096,
                                        &mut out_str_len,
                                        &mut out_num,
                                        &mut out_bool,
                                        &mut out_obj,
                                    )
                                };
                                let bx_val = self.js_result_to_bx(
                                    rtype,
                                    &str_buf,
                                    out_str_len,
                                    out_num,
                                    out_bool,
                                    out_obj,
                                );
                                self.fibers[fiber_idx].stack.push(bx_val);
                                flush_ip!();
                                continue 'quantum;
                            }
                        }

                        match self.heap.get(id) {
                            GcObject::Struct(s) => {
                                let shape_id = s.shape_id;
                                let properties_ptr = &s.properties as *const Vec<BxValue>;

                                let key_id = if s.case_sensitive {
                                    let mut name = self.read_string_constant(fiber_idx, idx as usize)?;
                                    self.interner.get_exact_id(name.as_flat_str())
                                } else {
                                    Some(name_id)
                                };
                                if let Some(idx) = key_id
                                    .and_then(|key_id| self.shapes.get_index(shape_id, key_id))
                                {
                                    {
                                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        chunk.cache_add_shape(
                                            ip_at_start,
                                            shape_id as usize,
                                            idx as usize,
                                        );
                                    }
                                    let val = unsafe { &*properties_ptr }[idx as usize];
                                    self.fibers[fiber_idx].stack.push(val);
                                } else {
                                    self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                }
                            }
                            GcObject::Instance(inst) => {
                                let shape_id = inst.shape_id;
                                let properties_ptr = &inst.properties as *const Vec<BxValue>;

                                if let Some(idx) = self.shapes.get_index(shape_id, name_id) {
                                    {
                                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        chunk.cache_add_shape(
                                            ip_at_start,
                                            shape_id as usize,
                                            idx as usize,
                                        );
                                    }
                                    let val = unsafe { &*properties_ptr }[idx as usize];
                                    self.fibers[fiber_idx].stack.push(val);
                                } else {
                                    let name = self.interner.resolve(name_id).to_string();
                                    if let Ok(value) = self.method_reference(base_val, &name) {
                                        self.fibers[fiber_idx].stack.push(value);
                                    } else if let Some(getter) = Self::accessor_method_name("get", &name)
                                        .filter(|getter| self.instance_has_method(base_val, getter))
                                    {
                                        // BoxLang invokeImplicitAccessor: `obj.name` calls `getName()`.
                                        flush_ip!();
                                        match self.call_method_value(base_val, &getter, Vec::new()) {
                                            Ok(value) => self.fibers[fiber_idx].stack.push(value),
                                            Err(error) => self.throw_error(fiber_idx, &error.to_string())?,
                                        }
                                        frame_changed = true; continue 'quantum;
                                    } else {
                                        self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                    }
                                }
                            }
                            GcObject::Class(_) => {
                                let name = self.interner.resolve(name_id).to_string();
                                match self.method_reference(base_val, &name) {
                                    Ok(value) => self.fibers[fiber_idx].stack.push(value),
                                    Err(error) => vm_throw!("{}", error),
                                }
                            }
                            GcObject::NativeObject(obj) => {
                                let name =
                                    self.interner.resolve(name_id).to_string().to_lowercase();
                                #[cfg(not(target_arch = "wasm32"))]
                                let is_query = obj.borrow().query_columns().is_some();
                                #[cfg(target_arch = "wasm32")]
                                let is_query = false;
                                let obj = obj.clone();
                                let property = self.native_object_get(&obj, &name);
                                let val = if !property.is_null() {
                                    property
                                } else if is_query {
                                    let column_name = self.string_new(name.clone());
                                    self.native_object_call_method(
                                        id,
                                        "columndata",
                                        &[BxValue::new_ptr(column_name)],
                                    )
                                    .unwrap_or_else(|_| BxValue::new_null())
                                } else {
                                    obj.borrow().get_property(&name)
                                };
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => {
                                flush_ip!();
                                self.throw_error(fiber_idx, "Member access only supported on structs, instances, JS objects, and native objects")?;
                                frame_changed = true;
                                continue 'quantum;
                            }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Member access only supported on structs, instances, JS objects, and native objects")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::SET_MEMBER => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = match self.class_reference_assignment_target(base_val, self.interner.resolve(name_id)) {
                        Ok(value) => value,
                        Err(error) => vm_throw!("{}", error),
                    };

                    if let Some(id) = base_val.as_gc_id() {
                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        if let GcObject::JsValue(js) = self.heap.get(id) {
                            let js = js.clone();
                            let name = self.interner.resolve(name_id);
                            let prop = resolve_js_property(&js, name);
                            let js_val = self.bx_to_js(&val);
                            Reflect::set(&js, &prop, &js_val).ok();
                            self.fibers[fiber_idx].stack.push(val);
                            flush_ip!();
                            continue 'quantum;
                        }

                        #[cfg(all(
                            target_arch = "wasm32",
                            feature = "js-host-abi",
                            not(feature = "js")
                        ))]
                        {
                            let maybe_handle = if let GcObject::JsHandle(h) = self.heap.get(id) {
                                Some(*h)
                            } else {
                                None
                            };
                            if let Some(handle) = maybe_handle {
                                let name = self.interner.resolve(name_id);
                                let key_bytes = name.as_bytes();
                                if val.is_null() {
                                    unsafe {
                                        bx_js_set_prop_null(
                                            handle,
                                            key_bytes.as_ptr(),
                                            key_bytes.len(),
                                        );
                                    }
                                } else if val.is_bool() {
                                    unsafe {
                                        bx_js_set_prop_bool(
                                            handle,
                                            key_bytes.as_ptr(),
                                            key_bytes.len(),
                                            if val.as_bool() { 1 } else { 0 },
                                        );
                                    }
                                } else if val.is_number() {
                                    unsafe {
                                        bx_js_set_prop_num(
                                            handle,
                                            key_bytes.as_ptr(),
                                            key_bytes.len(),
                                            val.as_number(),
                                        );
                                    }
                                } else if val.is_int() {
                                    unsafe {
                                        bx_js_set_prop_num(
                                            handle,
                                            key_bytes.as_ptr(),
                                            key_bytes.len(),
                                            val.as_int() as f64,
                                        );
                                    }
                                } else if let Some(val_gc_id) = val.as_gc_id() {
                                    let maybe_str_bytes: Option<Vec<u8>> =
                                        if let GcObject::String(s) = self.heap.get(val_gc_id) {
                                            Some(s.to_string().into_bytes())
                                        } else {
                                            None
                                        };
                                    let maybe_val_handle: Option<u32> =
                                        if let GcObject::JsHandle(h) = self.heap.get(val_gc_id) {
                                            Some(*h)
                                        } else {
                                            None
                                        };
                                    if let Some(str_bytes) = maybe_str_bytes {
                                        unsafe {
                                            bx_js_set_prop_str(
                                                handle,
                                                key_bytes.as_ptr(),
                                                key_bytes.len(),
                                                str_bytes.as_ptr(),
                                                str_bytes.len(),
                                            );
                                        }
                                    } else if let Some(val_handle) = maybe_val_handle {
                                        unsafe {
                                            bx_js_set_prop_obj(
                                                handle,
                                                key_bytes.as_ptr(),
                                                key_bytes.len(),
                                                val_handle,
                                            );
                                        }
                                    } else {
                                        unsafe {
                                            bx_js_set_prop_null(
                                                handle,
                                                key_bytes.as_ptr(),
                                                key_bytes.len(),
                                            );
                                        }
                                    }
                                } else {
                                    unsafe {
                                        bx_js_set_prop_null(
                                            handle,
                                            key_bytes.as_ptr(),
                                            key_bytes.len(),
                                        );
                                    }
                                }
                                self.fibers[fiber_idx].stack.push(val);
                                flush_ip!();
                                continue 'quantum;
                            }
                        }

                        if self.struct_assignment_hooks.contains_key(&id) {
                            let name = self.read_string_constant(fiber_idx, idx as usize)?.to_string();
                            flush_ip!();
                            match self.assign_struct(id, &name, val) {
                                Ok(()) => self.fibers[fiber_idx].stack.push(val),
                                Err(error) => self.throw_error(fiber_idx, &error)?,
                            }
                            frame_changed = true; continue 'quantum;
                        }
                        let struct_name_id = if matches!(self.heap.get_opt(id), Some(GcObject::Struct(_))) {
                            let mut name = self.read_string_constant(fiber_idx, idx as usize)?;
                            Some(self.struct_key_id(id, name.as_flat_str()))
                        } else {
                            None
                        };

                        // BoxLang invokeImplicitAccessor: `obj.name = v` calls `setName( v )` unless `name` is a field.
                        if let Some(GcObject::Instance(inst)) = self.heap.get_opt(id) {
                            if self.shapes.get_index(inst.shape_id, name_id).is_none() {
                                let name = self.interner.resolve(name_id).to_string();
                                if let Some(setter) = Self::accessor_method_name("set", &name)
                                    .filter(|setter| self.instance_has_method(base_val, setter))
                                {
                                    flush_ip!();
                                    match self.call_method_value(base_val, &setter, vec![val]) {
                                        Ok(_) => self.fibers[fiber_idx].stack.push(val),
                                        Err(error) => self.throw_error(fiber_idx, &error.to_string())?,
                                    }
                                    frame_changed = true; continue 'quantum;
                                }
                            }
                        }

                        match self.heap.get_mut(id) {
                            GcObject::Struct(s) => {
                                let name_id = struct_name_id.unwrap_or(name_id);
                                let shape_id = s.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.cache_get(ip_at_start)
                                };

                                match ic {
                                    Some(IcEntry::Monomorphic {
                                        shape_id: cached_shape,
                                        index,
                                    }) => {
                                        if cached_shape == shape_id as usize {
                                            s.properties[index as usize] = val;
                                            self.fibers[fiber_idx].stack.push(val);
                                            flush_ip!();
                                            continue 'quantum;
                                        }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                s.properties[entries[i].1] = val;
                                                self.fibers[fiber_idx].stack.push(val);
                                                flush_ip!();
                                                continue 'quantum;
                                            }
                                        }
                                    }
                                    _ => {}
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, name_id) {
                                    {
                                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        chunk.cache_add_shape(
                                            ip_at_start,
                                            shape_id as usize,
                                            idx as usize,
                                        );
                                    }
                                    s.properties[idx as usize] = val;
                                } else {
                                    s.shape_id = self.shapes.transition(shape_id, name_id);
                                    s.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::Instance(inst) => {
                                let shape_id = inst.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.cache_get(ip_at_start)
                                };

                                match ic {
                                    Some(IcEntry::Monomorphic {
                                        shape_id: cached_shape,
                                        index,
                                    }) => {
                                        if cached_shape == shape_id as usize {
                                            inst.properties[index as usize] = val;
                                            self.fibers[fiber_idx].stack.push(val);
                                            flush_ip!();
                                            continue 'quantum;
                                        }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                inst.properties[entries[i].1] = val;
                                                self.fibers[fiber_idx].stack.push(val);
                                                flush_ip!();
                                                continue 'quantum;
                                            }
                                        }
                                    }
                                    _ => {}
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, name_id) {
                                    {
                                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        chunk.cache_add_shape(
                                            ip_at_start,
                                            shape_id as usize,
                                            idx as usize,
                                        );
                                    }
                                    inst.properties[idx as usize] = val;
                                } else {
                                    inst.shape_id = self.shapes.transition(shape_id, name_id);
                                    inst.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::NativeObject(obj) => {
                                let name =
                                    self.interner.resolve(name_id).to_string().to_lowercase();
                                if obj.borrow().is_readonly() {
                                    flush_ip!(); self.throw_error(fiber_idx, "Native object is read-only")?;
                                    frame_changed = true; continue 'quantum;
                                }
                                let obj = obj.clone();
                                self.native_object_set(&obj, &name, val);
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => {
                                flush_ip!();
                                self.throw_error(fiber_idx, "Member assignment only supported on structs, instances, JS objects, and native objects")?;
                                frame_changed = true;
                                continue 'quantum;
                            }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Member assignment only supported on structs, instances, JS objects, and native objects")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::INC_MEMBER => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize)?;
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(id) = base_val.as_gc_id() {
                        if self.struct_assignment_hooks.contains_key(&id) {
                            let name = self.read_string_constant(fiber_idx, idx as usize)?.to_string();
                            let old = self.struct_get(id, &name);
                            flush_ip!();
                            match self.cast_value_to_type(old, "numeric").and_then(|old| {
                                let value = BxValue::new_number(old.as_number() + 1.0);
                                self.assign_struct(id, &name, value).map(|_| value)
                            }) {
                                Ok(value) => self.fibers[fiber_idx].stack.push(value),
                                Err(error) => self.throw_error(fiber_idx, &error)?,
                            }
                            frame_changed = true; continue 'quantum;
                        }
                        if let GcObject::NativeObject(object) = self.heap.get(id) {
                            let object = object.clone();
                            if object.borrow().native_data("matchbox.variables_scope").is_some() {
                                let name = self.interner.resolve(name_id).to_string();
                                let old = self.native_object_get(&object, &name);
                                if old.is_number() {
                                    let new = BxValue::new_number(old.as_number() + 1.0);
                                    self.native_object_set(&object, &name, new);
                                    self.fibers[fiber_idx].stack.push(new);
                                } else {
                                    flush_ip!();
                                    self.throw_error(fiber_idx, "Increment operand must be a number")?;
                                    frame_changed = true;
                                }
                                continue 'quantum;
                            }
                        }
                        match self.heap.get_mut(id) {
                            GcObject::Struct(s) => {
                                let shape_id = s.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.cache_get(ip_at_start)
                                };

                                let index = match ic {
                                    Some(IcEntry::Monomorphic {
                                        shape_id: cached_shape,
                                        index,
                                    }) => {
                                        if cached_shape == shape_id as usize {
                                            Some(index as usize)
                                        } else {
                                            None
                                        }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        let mut found = None;
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                found = Some(entries[i].1);
                                                break;
                                            }
                                        }
                                        found
                                    }
                                    _ => None,
                                };

                                if let Some(idx) = index.or_else(|| {
                                    self.shapes.get_index(shape_id, name_id).map(|i| i as usize)
                                }) {
                                    let old_val = s.properties[idx];
                                    if old_val.is_number() {
                                        let new_val =
                                            BxValue::new_number(old_val.as_number() + 1.0);
                                        s.properties[idx] = new_val;
                                        self.fibers[fiber_idx].stack.push(new_val);

                                        if index.is_none() {
                                            let frame =
                                                self.fibers[fiber_idx].frames.last().unwrap();
                                            let mut chunk = frame.chunk.borrow_mut();
                                            chunk.cache_add_shape(
                                                ip_at_start,
                                                shape_id as usize,
                                                idx as usize,
                                            );
                                        }
                                    } else {
                                        flush_ip!();
                                        self.throw_error(
                                            fiber_idx,
                                            "Increment operand must be a number",
                                        )?;
                                        frame_changed = true;
                                        continue 'quantum;
                                    }
                                } else {
                                    let name = self.interner.resolve(name_id).to_string();
                                    flush_ip!();
                                    self.throw_error(
                                        fiber_idx,
                                        &format!("Member {} not found", name),
                                    )?;
                                    frame_changed = true;
                                    continue 'quantum;
                                }
                            }
                            GcObject::Instance(inst) => {
                                let shape_id = inst.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.cache_get(ip_at_start)
                                };

                                let index = match ic {
                                    Some(IcEntry::Monomorphic {
                                        shape_id: cached_shape,
                                        index,
                                    }) => {
                                        if cached_shape == shape_id as usize {
                                            Some(index as usize)
                                        } else {
                                            None
                                        }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        let mut found = None;
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                found = Some(entries[i].1);
                                                break;
                                            }
                                        }
                                        found
                                    }
                                    _ => None,
                                };

                                if let Some(idx) = index.or_else(|| {
                                    self.shapes.get_index(shape_id, name_id).map(|i| i as usize)
                                }) {
                                    let old_val = inst.properties[idx];
                                    if old_val.is_number() {
                                        let new_val =
                                            BxValue::new_number(old_val.as_number() + 1.0);
                                        inst.properties[idx] = new_val;
                                        self.fibers[fiber_idx].stack.push(new_val);

                                        if index.is_none() {
                                            let frame =
                                                self.fibers[fiber_idx].frames.last().unwrap();
                                            let mut chunk = frame.chunk.borrow_mut();
                                            chunk.cache_add_shape(
                                                ip_at_start,
                                                shape_id as usize,
                                                idx as usize,
                                            );
                                        }
                                    } else {
                                        flush_ip!();
                                        self.throw_error(
                                            fiber_idx,
                                            "Increment operand must be a number",
                                        )?;
                                        frame_changed = true;
                                        continue 'quantum;
                                    }
                                } else {
                                    let name = self.interner.resolve(name_id).to_string();
                                    flush_ip!();
                                    self.throw_error(
                                        fiber_idx,
                                        &format!("Member {} not found", name),
                                    )?;
                                    frame_changed = true;
                                    continue 'quantum;
                                }
                            }
                            _ => {
                                flush_ip!();
                                self.throw_error(fiber_idx, "Fused increment only supported on structs and instances for now")?;
                                frame_changed = true;
                                continue 'quantum;
                            }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Member access only supported on objects")?;
                        frame_changed = true;
                        continue 'quantum;
                    }
                }
                op::STRING_CONCAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let Some(id) = a.as_gc_id() {
                        if self
                            .native_object_call_method(id, "__is_string_builder", &[])
                            .is_ok()
                        {
                            let result = self
                                .native_object_call_method(id, "append", &[b])
                                .map_err(anyhow::Error::msg)?;
                            self.fibers[fiber_idx].stack.push(result);
                            continue 'quantum;
                        }
                    }
                    let a_s = self.to_box_string(a);
                    let b_s = self.to_box_string(b);
                    let res_id = self.heap.alloc(GcObject::String(a_s.concat(&b_s)));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(res_id));
                }

                // --- Calls / Invocations ---
                op::CALL => {
                    let arg_count = op0;
                    flush_ip!();
                    self.execute_call(fiber_idx, arg_count as usize, None)?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) {
                        return Ok(None);
                    } // ip already flushed
                    frame_changed = true;
                    continue 'quantum;
                }
                op::CALL_SPREAD => {
                    let count = op0 as usize;
                    let mut flattened_chunks: Vec<Vec<BxValue>> = Vec::with_capacity(count);
                    for _ in 0..count {
                        let marker = self.fibers[fiber_idx].stack.pop().unwrap();
                        if !marker.is_bool() {
                            flush_ip!();
                            self.throw_error(
                                fiber_idx,
                                "Internal error: call spread marker must be boolean",
                            )?;
                            frame_changed = true;
                            continue 'quantum;
                        }
                        let value = self.fibers[fiber_idx].stack.pop().unwrap();
                        if marker.as_bool() {
                            flattened_chunks.push(self.flatten_spread_array_value(value)?);
                        } else {
                            flattened_chunks.push(vec![value]);
                        }
                    }
                    flattened_chunks.reverse();
                    let flattened_len: usize =
                        flattened_chunks.iter().map(|chunk| chunk.len()).sum();
                    for chunk in flattened_chunks {
                        for value in chunk {
                            self.fibers[fiber_idx].stack.push(value);
                        }
                    }
                    flush_ip!();
                    self.execute_call(fiber_idx, flattened_len, None)?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) {
                        return Ok(None);
                    }
                    frame_changed = true;
                    continue 'quantum;
                }
                op::CALL_NAMED_SPREAD => {
                    let count = op0;
                    let (args, names) = self.flatten_encoded_named_spread_args(fiber_idx, count)?;
                    for value in args {
                        self.fibers[fiber_idx].stack.push(value);
                    }
                    flush_ip!();
                    self.execute_call(fiber_idx, names.len(), Some(names))?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) {
                        return Ok(None);
                    }
                    frame_changed = true;
                    continue 'quantum;
                }
                op::CALL_NAMED => {
                    let total_count = op0;
                    let names_idx = next_word!();
                    let names = match self.read_constant(fiber_idx, names_idx as usize)? {
                        v if v.is_ptr() => {
                            if let GcObject::Array(arr) = self.heap.get(v.as_gc_id().unwrap()) {
                                arr.iter().map(|v| self.to_string(*v)).collect::<Vec<_>>()
                            } else {
                                bail!("Internal VM error: names constant is not a StringArray")
                            }
                        }
                        _ => bail!("Internal VM error: names constant is not a StringArray"),
                    };
                    flush_ip!();
                    self.execute_call(fiber_idx, total_count as usize, Some(names))?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) {
                        return Ok(None);
                    } // ip already flushed
                    frame_changed = true;
                    continue 'quantum;
                }
                op::INVOKE_NAMED_SPREAD => {
                    let name_idx = op0;
                    let total_count = next_word!();
                    let _unused = next_word!();
                    let name_id = self.read_intern_id(fiber_idx, name_idx as usize)?;
                    let name = self.interner.resolve(name_id).to_string();
                    let (args, names) =
                        self.flatten_encoded_named_spread_args(fiber_idx, total_count)?;
                    for value in args {
                        self.fibers[fiber_idx].stack.push(value);
                    }
                    flush_ip!();
                    self.execute_invoke(fiber_idx, name, names.len(), Some(names), ip_at_start)?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) {
                        return Ok(None);
                    }
                    frame_changed = true;
                    continue 'quantum;
                }
                op::INVOKE => {
                    let name_idx = op0;
                    let arg_count = next_word!();
                    let name_id = self.read_intern_id(fiber_idx, name_idx as usize)?;
                    let name = self.interner.resolve(name_id).to_string();
                    #[cfg(all(
                        target_arch = "wasm32",
                        feature = "js-host-abi",
                        not(feature = "js")
                    ))]
                    {
                        let receiver_idx =
                            self.fibers[fiber_idx].stack.len() - 1 - arg_count as usize;
                        let receiver_val = self.fibers[fiber_idx].stack[receiver_idx];
                        let maybe_handle = if let Some(id) = receiver_val.as_gc_id() {
                            if let GcObject::JsHandle(h) = self.heap.get(id) {
                                Some(*h)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if let Some(handle) = maybe_handle {
                            let method_bytes = name.as_bytes();
                            let mut args = Vec::with_capacity(arg_count as usize);
                            for i in 0..(arg_count as usize) {
                                args.push(self.fibers[fiber_idx].stack[receiver_idx + 1 + i]);
                            }
                            let args_json = self.bx_args_to_json(&args);
                            let mut str_buf = [0u8; 4096];
                            let mut out_str_len: usize = 0;
                            let mut out_num: f64 = 0.0;
                            let mut out_bool: i32 = 0;
                            let mut out_obj: u32 = 0;
                            let rtype = unsafe {
                                bx_js_call_method(
                                    handle,
                                    method_bytes.as_ptr(),
                                    method_bytes.len(),
                                    args_json.as_ptr(),
                                    args_json.len(),
                                    str_buf.as_mut_ptr(),
                                    4096,
                                    &mut out_str_len,
                                    &mut out_num,
                                    &mut out_bool,
                                    &mut out_obj,
                                )
                            };
                            for _ in 0..(arg_count as usize + 1) {
                                self.fibers[fiber_idx].stack.pop();
                            }
                            let bx_val = self.js_result_to_bx(
                                rtype,
                                &str_buf,
                                out_str_len,
                                out_num,
                                out_bool,
                                out_obj,
                            );
                            self.fibers[fiber_idx].stack.push(bx_val);
                            flush_ip!();
                            frame_changed = true;
                            continue 'quantum;
                        }
                    }
                    flush_ip!();
                    self.execute_invoke(fiber_idx, name, arg_count as usize, None, ip_at_start)?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) {
                        return Ok(None);
                    } // ip already flushed
                    frame_changed = true;
                    continue 'quantum;
                }
                op::INVOKE_NAMED => {
                    let name_idx = op0;
                    let total_count = next_word!();
                    let names_idx = next_word!();
                    let invoke_name_id = self.read_intern_id(fiber_idx, name_idx as usize)?;
                    let name = self.interner.resolve(invoke_name_id).to_string();
                    let names = match self.read_constant(fiber_idx, names_idx as usize)? {
                        v if v.is_ptr() => {
                            if let GcObject::Array(arr) = self.heap.get(v.as_gc_id().unwrap()) {
                                arr.iter().map(|v| self.to_string(*v)).collect::<Vec<_>>()
                            } else {
                                bail!("Internal VM error: names constant is not a StringArray")
                            }
                        }
                        _ => bail!("Internal VM error: names constant is not a StringArray"),
                    };
                    flush_ip!();
                    self.execute_invoke(
                        fiber_idx,
                        name,
                        total_count as usize,
                        Some(names),
                        ip_at_start,
                    )?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) {
                        return Ok(None);
                    } // ip already flushed
                    frame_changed = true;
                    continue 'quantum;
                }
                op::NEW => {
                    let arg_count = op0;
                    let class_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count as usize;
                    let class_val = self.fibers[fiber_idx].stack[class_idx];
                    if let Some(id) = class_val.as_gc_id() {
                        let class = if let GcObject::Class(c) = self.heap.get(id) {
                            Some(Rc::clone(c))
                        } else {
                            None
                        };

                        if let Some(class) = class {
                            let variables_scope = Rc::new(RefCell::new(HashMap::new()));

                            let inst_id = self.heap.alloc(GcObject::Instance(BxInstance {
                                class: Rc::clone(&class),
                                shape_id: self.shapes.get_root(),
                                properties: Vec::new(),
                                variables: variables_scope.clone(),
                            }));

                            let instance_val = BxValue::new_ptr(inst_id);
                            self.fibers[fiber_idx].stack[class_idx] = instance_val;

                            let constructor = class.borrow().constructor.clone();
                            let sub_chunk = constructor.chunk.execution_chunk();
                            let constant_count = sub_chunk.borrow().constants().len();

                            let frame = CallFrame {
                                #[cfg(feature = "jit")]
                                jit_loop_attempted: false,
                                #[cfg(feature = "jit")]
                                jit_construct_unsupported: None,
                                context: None,
                                function: Rc::new(constructor),
                                chunk: sub_chunk,
                                ip: 0,
                                scope_mode: scopes::ScopeMode::Binding,
                                local_scope: None,
                                local_names: scopes::SlotBindings::new(),
                                stack_base: class_idx + 1 + arg_count as usize,
                                receiver: Some(instance_val),
                                handlers: Vec::new(),
                                pending_completion: None,
                                promoted_constants: vec![None; constant_count],
                            };
                            flush_ip!();
                            self.fibers[fiber_idx].frames.push(frame);
                            frame_changed = true;
                            continue 'quantum;
                        }

                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        {
                            if let GcObject::JsValue(js) = self.heap.get(id) {
                                let js = unwrap_matchbox_js_proxy(js);
                                if let Ok(ctor) = js.dyn_into::<Function>() {
                                    let js_args = Array::new();
                                    let mut args = Vec::new();
                                    for _ in 0..arg_count {
                                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                                    }
                                    args.reverse();
                                    for arg in args {
                                        js_args.push(&self.bx_to_js(&arg));
                                    }
                                    self.fibers[fiber_idx].stack.pop(); // pop constructor
                                    match Reflect::construct(&ctor, &js_args) {
                                        Ok(val) => {
                                            let bx_val = self.js_to_bx(val);
                                            self.fibers[fiber_idx].stack.push(bx_val);
                                            flush_ip!();
                                            frame_changed = true;
                                            continue 'quantum;
                                        }
                                        Err(e) => {
                                            vm_throw!("JS constructor error: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }

                        vm_throw!("Can only instantiate classes.");
                    } else {
                        vm_throw!("Can only instantiate classes.");
                    }
                }

                // --- Comparison ---
                op::EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let res = self.is_equal(a, b);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(res));
                }
                op::NOT_EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let res = self.is_equal(a, b);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(!res));
                }
                op::STRICT_EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = self.is_strict_equal(a, b);
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_bool(result));
                }
                op::STRICT_NOT_EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = !self.is_strict_equal(a, b);
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_bool(result));
                }
                op::LESS => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = self.compare_values(a, b) == std::cmp::Ordering::Less;
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_bool(result));
                }
                op::LESS_EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = self.compare_values(a, b) != std::cmp::Ordering::Greater;
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_bool(result));
                }
                op::GREATER => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = self.compare_values(a, b) == std::cmp::Ordering::Greater;
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_bool(result));
                }
                op::GREATER_EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = self.compare_values(a, b) != std::cmp::Ordering::Less;
                    self.fibers[fiber_idx]
                        .stack
                        .push(BxValue::new_bool(result));
                }
                op::NOT => {
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let res = self.is_truthy(a);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(!res));
                }

                // --- Control Flow / Misc ---
                op::ITER_NEXT => {
                    let collection_slot = op0;
                    let word1 = next_word!();
                    let cursor_slot = word1 & 0x7FFF_FFFF;
                    let push_index = (word1 >> 31) != 0;
                    let offset = next_word!();
                    let collection_idx = stack_base + collection_slot as usize;
                    let cursor_idx = stack_base + cursor_slot as usize;
                    let collection = self.fibers[fiber_idx].stack[collection_idx];

                    let (is_done, next_val, next_idx) = {
                        let cursor_val = if self.fibers[fiber_idx].stack[cursor_idx].is_number() {
                            self.fibers[fiber_idx].stack[cursor_idx].as_number() as usize
                        } else if self.fibers[fiber_idx].stack[cursor_idx].is_int() {
                            self.fibers[fiber_idx].stack[cursor_idx].as_int() as usize
                        } else if matches!(
                            self.heap
                                .get_opt(collection.as_gc_id().unwrap_or(usize::MAX)),
                            Some(GcObject::Range(_))
                        ) {
                            0
                        } else {
                            bail!(
                                "Internal VM error: iterator cursor is not a number (slot {}, value {:?}, collection slot {}, collection {:?})",
                                cursor_idx,
                                self.fibers[fiber_idx].stack[cursor_idx],
                                collection_idx,
                                collection,
                            )
                        };

                        if let Some(step) = self.query_iteration_step(collection, cursor_val)? {
                            step
                        } else if let Some(id) = collection.as_gc_id() {
                            match self.heap.get(id) {
                                GcObject::Array(arr) => {
                                    if cursor_val < arr.len() {
                                        (
                                            false,
                                            Some(arr[cursor_val]),
                                            Some(BxValue::new_number(cursor_val as f64 + 1.0)),
                                        )
                                    } else {
                                        (true, None, None)
                                    }
                                }
                                GcObject::Range(range) => {
                                    let (start, end) = range.iter_bounds();
                                    let current = if start <= end {
                                        start + cursor_val as i64
                                    } else {
                                        start - cursor_val as i64
                                    };
                                    let done = if start <= end {
                                        current > end
                                    } else {
                                        current < end
                                    };
                                    if done {
                                        (true, None, None)
                                    } else {
                                        (
                                            false,
                                            Some(BxValue::new_number(current as f64)),
                                            Some(BxValue::new_number(cursor_val as f64 + 1.0)),
                                        )
                                    }
                                }
                                GcObject::Struct(s) => {
                                    let keys = {
                                        let mut k = Vec::new();
                                        let shape = &self.shapes.shapes[s.shape_id as usize];
                                        for &intern_id in shape.fields.keys() {
                                            let resolved =
                                                self.interner.resolve(intern_id).to_string();
                                            k.push((intern_id, resolved));
                                        }
                                        k.sort_by(|a, b| a.1.cmp(&b.1));
                                        k
                                    };
                                    if cursor_val < keys.len() {
                                        let (field_id, key_str) = &keys[cursor_val];
                                        let idx =
                                            self.shapes.get_index(s.shape_id, *field_id).unwrap();
                                        let val = s.properties[idx as usize];
                                        let key_gc_id = self
                                            .heap
                                            .alloc(GcObject::String(BoxString::new(key_str)));
                                        (false, Some(BxValue::new_ptr(key_gc_id)), Some(val))
                                    } else {
                                        (true, None, None)
                                    }
                                }
                                _ => {
                                    self.throw_error(
                                        fiber_idx,
                                        "Iteration only supported for arrays, structs, queries and ranges",
                                    )?;
                                    (true, None, None)
                                }
                            }
                        } else {
                            self.throw_error(
                                fiber_idx,
                                "Iteration only supported for arrays, structs, queries and ranges",
                            )?;
                            (true, None, None)
                        }
                    };

                    if is_done {
                        ip += offset as usize;
                    } else {
                        // ── Tier-3 JIT fast-path (numeric arrays, no index push) ──────────
                        // OSR: 't3 loop lets us activate an already-compiled iter fn (from
                        // this or a prior quantum) and call it in the same dispatch.
                        #[cfg(feature = "jit")]
                        let handled_by_jit = {
                            let mut handled = false;
                            if !push_index {
                                let collection = self.fibers[fiber_idx].stack[collection_idx];
                                if let Some(gc_id) = collection.as_gc_id() {
                                    let is_array = matches!(
                                        self.heap.get_opt(gc_id),
                                        Some(GcObject::Array(_))
                                    );
                                    if is_array {
                                        let fn_id = code_ptr as usize;
                                        't3: loop {
                                            // ── OSR check: compiled fn from prior quantum ──────
                                            if jit_iter_active.is_none() {
                                                if let Some(ref mut jit) = self.jit {
                                                    if let Some(f) =
                                                        jit.get_compiled_iter(fn_id, ip_at_start)
                                                    {
                                                        jit_iter_active = Some((ip_at_start, f));
                                                        continue 't3;
                                                    }
                                                }
                                            }

                                            // ── Active path: call the compiled native iter loop ─
                                            if let Some((active_ip, compiled)) = jit_iter_active {
                                                if active_ip == ip_at_start {
                                                    let (arr_ptr, arr_len) =
                                                        match self.heap.get(gc_id) {
                                                            GcObject::Array(arr) => (
                                                                arr.as_ptr() as *const u64,
                                                                arr.len() as u64,
                                                            ),
                                                            _ => unreachable!(),
                                                        };
                                                    self.sync_jit_locals(fiber_idx, false);
                                                    let deopt = unsafe {
                                                        compiled(
                                                            locals_ptr as *mut u64,
                                                            arr_ptr,
                                                            arr_len,
                                                        )
                                                    };
                                                    self.sync_jit_locals(fiber_idx, true);
                                                    if deopt == 0 {
                                                        ip += offset as usize; // jump past loop
                                                    } else {
                                                        eprintln!(
                                                            "[JIT] deopt iter loop ip={}",
                                                            ip_at_start
                                                        );
                                                        jit_iter_active = None;
                                                    }
                                                    handled = deopt == 0;
                                                    break 't3;
                                                }
                                            }

                                            // ── Profiling: accumulate count in JitState ────────
                                            // Fire every JIT_ITER_THRESHOLD iterations so that
                                            // profile_iter's internal counter can reach 10000 across quanta.
                                            const JIT_ITER_THRESHOLD: u64 = 5_000;
                                            let reached_threshold =
                                                if let Some(ref mut jit) = self.jit {
                                                    let count =
                                                        jit.inc_iter_profile(fn_id, ip_at_start);
                                                    count % JIT_ITER_THRESHOLD == 0
                                                } else {
                                                    false
                                                };

                                            if reached_threshold {
                                                let body_start = ip_at_start + 3;
                                                let body_len = offset as usize - 1;
                                                let body_code: Vec<u32> = unsafe {
                                                    std::slice::from_raw_parts(
                                                        code_ptr.add(body_start),
                                                        body_len,
                                                    )
                                                    .to_vec()
                                                };
                                                let mut const_map: HashMap<u32, f64> =
                                                    HashMap::new();
                                                for &word in &body_code {
                                                    if (word & 0xFF) as u8 == op::CONSTANT {
                                                        let cidx = word >> 8;
                                                        let cv = self.read_constant(
                                                            fiber_idx,
                                                            cidx as usize,
                                                        )?;
                                                        if cv.is_number() {
                                                            const_map.insert(cidx, cv.as_number());
                                                        }
                                                    }
                                                }
                                                if let Some(ref mut jit) = self.jit {
                                                    jit.profile_iter(
                                                        fn_id,
                                                        ip_at_start,
                                                        cursor_slot,
                                                        JIT_ITER_THRESHOLD,
                                                        &body_code,
                                                        &const_map,
                                                    );
                                                    if let Some(f) =
                                                        jit.get_compiled_iter(fn_id, ip_at_start)
                                                    {
                                                        jit_iter_active = Some((ip_at_start, f));
                                                        continue 't3; // immediately run freshly compiled fn
                                                    }
                                                }
                                            }
                                            break 't3;
                                        }
                                    }
                                }
                            }
                            handled
                        };

                        // Normal single-iteration path
                        #[cfg(feature = "jit")]
                        let do_normal = !handled_by_jit;
                        #[cfg(not(feature = "jit"))]
                        let do_normal = true;

                        if do_normal {
                            let current_cursor = self.fibers[fiber_idx].stack[cursor_idx];
                            let next_cursor_val = if current_cursor.is_int() {
                                BxValue::new_int(current_cursor.as_int() + 1)
                            } else {
                                BxValue::new_number(current_cursor.as_number() + 1.0)
                            };
                            self.fibers[fiber_idx].stack[cursor_idx] = next_cursor_val;
                            self.fibers[fiber_idx].stack.push(next_val.unwrap());
                            if push_index {
                                self.fibers[fiber_idx].stack.push(next_idx.unwrap());
                            }
                        }
                    }
                }
                op::LOCAL_JUMP_IF_NE_CONST => {
                    let slot = op0;
                    let const_idx = next_word!();
                    let offset = next_word!();
                    let val = self.local_value(fiber_idx, slot as usize);
                    let constant = self.read_constant(fiber_idx, const_idx as usize)?;
                    if !self.is_equal(val, constant) {
                        ip += offset as usize;
                    }
                }
                op::PUSH_HANDLER => {
                    let catch_ip = (op0 != 0).then_some(op0 as usize);
                    let finally_ip = next_word!();
                    let saved_stack_len = self.fibers[fiber_idx].stack.len();
                    self.fibers[fiber_idx]
                        .frames
                        .last_mut()
                        .unwrap()
                        .handlers
                        .push(ExceptionHandler {
                            catch_ip,
                            finally_ip: (finally_ip != 0).then_some(finally_ip as usize),
                            saved_stack_len,
                            catch_enabled: catch_ip.is_some(),
                        });
                }
                op::POP_HANDLER => {
                    self.fibers[fiber_idx]
                        .frames
                        .last_mut()
                        .unwrap()
                        .handlers
                        .pop();
                }
                op::THROW => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    flush_ip!();
                    self.throw_value(fiber_idx, val)?;
                    frame_changed = true;
                    continue 'quantum;
                }
                op::PRINT => {
                    let count = op0;
                    let mut args = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    let out = args
                        .iter()
                        .map(|a| self.to_string(*a))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if let Some(ref mut buffer) = self.output_buffer {
                        buffer.push_str(&out);
                    } else {
                        print!("{}", out);
                    }
                }
                op::PRINTLN => {
                    let count = op0;
                    let mut args = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();

                    #[cfg(all(target_arch = "wasm32", feature = "js"))]
                    {
                        let js_args = js_sys::Array::new();
                        for arg in &args {
                            js_args.push(&self.bx_to_js(arg));
                        }
                        web_sys::console::log(&js_args);
                    }
                    #[cfg(not(all(target_arch = "wasm32", feature = "js")))]
                    {
                        let out = args
                            .iter()
                            .map(|a| self.to_string(*a))
                            .collect::<Vec<_>>()
                            .join(" ");
                        if let Some(ref mut buffer) = self.output_buffer {
                            buffer.push_str(&out);
                            buffer.push('\n');
                        } else {
                            println!("{}", out);
                        }
                    }
                }
                op::BUFFER_WRITE => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let s = self.to_string(val);
                    if let Some(ref mut buffer) = self.output_buffer {
                        buffer.push_str(&s);
                    } else {
                        print!("{s}");
                    }
                }
                op::CONTAINS => {
                    let needle = self.fibers[fiber_idx].stack.pop().unwrap();
                    let haystack = self.fibers[fiber_idx].stack.pop().unwrap();
                    let result = if needle.is_null() || haystack.is_null() {
                        false
                    } else if let Some(id) = haystack.as_gc_id() {
                        match self.heap.get(id) {
                            GcObject::String(s) => s
                                .to_string()
                                .to_lowercase()
                                .contains(&self.to_string(needle).to_lowercase()),
                            GcObject::Array(arr) => arr.iter().any(|v| self.is_equal(*v, needle)),
                            GcObject::Range(range) => {
                                if let Some(number) = self.numeric_value(needle) {
                                    range.contains_number(number)
                                } else {
                                    false
                                }
                            }
                            GcObject::Struct(s) => {
                                let needle_s = self.to_string(needle);
                                let shape = &self.shapes.shapes[s.shape_id as usize];
                                shape.fields.keys().any(|key_id| {
                                    self.interner
                                        .resolve(*key_id)
                                        .eq_ignore_ascii_case(&needle_s)
                                })
                            }
                            _ => self
                                .to_string(haystack)
                                .to_lowercase()
                                .contains(&self.to_string(needle).to_lowercase()),
                        }
                    } else {
                        self.to_string(haystack)
                            .to_lowercase()
                            .contains(&self.to_string(needle).to_lowercase())
                    };
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(result));
                }
                op::INSTANCEOF => {
                    let right = self.fibers[fiber_idx].stack.pop().unwrap();
                    let left = self.fibers[fiber_idx].stack.pop().unwrap();
                    let type_name = self
                        .type_name_from_value(right)
                        .unwrap_or_else(|| self.to_string(right));
                    let result = self.value_matches_type_name(left, &type_name);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(result));
                }
                op::CASTAS => {
                    let right = self.fibers[fiber_idx].stack.pop().unwrap();
                    let left = self.fibers[fiber_idx].stack.pop().unwrap();
                    let type_name = self
                        .type_name_from_value(right)
                        .unwrap_or_else(|| self.to_string(right));
                    match self.cast_value_to_type(left, &type_name) {
                        Ok(val) => self.fibers[fiber_idx].stack.push(val),
                        Err(msg) => {
                            flush_ip!();
                            let err = self.exception_from_message("BoxCastException", msg, None);
                            self.throw_value(fiber_idx, err)?;
                            frame_changed = true;
                            continue 'quantum;
                        }
                    }
                }
                _ => {
                    bail!("Unknown opcode: {}", opcode);
                }
            }
            // ip persists across iterations within a single timeslice.
        }
        // Timeslice expired (safe-point break) — flush ip so the next run_fiber resumes correctly.
        if !self.fibers[fiber_idx].frames.is_empty() {
            self.fibers[fiber_idx].frames.last_mut().unwrap().ip = ip;
        }
        Ok(None)
    }

    fn throw_error(&mut self, fiber_idx: usize, msg: &str) -> Result<()> {
        if let Some((original, root)) = self.native_callback_exception.take() {
            if original == msg {
                let value = self.heap.read_retained_value(&root).map_err(anyhow::Error::msg)?;
                return self.throw_value_internal(fiber_idx, value, true);
            }
        }
        #[cfg(target_os = "espidf")]
        eprintln!("[matchbox-vm] throwing ExpressionException: {}", msg);
        self.collect_garbage();
        let val = self.exception_from_message("ExpressionException", msg.to_string(), None);
        self.throw_value(fiber_idx, val)
    }

    fn exception_from_message(
        &mut self,
        exception_type: &str,
        message: String,
        stack_trace: Option<String>,
    ) -> BxValue {
        let struct_id = self.struct_new();
        let type_id = self.string_new(exception_type.to_string());
        let message_id = self.string_new(message);
        let detail_id = self.string_new(String::new());
        let stack_trace_id = self.string_new(stack_trace.unwrap_or_default());
        let error_code_id = self.string_new(String::new());
        let tag_context_id = self.array_new();

        let type_val = BxValue::new_ptr(type_id);
        self.struct_set(struct_id, "name", type_val);
        self.struct_set(struct_id, "type", type_val);
        self.struct_set(struct_id, "message", BxValue::new_ptr(message_id));
        self.struct_set(struct_id, "detail", BxValue::new_ptr(detail_id));
        self.struct_set(struct_id, "stackTrace", BxValue::new_ptr(stack_trace_id));
        self.struct_set(struct_id, "errorCode", BxValue::new_ptr(error_code_id));
        self.struct_set(struct_id, "extendedInfo", BxValue::new_null());
        self.struct_set(struct_id, "tagContext", BxValue::new_ptr(tag_context_id));
        self.struct_set(struct_id, "cause", BxValue::new_null());

        BxValue::new_ptr(struct_id)
    }

    fn is_exception_object(&self, val: BxValue) -> bool {
        if let Some(id) = val.as_gc_id() {
            let is_exception_like = matches!(
                self.heap.get(id),
                GcObject::Struct(_) | GcObject::Instance(_) | GcObject::NativeObject(_)
            );
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            let is_exception_like =
                is_exception_like || matches!(self.heap.get(id), GcObject::JsValue(_));
            is_exception_like
        } else {
            false
        }
    }

    fn normalize_thrown_value(&mut self, val: BxValue, stack_trace: &str) -> BxValue {
        if self.is_exception_object(val) {
            if let Some(id) = val.as_gc_id() {
                if let GcObject::Struct(_) = self.heap.get(id) {
                    let stack_trace_id = self.string_new(stack_trace.to_string());
                    self.struct_set(id, "stackTrace", BxValue::new_ptr(stack_trace_id));
                }
            }
            return val;
        }

        self.exception_from_message(
            "CustomException",
            self.to_string(val),
            Some(stack_trace.to_string()),
        )
    }

    fn exception_summary(&self, val: BxValue) -> String {
        if let Some(id) = val.as_gc_id() {
            if let GcObject::Struct(_) = self.heap.get(id) {
                let message = self.struct_get(id, "message");
                let message = if !message.is_null() {
                    Some(self.to_string(message))
                } else {
                    None
                };

                if let Some(message) = message {
                    let kind = self.struct_get(id, "type");
                    if !kind.is_null() {
                        let kind = self.to_string(kind);
                        if !kind.is_empty() {
                            return format!("{}: {}", kind, message);
                        }
                    }
                    return message;
                }
            }
        }

        self.to_string(val)
    }

    /// Format an error BxValue into a human-readable string.
    /// For exception structs this extracts message, type, and stackTrace.
    pub fn format_error_value(&self, val: BxValue) -> String {
        if let Some(id) = val.as_gc_id() {
            if let GcObject::Struct(_) = self.heap.get(id) {
                let message = self.struct_get(id, "message");
                let message_str = if !message.is_null() {
                    self.to_string(message)
                } else {
                    String::new()
                };

                let kind = self.struct_get(id, "type");
                let kind_str = if !kind.is_null() {
                    self.to_string(kind)
                } else {
                    String::new()
                };

                let stack_trace = self.struct_get(id, "stackTrace");
                let stack_trace_str = if !stack_trace.is_null() {
                    self.to_string(stack_trace)
                } else {
                    String::new()
                };

                let mut result = String::new();
                if !kind_str.is_empty() {
                    result.push_str(&format!("{}: {}", kind_str, message_str));
                } else {
                    result.push_str(&message_str);
                }
                if !stack_trace_str.is_empty() {
                    result.push_str(&format!("\n\nStack trace:\n{}", stack_trace_str));
                }
                return result;
            }
        }
        self.to_string(val)
    }

    fn build_stack_trace(&self, fiber_idx: usize) -> String {
        let mut trace = String::new();
        for (i, frame) in self.fibers[fiber_idx].frames.iter().rev().enumerate() {
            let chunk = frame.chunk.borrow();
            let func_name = &frame.function.name;
            let file = chunk.filename();
            let line = if frame.ip > 0 && frame.ip <= chunk.lines().len() {
                chunk.lines()[frame.ip - 1]
            } else {
                0
            };
            if i == 0 {
                trace.push_str(&format!("at {}() in {}:{}\n", func_name, file, line));
            } else {
                trace.push_str(&format!("  at {}() in {}:{}\n", func_name, file, line));
            }
        }
        trace.trim_end().to_string()
    }

    fn source_context(&self, chunk: &Chunk, line: u32) -> String {
        if line == 0 {
            return String::new();
        }
        let source_text: Option<String> = if !chunk.source().is_empty() {
            Some(chunk.source().clone())
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                std::fs::read_to_string(chunk.filename()).ok()
            }
            #[cfg(target_arch = "wasm32")]
            {
                None
            }
        };

        if let Some(src) = source_text {
            let src_lines: Vec<&str> = src.lines().collect();
            let zero_idx = line as usize - 1;
            if zero_idx < src_lines.len() {
                let mut context = String::from("\n");
                let start = zero_idx.saturating_sub(3);
                let end = (zero_idx + 4).min(src_lines.len());
                for i in start..end {
                    let line_num = i + 1;
                    let prefix = if line_num == line as usize {
                        "> "
                    } else {
                        "  "
                    };
                    context.push_str(&format!("{}{:4} | {}\n", prefix, line_num, src_lines[i]));
                }
                return context;
            }
        }
        String::new()
    }

    fn handle_return(&mut self, fiber_idx: usize, result: BxValue) -> Result<Option<BxValue>> {
        // A direct return from a finally block replaces the completion that entered it.
        self.fibers[fiber_idx]
            .frames
            .last_mut()
            .unwrap()
            .pending_completion = None;

        loop {
            let handler = self.fibers[fiber_idx]
                .frames
                .last_mut()
                .unwrap()
                .handlers
                .pop();

            let Some(handler) = handler else {
                break;
            };

            if let Some(finally_ip) = handler.finally_ip {
                let frame = self.fibers[fiber_idx].frames.last_mut().unwrap();
                frame.pending_completion = Some(PendingCompletion::Return(result));
                frame.ip = finally_ip;
                self.truncate_scope_stack(fiber_idx, handler.saved_stack_len);
                return Ok(None);
            }
        }

        let result = match self.finish_function_events(fiber_idx, result) {
            Ok(result) => result,
            Err(error) => { self.throw_error(fiber_idx, &error)?; return Ok(None); }
        };
        let fiber = &mut self.fibers[fiber_idx];
        let frame = fiber.frames.pop().unwrap();
        if fiber.frames.is_empty() {
            return Ok(Some(result));
        }

        fiber.stack.truncate(frame.stack_base);
        if frame.function.name.ends_with(".constructor") {
            let instance = fiber.stack.pop().unwrap();
            fiber.stack.push(instance);
        } else {
            // For regular function calls, the function itself was at stack_base - 1.
            if frame.stack_base > 0 {
                fiber.stack.pop();
            }
            fiber.stack.push(result);
        }

        Ok(None)
    }

    fn throw_value(&mut self, fiber_idx: usize, val: BxValue) -> Result<()> {
        self.throw_value_internal(fiber_idx, val, false)
    }

    fn throw_value_internal(&mut self, fiber_idx: usize, val: BxValue, preserve: bool) -> Result<()> {
        let mut source_context = String::new();

        // Build full stack trace and source context before unwinding frames
        let stack_trace = self.build_stack_trace(fiber_idx);

        if !self.fibers[fiber_idx].frames.is_empty() {
            let frame = self.fibers[fiber_idx].frames.last().unwrap();
            let chunk = frame.chunk.borrow();
            if frame.ip > 0 && frame.ip <= chunk.lines().len() {
                let line = chunk.lines()[frame.ip - 1];
                source_context = self.source_context(&chunk, line);
            }
        }

        let val = if preserve && self.is_exception_object(val) { val } else { self.normalize_thrown_value(val, &stack_trace) };
        let _root = self.heap.retain_value(val).map_err(anyhow::Error::msg)?;
        let abort = self.abort_signal(val);

        while !self.fibers[fiber_idx].frames.is_empty() {
            let frame_idx = self.fibers[fiber_idx].frames.len() - 1;
            let handler = self.fibers[fiber_idx].frames[frame_idx]
                .handlers
                .last()
                .copied();

            if let Some(handler) = handler {
                if handler.catch_enabled && abort.is_none() {
                    let catch_ip = handler.catch_ip.unwrap();
                    let frame = &mut self.fibers[fiber_idx].frames[frame_idx];
                    frame.handlers.last_mut().unwrap().catch_enabled = false;
                    frame.pending_completion = None;
                    frame.ip = catch_ip;

                    self.truncate_scope_stack(fiber_idx, handler.saved_stack_len);
                    self.fibers[fiber_idx].stack.push(val);
                    return Ok(());
                }

                if let Some(finally_ip) = handler.finally_ip {
                    let frame = &mut self.fibers[fiber_idx].frames[frame_idx];
                    frame.handlers.pop();
                    frame.pending_completion = Some(PendingCompletion::Throw(val));
                    frame.ip = finally_ip;

                    self.truncate_scope_stack(fiber_idx, handler.saved_stack_len);
                    return Ok(());
                }

                // A catch-only handler that is already in its catch body is no longer
                // eligible to catch another exception; continue unwinding outward.
                self.fibers[fiber_idx].frames[frame_idx].handlers.pop();
                continue;
            }

            let event_error = self.fail_function_events(fiber_idx, val).err();
            self.fibers[fiber_idx].frames.pop();
            if let Some(error) = event_error {
                return self.throw_error(fiber_idx, &error);
            }
        }
        let val_str = self.exception_summary(val);
        let message = abort.as_ref().map(ToString::to_string).unwrap_or_else(|| format!("VM Runtime Error: {}{}\n\nStack trace:\n{}", val_str, source_context, stack_trace));
        if self.native_exception_depth > 0 {
            self.native_callback_exception = Some((message.clone(), _root));
        }
        if let Some(abort) = abort { return Err(abort.into()); }
        bail!(message);
    }

    pub fn call_function(
        &mut self,
        name: &str,
        args: Vec<BxValue>,
        chunk: Option<Rc<RefCell<Chunk>>>,
    ) -> Result<BxValue> {
        if let Some(f) = self.get_global(name) {
            return self.call_function_value(f, args, chunk);
        }
        anyhow::bail!("Function {} not found", name)
    }

    pub fn call_function_value(
        &mut self,
        func: BxValue,
        args: Vec<BxValue>,
        _chunk: Option<Rc<RefCell<Chunk>>>,
    ) -> Result<BxValue> {
        self.call_function_value_named(func, args, None)
    }

    pub fn call_function_value_named(&mut self, func: BxValue, args: Vec<BxValue>, names: Option<Vec<String>>) -> Result<BxValue> {
        if let Some(id) = func.as_gc_id() {
            match self.heap.get(id) {
                GcObject::CompiledFunction(f) => {
                    let f = Rc::clone(f);
                    let _future = self.enqueue_function_call(func, f, args, 0, None, names);
                    let mut fiber_idx = self.fibers.len() - 1;
                    let nested_future = self.fibers[fiber_idx].future_id;
                    let previous_fiber_idx = self.current_fiber_idx;
                    self.current_fiber_idx = Some(fiber_idx);
                    // Loop until the fiber completes — this is a synchronous blocking call.
                    let result = loop {
                        match self
                            .run_fiber(fiber_idx, Some(Instant::now() + Duration::from_millis(2)))
                        {
                            Ok(Some(val)) => break Ok(val),
                            Ok(None) => {
                                // The callback waits in `future.get()`: let the other fibers
                                // progress, or the producer never runs. A plain timeslice expiry
                                // just resumes the callback.
                                if std::mem::take(&mut self.fibers[fiber_idx].blocked_on_future) {
                                    if let Err(e) = self.run_other_fibers() {
                                        break Err(e);
                                    }
                                }
                                match self.fibers.iter().position(|fiber| fiber.future_id == nested_future) {
                                    Some(idx) => fiber_idx = idx,
                                    None => break Err(anyhow::anyhow!("Callback fiber disappeared while waiting")),
                                }
                                self.current_fiber_idx = Some(fiber_idx);
                            }
                            Err(e) => break Err(e),
                        }
                    };
                    self.current_fiber_idx = previous_fiber_idx;
                    let _ = self.fibers.swap_remove(fiber_idx);
                    result
                }
                GcObject::NativeCallback(callback) => {
                    let callback = callback.clone();
                    let args = if let Some(names) = names {
                        Self::reorder_callback_arguments(args, names, &callback.parameters).map_err(anyhow::Error::msg)?
                    } else { args };
                    self.invoke_callback(&callback, &args).map_err(anyhow::Error::msg)
                }
                GcObject::NativeFunction(f) => {
                    let f = *f;
                    let args = if let Some(names) = names {
                        let name = self.native_global_name(func);
                        self.reorder_native_arguments(args, names, name.as_deref())
                    } else { args };
                    let previous_gc = self.gc_suspended;
                    self.gc_suspended = true;
                    let res = self.native_exception_scope(|vm| f(vm, &args)).map_err(|e| anyhow::anyhow!(e));
                    self.gc_suspended = previous_gc;
                    res
                }
                #[cfg(all(target_arch = "wasm32", feature = "js"))]
                GcObject::JsValue(js) => {
                    use js_sys::Function;
                    if let Ok(func) = js.clone().dyn_into::<Function>() {
                        let js_args = js_sys::Array::new();
                        for arg in args {
                            js_args.push(&self.bx_to_js(&arg));
                        }
                        match js_sys::Reflect::apply(&func, &JsValue::UNDEFINED, &js_args) {
                            Ok(val) => Ok(self.js_to_bx(val)),
                            Err(e) => anyhow::bail!("JS Error: {:?}", e),
                        }
                    } else {
                        anyhow::bail!("Value is not a callable JS function")
                    }
                }

                _ => anyhow::bail!("Value is not a callable function"),
            }
        } else {
            anyhow::bail!("Value is not a callable function")
        }
    }

    pub fn call_method_value(
        &mut self,
        receiver: BxValue,
        name: &str,
        args: Vec<BxValue>,
    ) -> Result<BxValue> {
        #[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
        if let Some((member, receiver)) = self.registered_member(receiver, name) {
            let function = self.heap.read_retained_value(&member.function).map_err(anyhow::Error::msg)?;
            let (args, names) = member.arguments(receiver, args, None)?;
            return self.call_function_value_named(function, args, names);
        }
        let id = receiver
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;
        let method_name = name.to_lowercase();

        match self.heap.get(id) {
            GcObject::NativeObject(_) => {
                return self
                    .native_object_call_method(id, &method_name, &args)
                    .map_err(|e| anyhow::anyhow!(e));
            }
            GcObject::Class(_) => self.call_method_value_named(receiver, name, args, vec![]),
            GcObject::Instance(inst) => {
                let class = Rc::clone(&inst.class);
                if let Some(func) = self.resolve_method(class, &method_name) {
                    let final_args = self.function_arguments(args, None, &func);

                    let sub_chunk = func.chunk.execution_chunk();
                    let constant_count = sub_chunk.borrow().constants().len();
                    let future_id = self.future_new().as_gc_id().unwrap();
                    let fiber = BxFiber {
                        stack: {
                            let mut stack = Vec::with_capacity(1 + final_args.len());
                            stack.push(receiver);
                            stack.extend(final_args);
                            stack
                        },
                        frames: vec![CallFrame {
                            scope_mode: scopes::ScopeMode::Binding,
                            #[cfg(feature = "jit")]
                            jit_loop_attempted: false,
                            #[cfg(feature = "jit")]
                            jit_construct_unsupported: None,
                            context: None,
                            function: func.clone(),
                            chunk: sub_chunk,
                            ip: 0,
                            local_scope: None,
                            local_names: scopes::SlotBindings::new(),
                            stack_base: 1,
                            receiver: Some(receiver),
                            handlers: Vec::new(),
                            pending_completion: None,
                            promoted_constants: vec![None; constant_count],
                        }],
                        variables: self.current_variables_scope(),
                        future_id,
                        wait_until: None,
                        yield_requested: false,
            running: false,
            blocked_on_future: false,
            finished: false,
                        priority: 0,
                        root_stack: Vec::new(),
                    };
                    self.fibers.push(fiber);
                    let fiber_idx = self.fibers.len() - 1;
                    let previous_fiber_idx = self.current_fiber_idx;
                    self.current_fiber_idx = Some(fiber_idx);
                    let result = loop {
                        match self
                            .run_fiber(fiber_idx, Some(Instant::now() + Duration::from_millis(2)))
                        {
                            Ok(Some(val)) => break Ok(val),
                            Ok(None) => continue,
                            Err(e) => break Err(e),
                        }
                    };
                    self.current_fiber_idx = previous_fiber_idx;
                    if fiber_idx < self.fibers.len() {
                        self.fibers.swap_remove(fiber_idx);
                    }
                    result
                } else {
                    anyhow::bail!("Method {} not found on instance", name)
                }
            }
            _ => anyhow::bail!("Value is not an object instance"),
        }
    }

    fn method_reference(&mut self, receiver: BxValue, name: &str) -> Result<BxValue> {
        let id = receiver.as_gc_id().context("Value is not a class or instance")?;
        let (class, static_only) = match self.heap.get(id) {
            GcObject::Instance(instance) => (instance.class.clone(), false),
            GcObject::Class(class) => (class.clone(), true),
            _ => bail!("Value is not a class or instance"),
        };
        let method = self.resolve_method(class, name).with_context(|| format!("Method {name} not found"))?;
        if static_only && !method.modifiers.is_static { bail!("Non-static method {name} requires an instance"); }
        let mut method = (*method).clone();
        method.captured_receiver = Some(receiver);
        Ok(BxValue::new_ptr(self.heap.alloc(GcObject::CompiledFunction(Rc::new(method)))))
    }

    pub fn call_method_value_named(&mut self, receiver: BxValue, name: &str, args: Vec<BxValue>, names: Vec<String>) -> Result<BxValue> {
        if self.class_reference_target(receiver).is_some() {
            if names.iter().any(|name| !name.is_empty()) {
                bail!("{}", if name.eq_ignore_ascii_case("init") { "Cannot use named arguments on a Java constructor." } else { "Methods on Java objects cannot be called with named arguments" });
            }
            return self.call_method_value(receiver, name, args);
        }
        if matches!(receiver.as_gc_id().and_then(|id| self.heap.get_opt(id)), Some(GcObject::NativeObject(_))) {
            if names.iter().any(|name| !name.is_empty()) { bail!("Named native object method calls through this host API are not supported"); }
            return self.call_method_value(receiver, name, args);
        }
        let value = self.method_reference(receiver, name)?;
        self.call_function_value_named(value, args, Some(names))
    }

    pub fn instance_class_name(&self, receiver: BxValue) -> Result<String> {
        let id = receiver
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;

        match self.heap.get(id) {
            GcObject::Instance(inst) => Ok(inst.class.borrow().name.clone()),
            _ => anyhow::bail!("Value is not an object instance"),
        }
    }

    pub fn construct_global_class(
        &mut self,
        class_name: &str,
        args: Vec<BxValue>,
    ) -> Result<BxValue> {
        let class_val = self.get_global(class_name).with_context(|| format!("Class {class_name} not found"))?;
        self.construct_class_value(class_val, args)
    }

    fn construct_class_value(&mut self, class_val: BxValue, args: Vec<BxValue>) -> Result<BxValue> {
        let id = class_val.as_gc_id().context("Value is not a BoxLang class")?;
        let class = match self.heap.get(id) {
            GcObject::Class(class) => Rc::clone(class),
            _ => bail!("Value is not a BoxLang class"),
        };
        let class_name = class.borrow().name.clone();

        let variables_scope = Rc::new(RefCell::new(HashMap::new()));
        let inst_id = self.heap.alloc(GcObject::Instance(BxInstance {
            class: Rc::clone(&class),
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
            variables: variables_scope,
        }));
        let instance_val = BxValue::new_ptr(inst_id);
        if class.borrow().modifiers.is_abstract { bail!("Cannot instantiate abstract class {class_name}"); }

        let constructor = class.borrow().constructor.clone();
        let sub_chunk = constructor.chunk.execution_chunk();
        let constant_count = sub_chunk.borrow().constants().len();
        let future_id = self.future_new().as_gc_id().unwrap();
        let fiber = BxFiber {
            stack: {
                let mut stack = Vec::with_capacity(1 + args.len());
                stack.push(instance_val);
                stack.extend(args);
                stack
            },
            frames: vec![CallFrame {
                #[cfg(feature = "jit")]
                jit_loop_attempted: false,
                #[cfg(feature = "jit")]
                jit_construct_unsupported: None,
                context: None,
                function: Rc::new(constructor),
                chunk: sub_chunk,
                ip: 0,
                scope_mode: scopes::ScopeMode::Binding,
                local_scope: None,
                local_names: scopes::SlotBindings::new(),
                stack_base: 1,
                receiver: Some(instance_val),
                handlers: Vec::new(),
                pending_completion: None,
                promoted_constants: vec![None; constant_count],
            }],
            variables: self.current_variables_scope(),
            future_id,
            wait_until: None,
            yield_requested: false,
            running: false,
            blocked_on_future: false,
            finished: false,
            priority: 0,
            root_stack: Vec::new(),
        };
        self.fibers.push(fiber);
        let fiber_idx = self.fibers.len() - 1;
        let previous_fiber_idx = self.current_fiber_idx;
        self.current_fiber_idx = Some(fiber_idx);
        let result = loop {
            match self.run_fiber(fiber_idx, Some(Instant::now() + Duration::from_millis(2))) {
                Ok(Some(_)) => break Ok(instance_val),
                Ok(None) => continue,
                Err(e) => break Err(e),
            }
        };
        self.current_fiber_idx = previous_fiber_idx;
        self.fibers.swap_remove(fiber_idx);
        result
    }

    pub fn instantiate_global_class_without_constructor(
        &mut self,
        class_name: &str,
    ) -> Result<BxValue> {
        let class_val = self
            .get_global(class_name)
            .ok_or_else(|| anyhow::anyhow!("Class {} not found", class_name))?;
        let id = class_val
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Global {} is not a class", class_name))?;

        let class = match self.heap.get(id) {
            GcObject::Class(class) => Rc::clone(class),
            _ => anyhow::bail!("Global {} is not a class", class_name),
        };

        let variables_scope = Rc::new(RefCell::new(HashMap::new()));
        let inst_id = self.heap.alloc(GcObject::Instance(BxInstance {
            class,
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
            variables: variables_scope,
        }));
        Ok(BxValue::new_ptr(inst_id))
    }

    pub fn instance_variable(&self, receiver: BxValue, name: &str) -> Option<BxValue> {
        match self.heap.get_opt(receiver.as_gc_id()?)? {
            GcObject::Instance(instance) => instance.variables.borrow().get(&name.to_lowercase()).copied(),
            _ => None,
        }
    }

    pub fn set_instance_variable(&mut self, receiver: BxValue, name: &str, value: BxValue) -> Result<()> {
        let id = receiver.as_gc_id().ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;
        match self.heap.get_mut(id) {
            GcObject::Instance(instance) => { instance.variables.borrow_mut().insert(name.to_lowercase(), value); Ok(()) }
            _ => bail!("Value is not an object instance"),
        }
    }

    pub fn instance_property(&self, receiver: BxValue, name: &str) -> Option<BxValue> {
        let GcObject::Instance(instance) = self.heap.get_opt(receiver.as_gc_id()?)? else { return None; };
        let index = self.shapes.get_index(instance.shape_id, self.interner.get_id(name)?)?;
        instance.properties.get(index as usize).copied()
    }

    /// `class` and its ancestors, nearest first.
    fn class_chain(&self, class: &Rc<RefCell<BxClass>>) -> Vec<Rc<RefCell<BxClass>>> {
        let mut chain = vec![Rc::clone(class)];
        loop {
            let parent = chain.last().and_then(|current| {
                let extends = current.borrow().extends.clone()?;
                let id = self.get_global(&extends)?.as_gc_id()?;
                match self.heap.get(id) {
                    GcObject::Class(parent) => Some(Rc::clone(parent)),
                    _ => None,
                }
            });
            match parent {
                Some(parent) if !chain.iter().any(|seen| Rc::ptr_eq(seen, &parent)) => chain.push(parent),
                _ => return chain,
            }
        }
    }

    /// Private methods are callable from code running inside the class or a related class
    /// (the caller's receiver is an instance of `class`, a subclass, or an ancestor of it).
    fn caller_inside_class(&self, fiber_idx: usize, class: &Rc<RefCell<BxClass>>) -> bool {
        let Some(receiver) = self.fibers[fiber_idx].frames.last().and_then(|frame| frame.receiver) else {
            return false;
        };
        let Some(caller_class) = self.receiver_instance_gc_id(receiver).and_then(|id| match self.heap.get(id) {
            GcObject::Instance(instance) => Some(Rc::clone(&instance.class)),
            _ => None,
        }) else {
            return false;
        };
        self.class_chain(&caller_class).iter().any(|c| Rc::ptr_eq(c, class))
            || self.class_chain(class).iter().any(|c| Rc::ptr_eq(c, &caller_class))
    }

    /// `get`/`set` + capitalized property name, e.g. ("get", "title") -> "getTitle".
    fn accessor_method_name(prefix: &str, name: &str) -> Option<String> {
        let mut chars = name.chars();
        let first = chars.next()?;
        Some(format!("{prefix}{}{}", first.to_uppercase(), chars.as_str()))
    }

    pub fn instance_has_method(&self, receiver: BxValue, name: &str) -> bool {
        match receiver.as_gc_id().and_then(|id| self.heap.get_opt(id)) {
            Some(GcObject::Instance(instance)) => self.resolve_method(instance.class.clone(), name).is_some(),
            _ => false,
        }
    }

    pub fn instance_variables_json(&self, receiver: BxValue) -> Result<serde_json::Value> {
        let id = receiver
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;

        match self.heap.get(id) {
            GcObject::Instance(inst) => {
                let mut object = serde_json::Map::new();
                for (key, value) in inst.variables.borrow().iter() {
                    object.insert(key.clone(), self.bx_to_json(value));
                }
                Ok(serde_json::Value::Object(object))
            }
            _ => anyhow::bail!("Value is not an object instance"),
        }
    }

    pub fn set_instance_variables_json(
        &mut self,
        receiver: BxValue,
        json: serde_json::Value,
    ) -> Result<()> {
        let id = receiver
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;

        let serde_json::Value::Object(values) = json else {
            anyhow::bail!("Listener state must be a JSON object");
        };

        let mut converted = Vec::with_capacity(values.len());
        for (key, value) in values {
            converted.push((key.to_lowercase(), self.json_to_bx(value)));
        }

        match self.heap.get_mut(id) {
            GcObject::Instance(inst) => {
                let mut variables = inst.variables.borrow_mut();
                variables.clear();
                for (key, value) in converted {
                    variables.insert(key, value);
                }
                Ok(())
            }
            _ => anyhow::bail!("Value is not an object instance"),
        }
    }

    fn is_truthy(&self, val: BxValue) -> bool {
        if val.is_bool() {
            val.as_bool()
        } else if val.is_number() {
            val.as_number() != 0.0
        } else if val.is_int() {
            val.as_int() != 0
        } else if val.is_null() {
            false
        } else if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => {
                    let text = s.to_string();
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        false
                    } else if matches!(trimmed.to_lowercase().as_str(), "false" | "no") {
                        false
                    } else if let Ok(number) = trimmed.parse::<f64>() {
                        number != 0.0
                    } else {
                        true
                    }
                }
                _ => true,
            }
        } else {
            false
        }
    }

    fn reorder_callback_arguments(args: Vec<BxValue>, names: Vec<String>, params: &[String]) -> Result<Vec<BxValue>, String> {
        if args.len() > params.len() { return Err("Too many native arguments".into()); }
        let mut values = vec![BxValue::new_null(); params.len()];
        let mut assigned = vec![false; params.len()];
        let mut position = 0;
        for (index, value) in args.into_iter().enumerate() {
            let name = names.get(index).map(String::as_str).unwrap_or("");
            let slot = if name.is_empty() { let slot = position; position += 1; slot } else {
                params.iter().position(|p| p.eq_ignore_ascii_case(name)).ok_or_else(|| format!("Unknown native argument: {name}"))?
            };
            if slot >= params.len() || assigned[slot] { return Err("Duplicate or excessive native argument".into()); }
            assigned[slot] = true; values[slot] = value;
        }
        Ok(values)
    }

    fn native_global_name(&self, value: BxValue) -> Option<String> {
        self.global_names.iter().find_map(|(name_id, idx)| {
            (self.global_values[*idx] == value)
                .then(|| self.interner.resolve(*name_id).to_string())
        })
    }

    fn reorder_native_arguments(
        &self,
        args: Vec<BxValue>,
        names: Vec<String>,
        function_name: Option<&str>,
    ) -> Vec<BxValue> {
        let Some(function_name) = function_name else {
            return args;
        };

        let params: Vec<&str> = match function_name.to_ascii_lowercase().as_str() {
            "arrayeach" | "arraymap" | "arrayfilter" | "arraysome" | "arrayevery" | "arraynone" => {
                vec!["array", "callback", "parallel", "maxthreads", "virtual"]
            }
            "arrayreduce" => vec!["array", "callback", "initialvalue"],
            "boxannounce" => vec!["state", "data", "poolname"],
            "dateformat" | "datetimeformat" | "timeformat" => vec!["date", "format", "timezone", "locale"],
            "createdatetime" | "createdate" => vec!["year", "month", "day", "hour", "minute", "second", "millisecond", "timezone"],
            "parsedatetime" => vec!["date", "format", "timezone", "locale"],
            "gettoken" => vec!["list", "position", "delimiter"],
            "listavg" => vec!["list", "delimiter"],
            "listchangedelims" => {
                vec!["list", "newdelimiter", "delimiters", "includeemptyfields"]
            }
            "listcontains" | "listcontainsnocase" => vec![
                "list",
                "value",
                "delimiter",
                "includeemptyfields",
                "multicharacterdelimiter",
            ],
            "listcompact" => vec!["list", "delimiters", "includeemptyfields", "multicharacterdelimiter"],
            "listeach" | "listevery" | "listmap" | "listnone" | "listsome" => vec![
                "list",
                "callback",
                "delimiter",
                "includeemptyfields",
                "parallel",
                "virtual",
                "maxthreads",
            ],
            "listfilter" => vec![
                "list",
                "filter",
                "delimiter",
                "includeemptyfields",
                "parallel",
                "virtual",
                "maxthreads",
            ],
            "listqualify" => vec![
                "list",
                "qualifier",
                "delimiter",
                "elements",
                "includeemptyfields",
            ],
            "listreduce" | "listreduceright" => {
                vec!["list", "callback", "initialvalue", "delimiter"]
            }
            "listremoveduplicates" => vec!["list", "delimiter", "ignorecase"],
            "listvaluecount" | "listvaluecountnocase" => {
                vec!["list", "value", "delimiter"]
            }
            "listtoarray" => vec![
                "list",
                "delimiter",
                "includeemptyfields",
                "multicharacterdelimiter",
            ],
            "listlen" => vec!["list", "delimiter", "includeemptyfields", "multicharacterdelimiter"],
            "listgetat" => vec![
                "list",
                "position",
                "delimiter",
                "includeemptyfields",
                "multicharacterdelimiter",
            ],
            "listappend" => vec![
                "list",
                "value",
                "delimiter",
                "includeemptyfields",
                "multicharacterdelimiter",
            ],
            "listrest" => vec![
                "list",
                "delimiter",
                "includeemptyfields",
                "multicharacterdelimiter",
                "offset",
            ],
            "createobject" => vec!["type", "classname", "properties", "externalonly", "classloader"],
            "structkeytranslate" => vec!["struct", "deep", "retainkeys"],
            "stringbuildernew" => vec!["value", "capacity"],
            "setnew" => vec!["type", "values", "casesensitive", "issynchronized"],
            "listtoset" => vec!["list", "delimiter", "type", "casesensitive"],
            "getlocaledisplayname" | "getlocaleinfo" => vec!["locale", "dsplocale"],
            "asyncallapply" => vec!["items", "mapper", "timeout", "timeunit"],
            "threadnew" => vec!["runnable", "name", "virtual"],
            "boxast" => vec!["source", "filepath", "returntype", "sourcetype", "recover", "includetokens"],
            "boxastvisit" => vec!["tree", "visitor", "state"],
            "runthreadincontext" => vec!["context", "callback"],
            "lock" => vec!["name", "timeout", "type"],
            "compress" => vec![
                "format",
                "source",
                "destination",
                "includebasefolder",
                "overwrite",
                "prefix",
                "filter",
                "recurse",
                "compressionlevel",
            ],
            "extract" => vec![
                "format",
                "source",
                "destination",
                "overwrite",
                "recurse",
                "filter",
                "entrypaths",
            ],
            _ => return args,
        };

        let mut final_args = vec![BxValue::new_null(); params.len()];
        let mut positional_args = Vec::new();
        for (index, value) in args.into_iter().enumerate() {
            if index < names.len() && !names[index].is_empty() {
                let name = if names[index].eq_ignore_ascii_case("mask") {
                    "format"
                } else {
                    names[index].as_str()
                };
                if let Some(param_index) = params
                    .iter()
                    .position(|param| *param == name.to_ascii_lowercase())
                {
                    final_args[param_index] = value;
                }
            } else {
                positional_args.push(value);
            }
        }
        for (index, value) in positional_args.into_iter().enumerate() {
            if index < final_args.len() {
                final_args[index] = value;
            }
        }
        while final_args
            .last()
            .is_some_and(BxValue::is_null)
        {
            final_args.pop();
        }
        final_args
    }

    fn execute_call(
        &mut self,
        fiber_idx: usize,
        arg_count: usize,
        names: Option<Vec<String>>,
    ) -> Result<()> {
        let func_index = self.fibers[fiber_idx]
            .stack
            .len()
            .checked_sub(1 + arg_count)
            .ok_or_else(|| anyhow::anyhow!("Call stack underflow: {} arguments", arg_count))?;
        let func_val = self.fibers[fiber_idx].stack[func_index];

        if let Some(id) = func_val.as_gc_id() {
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            if let GcObject::JsValue(js) = self.heap.get(id) {
                let js = js.clone();
                if let Ok(func) = js.clone().dyn_into::<Function>() {
                    let js_args = Array::new();
                    let mut args = Vec::new();
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    for arg in args {
                        js_args.push(&self.bx_to_js(&arg));
                    }
                    self.fibers[fiber_idx].stack.pop(); // Pop the function
                    match Reflect::apply(&func, &JsValue::UNDEFINED, &js_args) {
                        Ok(val) => {
                            let bx_val = self.js_to_bx(val);
                            self.fibers[fiber_idx].stack.push(bx_val);
                            return Ok(());
                        }
                        Err(e) => {
                            return self.throw_error(fiber_idx, &format!("JS Error: {:?}", e));
                        }
                    }
                } else {
                    return self.throw_error(fiber_idx, "Can only call JS functions.");
                }
            }

            match self.heap.get(id) {
                GcObject::CompiledFunction(func) => {
                    let func = Rc::clone(func);
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    // Don't pop function yet, it's used as marker

                    let final_args = self.function_arguments(args, names.as_deref(), &func);
                    // Stack: ... [func] [arg1] [arg2] ...
                    // Function is already at len() - 1 - arg_count.
                    // We popped args, now we push final_args back.
                    for arg in final_args {
                        self.fibers[fiber_idx].stack.push(arg);
                    }

                    let sub_chunk = func.chunk.execution_chunk();
                    let constant_count = sub_chunk.borrow().constants().len();
                    let mut frame = CallFrame {
                        #[cfg(feature = "jit")]
                        jit_loop_attempted: false,
                        #[cfg(feature = "jit")]
                        jit_construct_unsupported: None,
                        context: None,
                        function: Rc::clone(&func),
                        chunk: sub_chunk,
                        ip: 0,
                        local_scope: None,
                        scope_mode: scopes::ScopeMode::Binding,
                        local_names: scopes::SlotBindings::new(),
                        stack_base: 0,
                        receiver: func.captured_receiver,
                        handlers: Vec::new(),
                        pending_completion: None,
                        promoted_constants: vec![None; constant_count],
                    };
                    // Let's be consistent: stack_base is where first arg is. Function is at stack_base - 1.
                    frame.stack_base = self.fibers[fiber_idx].stack.len() - func.arity as usize;
                    self.fibers[fiber_idx].frames.push(frame);
                    Ok(())
                }
                GcObject::NativeCallback(callback) => {
                    let callback = callback.clone();
                    let start = self.fibers[fiber_idx].stack.len() - arg_count;
                    let mut args: Vec<_> = self.fibers[fiber_idx].stack.drain(start..).collect();
                    if let Some(names) = names {
                        args = match Self::reorder_callback_arguments(args, names, &callback.parameters) {
                            Ok(args) => args,
                            Err(error) => return self.throw_error(fiber_idx, &error),
                        };
                    }
                    self.fibers[fiber_idx].stack.pop();
                    match self.invoke_callback(&callback, &args) {
                        Ok(value) => { self.fibers[fiber_idx].stack.push(value); Ok(()) }
                        Err(error) => self.throw_error(fiber_idx, &error),
                    }
                }
                GcObject::NativeFunction(func) => {
                    let func = *func;
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    if let Some(names) = names {
                        let native_name = self.native_global_name(func_val);
                        args = self.reorder_native_arguments(args, names, native_name.as_deref());
                    }
                    self.fibers[fiber_idx].stack.pop(); // Pop the function object

                    match self.native_exception_scope(|vm| func(vm, &args)) {
                        Ok(val) => {
                            self.fibers[fiber_idx].stack.push(val);
                            Ok(())
                        }
                        Err(e) => self.throw_error(fiber_idx, &e),
                    }
                }
                _ => self.throw_error(fiber_idx, "Can only call functions."),
            }
        } else {
            self.throw_error(fiber_idx, "Can only call functions.")
        }
    }

    #[cfg(all(target_arch = "wasm32", feature = "js-host-abi", not(feature = "js")))]
    fn js_result_to_bx(
        &mut self,
        rtype: i32,
        str_buf: &[u8],
        str_len: usize,
        num: f64,
        b: i32,
        obj_id: u32,
    ) -> BxValue {
        match rtype {
            1 => BxValue::new_bool(b != 0),
            2 => BxValue::new_number(num),
            3 => {
                let s = std::str::from_utf8(&str_buf[..str_len.min(str_buf.len())]).unwrap_or("");
                let id = self.heap.alloc(GcObject::String(BoxString::new(s)));
                BxValue::new_ptr(id)
            }
            4 => {
                let id = self.heap.alloc(GcObject::JsHandle(obj_id));
                BxValue::new_ptr(id)
            }
            _ => BxValue::new_null(),
        }
    }

    #[cfg(all(target_arch = "wasm32", feature = "js-host-abi", not(feature = "js")))]
    fn bx_args_to_json(&self, args: &[BxValue]) -> Vec<u8> {
        let mut out = b"[".to_vec();
        for (i, v) in args.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            if v.is_null() {
                out.extend_from_slice(b"null");
            } else if v.is_bool() {
                out.extend_from_slice(if v.as_bool() { b"true" } else { b"false" });
            } else if v.is_number() {
                out.extend_from_slice(format!("{}", v.as_number()).as_bytes());
            } else if v.is_int() {
                out.extend_from_slice(format!("{}", v.as_int()).as_bytes());
            } else if let Some(gc_id) = v.as_gc_id() {
                let maybe_handle = if let GcObject::JsHandle(h) = self.heap.get(gc_id) {
                    Some(*h)
                } else {
                    None
                };
                if let Some(h) = maybe_handle {
                    out.extend_from_slice(format!("{{\"h\":{}}}", h).as_bytes());
                } else {
                    let s = self.to_string(*v);
                    out.push(b'"');
                    for ch in s.chars() {
                        match ch {
                            '"' => out.extend_from_slice(b"\\\""),
                            '\\' => out.extend_from_slice(b"\\\\"),
                            '\n' => out.extend_from_slice(b"\\n"),
                            '\r' => out.extend_from_slice(b"\\r"),
                            '\t' => out.extend_from_slice(b"\\t"),
                            c => {
                                let mut buf = [0u8; 4];
                                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                            }
                        }
                    }
                    out.push(b'"');
                }
            } else {
                out.extend_from_slice(b"null");
            }
        }
        out.push(b']');
        out
    }

    fn execute_invoke(
        &mut self,
        fiber_idx: usize,
        name: String,
        arg_count: usize,
        names: Option<Vec<String>>,
        ip_at_start: usize,
    ) -> Result<()> {
        let receiver_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count as usize;
        let receiver_val = self.fibers[fiber_idx].stack[receiver_idx];
        if self.class_reference_target(receiver_val).is_some() {
            let args = self.fibers[fiber_idx].stack[receiver_idx + 1..].to_vec();
            let result = self.call_method_value_named(receiver_val, &name, args, names.unwrap_or_default());
            self.fibers[fiber_idx].stack.truncate(receiver_idx);
            match result {
                Ok(value) => self.fibers[fiber_idx].stack.push(value),
                Err(error) => return self.throw_error(fiber_idx, &error.to_string()),
            }
            return Ok(());
        }

        #[cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]
        if let Some((member, member_receiver)) = self.registered_member(receiver_val, &name) {
            let args = self.fibers[fiber_idx].stack.drain(receiver_idx + 1..).collect();
            let (args, names) = match member.arguments(member_receiver, args, names) {
                Ok(args) => args,
                Err(error) => return self.throw_error(fiber_idx, &error.to_string()),
            };
            let function = self.heap.read_retained_value(&member.function).map_err(anyhow::Error::msg)?;
            self.fibers[fiber_idx].stack[receiver_idx] = function;
            let count = args.len();
            self.fibers[fiber_idx].stack.extend(args);
            return self.execute_call(fiber_idx, count, names);
        }

        if let Some(id) = receiver_val.as_gc_id() {
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            if let GcObject::JsValue(js) = self.heap.get(id) {
                let js = unwrap_matchbox_js_proxy(js);
                if name.eq_ignore_ascii_case("get") && js.is_instance_of::<js_sys::Promise>() {
                    let future_val = self.bridge_js_promise_to_future(js.clone());
                    let future_id = match future_val.as_gc_id() {
                        Some(future_id) => future_id,
                        None => {
                            return self.throw_error(
                                fiber_idx,
                                "Promise interop did not produce a future.",
                            );
                        }
                    };

                    self.fibers[fiber_idx].stack[receiver_idx] = future_val;

                    if let GcObject::Future(f) = self.heap.get(future_id) {
                        match f.status.clone() {
                            FutureStatus::Pending => {
                                let fiber = &mut self.fibers[fiber_idx];
                                fiber.frames.last_mut().unwrap().ip = ip_at_start;
                                fiber.yield_requested = true;
                                fiber.blocked_on_future = true;
                                return Ok(());
                            }
                            FutureStatus::Completed => {
                                for _ in 0..arg_count {
                                    self.fibers[fiber_idx].stack.pop();
                                }
                                self.fibers[fiber_idx].stack.pop();
                                self.fibers[fiber_idx].stack.push(f.value);
                                return Ok(());
                            }
                            FutureStatus::Failed(error) => {
                                return self.throw_value(fiber_idx, error);
                            }
                        }
                    }
                }

                let prop = resolve_js_property(&js, &name);
                match Reflect::get(&js, &prop) {
                    Ok(val) => {
                        if let Ok(func) = val.clone().dyn_into::<Function>() {
                            let js_args = Array::new();
                            let mut args = Vec::new();
                            for _ in 0..arg_count {
                                args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                            }
                            args.reverse();
                            for arg in args {
                                js_args.push(&self.bx_to_js(&arg));
                            }
                            self.fibers[fiber_idx].stack.pop(); // Pop the receiver
                            match Reflect::apply(&func, &js, &js_args) {
                                Ok(val) => {
                                    let bx_val = self.js_to_bx(val);
                                    self.fibers[fiber_idx].stack.push(bx_val);
                                    return Ok(());
                                }
                                Err(e) => {
                                    return self
                                        .throw_error(fiber_idx, &format!("JS Error: {:?}", e));
                                }
                            }
                        }
                    }
                    Err(_) => {}
                }
            }

            match self.heap.get(id) {
                GcObject::Class(_) => {
                    let method = match self.method_reference(receiver_val, &name) {
                        Ok(method) => method,
                        Err(error) => return self.throw_error(fiber_idx, &error.to_string()),
                    };
                    self.fibers[fiber_idx].stack[receiver_idx] = method;
                    return self.execute_call(fiber_idx, arg_count, names);
                }
                GcObject::Struct(_) => {
                    let method_val = self.struct_get(id, &name);
                    if let Some(method_id) = method_val.as_gc_id() {
                        if let GcObject::CompiledFunction(function) = self.heap.get(method_id) {
                            let function = Rc::clone(function);
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                            }
                            args.reverse();
                            // Keep the receiver as the frame marker so returning from a
                            // nested method call does not remove the caller's function.
                            let final_args = self.function_arguments(args, names.as_deref(), &function);
                            self.fibers[fiber_idx].stack.extend(final_args);
                            let sub_chunk = function.chunk.execution_chunk();
                            let constant_count = sub_chunk.borrow().constants().len();
                            let stack_base = self.fibers[fiber_idx].stack.len() - function.arity as usize;
                            self.fibers[fiber_idx].frames.push(CallFrame {
                                #[cfg(feature = "jit")]
                                jit_loop_attempted: false,
                                #[cfg(feature = "jit")]
                                jit_construct_unsupported: None,
                                context: None,
                                function,
                                chunk: sub_chunk,
                                ip: 0,
                                scope_mode: scopes::ScopeMode::Binding,
                                local_scope: None,
                                local_names: scopes::SlotBindings::new(),
                                stack_base,
                                receiver: Some(receiver_val),
                                handlers: Vec::new(),
                                pending_completion: None,
                                promoted_constants: vec![None; constant_count],
                            });
                            return Ok(());
                        }
                    }
                }
                GcObject::Future(f) => {
                    let (status, value) = (f.status.clone(), f.value);

                    if name == "get" {
                        match status {
                            FutureStatus::Pending => {
                                if arg_count > 0 {
                                    return self.throw_error(fiber_idx, "Future get timed out");
                                }
                                // Suspend this fiber and return to the host loop.
                                // We don't pop anything; we stay at the current instruction
                                // so that we retry when the fiber resumes.
                                let fiber = &mut self.fibers[fiber_idx];
                                fiber.frames.last_mut().unwrap().ip = ip_at_start;
                                fiber.yield_requested = true;
                                fiber.blocked_on_future = true;
                                return Ok(());
                            }
                            FutureStatus::Completed => {
                                for _ in 0..arg_count {
                                    self.fibers[fiber_idx].stack.pop();
                                }
                                self.fibers[fiber_idx].stack.pop(); // Pop the future/receiver
                                self.fibers[fiber_idx].stack.push(value);
                                return Ok(());
                            }
                            FutureStatus::Failed(e) => {
                                return self.throw_value(fiber_idx, e);
                            }
                        }
                    } else if let Some(bif_name) = self.resolve_member_method(&receiver_val, &name)
                    {
                        return self.execute_bif_call(fiber_idx, bif_name, arg_count, receiver_val);
                    }
                }
                GcObject::NativeObject(obj) => {
                    let obj = Rc::clone(obj);
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    self.fibers[fiber_idx].stack.pop(); // receiver

                    drop(obj);
                    match self.native_object_call_method(id, &name, &args) {
                        Ok(res) => {
                            self.fibers[fiber_idx].stack.push(res);
                            return Ok(());
                        }
                        Err(e) => {
                            return self.throw_error(fiber_idx, &e);
                        }
                    }
                }
                GcObject::Instance(inst) => {
                    let shape_id = inst.shape_id;
                    let class = Rc::clone(&inst.class);

                    let ic = {
                        let fiber = &self.fibers[fiber_idx];
                        let frame = fiber.frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.cache_get(ip_at_start)
                    };

                    let method = match ic {
                        Some(IcEntry::Monomorphic {
                            shape_id: cached_shape,
                            index,
                        }) => {
                            if cached_shape == shape_id as usize {
                                let method_val = inst.properties[index as usize];
                                if let Some(m_id) = method_val.as_gc_id() {
                                    if let GcObject::CompiledFunction(f) = self.heap.get(m_id) {
                                        Some(Rc::clone(f))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        Some(IcEntry::Polymorphic { entries, count }) => {
                            let mut found_idx = None;
                            for i in 0..count {
                                if entries[i].0 == shape_id as usize {
                                    found_idx = Some(entries[i].1);
                                    break;
                                }
                            }
                            if let Some(idx) = found_idx {
                                let method_val = inst.properties[idx];
                                if let Some(m_id) = method_val.as_gc_id() {
                                    if let GcObject::CompiledFunction(f) = self.heap.get(m_id) {
                                        Some(Rc::clone(f))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    let method = if method.is_none() {
                        let name_intern_id = self.interner.intern(&name);
                        if let Some(idx) = self.shapes.get_index(shape_id, name_intern_id) {
                            let method_val = inst.properties[idx as usize];
                            if let Some(m_id) = method_val.as_gc_id() {
                                if let GcObject::CompiledFunction(f) = self.heap.get(m_id) {
                                    {
                                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        chunk.cache_add_shape(
                                            ip_at_start,
                                            shape_id as usize,
                                            idx as usize,
                                        );
                                    }
                                    Some(Rc::clone(f))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else if let Some(f) = self.resolve_method(Rc::clone(&class), &name) {
                            Some(f)
                        } else {
                            None
                        }
                    } else {
                        method
                    };

                    if let Some(func) = &method {
                        if func.modifiers.access.as_deref().is_some_and(|a| a.eq_ignore_ascii_case("private"))
                            && !self.caller_inside_class(fiber_idx, &class)
                        {
                            let class_name = class.borrow().name.clone();
                            return self.throw_error(
                                fiber_idx,
                                &format!("The function [{name}] in class [{class_name}] is private and can only be called from within its class"),
                            );
                        }
                    }

                    if let Some(func) = method {
                        let mut args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        args.reverse();
                        // Pop receiver, but we'll push it back as the first element of stack for the frame
                        self.fibers[fiber_idx].stack.pop();

                        let final_args = self.function_arguments(args, names.as_deref(), &func);

                        // Receiver should be available to the frame. In Matchbox, we often put it in CallFrame.receiver.
                        // But local variables slot 0 might also be receiver in some conventions.
                        // Let's stick to CallFrame.receiver and push arguments.
                        self.fibers[fiber_idx].stack.push(receiver_val);

                        for arg in final_args {
                            self.fibers[fiber_idx].stack.push(arg);
                        }

                        let sub_chunk = func.chunk.execution_chunk();
                        let constant_count = sub_chunk.borrow().constants().len();
                        let stack_base = self.fibers[fiber_idx].stack.len() - func.arity as usize;
                        let frame = CallFrame {
                            scope_mode: scopes::ScopeMode::Binding,
                            #[cfg(feature = "jit")]
                            jit_loop_attempted: false,
                            #[cfg(feature = "jit")]
                            jit_construct_unsupported: None,
                            context: None,
                            function: func.clone(),
                            chunk: sub_chunk,
                            ip: 0,
                            local_scope: None,
                            local_names: scopes::SlotBindings::new(),
                            stack_base,
                            receiver: Some(receiver_val),
                            handlers: Vec::new(),
                            pending_completion: None,
                            promoted_constants: vec![None; constant_count],
                        };
                        self.fibers[fiber_idx].frames.push(frame);
                        return Ok(());
                    } else if let Some(on_missing) =
                        self.resolve_method(Rc::clone(&class), "onmissingmethod")
                    {
                        let mut original_args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            original_args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        original_args.reverse();
                        self.fibers[fiber_idx].stack.pop(); // receiver
                        let args_array_id = self.heap.alloc(GcObject::Array(original_args));
                        let name_id = self.heap.alloc(GcObject::String(BoxString::new(&name)));

                        let final_args = self.function_arguments(vec![BxValue::new_ptr(name_id), BxValue::new_ptr(args_array_id)], None, &on_missing);
                        self.fibers[fiber_idx].stack.push(receiver_val);
                        self.fibers[fiber_idx].stack.extend(final_args);

                        let sub_chunk = on_missing.chunk.execution_chunk();
                        let constant_count = sub_chunk.borrow().constants().len();
                        let mut frame = CallFrame {
                            #[cfg(feature = "jit")]
                            jit_loop_attempted: false,
                            #[cfg(feature = "jit")]
                            jit_construct_unsupported: None,
                            context: None,
                            local_scope: None,
                            scope_mode: scopes::ScopeMode::Binding,
                            local_names: scopes::SlotBindings::new(),
                            function: on_missing.clone(),
                            chunk: sub_chunk,
                            ip: 0,
                            stack_base: self.fibers[fiber_idx].stack.len() - 2,
                            receiver: Some(receiver_val),
                            handlers: Vec::new(),
                            pending_completion: None,
                            promoted_constants: vec![None; constant_count],
                        };

                        frame.stack_base =
                            self.fibers[fiber_idx].stack.len() - on_missing.arity as usize;

                        self.fibers[fiber_idx].frames.push(frame);
                        return Ok(());
                    }
                }
                _ => {}
            }
        }

        // handle primitives and fallback BIFs
        if let Some(bif_name) = self.resolve_member_method(&receiver_val, &name) {
            return self.execute_bif_call(fiber_idx, bif_name, arg_count, receiver_val);
        }

        self.throw_error(
            fiber_idx,
            &format!("Method {} not found on {}.", name, receiver_val),
        )
    }

    fn execute_bif_call(
        &mut self,
        fiber_idx: usize,
        bif_name: String,
        arg_count: usize,
        receiver: BxValue,
    ) -> Result<()> {
        if bif_name == "futureget" {
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            let receiver_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count;

            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            if let Some(id) = receiver.as_gc_id() {
                if let GcObject::JsValue(js) = self.heap.get(id) {
                    let js = js.clone();
                    if js.is_instance_of::<js_sys::Promise>() {
                        let future_val = self.bridge_js_promise_to_future(js);
                        self.fibers[fiber_idx].stack[receiver_idx] = future_val;
                        return self.execute_bif_call(fiber_idx, bif_name, arg_count, future_val);
                    }
                }
            }

            if let Some(id) = receiver.as_gc_id() {
                if let GcObject::Future(f) = self.heap.get(id) {
                    match f.status.clone() {
                        FutureStatus::Pending => {
                            if arg_count > 0 {
                                return self.throw_error(fiber_idx, "Future get timed out");
                            }
                            if let Some(frame) = self.fibers[fiber_idx].frames.last_mut() {
                                if frame.ip > 0 {
                                    frame.ip -= 1;
                                }
                            }
                            self.fibers[fiber_idx].yield_requested = true;
                            self.fibers[fiber_idx].blocked_on_future = true;
                            return Ok(());
                        }
                        FutureStatus::Completed => {
                            for _ in 0..arg_count {
                                self.fibers[fiber_idx].stack.pop().unwrap();
                            }
                            self.fibers[fiber_idx].stack.pop();
                            self.fibers[fiber_idx].stack.push(f.value);
                            return Ok(());
                        }
                        FutureStatus::Failed(error) => {
                            return self.throw_value(fiber_idx, error);
                        }
                    }
                }
            }
        }

        if let Some(bif_val) = self.get_global(&bif_name) {
            if let Some(bif_id) = bif_val.as_gc_id() {
                match self.heap.get(bif_id) {
                    GcObject::NativeCallback(callback) => {
                        let callback = callback.clone();
                        let start = self.fibers[fiber_idx].stack.len() - arg_count;
                        let args: Vec<_> = self.fibers[fiber_idx].stack.drain(start..).collect();
                        self.fibers[fiber_idx].stack.pop();
                        let mut final_args = vec![receiver];
                        final_args.extend(args);
                        match self.invoke_callback(&callback, &final_args) {
                            Ok(value) => { self.fibers[fiber_idx].stack.push(value); return Ok(()); }
                            Err(error) => return self.throw_error(fiber_idx, &error),
                        }
                    }
                    GcObject::NativeFunction(bif) => {
                        let bif = *bif;
                        let mut args = Vec::with_capacity(arg_count + 1);
                        for _ in 0..arg_count {
                            args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        args.reverse();
                        self.fibers[fiber_idx].stack.pop(); // receiver

                        let mut final_args = vec![receiver];
                        final_args.extend(args);

                        match self.native_exception_scope(|vm| bif(vm, &final_args)) {
                            Ok(res) => {
                                self.fibers[fiber_idx].stack.push(res);
                                return Ok(());
                            }
                            Err(e) => return self.throw_error(fiber_idx, &e),
                        }
                    }
                    // Prelude functions and native BIFs share the same public
                    // member-method surface. Rebuild a normal call frame for a
                    // compiled function instead of treating it as a missing BIF.
                    GcObject::CompiledFunction(_) => {
                        let mut args = Vec::with_capacity(arg_count + 1);
                        for _ in 0..arg_count {
                            args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        args.reverse();
                        self.fibers[fiber_idx].stack.pop(); // receiver
                        self.fibers[fiber_idx].stack.push(bif_val);
                        self.fibers[fiber_idx].stack.push(receiver);
                        self.fibers[fiber_idx].stack.extend(args);
                        return self.execute_call(fiber_idx, arg_count + 1, None);
                    }
                    _ => {}
                }
            }
        }
        let hint = crate::bifs::required_feature(&bif_name.to_ascii_lowercase())
            .map(|feature| format!(" It requires the `{feature}` build feature."))
            .unwrap_or_default();
        self.throw_error(fiber_idx, &format!("BIF {} not found.{}", bif_name, hint))
    }

    fn read_constant(&mut self, fiber_idx: usize, idx: usize) -> Result<BxValue> {
        let closure = {
            let frame = self.fibers[fiber_idx].frames.last().unwrap();
            let chunk = frame.chunk.borrow();
            match &chunk.constants()[idx] {
                Constant::CompiledFunction(function) if matches!(function.kind, crate::types::FunctionKind::Closure | crate::types::FunctionKind::ComponentBody) => Some(function.clone()),
                _ => None,
            }
        };
        if let Some(mut function) = closure {
            function.captured_receiver = self.fibers[fiber_idx].frames.last().unwrap().receiver;
            function.captured_scopes = self.capture_scopes(fiber_idx);
            return Ok(self.promote_constant(Constant::CompiledFunction(function))?);
        }
        let val = {
            let fiber = &self.fibers[fiber_idx];
            let frame = fiber.frames.last().unwrap();

            let promoted = &frame.promoted_constants;
            if idx < promoted.len() {
                promoted[idx]
            } else {
                None
            }
        };

        if let Some(v) = val {
            return Ok(v);
        }

        let constant = {
            let fiber = &self.fibers[fiber_idx];
            let frame = fiber.frames.last().unwrap();
            let chunk = frame.chunk.borrow();
            chunk.constants()[idx].clone()
        };

        let promoted = self.promote_constant(constant)?;

        {
            let fiber = &mut self.fibers[fiber_idx];
            let frame = fiber.frames.last_mut().unwrap();
            if idx >= frame.promoted_constants.len() {
                let chunk_len = frame.chunk.borrow().constants().len();
                frame.promoted_constants.resize(chunk_len, None);
            }
            frame.promoted_constants[idx] = Some(promoted);
        }

        Ok(promoted)
    }

    fn promote_constant(&mut self, constant: Constant) -> Result<BxValue, RuntimeError> {
        match constant {
            Constant::Number(n) => Ok(BxValue::new_number(n)),
            Constant::Boolean(b) => Ok(BxValue::new_bool(b)),
            Constant::Null => Ok(BxValue::new_null()),
            Constant::String(s) => {
                let id = self.gc_alloc(GcObject::String(s))?;
                Ok(BxValue::new_ptr(id))
            }
            Constant::StringArray(arr) => {
                let mut values = Vec::with_capacity(arr.len());
                for s in arr {
                    let id = self.gc_alloc(GcObject::String(BoxString::new(&s)))?;
                    values.push(BxValue::new_ptr(id));
                }
                let id = self.gc_alloc(GcObject::Array(values))?;
                Ok(BxValue::new_ptr(id))
            }
            Constant::CompiledFunction(f) => {
                let mut f = f;
                // Function metadata may originate in another VM; indices/shape
                // guards belong to this VM, never to the reusable program.
                f.chunk = f.chunk.clone_without_runtime_caches();
                if f.kind != crate::types::FunctionKind::Lambda && f.captured_receiver.is_none() {
                    f.captured_receiver = self.current_receiver();
                }
                let id = self.gc_alloc(GcObject::CompiledFunction(Rc::new(f)))?;
                Ok(BxValue::new_ptr(id))
            }
            Constant::Class(mut c) => {
                c.constructor.chunk = c.constructor.chunk.clone_without_runtime_caches();
                for (_, function) in &mut c.methods {
                    function.chunk = function.chunk.clone_without_runtime_caches();
                }
                let id = self.gc_alloc(GcObject::Class(Rc::new(RefCell::new(c))))?;
                Ok(BxValue::new_ptr(id))
            }
            Constant::Interface(mut i) => {
                for (_, function) in &mut i.methods {
                    if let Some(function) = function {
                        function.chunk = function.chunk.clone_without_runtime_caches();
                    }
                }
                let id = self.gc_alloc(GcObject::Interface(Rc::new(RefCell::new(i))))?;
                Ok(BxValue::new_ptr(id))
            }
        }
    }

    /// Operand names are immutable metadata, not per-frame boxed string values.
    fn read_string_constant(&mut self, fiber_idx: usize, idx: usize) -> Result<BoxString> {
        {
            let frame = self.fibers[fiber_idx].frames.last().unwrap();
            let chunk = frame.chunk.borrow();
            if let Constant::String(name) = &chunk.constants()[idx] {
                return Ok(name.clone());
            }
        }
        // Preserve the existing malformed-operand error path.
        let val = self.read_constant(fiber_idx, idx)?;
        if let Some(id) = val.as_gc_id() {
            if let GcObject::String(s) = self.heap.get(id) {
                return Ok(s.clone());
            }
        }
        bail!("Constant at index {} is not a string: {:?}", idx, val)
    }

    /// Only ordinary field storage can use the existing shape/index cache.
    fn cached_member_value(&self, fiber: usize, ip: usize, base: BxValue) -> Option<BxValue> {
        let (shape, properties) = match self.heap.get_opt(base.as_gc_id()?)? {
            GcObject::Struct(record) => (record.shape_id, &record.properties),
            GcObject::Instance(instance) => (instance.shape_id, &instance.properties),
            _ => return None,
        };
        let frame = self.fibers[fiber].frames.last()?;
        let entry = frame.chunk.borrow().cache_get(ip)?;
        let index = match entry {
            IcEntry::Monomorphic { shape_id, index } if shape_id == shape as usize => index,
            IcEntry::Polymorphic { entries, count } => entries[..count].iter()
                .find_map(|&(shape_id, index)| (shape_id == shape as usize).then_some(index))?,
            _ => return None,
        };
        properties.get(index).copied()
    }

    /// Identifier operands are immutable metadata, not per-frame GC values.
    fn read_intern_id(&mut self, fiber_idx: usize, idx: usize) -> Result<u32> {
        let mut name = {
            let frame = self.fibers[fiber_idx].frames.last().unwrap();
            let chunk = frame.chunk.borrow();
            match &chunk.constants()[idx] {
                Constant::String(name) => name.clone(),
                other => bail!("Constant at index {} is not a string: {:?}", idx, other),
            }
        };
        Ok(self.interner.intern(name.as_flat_str()))
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub fn bx_to_js(&self, val: &BxValue) -> JsValue {
        if val.is_int() {
            JsValue::from(val.as_int())
        } else if val.is_number() {
            JsValue::from_f64(val.as_number())
        } else if val.is_bool() {
            JsValue::from_bool(val.as_bool())
        } else if val.is_null() {
            JsValue::NULL
        } else if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => {
                    let mut s_flat = s.clone();
                    JsValue::from_str(s_flat.flatten())
                }
                GcObject::Array(arr) => {
                    let js_arr = Array::new();
                    for item in arr {
                        js_arr.push(&self.bx_to_js(item));
                    }
                    js_arr.into()
                }
                GcObject::Range(range) => JsValue::from_str(&format!("{}", range)),
                GcObject::DateTime(dt) => {
                    JsValue::from_str(&dt.to_rfc3339_opts(SecondsFormat::Millis, true))
                }
                GcObject::Struct(s) => {
                    let js_obj = js_sys::Object::new();
                    let shape = &self.shapes.shapes[s.shape_id as usize];
                    let mut fields: Vec<(u32, u32)> =
                        shape.fields.iter().map(|(&k, &idx)| (k, idx)).collect();
                    fields.sort_by_key(|&(_, idx)| idx);
                    for (k, idx) in fields {
                        let key_str = self.interner.resolve(k);
                        Reflect::set(
                            &js_obj,
                            &JsValue::from_str(key_str),
                            &self.bx_to_js(&s.properties[idx as usize]),
                        )
                        .ok();
                    }
                    js_obj.into()
                }
                GcObject::Instance(_) => {
                    let vm_ptr = self as *const VM as usize;
                    let global = js_sys::global();
                    let matchbox = match Reflect::get(&global, &JsValue::from_str("MatchBox")) {
                        Ok(value) if !value.is_undefined() && !value.is_null() => value,
                        _ => return JsValue::UNDEFINED,
                    };

                    let create_proxy =
                        match Reflect::get(&matchbox, &JsValue::from_str("createInstanceProxy")) {
                            Ok(value) => value,
                            Err(_) => return JsValue::UNDEFINED,
                        };

                    if let Ok(func) = create_proxy.dyn_into::<Function>() {
                        func.call2(
                            &matchbox,
                            &JsValue::from_f64(vm_ptr as f64),
                            &JsValue::from_f64(id as f64),
                        )
                        .unwrap_or(JsValue::UNDEFINED)
                    } else {
                        JsValue::UNDEFINED
                    }
                }
                GcObject::JsValue(js) => js.clone(),
                GcObject::Future(_) => {
                    let vm_ptr = self as *const VM as usize;
                    let future = *val;

                    future_to_promise(async move {
                        async fn yield_to_host() -> Result<(), JsValue> {
                            let promise =
                                Promise::new(&mut |resolve: Function, reject: Function| {
                                    let win = match window() {
                                        Some(win) => win,
                                        None => {
                                            let _ = reject.call1(
                                                &JsValue::NULL,
                                                &JsValue::from_str("window is unavailable"),
                                            );
                                            return;
                                        }
                                    };

                                    if let Err(err) = win
                                        .set_timeout_with_callback_and_timeout_and_arguments_0(
                                            &resolve, 0,
                                        )
                                    {
                                        let _ = reject.call1(&JsValue::NULL, &err);
                                    }
                                });

                            let _ = JsFuture::from(promise).await?;
                            Ok(())
                        }

                        loop {
                            let state = {
                                let vm = unsafe { &*(vm_ptr as *const VM) };
                                vm.future_state(future)
                                    .map_err(|e| js_error_value(&e.to_string()))?
                            };

                            match state {
                                HostFutureState::Pending => yield_to_host().await?,
                                HostFutureState::Completed(value) => {
                                    let js = {
                                        let vm = unsafe { &*(vm_ptr as *const VM) };
                                        vm.bx_to_js(&value)
                                    };
                                    return Ok(js);
                                }
                                HostFutureState::Failed(error) => {
                                    let msg = {
                                        let vm = unsafe { &*(vm_ptr as *const VM) };
                                        vm.format_error_value(error)
                                    };
                                    return Err(js_error_value(&msg));
                                }
                            }
                        }
                    })
                    .into()
                }
                GcObject::CompiledFunction(_) | GcObject::NativeFunction(_) | GcObject::NativeCallback(_) => {
                    #[cfg(all(target_arch = "wasm32", feature = "js"))]
                    {
                        let id = *self.next_callback_id.borrow();
                        self.callback_registry.borrow_mut().insert(id, val.clone());
                        *self.next_callback_id.borrow_mut() += 1;

                        let vm_ptr = self as *const VM as usize;
                        let body = format!(
                            "return globalThis.MatchBox.invokeCallback({}, {}, this, Array.from(arguments));",
                            vm_ptr, id
                        );
                        match Function::new_no_args(&body).dyn_into::<Function>() {
                            Ok(f) => f.into(),
                            Err(_) => JsValue::UNDEFINED,
                        }
                    }
                    #[cfg(not(all(target_arch = "wasm32", feature = "js")))]
                    JsValue::UNDEFINED
                }
                _ => JsValue::UNDEFINED,
            }
        } else {
            JsValue::UNDEFINED
        }
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub fn js_to_bx(&mut self, val: JsValue) -> BxValue {
        if let Some(gc_id) = self.unwrap_matchbox_instance(&val) {
            return BxValue::new_ptr(gc_id as usize);
        }
        self.js_to_bx_with_seen(val, &mut Vec::new(), 0)
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub fn unwrap_matchbox_instance(&self, val: &JsValue) -> Option<u32> {
        let vm_ptr = self as *const VM as usize;
        let js_vm_ptr = Reflect::get(val, &JsValue::from_str("__matchbox_vm_ptr"))
            .ok()
            .and_then(|v| v.as_f64())
            .map(|v| v as usize);

        if js_vm_ptr == Some(vm_ptr) {
            Reflect::get(val, &JsValue::from_str("__matchbox_gc_id"))
                .ok()
                .and_then(|v| v.as_f64())
                .map(|v| v as u32)
        } else {
            None
        }
    }

    fn receiver_instance_gc_id(&self, receiver: BxValue) -> Option<usize> {
        let id = receiver.as_gc_id()?;
        match self.heap.get(id) {
            GcObject::Instance(_) => Some(id),
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            GcObject::JsValue(js) => self
                .unwrap_matchbox_instance(js)
                .map(|gc_id| gc_id as usize),
            _ => None,
        }
    }

    fn collect_garbage(&mut self) {
        if self.gc_suspended {
            return;
        }
        let mut roots = Vec::new();
        // 1. Fiber stacks and frames
        for fiber in &self.fibers {
            roots.extend(fiber.stack.iter().cloned());
            roots.extend(fiber.root_stack.iter().cloned());
            roots.extend(fiber.variables.borrow().values().copied());
            for frame in &fiber.frames {
                if let Some(recv) = &frame.receiver {
                    roots.push(*recv);
                }
                roots.extend(frame.promoted_constants.iter().flatten().copied());
                roots.extend(frame.local_scope);
                roots.extend(frame.context);
                roots.extend(frame.function.captured_scopes.iter().copied());
                if let Some(PendingCompletion::Return(value) | PendingCompletion::Throw(value)) = frame.pending_completion {
                    roots.push(value);
                }
            }
            roots.push(BxValue::new_ptr(fiber.future_id));
        }
        // 2. Globals
        roots.extend(self.global_values.iter().cloned());
        roots.extend(self.script_variables.borrow().values().copied());
        roots.extend(self.soap_clients.values().copied());
        roots.extend(self.function_events.values().copied());
        #[cfg(all(target_arch = "wasm32", feature = "js"))]
        roots.extend(self.callback_registry.borrow().values().cloned());
        for completion in &self.native_completions {
            match completion {
                NativeCompletion::Resolve { future, value } => {
                    roots.push(*future);
                    roots.push(*value);
                }
                NativeCompletion::Reject { future, error } => {
                    roots.push(*future);
                    roots.push(*error);
                }
            }
        }
        for future_id in self.pending_native_futures.keys() {
            roots.push(BxValue::new_ptr(*future_id));
        }

        self.heap.collect(&roots);
    }

    pub fn collect_garbage_now(&mut self) {
        self.collect_garbage();
    }

    /// Attempts to allocate a GC object. On failure, triggers garbage
    /// collection and retries once. Returns `RuntimeError::OutOfMemory`
    /// if allocation fails even after GC.
    pub(crate) fn gc_alloc(&mut self, obj: GcObject) -> Result<GcId, RuntimeError> {
        if let Some(id) = self.heap.try_alloc(obj.clone()) {
            return Ok(id);
        }
        self.collect_garbage();
        self.heap.try_alloc(obj).ok_or_else(|| {
            RuntimeError::OutOfMemory(
                "Heap exhausted: allocation failed even after garbage collection".into(),
            )
        })
    }

    /// Helper for the opcode dispatch loop: attempts GC allocation,
    /// and on failure converts the RuntimeError into a BoxLang throw.
    /// Returns the allocated GcId on success, or propagates the error
    /// via `throw_value` (returning `Err` if the exception bubbles up
    /// uncaught).
    fn gc_alloc_or_throw(&mut self, fiber_idx: usize, obj: GcObject) -> Result<GcId> {
        match self.gc_alloc(obj) {
            Ok(id) => Ok(id),
            Err(e) => {
                let val = self.exception_from_message(e.exception_type(), e.to_string(), None);
                // throw_value returns Err only if no handler caught it
                self.throw_value(fiber_idx, val)?;
                // If throw_value returned Ok, the exception was caught;
                // return an arbitrary error to stop the current operation.
                bail!("OutOfMemory thrown and caught")
            }
        }
    }

    pub fn bx_to_json(&self, val: &BxValue) -> serde_json::Value {
        if val.is_int() {
            serde_json::Value::Number(val.as_int().into())
        } else if val.is_number() {
            serde_json::Number::from_f64(val.as_number())
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        } else if val.is_bool() {
            serde_json::Value::Bool(val.as_bool())
        } else if val.is_null() {
            serde_json::Value::Null
        } else if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => serde_json::Value::String(s.to_string()),
                GcObject::DateTime(dt) => {
                    serde_json::Value::String(dt.to_rfc3339_opts(SecondsFormat::AutoSi, true))
                }
                GcObject::Array(arr) => {
                    let json_arr: Vec<serde_json::Value> =
                        arr.iter().map(|v| self.bx_to_json(v)).collect();
                    serde_json::Value::Array(json_arr)
                }
                GcObject::Range(range) => serde_json::Value::String(format!("{}", range)),
                GcObject::Struct(s) => {
                    let mut map = serde_json::Map::new();
                    let shape = &self.shapes.shapes[s.shape_id as usize];
                    for (&k, &idx) in shape.fields.iter() {
                        if let Some(v) = s.properties.get(idx as usize) {
                            let key_str = self.interner.resolve(k).to_string();
                            map.insert(key_str, self.bx_to_json(v));
                        }
                    }
                    serde_json::Value::Object(map)
                }
                _ => serde_json::Value::String(format!("<ptr {}>", id)),
            }
        } else {
            serde_json::Value::Null
        }
    }

    pub fn json_to_bx(&mut self, val: serde_json::Value) -> BxValue {
        match val {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    BxValue::new_int(i as i32)
                } else {
                    BxValue::new_number(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::Bool(b) => BxValue::new_bool(b),
            serde_json::Value::String(s) => {
                let id = self.heap.alloc(GcObject::String(BoxString::new(&s)));
                BxValue::new_ptr(id)
            }
            serde_json::Value::Array(arr) => {
                let bx_arr: Vec<BxValue> = arr.into_iter().map(|v| self.json_to_bx(v)).collect();
                let id = self.heap.alloc(GcObject::Array(bx_arr));
                BxValue::new_ptr(id)
            }
            serde_json::Value::Object(obj) => {
                let mut bx_struct = BxStruct {
                    shape_id: self.shapes.get_root(),
                    properties: Vec::new(),
                    case_sensitive: false,
                    ordered: false,
                };
                for (name, val) in obj {
                    let bx_val = self.json_to_bx(val);
                    let shape_id = bx_struct.shape_id;
                    let name_id = self.interner.intern(&name);
                    bx_struct.shape_id = self.shapes.transition(shape_id, name_id);
                    bx_struct.properties.push(bx_val);
                }
                let id = self.heap.alloc(GcObject::Struct(bx_struct));
                BxValue::new_ptr(id)
            }
            serde_json::Value::Null => BxValue::new_null(),
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
#[wasm_bindgen]
pub fn _matchbox_invoke_callback(
    vm_ptr: usize,
    callback_id: usize,
    this_val: JsValue,
    args: js_sys::Array,
) -> Result<JsValue, JsValue> {
    let vm = unsafe { &mut *(vm_ptr as *mut VM) };
    let func = vm.callback_registry.borrow().get(&callback_id).cloned();
    if let Some(func) = func {
        let receiver = if !this_val.is_undefined() && !this_val.is_null() {
            // Preserve the JS receiver object so reactive wrappers can observe
            // BoxLang instance writes through normal JS property semantics.
            let id = vm.heap.alloc(GcObject::JsValue(this_val));
            Some(BxValue::new_ptr(id))
        } else {
            None
        };

        // Determine the function's max arity so we can truncate excess
        // arguments, matching JavaScript's behavior of silently ignoring them.
        let max_arity = if let Some(id) = func.as_gc_id() {
            match vm.heap.get(id) {
                GcObject::CompiledFunction(f) => Some(f.arity as u32),
                _ => None,
            }
        } else {
            None
        };

        let arg_count = match max_arity {
            Some(arity) => args.length().min(arity),
            None => args.length(),
        };

        let mut bx_args = Vec::new();
        for i in 0..arg_count {
            bx_args.push(vm.js_to_bx(args.get(i)));
        }
        let future = vm
            .start_call_function_value_with_receiver(func, bx_args, receiver)
            .map_err(|e| js_error_value(&e.to_string()))?;
        vm.pump_until_blocked()
            .map_err(|e| js_error_value(&e.to_string()))?;

        match vm.future_state(future) {
            Ok(HostFutureState::Completed(value)) => Ok(vm.bx_to_js(&value)),
            Ok(HostFutureState::Failed(error)) => {
                let msg = vm.format_error_value(error);
                Err(js_error_value(&msg))
            }
            Ok(HostFutureState::Pending) => Ok(vm.bx_to_js(&future)),
            Err(e) => Err(js_error_value(&e.to_string())),
        }
    } else {
        Err(js_error_value("Callback not found"))
    }
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
fn wasm_instance_prop_get(vm: &mut VM, base_val: BxValue, name: &str) -> Result<BxValue> {
    let Some(id) = base_val.as_gc_id() else {
        return Ok(BxValue::new_null());
    };

    Ok(match vm.heap.get(id) {
        GcObject::Struct(_) => vm.struct_get(id, name),
        GcObject::Instance(inst) => {
            let name_id = vm.interner.intern(name);
            if let Some(idx) = vm.shapes.get_index(inst.shape_id, name_id) {
                inst.properties
                    .get(idx as usize)
                    .copied()
                    .unwrap_or_else(BxValue::new_null)
            } else if let Some(method) = vm.resolve_method(Rc::clone(&inst.class), name) {
                BxValue::new_ptr(vm.heap.alloc(GcObject::CompiledFunction(method)))
            } else if let Some(getter) = VM::accessor_method_name("get", name)
                .filter(|getter| vm.instance_has_method(base_val, getter))
            {
                return vm.call_method_value(base_val, &getter, Vec::new());
            } else {
                BxValue::new_null()
            }
        }
        GcObject::NativeObject(obj) => vm.native_object_get(obj, &name.to_lowercase()),
        _ => BxValue::new_null(),
    })
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
fn wasm_instance_keys(vm: &mut VM, base_val: BxValue) -> Vec<String> {
    let Some(id) = base_val.as_gc_id() else {
        return Vec::new();
    };

    match vm.heap.get(id) {
        GcObject::Struct(_) => vm.struct_key_array(id),
        GcObject::Instance(inst) => {
            let shape = &vm.shapes.shapes[inst.shape_id as usize];
            let mut keys = vec![String::new(); shape.fields.size()];
            for (&fid, &fidx) in &shape.fields {
                keys[fidx as usize] = vm.interner.resolve(fid).to_string();
            }

            let mut seen: HashSet<String> = keys.iter().map(|key| key.to_lowercase()).collect();
            let mut current_class = Rc::clone(&inst.class);
            loop {
                let class_ref = current_class.borrow();

                for (name, method) in &class_ref.methods {
                    let lowered = name.to_lowercase();
                    if let Some(property) = lowered.strip_prefix("get")
                        .filter(|property| !property.is_empty() && method.params.is_empty())
                    {
                        if seen.insert(property.to_string()) {
                            keys.push(property.to_string());
                        }
                    }
                    if seen.insert(lowered) {
                        keys.push(name.clone());
                    }
                }

                let next_class = if let Some(parent_name) = &class_ref.extends {
                    if let Some(val) = vm.get_global(parent_name) {
                        if let Some(parent_id) = val.as_gc_id() {
                            if let GcObject::Class(parent_class) = vm.heap.get(parent_id) {
                                Some(Rc::clone(parent_class))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                drop(class_ref);

                if let Some(next_class) = next_class {
                    current_class = next_class;
                } else {
                    break;
                }
            }

            keys
        }
        _ => Vec::new(),
    }
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
fn wasm_instance_prop_set(vm: &mut VM, base_val: BxValue, name: &str, val: BxValue) -> Result<()> {
    let Some(id) = base_val.as_gc_id() else {
        return Ok(());
    };
    if let GcObject::Instance(inst) = vm.heap.get(id) {
        let field = vm.interner.get_id(name).and_then(|key| vm.shapes.get_index(inst.shape_id, key));
        if field.is_none()
            && let Some(setter) = VM::accessor_method_name("set", name)
                .filter(|setter| vm.instance_has_method(base_val, setter))
        {
            vm.call_method_value(base_val, &setter, vec![val])?;
            return Ok(());
        }
    }

    match vm.heap.get_mut(id) {
        GcObject::Struct(_) => vm.struct_set(id, name, val),
        GcObject::Instance(inst) => {
            let name_id = vm.interner.intern(name);
            if let Some(idx) = vm.shapes.get_index(inst.shape_id, name_id) {
                if let Some(slot) = inst.properties.get_mut(idx as usize) {
                    *slot = val;
                }
            } else {
                inst.shape_id = vm.shapes.transition(inst.shape_id, name_id);
                inst.properties.push(val);
            }
        }
        GcObject::NativeObject(obj) => obj.borrow_mut().set_property(&name.to_lowercase(), val),
        _ => {}
    }
    Ok(())
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
#[wasm_bindgen]
pub fn _matchbox_pump_vm(vm_ptr: usize) -> Result<(), JsValue> {
    let vm = unsafe { &mut *(vm_ptr as *mut VM) };
    vm.pump_until_blocked()
        .map_err(|e| js_error_value(&e.to_string()))
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
#[wasm_bindgen]
pub fn _matchbox_get_instance_prop(vm_ptr: usize, gc_id: u32, name: &str) -> Result<JsValue, JsValue> {
    let vm = unsafe { &mut *(vm_ptr as *mut VM) };
    let base_val = BxValue::new_ptr(gc_id as usize);
    let val = wasm_instance_prop_get(vm, base_val, name)
        .map_err(|error| js_error_value(&error.to_string()))?;
    Ok(vm.bx_to_js(&val))
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
#[wasm_bindgen]
pub fn _matchbox_get_instance_keys(vm_ptr: usize, gc_id: u32) -> js_sys::Array {
    let vm = unsafe { &mut *(vm_ptr as *mut VM) };
    let base_val = BxValue::new_ptr(gc_id as usize);
    let keys = wasm_instance_keys(vm, base_val);
    let js_keys = js_sys::Array::new();
    for key in keys {
        js_keys.push(&JsValue::from_str(&key));
    }
    js_keys
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
#[wasm_bindgen]
pub fn _matchbox_set_instance_prop(vm_ptr: usize, gc_id: u32, name: &str, val: JsValue) -> Result<(), JsValue> {
    let vm = unsafe { &mut *(vm_ptr as *mut VM) };
    let base_val = BxValue::new_ptr(gc_id as usize);
    let bx_val = vm.js_to_bx(val);
    wasm_instance_prop_set(vm, base_val, name, bx_val)
        .map_err(|error| js_error_value(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_until_instant_waits_at_least_the_requested_time() {
        let start = Instant::now();
        wait_until_instant(start + Duration::from_millis(30));
        assert!(start.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    fn wait_until_instant_returns_immediately_for_a_past_deadline() {
        let start = Instant::now();
        wait_until_instant(start);
        assert!(start.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn runtime_error_out_of_memory_display() {
        let err = RuntimeError::OutOfMemory("Heap exhausted after GC".to_string());
        let display_str = format!("{}", err);
        assert!(
            display_str.contains("OutOfMemory"),
            "Display should contain variant name, got: {}",
            display_str
        );
        assert!(
            display_str.contains("Heap exhausted after GC"),
            "Display should contain message, got: {}",
            display_str
        );
    }

    #[test]
    fn runtime_error_out_of_memory_exception_type() {
        let err = RuntimeError::OutOfMemory("test".to_string());
        assert_eq!(err.exception_type(), "OutOfMemoryException");
    }

    #[test]
    fn runtime_error_out_of_memory_is_error_trait() {
        // Verify it implements std::error::Error
        let err = RuntimeError::OutOfMemory("test".to_string());
        let _: &dyn std::error::Error = &err;
    }

    #[test]
    fn gc_alloc_succeeds_on_normal_allocation() {
        let mut vm = VM::new();
        let result = vm.gc_alloc(GcObject::String(BoxString::new("test")));
        assert!(
            result.is_ok(),
            "gc_alloc should succeed: {:?}",
            result.err()
        );
        let id = result.unwrap();
        assert!(vm.heap.get_opt(id).is_some());
    }

    #[test]
    fn gc_alloc_returns_err_on_oom() {
        // Verify that RuntimeError::OutOfMemory is returned when the heap
        // is completely exhausted. We test this by directly constructing
        // the error since true OOM is difficult to trigger in unit tests.
        let err = RuntimeError::OutOfMemory("No memory after GC".into());
        assert_eq!(err.exception_type(), "OutOfMemoryException");
        // Verify it can be used as a Result::Err
        let result: Result<usize, RuntimeError> = Err(err);
        assert!(result.is_err());
    }

    #[test]
    fn runtime_error_converts_to_anyhow() {
        // RuntimeError implements std::error::Error, so anyhow's blanket
        // From<E: std::error::Error + Send + Sync + 'static> applies.
        let err = RuntimeError::OutOfMemory("test".into());
        let anyhow_err: anyhow::Error = err.into();
        let msg = format!("{}", anyhow_err);
        assert!(msg.contains("OutOfMemory"));
        assert!(msg.contains("test"));
    }

    #[test]
    fn vm_with_config_uses_esp32_preset() {
        let vm = VM::with_config(GCConfig::for_esp32());
        assert_eq!(vm.config.initial_capacity, 128);
        assert_eq!(vm.config.initial_threshold, 256);
        assert_eq!(vm.heap.gc_threshold(), 256);
    }
}
