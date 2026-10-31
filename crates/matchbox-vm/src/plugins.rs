//! Desktop native plugin loader. Only the C ABI in matchbox-plugin-sdk crosses libraries.
use crate::types::{BxNativeObject, BxVM, BxValue, NativeCallback};
use libloading::Library;
use matchbox_plugin_sdk::*;
use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    path::Path,
    rc::{Rc, Weak},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Argument {
    name: String,
    #[serde(default)]
    required: bool,
    #[serde(default)]
    default: serde_json::Value,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Function {
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    arguments: Vec<Argument>,
}
struct Registration {
    functions: Vec<(Function, Invoke)>,
    error: Option<String>,
}

// Libraries are retained by callbacks and native objects, not just the module manager.
struct CallContext<'a> {
    vm: &'a mut dyn BxVM,
    values: Vec<BxValue>,
    strings: Vec<String>,
    bytes: Vec<Vec<u8>>,
    library: Rc<Library>,
    functions: Rc<HashMap<String, (Function, Invoke)>>,
    error: Option<String>,
}
impl CallContext<'_> {
    fn value(&self, handle: Value) -> Result<BxValue, String> {
        handle
            .checked_sub(1)
            .and_then(|id| usize::try_from(id).ok())
            .and_then(|id| self.values.get(id))
            .copied()
            .ok_or_else(|| "Invalid plugin value handle".into())
    }
    fn add(&mut self, value: BxValue) -> Value {
        self.values.push(value);
        self.values.len() as Value
    }
}
unsafe fn text<'a>(value: Text) -> Result<&'a str, String> {
    if value.len == 0 {
        return Ok("");
    }
    if value.ptr.is_null() {
        return Err("Null plugin string".into());
    }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(value.ptr, value.len) })
        .map_err(|_| "Invalid UTF-8 in plugin string".into())
}
unsafe fn context<'a>(ctx: Context) -> &'a mut CallContext<'a> {
    unsafe { &mut *ctx.cast::<CallContext<'a>>() }
}
unsafe extern "C" fn capability(name: Text) -> *const c_void {
    match unsafe { text(name) }.ok() {
        Some(VALUES_V1) => (&VALUES as *const ValuesV1).cast(),
        Some(ROOTS_V1) => (&ROOTS as *const RootsV1).cast(),
        Some(BYTES_V1) => (&BYTES as *const BytesV1).cast(),
        _ => std::ptr::null(),
    }
}
unsafe extern "C" fn registration_error(ctx: Context, message: Text) {
    unsafe { &mut *ctx.cast::<Registration>() }.error = Some(
        unsafe { text(message) }
            .map(str::to_owned)
            .unwrap_or_else(|e| e),
    );
}
unsafe extern "C" fn register(ctx: Context, descriptor: Text, invoke: Invoke) -> i32 {
    let registration = unsafe { &mut *ctx.cast::<Registration>() };
    let result = (|| {
        let function: Function =
            serde_json::from_str(unsafe { text(descriptor)? }).map_err(|e| e.to_string())?;
        if function.name.is_empty()
            || function
                .arguments
                .iter()
                .any(|a| a.name.is_empty() || a.default.is_array() || a.default.is_object())
        {
            return Err("Invalid plugin function metadata".into());
        }
        let mut names = std::collections::HashSet::new();
        if function
            .arguments
            .iter()
            .any(|a| !names.insert(a.name.to_lowercase()))
        {
            return Err("Duplicate plugin argument".into());
        }
        names.clear();
        for name in std::iter::once(&function.name).chain(&function.aliases) {
            if name.is_empty()
                || !names.insert(name.to_lowercase())
                || registration.functions.iter().any(|(f, _)| {
                    std::iter::once(&f.name)
                        .chain(&f.aliases)
                        .any(|existing| existing.eq_ignore_ascii_case(name))
                })
            {
                return Err("Empty or duplicate plugin registration".into());
            }
        }
        registration.functions.push((function, invoke));
        Ok::<_, String>(())
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            registration.error = Some(error);
            1
        }
    }
}

