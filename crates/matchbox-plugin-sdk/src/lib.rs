#![doc = include_str!("../README.md")]
#![deny(missing_docs)]
use std::{ffi::c_void, ptr::NonNull};

/// The plugin ABI major version this SDK targets. The host refuses a plugin whose
/// `abi` in `box.json` does not match, and `require_*` fails if the host reports another one.
pub const ABI_MAJOR: u32 = 1;
/// Name of the core capability: numbers, strings, booleans, null, structs, native objects
/// and errors. See [`ValuesV1`] and [`require_values`].
pub const VALUES_V1: &str = "matchbox.values.v1";
/// Name of the capability that keeps values alive between calls. See [`RootsV1`] and [`require_roots`].
pub const ROOTS_V1: &str = "matchbox.roots.v1";
/// Name of the binary value capability. See [`BytesV1`] and [`require_bytes`].
pub const BYTES_V1: &str = "matchbox.bytes.v1";
/// Name of the capability that reports a value's kind and reads booleans. See [`TypesV1`] and [`require_types`].
pub const TYPES_V1: &str = "matchbox.types.v1";
/// Name of the array and struct capability. See [`CollectionsV1`] and [`require_collections`].
pub const COLLECTIONS_V1: &str = "matchbox.collections.v1";
/// Name of the module settings capability. See [`SettingsV1`] and [`require_settings`].
pub const SETTINGS_V1: &str = "matchbox.settings.v1";
/// Name of the path capability. See [`PathsV1`] and [`require_paths`].
pub const PATHS_V1: &str = "matchbox.paths.v1";
/// Name of the whole-number capability. See [`IntegersV1`] and [`require_integers`].
pub const INTEGERS_V1: &str = "matchbox.integers.v1";

/// An opaque handle to a BoxLang value. Valid only during the invocation that received or
/// created it; use [`Call::retain`] to keep a value longer. Never inspect its bits.
pub type Value = u64;
/// Host-owned opaque allocation, valid until released exactly once on the VM thread.
pub type RootHandle = *mut c_void;
/// The host's opaque per-call (or per-initialization) context. Pass it back unchanged;
/// never keep it after the call returns.
pub type Context = *mut c_void;
/// The signature of a registered function: `(context, values table, reserved, arguments,
/// argument count, result slot)`. Return `0` and write the result on success; report an
/// error and return non-zero on failure. Wrap the body in [`guarded`].
pub type Invoke = unsafe extern "C" fn(
    Context,
    *const ValuesV1,
    *mut c_void,
    *const Value,
    usize,
    *mut Value,
) -> i32;
/// Frees a native object's payload. Called once by the host when the object is collected.
/// It must not unwind.
pub type DropObject = unsafe extern "C" fn(*mut c_void);
/// The signature of the exported `matchbox_plugin_init_v1` entry point. Return `0` on success.
pub type Init = unsafe extern "C" fn(*const HostV1, Context) -> i32;

/// A borrowed UTF-8 string passed across the ABI. It does not own its bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Text {
    /// Start of the UTF-8 bytes.
    pub ptr: *const u8,
    /// Length in bytes.
    pub len: usize,
}
impl Text {
    /// Borrows `s`. The result is valid only while `s` is alive.
    pub fn new(s: &str) -> Self {
        Self {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }
}

/// Arbitrary bytes, not UTF-8. Borrowed only for the documented call lifetime.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bytes {
    /// Start of the bytes.
    pub ptr: *const u8,
    /// Length in bytes.
    pub len: usize,
}
impl Bytes {
    /// Borrows `bytes`. The result is valid only while `bytes` is alive.
    pub fn new(bytes: &[u8]) -> Self { Self { ptr: bytes.as_ptr(), len: bytes.len() } }
}

/// Additive binary-value capability. `get` borrows until the invocation ends;
/// `new` copies the supplied bytes before returning. No VM layout crosses the ABI.
#[repr(C)]
pub struct BytesV1 {
    /// Borrows a binary value's bytes until the invocation ends. Fails for a non-binary value.
    pub get: unsafe extern "C" fn(Context, Value, *mut Bytes) -> i32,
    /// Creates a binary value from a copy of the bytes.
    pub new: unsafe extern "C" fn(Context, Bytes, *mut Value) -> i32,
}

