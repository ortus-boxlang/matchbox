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
