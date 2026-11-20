use matchbox_plugin_sdk::*;
use std::{
    cell::RefCell,
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
static ROOT_API: OnceLock<&'static RootsV1> = OnceLock::new();
static TYPES_API: OnceLock<&'static TypesV1> = OnceLock::new();
static COLLECTIONS_API: OnceLock<&'static CollectionsV1> = OnceLock::new();
static INTEGERS_API: OnceLock<&'static IntegersV1> = OnceLock::new();
static PATHS_API: OnceLock<&'static PathsV1> = OnceLock::new();
static SETTINGS_API: OnceLock<&'static SettingsV1> = OnceLock::new();
static DATETIME_API: OnceLock<&'static DatetimeV1> = OnceLock::new();
static STRUCTS_API: OnceLock<&'static StructsV1> = OnceLock::new();
fn structs() -> Result<&'static StructsV1, String> { STRUCTS_API.get().copied().ok_or_else(|| "Missing structs capability".into()) }
fn datetime() -> Result<&'static DatetimeV1, String> { DATETIME_API.get().copied().ok_or_else(|| "Missing datetime capability".into()) }
fn settings_api() -> Result<&'static SettingsV1, String> { SETTINGS_API.get().copied().ok_or_else(|| "Missing settings capability".into()) }
fn paths() -> Result<&'static PathsV1, String> { PATHS_API.get().copied().ok_or_else(|| "Missing paths capability".into()) }
fn integers() -> Result<&'static IntegersV1, String> { INTEGERS_API.get().copied().ok_or_else(|| "Missing integers capability".into()) }
fn types() -> Result<&'static TypesV1, String> { TYPES_API.get().copied().ok_or_else(|| "Missing types capability".into()) }
fn collections() -> Result<&'static CollectionsV1, String> {
    COLLECTIONS_API.get().copied().ok_or_else(|| "Missing collections capability".into())
}
thread_local! { static RETAINED: RefCell<Option<RetainedValue>> = const { RefCell::new(None) }; }
static DROPS: AtomicUsize = AtomicUsize::new(0);
unsafe extern "C" fn drop_class(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data.cast::<f64>()) });
}
fn number_object(call: &Call<'_>, value: f64) -> Result<Value, String> {
    let data = Box::into_raw(Box::new(value)).cast();
    match unsafe {
        call.object_new(
            "fixture.number.v1",
            data,
            drop_object,
            r#"{"info":"fixtureInfo"}"#,
        )
    } {
        Ok(value) => Ok(value),
        Err(error) => {
            unsafe {
                drop_object(data);
            }
            Err(error)
        }
    }
}
unsafe extern "C" fn class_reference(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let data = Box::into_raw(Box::new(0.0_f64)).cast();
            match call.object_new(
                "fixture.class.v1",
                data,
                drop_class,
                r#"{"init":"fixtureClassInit","answer":"fixtureClassAnswer"}"#,
            ) {
                Ok(value) => Ok(value),
                Err(error) => {
                    drop_class(data);
                    Err(error)
                }
            }
        })
    }
}
unsafe extern "C" fn class_init(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            call.object_data(call.argument(0)?, "fixture.class.v1")?;
            number_object(&call, call.number(call.argument(1)?)?)
        })
    }
}
unsafe extern "C" fn class_answer(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            call.object_data(call.argument(0)?, "fixture.class.v1")?;
            call.number_new(42.0)
        })
    }
}
unsafe extern "C" fn drop_object(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data.cast::<f64>()) });
    DROPS.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C" fn make(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            number_object(&call, call.number(call.argument(0)?)?)
        })
    }
}
unsafe extern "C" fn info(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let number = *call
                .object_data(call.argument(0)?, "fixture.number.v1")?
                .cast::<f64>().as_ref();
            let result = call.struct_new()?;
            call.struct_set(result, "value", call.number_new(number)?)?;
            Ok(result)
        })
    }
}
unsafe extern "C" fn drops(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            Call::new(ctx, api, args, len).number_new(DROPS.load(Ordering::SeqCst) as f64)
        })
    }
}
unsafe extern "C" fn panic_callback(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    _: *const Value,
    _: usize,
    out: *mut Value,
) -> i32 {
    unsafe { guarded(ctx, api, out, || panic!("fixture panic")) }
}
unsafe extern "C" fn bad_handle(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    _: *const Value,
    _: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            Call::new(ctx, api, std::ptr::null(), 0)
                .number(9999)
                .and_then(|n| Call::new(ctx, api, std::ptr::null(), 0).number_new(n))
        })
    }
}
unsafe extern "C" fn store(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let root = call.retain(
                ROOT_API.get().copied().ok_or("Missing root capability")?,
                call.argument(0)?,
            )?;
            RETAINED.with(|value| *value.borrow_mut() = Some(root));
            call.number_new(0.0)
        })
    }
}
unsafe extern "C" fn recall(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            RETAINED.with(|value| {
                value
                    .borrow()
                    .as_ref()
                    .ok_or("No retained value")?
                    .value(&call)
            })
        })
    }
}
unsafe extern "C" fn clear(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            RETAINED.with(|value| {
                value.borrow_mut().take();
            });
            Call::new(ctx, api, args, len).number_new(0.0)
        })
    }
}
unsafe extern "C" fn drop_root(data: *mut c_void) {
    drop(unsafe { Box::from_raw(data.cast::<RetainedValue>()) });
}
unsafe extern "C" fn hold(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let root = call.retain(
                ROOT_API.get().copied().ok_or("Missing root capability")?,
                call.argument(0)?,
            )?;
            let data = Box::into_raw(Box::new(root)).cast();
            match call.object_new(
                "fixture.root.v1",
                data,
                drop_root,
                r#"{"get":"fixtureHeld"}"#,
            ) {
                Ok(value) => Ok(value),
                Err(error) => {
                    drop_root(data);
                    Err(error)
                }
            }
        })
    }
}
unsafe extern "C" fn held(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let root = call
                .object_data(call.argument(0)?, "fixture.root.v1")?
                .cast::<RetainedValue>().as_ref();
            root.value(&call)
        })
    }
}
#[cfg(feature = "bundled-libraries")]
unsafe extern "C" {
    fn matchbox_fixture_codec() -> i32;
}