/// The kind of a value, as reported by [`TypesV1::kind_of`].
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `null`.
    Null = 0,
    /// A boolean.
    Bool = 1,
    /// A number.
    Number = 2,
    /// A string.
    String = 3,
    /// An array.
    Array = 4,
    /// A struct.
    Struct = 5,
    /// A binary value.
    Binary = 6,
    /// Anything else: native objects, classes, functions, queries and so on.
    Other = 7,
}
impl Kind {
    /// Converts a code from [`TypesV1::kind_of`]. Unknown codes, including ones a newer
    /// host may add, become [`Kind::Other`].
    pub fn from_raw(raw: u32) -> Kind {
        match raw {
            0 => Kind::Null,
            1 => Kind::Bool,
            2 => Kind::Number,
            3 => Kind::String,
            4 => Kind::Array,
            5 => Kind::Struct,
            6 => Kind::Binary,
            _ => Kind::Other,
        }
    }
}

/// Additive capability: inspect a value's kind and read booleans (strictly; no truthiness).
#[repr(C)]
pub struct TypesV1 {
    /// Writes the value's [`Kind`] code.
    pub kind_of: unsafe extern "C" fn(Context, Value, *mut u32) -> i32,
    /// Reads a boolean. Fails for anything that is not a boolean, including `"yes"` and `1`.
    pub bool_get: unsafe extern "C" fn(Context, Value, *mut bool) -> i32,
}

/// Additive capability: build and read arrays, and read structs. Array indexes are 0-based.
/// `struct_get` reports a missing key through `found` instead of failing. `struct_keys`
/// returns a new array of the keys, in the struct's order.
#[repr(C)]
pub struct CollectionsV1 {
    /// Creates an empty array.
    pub array_new: unsafe extern "C" fn(Context, *mut Value) -> i32,
    /// Appends a value to an array. Fails for a non-array.
    pub array_push: unsafe extern "C" fn(Context, Value, Value) -> i32,
    /// Writes an array's length. Fails for a non-array.
    pub array_len: unsafe extern "C" fn(Context, Value, *mut usize) -> i32,
    /// Reads the element at a 0-based index. Fails for a non-array or an index past the end.
    pub array_get: unsafe extern "C" fn(Context, Value, usize, *mut Value) -> i32,
    /// Reads a struct value by key, case-insensitively, and sets `found`. Fails for a non-struct.
    pub struct_get: unsafe extern "C" fn(Context, Value, Text, *mut Value, *mut bool) -> i32,
    /// Creates a new array of a struct's keys, in the struct's order. Fails for a non-struct.
    pub struct_keys: unsafe extern "C" fn(Context, Value, *mut Value) -> i32,
}

/// Additive capability: whole numbers. A number made with `number_new` is a decimal and shows as `5.0`; one made with `int_new`
/// is whole and shows as `5`, as BoxLang's own integers do.
#[repr(C)]
pub struct IntegersV1 {
    /// Creates a whole number. Values outside the VM's 32-bit integer range become a decimal of the same value.
    pub int_new: unsafe extern "C" fn(Context, i64, *mut Value) -> i32,
    /// Reads a whole number: an integer, or a decimal with no fractional part within the `i64` range. Anything else fails.
    pub int_get: unsafe extern "C" fn(Context, Value, *mut i64) -> i32,
}

/// Additive capability: the settings of the module that supplied the plugin.
#[repr(C)]
pub struct SettingsV1 {
    /// Writes the module's settings struct: the defaults its `ModuleConfig.bx` declares with the overrides from
    /// `boxlang.json` (`modules.<name>.settings`) merged in. It is the live struct, so read it on every call rather than
    /// keeping a copy. Fails when the plugin was not loaded by a module.
    pub module_settings: unsafe extern "C" fn(Context, *mut Value) -> i32,
}