/// # Safety
/// The selected library is trusted native code, capable of arbitrary process access.
pub unsafe fn load(vm: &mut dyn BxVM, path: &Path) -> Result<(), String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    // ponytail: dependencies use the OS loader namespace; incompatible versions need distinct SONAMEs/install names/DLL names.
    #[cfg(not(windows))]
    let library = unsafe { Library::new(&path) };
    #[cfg(windows)]
    let library = {
        use libloading::os::windows::{
            LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
            Library as WindowsLibrary,
        };
        unsafe {
            WindowsLibrary::load_with_flags(
                &path,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        }
        .map(Library::from)
    };
    let library = Rc::new(library.map_err(|e| format!("{}: {e}", path.display()))?);
    let init =
        unsafe { library.get::<Init>(b"matchbox_plugin_init_v1\0") }.map_err(|e| e.to_string())?;
    let host = HostV1 {
        abi_major: ABI_MAJOR,
        capability,
        register,
        error: registration_error,
    };
    let mut registration = Registration {
        functions: vec![],
        error: None,
    };
    let status = unsafe { init(&host, (&mut registration as *mut Registration).cast()) };
    if status != 0 || registration.error.is_some() {
        return Err(registration
            .error
            .unwrap_or_else(|| "Native plugin initialization failed".into()));
    }
    // No unload: plugin state/destructors can outlive an individual VM.
    std::mem::forget(library.clone());
    let functions = Rc::new(
        registration
            .functions
            .iter()
            .flat_map(|(function, invoke)| {
                std::iter::once(&function.name)
                    .chain(&function.aliases)
                    .map(|name| (name.to_lowercase(), (function.clone(), *invoke)))
            })
            .collect::<HashMap<_, _>>(),
    );
    for (function, invoke) in registration.functions {
        let names: Vec<_> = std::iter::once(function.name.clone())
            .chain(function.aliases.iter().cloned())
            .collect();
        let callback = callback(library.clone(), functions.clone(), function, invoke);
        for name in names {
            vm.register_callback(&name, callback.clone())?;
        }
    }
    Ok(())
}
fn callback(
    library: Rc<Library>,
    functions: Rc<HashMap<String, (Function, Invoke)>>,
    function: Function,
    invoke: Invoke,
) -> NativeCallback {
    NativeCallback {
        parameters: function.arguments.iter().map(|a| a.name.clone()).collect(),
        callback: Rc::new(move |vm, args| {
            invoke_function(vm, &library, &functions, &function, invoke, args)
        }),
    }
}
fn invoke_function(
    vm: &mut dyn BxVM,
    library: &Rc<Library>,
    functions: &Rc<HashMap<String, (Function, Invoke)>>,
    function: &Function,
    invoke: Invoke,
    args: &[BxValue],
) -> Result<BxValue, String> {
    if args.len() > function.arguments.len() {
        return Err(format!("{}: too many arguments", function.name));
    }
    let mut values = Vec::new();
    for (index, argument) in function.arguments.iter().enumerate() {
        let value = args.get(index).copied().filter(|v| !v.is_null());
        let value = match value {
            Some(value) => value,
            None if argument.required => {
                return Err(format!("{}: missing {}", function.name, argument.name));
            }
            None => match &argument.default {
                serde_json::Value::Null => BxValue::new_null(),
                serde_json::Value::Bool(b) => BxValue::new_bool(*b),
                serde_json::Value::Number(n) => {
                    BxValue::new_number(n.as_f64().ok_or("Invalid numeric default")?)
                }
                serde_json::Value::String(s) => BxValue::new_ptr(vm.string_new(s.clone())),
                _ => unreachable!(),
            },
        };
        values.push(value);
    }
    let args: Vec<Value> = (1..=values.len() as u64).collect();
    let mut ctx = CallContext {
        vm,
        values,
        strings: Vec::new(),
        bytes: Vec::new(),
        library: library.clone(),
        functions: functions.clone(),
        error: None,
    };
    let mut out = 0;
    let status = unsafe {
        invoke(
            (&mut ctx as *mut CallContext).cast(),
            &VALUES,
            std::ptr::null_mut(),
            args.as_ptr(),
            args.len(),
            &mut out,
        )
    };
    if status != 0 || ctx.error.is_some() {
        return Err(ctx
            .error
            .unwrap_or_else(|| format!("{}: native operation failed", function.name)));
    }
    ctx.value(out)
}
fn operation(ctx: &mut CallContext<'_>, result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            ctx.error.get_or_insert(error);
            1
        }
    }
}
unsafe extern "C" fn error(ctx: Context, message: Text) {
    // Host failures are sticky; the SDK's generic error must not replace the cause.
    unsafe { context(ctx) }.error.get_or_insert_with(|| {
        unsafe { text(message) }
            .map(str::to_owned)
            .unwrap_or_else(|e| e)
    });
}
unsafe extern "C" fn string_get(ctx: Context, value: Value, out: *mut Text) -> i32 {
    let ctx = unsafe { context(ctx) };
    let result = (|| {
        if out.is_null() {
            return Err("Null output pointer".into());
        }
        let value = ctx.value(value)?;
        if !ctx.vm.is_string_value(value) {
            return Err("Expected string".into());
        }
        ctx.strings.push(ctx.vm.to_string(value));
        unsafe { *out = Text::new(ctx.strings.last().unwrap()) };
        Ok(())
    })();
    operation(ctx, result)
}
unsafe extern "C" fn number_get(ctx: Context, value: Value, out: *mut f64) -> i32 {
    let ctx = unsafe { context(ctx) };
    let result = (|| {
        let value = ctx.value(value)?;
        if !value.is_number() {
            return Err("Expected numeric value".into());
        }
        if out.is_null() {
            return Err("Null output pointer".into());
        }
        unsafe { *out = value.as_number() };
        Ok(())
    })();
    operation(ctx, result)
}
fn output(ctx: &mut CallContext<'_>, value: BxValue, out: *mut Value) -> i32 {
    if out.is_null() {
        return operation(ctx, Err("Null output pointer".into()));
    }
    unsafe { *out = ctx.add(value) };
    0
}
unsafe extern "C" fn number_new(ctx: Context, value: f64, out: *mut Value) -> i32 {
    output(unsafe { context(ctx) }, BxValue::new_number(value), out)
}
unsafe extern "C" fn bool_new(ctx: Context, value: bool, out: *mut Value) -> i32 {
    output(unsafe { context(ctx) }, BxValue::new_bool(value), out)
}
unsafe extern "C" fn null_new(ctx: Context, out: *mut Value) -> i32 {
    output(unsafe { context(ctx) }, BxValue::new_null(), out)
}
unsafe extern "C" fn string_new(ctx: Context, value: Text, out: *mut Value) -> i32 {
    let ctx = unsafe { context(ctx) };
    match unsafe { text(value) } {
        Ok(value) => {
            let id = ctx.vm.string_new(value.to_owned());
            output(ctx, BxValue::new_ptr(id), out)
        }
        Err(error) => operation(ctx, Err(error)),
    }
}
unsafe extern "C" fn struct_new(ctx: Context, out: *mut Value) -> i32 {
    let ctx = unsafe { context(ctx) };
    let id = ctx.vm.struct_new();
    output(ctx, BxValue::new_ptr(id), out)
}
unsafe extern "C" fn struct_set(ctx: Context, object: Value, key: Text, value: Value) -> i32 {
    let ctx = unsafe { context(ctx) };
    let result = (|| {
        let object = ctx.value(object)?;
        let value = ctx.value(value)?;
        if !ctx.vm.is_struct_value(object) {
            return Err("Expected struct".into());
        }
        ctx.vm
            .struct_set(object.as_gc_id().unwrap(), unsafe { text(key)? }, value);
        Ok(())
    })();
    operation(ctx, result)
}
struct PluginObject {
    kind: String,
    data: *mut c_void,
    drop: DropObject,
    methods: HashMap<String, NativeCallback>,
    _library: Rc<Library>,
}
impl std::fmt::Debug for PluginObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PluginObject").field(&self.kind).finish()
    }
}
impl Drop for PluginObject {
    fn drop(&mut self) {
        unsafe { (self.drop)(self.data) };
    }
}
impl BxNativeObject for PluginObject {
    fn delegated_callback(&self, name: &str) -> Option<NativeCallback> {
        self.methods.get(&name.to_lowercase()).cloned()
    }
    fn native_data(&self, kind: &str) -> Option<*mut c_void> {
        // Payload layouts are private to the library, not just a caller-chosen type name.
        (kind == format!("{:p}:{}", Rc::as_ptr(&self._library), self.kind)).then_some(self.data)
    }
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        let _ = (vm, id, args);
        Err(format!("Unknown {} method: {name}", self.kind))
    }
}
unsafe extern "C" fn object_new(
    ctx: Context,
    kind: Text,
    data: *mut c_void,
    drop: DropObject,
    methods: Text,
    out: *mut Value,
) -> i32 {
    let ctx = unsafe { context(ctx) };
    let result = (|| {
        if out.is_null() || data.is_null() {
            return Err("Null object pointer".into());
        }
        let kind = unsafe { text(kind)? }.to_owned();
        let methods: HashMap<String, String> =
            serde_json::from_str(unsafe { text(methods)? }).map_err(|e| e.to_string())?;
        let methods = methods
            .into_iter()
            .map(|(name, function)| {
                let (function, invoke) = ctx
                    .functions
                    .get(&function.to_lowercase())
                    .ok_or_else(|| format!("Unknown plugin member provider: {function}"))?;
                Ok((
                    name.to_lowercase(),
                    callback(
                        ctx.library.clone(),
                        ctx.functions.clone(),
                        function.clone(),
                        *invoke,
                    ),
                ))
            })
            .collect::<Result<HashMap<_, _>, String>>()?;
        let id = ctx.vm.native_object_new(Rc::new(RefCell::new(PluginObject {
            kind,
            data,
            drop,
            methods,
            _library: ctx.library.clone(),
        })));
        output(ctx, BxValue::new_ptr(id), out);
        Ok(())
    })();
    operation(ctx, result)
}
unsafe extern "C" fn object_data(
    ctx: Context,
    value: Value,
    kind: Text,
    out: *mut *mut c_void,
) -> i32 {
    let ctx = unsafe { context(ctx) };
    let result = (|| {
        if out.is_null() {
            return Err("Null output pointer".into());
        }
        let value = ctx.value(value)?;
        let kind = format!("{:p}:{}", Rc::as_ptr(&ctx.library), unsafe { text(kind)? });
        let data = ctx
            .vm
            .native_object_data(value, &kind)
            .ok_or("Incorrect native object type")?;
        unsafe { *out = data };
        Ok(())
    })();
    operation(ctx, result)
}
/// Packaging-time download only; applications never fetch executable code at startup.
pub fn download(url: &str) -> Result<Vec<u8>, String> {
    #[cfg(feature = "bif-http")]
    {
        use std::io::Read;
        if !url.starts_with("https://") {
            return Err("Native artifact downloads require HTTPS".into());
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .https_only(true)
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .get(url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|e| e.to_string())?;
        let limit = 256 * 1024 * 1024;
        let mut bytes = Vec::new();
        response
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > limit {
            return Err("Native artifact exceeds the 256 MiB download limit".into());
        }
        Ok(bytes)
    }
    #[cfg(not(feature = "bif-http"))]
    {
        let _ = url;
        Err("Remote native artifacts require the bif-http build feature".into())
    }
}

struct HostRoot {
    value: Rc<crate::vm::gc::RootedValue>,
    library: Weak<Library>,
}
unsafe extern "C" fn root_retain(ctx: Context, value: Value, out: *mut RootHandle) -> i32 {
    let ctx = unsafe { context(ctx) };
    let result = (|| {
        if out.is_null() {
            return Err("Null output pointer".into());
        }
        let value = ctx.value(value)?;
        let value = ctx.vm.retain_value(value)?;
        let root = Box::new(HostRoot {
            value,
            library: Rc::downgrade(&ctx.library),
        });
        unsafe {
            *out = Box::into_raw(root).cast();
        }
        Ok(())
    })();
    operation(ctx, result)
}
unsafe extern "C" fn root_get(ctx: Context, root: RootHandle, out: *mut Value) -> i32 {
    let ctx = unsafe { context(ctx) };
    let result = (|| {
        if root.is_null() || out.is_null() {
            return Err("Null retained value pointer".into());
        }
        let root = unsafe { &*root.cast::<HostRoot>() };
        let value = ctx.vm.read_retained_value(&root.value)?;
        if !root.library.ptr_eq(&Rc::downgrade(&ctx.library)) {
            return Err("Retained value belongs to another plugin provider".into());
        }
        output(ctx, value, out);
        Ok(())
    })();
    operation(ctx, result)
}
unsafe extern "C" fn root_release(root: RootHandle) {
    if !root.is_null() {
        drop(unsafe { Box::from_raw(root.cast::<HostRoot>()) });
    }
}
unsafe extern "C" fn bytes_get(ctx: Context, value: Value, out: *mut Bytes) -> i32 {
    let ctx = unsafe { context(ctx) };
    let result = (|| {
        if out.is_null() { return Err("Null output pointer".into()); }
        let value = ctx.value(value)?;
        ctx.bytes.push(ctx.vm.to_bytes(value)?);
        unsafe { *out = Bytes::new(ctx.bytes.last().unwrap()) };
        Ok(())
    })();
    operation(ctx, result)
}
unsafe extern "C" fn bytes_new(ctx: Context, bytes: Bytes, out: *mut Value) -> i32 {
    let ctx = unsafe { context(ctx) };
    if out.is_null() { return operation(ctx, Err("Null output pointer".into())); }
    if bytes.len != 0 && bytes.ptr.is_null() { return operation(ctx, Err("Null binary input pointer".into())); }
    let bytes = if bytes.len == 0 { Vec::new() } else {
        unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) }.to_vec()
    };
    let id = ctx.vm.bytes_new(bytes);
    output(ctx, BxValue::new_ptr(id), out)
}
static BYTES: BytesV1 = BytesV1 { get: bytes_get, new: bytes_new };

static ROOTS: RootsV1 = RootsV1 {
    retain: root_retain,
    get: root_get,
    release: root_release,
};

static VALUES: ValuesV1 = ValuesV1 {
    string_get,
    number_get,
    number_new,
    string_new,
    bool_new,
    null_new,
    struct_new,
    struct_set,
    object_new,
    object_data,
    error,
};