#[cfg(feature = "bundled-libraries")]
unsafe extern "C" fn library_value(
    ctx: Context,
    api: *const ValuesV1,
    _: *mut c_void,
    args: *const Value,
    len: usize,
    out: *mut Value,
) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            Call::new(ctx, api, args, len).number_new(matchbox_fixture_codec() as f64)
        })
    }
}

// The kind code of each element of an array argument, as a new array.
unsafe extern "C" fn kinds(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let (types, collections) = (types()?, collections()?);
            let input = call.argument(0)?;
            let result = call.array_new(collections)?;
            for index in 0..call.array_len(collections, input)? {
                let kind = call.kind(types, call.array_get(collections, input, index)?)?;
                call.array_push(collections, result, call.number_new(kind as u32 as f64)?)?;
            }
            Ok(result)
        })
    }
}
// Reads an options struct: width (number), invert (strict bool), mode (optional string), and its keys.
unsafe extern "C" fn options(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let (types, collections) = (types()?, collections()?);
            let input = call.argument(0)?;
            let width = call.number(call.struct_get(collections, input, "width")?.ok_or("width is required")?)?;
            let invert = match call.struct_get(collections, input, "invert")? {
                Some(value) => call.boolean(types, value)?,
                None => false,
            };
            let mode = match call.struct_get(collections, input, "mode")? {
                Some(value) => call.string(value)?,
                None => "block".into(),
            };
            let mut keys = call.struct_keys(collections, input)?;
            keys.sort_by_key(|key| key.to_ascii_lowercase());
            let result = call.struct_new()?;
            call.struct_set(result, "width", call.number_new(width * 2.0)?)?;
            call.struct_set(result, "invert", call.bool_new(!invert)?)?;
            call.struct_set(result, "mode", call.string_new(&mode)?)?;
            call.struct_set(result, "keys", call.string_new(&keys.join(","))?)?;
            Ok(result)
        })
    }
}
// Rows of numbers (x + y), the shape an image backend returns for pixels.
unsafe extern "C" fn grid(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let collections = collections()?;
            let (width, height) = (call.number(call.argument(0)?)? as usize, call.number(call.argument(1)?)? as usize);
            let rows = call.array_new(collections)?;
            for y in 0..height {
                let row = call.array_new(collections)?;
                for x in 0..width {
                    call.array_push(collections, row, call.number_new((x + y) as f64)?)?;
                }
                call.array_push(collections, rows, row)?;
            }
            Ok(rows)
        })
    }
}
// fixtureStructNew(ordered, caseSensitive): a struct with the keys b, A and a set in that order
unsafe extern "C" fn struct_new_fixture(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let ordered = call.boolean(types()?, call.argument(0)?)?;
            let case_sensitive = call.boolean(types()?, call.argument(1)?)?;
            let object = call.struct_new_with(structs()?, ordered, case_sensitive)?;
            for (key, value) in [("b", 1.0), ("A", 2.0), ("a", 3.0)] {
                call.struct_set(object, key, call.number_new(value)?)?;
            }
            Ok(object)
        })
    }
}
// fixtureStructFlags(struct): { ordered, caseSensitive }
unsafe extern "C" fn struct_flags_fixture(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let (ordered, case_sensitive) = call.struct_flags(structs()?, call.argument(0)?)?;
            let info = call.struct_new()?;
            call.struct_set(info, "ordered", call.bool_new(ordered)?)?;
            call.struct_set(info, "caseSensitive", call.bool_new(case_sensitive)?)?;
            Ok(info)
        })
    }
}
// fixtureIsDate(value): whether the value is a datetime
unsafe extern "C" fn is_date(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            call.bool_new(call.is_datetime(datetime()?, call.argument(0)?)?)
        })
    }
}
// fixtureDateNew(seconds, nanos, zone): a datetime from Unix seconds
unsafe extern "C" fn date_new(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let seconds = call.integer(integers()?, call.argument(0)?)?;
            let nanos = call.integer(integers()?, call.argument(1)?)?;
            let zone = call.string(call.argument(2)?)?;
            call.datetime_new(datetime()?, seconds, u32::try_from(nanos).map_err(|_| "nanos out of range")?, &zone)
        })
    }
}
// fixtureDateInfo(date): { seconds, nanos, zone }
unsafe extern "C" fn date_info(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let (seconds, nanos, zone) = call.datetime_get(datetime()?, call.argument(0)?)?;
            let info = call.struct_new()?;
            call.struct_set(info, "seconds", call.int_new(integers()?, seconds)?)?;
            call.struct_set(info, "nanos", call.int_new(integers()?, i64::from(nanos))?)?;
            call.struct_set(info, "zone", call.string_new(&zone)?)?;
            Ok(info)
        })
    }
}
// fixtureSetting(key): the value of one setting of this plugin's module, or null
unsafe extern "C" fn setting(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let key = call.string(call.argument(0)?)?;
            let settings = call.module_settings(settings_api()?)?;
            match call.struct_get(collections()?, settings, &key)? {
                Some(value) => Ok(value),
                None => call.null_new(),
            }
        })
    }
}
// fixtureExpandPath(path): the path resolved as expandPath does for the calling script
unsafe extern "C" fn expand(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let path = call.string(call.argument(0)?)?;
            call.string_new(&call.expand_path(paths()?, &path)?)
        })
    }
}
// fixtureWhole(value): the whole number read with int_get, times two, made with int_new
unsafe extern "C" fn whole(ctx: Context, api: *const ValuesV1, _: *mut c_void, args: *const Value, len: usize, out: *mut Value) -> i32 {
    unsafe {
        guarded(ctx, api, out, || {
            let call = Call::new(ctx, api, args, len);
            let n = call.integer(integers()?, call.argument(0)?)?;
            call.int_new(integers()?, n * 2)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn matchbox_plugin_init_v1(host: *const HostV1, ctx: Context) -> i32 {
    if unsafe { require_values(host, ctx) }.is_err() {
        return 1;
    }
    let roots = match unsafe { require_roots(host, ctx) } {
        Ok(roots) => roots,
        Err(status) => return status,
    };
    let _ = ROOT_API.set(roots);
    match unsafe { (require_types(host, ctx), require_collections(host, ctx)) } {
        (Ok(types), Ok(collections)) => {
            let _ = TYPES_API.set(types);
            let _ = COLLECTIONS_API.set(collections);
        }
        (Err(status), _) | (_, Err(status)) => return status,
    }
    match unsafe { require_integers(host, ctx) } {
        Ok(integers) => { let _ = INTEGERS_API.set(integers); }
        Err(status) => return status,
    }
    match unsafe { require_structs(host, ctx) } {
        Ok(structs) => { let _ = STRUCTS_API.set(structs); }
        Err(status) => return status,
    }
    match unsafe { require_datetime(host, ctx) } {
        Ok(datetime) => { let _ = DATETIME_API.set(datetime); }
        Err(status) => return status,
    }
    match unsafe { require_settings(host, ctx) } {
        Ok(settings) => { let _ = SETTINGS_API.set(settings); }
        Err(status) => return status,
    }
    match unsafe { require_paths(host, ctx) } {
        Ok(paths) => { let _ = PATHS_API.set(paths); }
        Err(status) => return status,
    }
    let host = unsafe { &*host };
    if std::env::var_os("MATCHBOX_FIXTURE_MISSING_CAPABILITY").is_some() {
        if unsafe { (host.capability)(Text::new("missing.capability.v1")) }.is_null() {
            unsafe {
                (host.error)(
                    ctx,
                    Text::new("Missing required capability: missing.capability.v1"),
                )
            };
            return 1;
        }
    }
    #[cfg(feature = "bundled-libraries")]
    if unsafe {
        (host.register)(
            ctx,
            Text::new(r#"{"name":"fixtureLibraryValue"}"#),
            library_value,
        )
    } != 0
    {
        return 1;
    }
    let functions = [
        (r#"{"name":"fixtureClass"}"#, class_reference as Invoke),
        (
            r#"{"name":"fixtureClassInit","arguments":[{"name":"class","required":true},{"name":"value","default":7}]}"#,
            class_init as Invoke,
        ),
        (
            r#"{"name":"fixtureClassAnswer","arguments":[{"name":"class","required":true}]}"#,
            class_answer as Invoke,
        ),
        (
            r#"{"name":"fixtureNew","aliases":["fixtureCreate"],"arguments":[{"name":"value","default":42}]}"#,
            make as Invoke,
        ),
        (
            r#"{"name":"fixtureInfo","arguments":[{"name":"object","required":true}]}"#,
            info as Invoke,
        ),
        (r#"{"name":"fixtureDrops"}"#, drops as Invoke),
        (r#"{"name":"fixtureKinds","arguments":[{"name":"values","required":true}]}"#, kinds as Invoke),
        (r#"{"name":"fixtureOptions","arguments":[{"name":"options","required":true}]}"#, options as Invoke),
        (r#"{"name":"fixtureGrid","arguments":[{"name":"width","required":true},{"name":"height","required":true}]}"#, grid as Invoke),
        (r#"{"name":"fixtureStructNew","arguments":[{"name":"ordered","required":true},{"name":"caseSensitive","required":true}]}"#, struct_new_fixture as Invoke),
        (r#"{"name":"fixtureStructFlags","arguments":[{"name":"value","required":true}]}"#, struct_flags_fixture as Invoke),
        (r#"{"name":"fixtureIsDate","arguments":[{"name":"value","required":true}]}"#, is_date as Invoke),
        (r#"{"name":"fixtureDateNew","arguments":[{"name":"seconds","required":true},{"name":"nanos","default":0},{"name":"zone","default":""}]}"#, date_new as Invoke),
        (r#"{"name":"fixtureDateInfo","arguments":[{"name":"date","required":true}]}"#, date_info as Invoke),
        (r#"{"name":"fixtureSetting","arguments":[{"name":"key","required":true}]}"#, setting as Invoke),
        (r#"{"name":"fixtureExpandPath","arguments":[{"name":"path","required":true}]}"#, expand as Invoke),
        (r#"{"name":"fixtureWhole","arguments":[{"name":"value","required":true}]}"#, whole as Invoke),
        (r#"{"name":"fixturePanic"}"#, panic_callback as Invoke),
        (r#"{"name":"fixtureBadHandle"}"#, bad_handle as Invoke),
        (
            r#"{"name":"fixtureStore","arguments":[{"name":"value"}]}"#,
            store as Invoke,
        ),
        (r#"{"name":"fixtureRecall"}"#, recall as Invoke),
        (r#"{"name":"fixtureClear"}"#, clear as Invoke),
        (
            r#"{"name":"fixtureHold","arguments":[{"name":"value"}]}"#,
            hold as Invoke,
        ),
        (
            r#"{"name":"fixtureHeld","arguments":[{"name":"holder","required":true}]}"#,
            held as Invoke,
        ),
    ];
    for (metadata, invoke) in functions {
        if unsafe { (host.register)(ctx, Text::new(metadata), invoke) } != 0 {
            return 1;
        }
    }
    0
}