/// Additive capability: paths, resolved as BoxLang's `expandPath` does for the code that called the plugin.
#[repr(C)]
pub struct PathsV1 {
    /// Writes a new string: `path` made absolute. A relative path starts in the folder of the calling file, `/` is the
    /// application root, mappings (`boxlang.json`, `Application.bx`, modules) resolve into their folders, and a trailing
    /// separator is kept. The file does not have to exist.
    pub expand_path: unsafe extern "C" fn(Context, Text, *mut Value) -> i32,
}

/// The host interface passed to `matchbox_plugin_init_v1`.
///
/// Borrowed only during initialization. Returned capability tables remain valid for
/// the host process lifetime; registration/error context pointers must not be retained.
#[repr(C)]
pub struct HostV1 {
    /// The host's ABI major version. Compare with [`ABI_MAJOR`].
    pub abi_major: u32,
    /// Looks up a capability table by name, such as [`VALUES_V1`]. Null when the host lacks it.
    pub capability: unsafe extern "C" fn(Text) -> *const c_void,
    /// Registers a function. The metadata is JSON:
    /// `{"name":"...","aliases":["..."],"arguments":[{"name":"...","required":true,"default":null}]}`.
    /// Defaults are null, numbers, strings or booleans. Returns `0` on success.
    pub register: unsafe extern "C" fn(Context, Text, Invoke) -> i32,
    /// Reports an initialization error message.
    pub error: unsafe extern "C" fn(Context, Text),
}

/// The core value capability, `matchbox.values.v1`. Every plugin requires it.
#[repr(C)]
pub struct ValuesV1 {
    /// Borrows a string value's UTF-8. Returned UTF-8 remains valid until this invocation ends.
    pub string_get: unsafe extern "C" fn(Context, Value, *mut Text) -> i32,
    /// Reads a number.
    pub number_get: unsafe extern "C" fn(Context, Value, *mut f64) -> i32,
    /// Creates a number.
    pub number_new: unsafe extern "C" fn(Context, f64, *mut Value) -> i32,
    /// Creates a string from a copy of the UTF-8.
    pub string_new: unsafe extern "C" fn(Context, Text, *mut Value) -> i32,
    /// Creates a boolean.
    pub bool_new: unsafe extern "C" fn(Context, bool, *mut Value) -> i32,
    /// Creates `null`.
    pub null_new: unsafe extern "C" fn(Context, *mut Value) -> i32,
    /// Creates an empty struct.
    pub struct_new: unsafe extern "C" fn(Context, *mut Value) -> i32,
    /// Sets a struct key. The key keeps the spelling given here.
    pub struct_set: unsafe extern "C" fn(Context, Value, Text, Value) -> i32,
    /// Creates a native object: `(kind, payload, drop, methods JSON)`.
    /// Ownership transfers only on success. method metadata: {"info":"imageInfo"}.
    pub object_new:
        unsafe extern "C" fn(Context, Text, *mut c_void, DropObject, Text, *mut Value) -> i32,
    /// Borrows a native object's payload, checking it was created with the same kind.
    pub object_data: unsafe extern "C" fn(Context, Value, Text, *mut *mut c_void) -> i32,
    /// Reports the error for a failing call. The host raises it as a BoxLang exception.
    pub error: unsafe extern "C" fn(Context, Text),
}

/// Additive capability: persistent GC roots, separate from invocation-local Value handles.
/// All operations, including release, run on the VM thread. Handles belong to one VM
/// and provider instance. Release remains valid after that VM has been destroyed.
#[repr(C)]
pub struct RootsV1 {
    /// Keeps a value alive and writes a new root handle.
    pub retain: unsafe extern "C" fn(Context, Value, *mut RootHandle) -> i32,
    /// Writes the rooted value as a handle for the current invocation.
    pub get: unsafe extern "C" fn(Context, RootHandle, *mut Value) -> i32,
    /// Releases a root. Null is a no-op; every nonnull handle must be live and host-issued.
    pub release: unsafe extern "C" fn(RootHandle),
}

/// An owned GC root. Dropping it releases the host allocation, even without a live call.
/// Strong roots can form cycles; release any root that refers back to its owning payload.
///
/// ```compile_fail
/// fn move_to_worker(root: matchbox_plugin_sdk::RetainedValue) {
///     std::thread::spawn(move || drop(root));
/// }
/// ```
pub struct RetainedValue {
    handle: RootHandle,
    api: &'static RootsV1,
}
impl RetainedValue {
    /// The retained value as a handle for `call`. Fails when `call` belongs to another VM
    /// or plugin than the one that retained it.
    pub fn value(&self, call: &Call<'_>) -> Result<Value, String> {
        call.value(|out| unsafe { (self.api.get)(call.context, self.handle, out) })
    }
}
impl Drop for RetainedValue {
    fn drop(&mut self) {
        unsafe { (self.api.release)(self.handle) };
    }
}

/// Ergonomic, call-scoped wrapper. Construct only from the host's invocation callback.
///
/// Every helper returns `Err(message)` when the host refuses the operation, ready to
/// return from a [`guarded`] body.
pub struct Call<'a> {
    context: Context,
    api: &'a ValuesV1,
    args: &'a [Value],
}
impl<'a> Call<'a> {
    /// Wraps the pointers an [`Invoke`] callback received.
    ///
    /// # Safety
    /// Inputs must originate from the host and remain valid throughout this invocation.
    pub unsafe fn new(
        context: Context,
        api: *const ValuesV1,
        args: *const Value,
        len: usize,
    ) -> Self {
        Self {
            context,
            api: unsafe { &*api },
            args: if len == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(args, len) }
            },
        }
    }
    /// The argument at `index` (0-based), in the order the registration metadata declares.
    /// Defaults are already filled in.
    pub fn argument(&self, index: usize) -> Result<Value, String> {
        self.args
            .get(index)
            .copied()
            .ok_or_else(|| format!("Missing argument {index}"))
    }
    fn value(&self, f: impl FnOnce(*mut Value) -> i32) -> Result<Value, String> {
        let mut out = 0;
        if f(&mut out) == 0 {
            Ok(out)
        } else {
            Err("Host value operation failed".into())
        }
    }
    /// Keeps `value` alive past this call. Requires [`ROOTS_V1`]; drop the result to release it.
    pub fn retain(&self, api: &'static RootsV1, value: Value) -> Result<RetainedValue, String> {
        let mut handle = std::ptr::null_mut();
        if unsafe { (api.retain)(self.context, value, &mut handle) } != 0 {
            return Err("Host value retention failed".into());
        }
        Ok(RetainedValue { handle, api })
    }
    /// Reads a number.
    pub fn number(&self, value: Value) -> Result<f64, String> {
        let mut out = 0.0;
        if unsafe { (self.api.number_get)(self.context, value, &mut out) } == 0 {
            Ok(out)
        } else {
            Err("Expected a number".into())
        }
    }
    /// Reads a string, copied into an owned `String`.
    pub fn string(&self, value: Value) -> Result<String, String> {
        let mut out = Text::new("");
        if unsafe { (self.api.string_get)(self.context, value, &mut out) } != 0 {
            return Err("Expected a string".into());
        }
        let bytes = if out.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(out.ptr, out.len) }
        };
        String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
    }
    /// Reads a binary value's bytes, copied. Requires [`BYTES_V1`].
    pub fn bytes(&self, api: &BytesV1, value: Value) -> Result<Vec<u8>, String> {
        let mut out = Bytes::new(&[]);
        if unsafe { (api.get)(self.context, value, &mut out) } != 0 { return Err("Expected binary value".into()); }
        if out.len == 0 { return Ok(Vec::new()); }
        Ok(unsafe { std::slice::from_raw_parts(out.ptr, out.len) }.to_vec())
    }
    /// Creates a binary value from a copy of `bytes`. Requires [`BYTES_V1`].
    pub fn bytes_new(&self, api: &BytesV1, bytes: &[u8]) -> Result<Value, String> {
        self.value(|out| unsafe { (api.new)(self.context, Bytes::new(bytes), out) })
    }
    /// The settings struct of the module that supplied the plugin, read with [`Call::struct_get`] and friends.
    /// Requires [`SETTINGS_V1`].
    pub fn module_settings(&self, api: &SettingsV1) -> Result<Value, String> {
        self.value(|out| unsafe { (api.module_settings)(self.context, out) })
    }
    /// `path` made absolute as BoxLang's `expandPath` does: relative to the calling file, through mappings, with `/` as the
    /// application root. Requires [`PATHS_V1`].
    pub fn expand_path(&self, api: &PathsV1, path: &str) -> Result<String, String> {
        let value = self.value(|out| unsafe { (api.expand_path)(self.context, Text::new(path), out) })?;
        self.string(value)
    }
    /// Creates a whole number, shown without a decimal point (`5`, not `5.0`). Requires [`INTEGERS_V1`].
    pub fn int_new(&self, api: &IntegersV1, value: i64) -> Result<Value, String> {
        self.value(|out| unsafe { (api.int_new)(self.context, value, out) })
    }
    /// Reads a whole number: `5` and `5.0` are 5; `2.5`, a string or anything else is an error. Requires [`INTEGERS_V1`].
    pub fn integer(&self, api: &IntegersV1, value: Value) -> Result<i64, String> {
        let mut out = 0;
        if unsafe { (api.int_get)(self.context, value, &mut out) } != 0 {
            return Err("Expected a whole number".into());
        }
        Ok(out)
    }
    /// Creates a boolean.
    pub fn bool_new(&self, value: bool) -> Result<Value, String> {
        self.value(|out| unsafe { (self.api.bool_new)(self.context, value, out) })
    }
    /// The value's [`Kind`]. Requires [`TYPES_V1`].
    pub fn kind(&self, api: &TypesV1, value: Value) -> Result<Kind, String> {
        let mut out = 0;
        if unsafe { (api.kind_of)(self.context, value, &mut out) } != 0 {
            return Err("Invalid value".into());
        }
        Ok(Kind::from_raw(out))
    }
    /// Reads a boolean strictly: `"yes"`, `1` and other truthy values are errors.
    /// Requires [`TYPES_V1`].
    pub fn boolean(&self, api: &TypesV1, value: Value) -> Result<bool, String> {
        let mut out = false;
        if unsafe { (api.bool_get)(self.context, value, &mut out) } != 0 {
            return Err("Expected a boolean".into());
        }
        Ok(out)
    }
    /// Creates an empty array. Requires [`COLLECTIONS_V1`].
    pub fn array_new(&self, api: &CollectionsV1) -> Result<Value, String> {
        self.value(|out| unsafe { (api.array_new)(self.context, out) })
    }
    /// Appends `value` to `array`. Requires [`COLLECTIONS_V1`].
    pub fn array_push(&self, api: &CollectionsV1, array: Value, value: Value) -> Result<(), String> {
        if unsafe { (api.array_push)(self.context, array, value) } == 0 {
            Ok(())
        } else {
            Err("Expected an array".into())
        }
    }
    /// The number of elements in `array`. Requires [`COLLECTIONS_V1`].
    pub fn array_len(&self, api: &CollectionsV1, array: Value) -> Result<usize, String> {
        let mut out = 0;
        if unsafe { (api.array_len)(self.context, array, &mut out) } != 0 {
            return Err("Expected an array".into());
        }
        Ok(out)
    }
    /// The element at `index`. 0-based; an index past the end is an error.
    /// Requires [`COLLECTIONS_V1`].
    pub fn array_get(&self, api: &CollectionsV1, array: Value, index: usize) -> Result<Value, String> {
        self.value(|out| unsafe { (api.array_get)(self.context, array, index, out) })
    }
    /// Reads `key` from a struct, case-insensitively. `None` when the key is missing.
    /// Requires [`COLLECTIONS_V1`].
    pub fn struct_get(&self, api: &CollectionsV1, object: Value, key: &str) -> Result<Option<Value>, String> {
        let (mut out, mut found) = (0, false);
        if unsafe { (api.struct_get)(self.context, object, Text::new(key), &mut out, &mut found) } != 0 {
            return Err("Expected a struct".into());
        }
        Ok(found.then_some(out))
    }
    /// A struct's keys, in the struct's order and spelled as stored. Requires [`COLLECTIONS_V1`].
    pub fn struct_keys(&self, api: &CollectionsV1, object: Value) -> Result<Vec<String>, String> {
        let keys = self.value(|out| unsafe { (api.struct_keys)(self.context, object, out) })?;
        (0..self.array_len(api, keys)?)
            .map(|i| self.string(self.array_get(api, keys, i)?))
            .collect()
    }
    /// Creates `null`.
    pub fn null_new(&self) -> Result<Value, String> {
        self.value(|out| unsafe { (self.api.null_new)(self.context, out) })
    }
    /// Creates a number.
    pub fn number_new(&self, number: f64) -> Result<Value, String> {
        self.value(|out| unsafe { (self.api.number_new)(self.context, number, out) })
    }
    /// Creates a string from a copy of `text`.
    pub fn string_new(&self, text: &str) -> Result<Value, String> {
        self.value(|out| unsafe { (self.api.string_new)(self.context, Text::new(text), out) })
    }
    /// Creates an empty struct.
    pub fn struct_new(&self) -> Result<Value, String> {
        self.value(|out| unsafe { (self.api.struct_new)(self.context, out) })
    }
    /// Sets `key` on a struct. A new key keeps the spelling given; an existing key is
    /// matched case-insensitively and keeps its first spelling.
    pub fn struct_set(&self, object: Value, key: &str, value: Value) -> Result<(), String> {
        if unsafe { (self.api.struct_set)(self.context, object, Text::new(key), value) } == 0 {
            Ok(())
        } else {
            Err("Host struct operation failed".into())
        }
    }
    /// Creates a native object holding `data`. `kind` names the payload type (for example
    /// `"stats.total.v1"`); `methods` is JSON mapping method names to registered functions,
    /// which receive the object as their first argument. On success the host owns `data`
    /// and calls `drop` when the object is collected; on error the caller still owns it.
    ///
    /// # Safety
    /// Payload must match the type identifier and drop callback, which must not unwind.
    pub unsafe fn object_new(
        &self,
        kind: &str,
        data: *mut c_void,
        drop: DropObject,
        methods: &str,
    ) -> Result<Value, String> {
        self.value(|out| unsafe {
            (self.api.object_new)(
                self.context,
                Text::new(kind),
                data,
                drop,
                Text::new(methods),
                out,
            )
        })
    }
    /// Returns a non-null payload borrowed for this invocation.
    /// Fails when `value` is not a native object created with the same `kind`.
    ///
    /// # Safety
    /// Cast/dereference the returned pointer only as the payload type registered for `kind`.
    pub unsafe fn object_data(&self, value: Value, kind: &str) -> Result<NonNull<c_void>, String> {
        let mut out = std::ptr::null_mut();
        if unsafe { (self.api.object_data)(self.context, value, Text::new(kind), &mut out) } != 0 {
            return Err(format!("Expected {kind}"));
        }
        NonNull::new(out).ok_or_else(|| format!("Expected {kind}"))
    }
}

/// Runs a callback body, writes its value to `out` and returns `0`; or reports its error
/// (or a caught panic) to the host and returns `1`.
///
/// Catch Rust panics inside the plugin, never at the foreign ABI boundary.
/// The plugin must build with panic=unwind for recovery; panic=abort terminates the app.
///
/// # Safety
/// `context`, `api`, and `out` must be the live host invocation pointers.
pub unsafe fn guarded(
    context: Context,
    api: *const ValuesV1,
    out: *mut Value,
    body: impl FnOnce() -> Result<Value, String>,
) -> i32 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body))
        .unwrap_or_else(|_| Err("Native plugin panicked".into()));
    match result {
        Ok(value) => {
            unsafe { *out = value };
            0
        }
        Err(error) => {
            unsafe { ((*api).error)(context, Text::new(&error)) };
            1
        }
    }
}

/// Requests the core [`VALUES_V1`] capability. Call it first in `matchbox_plugin_init_v1`
/// and return the error status unchanged if it fails.
///
/// # Safety
/// Must be called with the pointers supplied to matchbox_plugin_init_v1.
pub unsafe fn require_values(
    host: *const HostV1,
    context: Context,
) -> Result<*const ValuesV1, i32> {
    unsafe { require_capability(host, context, VALUES_V1).map(|api| api.cast()) }
}

/// Requests [`ROOTS_V1`], to keep values between calls with [`Call::retain`].
///
/// # Safety
/// Must be called with the pointers supplied to matchbox_plugin_init_v1.
/// The returned table is immutable and valid for the host process lifetime.
pub unsafe fn require_roots(
    host: *const HostV1,
    context: Context,
) -> Result<&'static RootsV1, i32> {
    unsafe { require_capability(host, context, ROOTS_V1).map(|api| &*api.cast()) }
}

/// Requests [`BYTES_V1`], for [`Call::bytes`] and [`Call::bytes_new`].
/// The returned table is valid for the host process lifetime.
///
/// # Safety
/// Must be called with the pointers supplied to matchbox_plugin_init_v1.
pub unsafe fn require_bytes(host: *const HostV1, context: Context) -> Result<&'static BytesV1, i32> {
    unsafe { require_capability(host, context, BYTES_V1).map(|api| &*api.cast()) }
}

/// Requests [`TYPES_V1`], for [`Call::kind`] and [`Call::boolean`].
/// The returned table is valid for the host process lifetime.
///
/// # Safety
/// Must be called with the pointers supplied to matchbox_plugin_init_v1.
pub unsafe fn require_types(host: *const HostV1, context: Context) -> Result<&'static TypesV1, i32> {
    unsafe { require_capability(host, context, TYPES_V1).map(|api| &*api.cast()) }
}

/// Requests [`COLLECTIONS_V1`], for the array and struct-read helpers on [`Call`].
/// The returned table is valid for the host process lifetime.
///
/// # Safety
/// Must be called with the pointers supplied to matchbox_plugin_init_v1.
pub unsafe fn require_collections(
    host: *const HostV1,
    context: Context,
) -> Result<&'static CollectionsV1, i32> {
    unsafe { require_capability(host, context, COLLECTIONS_V1).map(|api| &*api.cast()) }
}

/// Requests [`SETTINGS_V1`], for [`Call::module_settings`].
/// The returned table is valid for the host process lifetime.
///
/// # Safety
/// Must be called with the pointers supplied to matchbox_plugin_init_v1.
pub unsafe fn require_settings(host: *const HostV1, context: Context) -> Result<&'static SettingsV1, i32> {
    unsafe { require_capability(host, context, SETTINGS_V1).map(|api| &*api.cast()) }
}

/// Requests [`PATHS_V1`], for [`Call::expand_path`].
/// The returned table is valid for the host process lifetime.
///
/// # Safety
/// Must be called with the pointers supplied to matchbox_plugin_init_v1.
pub unsafe fn require_paths(host: *const HostV1, context: Context) -> Result<&'static PathsV1, i32> {
    unsafe { require_capability(host, context, PATHS_V1).map(|api| &*api.cast()) }
}

/// Requests [`INTEGERS_V1`], for [`Call::int_new`] and [`Call::integer`].
/// The returned table is valid for the host process lifetime.
///
/// # Safety
/// Must be called with the pointers supplied to matchbox_plugin_init_v1.
pub unsafe fn require_integers(host: *const HostV1, context: Context) -> Result<&'static IntegersV1, i32> {
    unsafe { require_capability(host, context, INTEGERS_V1).map(|api| &*api.cast()) }
}

/// Looks up `name`, checking the host's ABI version. Reports a missing capability to the
/// host as `Missing required capability: <name>` and returns status `1`.
unsafe fn require_capability(
    host: *const HostV1,
    context: Context,
    name: &str,
) -> Result<*const c_void, i32> {
    if host.is_null() {
        return Err(1);
    }
    let host = unsafe { &*host };
    if host.abi_major != ABI_MAJOR {
        return Err(1);
    }
    let api = unsafe { (host.capability)(Text::new(name)) };
    if api.is_null() {
        unsafe {
            (host.error)(
                context,
                Text::new(&format!("Missing required capability: {name}")),
            )
        };
        Err(1)
    } else {
        Ok(api)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! unavailable {
        ($name:ident($($ty:ty),*)) => {
            unsafe extern "C" fn $name($(_: $ty),*) -> i32 { 1 }
        };
    }
    unavailable!(string_get(Context, Value, *mut Text));
    unavailable!(number_get(Context, Value, *mut f64));
    unavailable!(number_new(Context, f64, *mut Value));
    unavailable!(string_new(Context, Text, *mut Value));
    unavailable!(bool_new(Context, bool, *mut Value));
    unavailable!(null_new(Context, *mut Value));
    unavailable!(struct_set(Context, Value, Text, Value));
    unavailable!(object_new(Context, Text, *mut c_void, DropObject, Text, *mut Value));
    unsafe extern "C" fn error(_: Context, _: Text) {}
    unsafe extern "C" fn object_data(
        ctx: Context, _: Value, _: Text, out: *mut *mut c_void,
    ) -> i32 {
        let (status, payload) = unsafe { &*ctx.cast::<(i32, *mut c_void)>() };
        if !payload.is_null() {
            unsafe { *out = *payload };
        }
        *status
    }

    #[test]
    fn object_data_requires_success_and_a_nonnull_payload() {
        let api = ValuesV1 {
            string_get, number_get, number_new, string_new, bool_new, null_new,
            struct_new: null_new, struct_set, object_new, object_data, error,
        };
        let mut payload = 42_u64;
        let pointer = (&mut payload as *mut u64).cast::<c_void>();
        for (status, output) in [(0, std::ptr::null_mut()), (0, pointer), (1, pointer), (1, std::ptr::null_mut())] {
            let mut context = (status, output);
            let call = unsafe {
                Call::new((&mut context as *mut (i32, *mut c_void)).cast(), &api, std::ptr::null(), 0)
            };
            let result: Result<NonNull<c_void>, String> = unsafe { call.object_data(1, "test.payload") };
            if status == 0 && !output.is_null() {
                let result = result.unwrap();
                assert_eq!(result.as_ptr(), pointer);
                assert_eq!(unsafe { *result.cast::<u64>().as_ref() }, 42);
            } else {
                assert!(result.is_err(), "accepted status={status} with payload={output:p}");
            }
        }
    }

    #[test]
    fn kinds_round_trip_and_unknown_codes_are_other() {
        for kind in [Kind::Null, Kind::Bool, Kind::Number, Kind::String, Kind::Array, Kind::Struct, Kind::Binary, Kind::Other] {
            assert_eq!(Kind::from_raw(kind as u32), kind);
        }
        assert_eq!(Kind::from_raw(99), Kind::Other);
    }

    #[test]
    fn published_capability_layouts_remain_fixed() {
        let pointer = std::mem::size_of::<usize>();
        assert_eq!(std::mem::size_of::<HostV1>(), 4 * pointer);
        assert_eq!(std::mem::offset_of!(HostV1, capability), pointer);
        assert_eq!(std::mem::size_of::<ValuesV1>(), 11 * pointer);
        assert_eq!(std::mem::offset_of!(ValuesV1, error), 10 * pointer);
        assert_eq!(std::mem::size_of::<RootsV1>(), 3 * pointer);
        assert_eq!(std::mem::size_of::<BytesV1>(), 2 * pointer);
        assert_eq!(std::mem::size_of::<Bytes>(), 2 * pointer);
        assert_eq!(std::mem::size_of::<TypesV1>(), 2 * pointer);
        assert_eq!(std::mem::size_of::<CollectionsV1>(), 6 * pointer);
        assert_eq!(std::mem::offset_of!(CollectionsV1, struct_keys), 5 * pointer);
        assert_eq!(std::mem::size_of::<IntegersV1>(), 2 * pointer);
        assert_eq!(std::mem::offset_of!(IntegersV1, int_get), pointer);
        assert_eq!(std::mem::size_of::<PathsV1>(), pointer);
        assert_eq!(std::mem::size_of::<SettingsV1>(), pointer);
        assert_eq!(std::mem::size_of::<Kind>(), 4);
    }
}
