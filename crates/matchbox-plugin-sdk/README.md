# MatchBox Plugin SDK

A dependency-free Rust SDK for independently built native plugins for the [MatchBox BoxLang runtime](https://github.com/ortus-boxlang/matchbox). A versioned C ABI separates plugins from VM internals and Rust compiler versions.

**Distribution:** this SDK is a build-time Cargo dependency for Rust authors. Publish the actual BoxLang module through **ForgeBox**, with JVM JARs, shared BoxLang code, and prebuilt native libraries in the same package. Consumers do not need Cargo or this SDK.

## Build a plugin

Create a Rust library with this manifest:

```toml
[package]
name = "my-native-provider"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
matchbox-plugin-sdk = "0.1"

[profile.release]
panic = "unwind"
```

Export `matchbox_plugin_init_v1` and register callbacks. For example, `src/lib.rs`:

```rust
use matchbox_plugin_sdk::*;
use std::ffi::c_void;

unsafe extern "C" fn double(
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
            call.number_new(call.number(call.argument(0)?)? * 2.0)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn matchbox_plugin_init_v1(host: *const HostV1, ctx: Context) -> i32 {
    unsafe {
        if let Err(status) = require_values(host, ctx) {
            return status;
        }
        ((*host).register)(
            ctx,
            Text::new(r#"{"name":"nativeDouble","arguments":[{"name":"value","required":true}]}"#),
            double,
        )
    }
}
```

`cargo build --release` produces a platform shared library. Module authors prebuild one artifact per supported target triple and put its package-relative path, SHA-256, and ABI major in `box.json` → `matchbox.artifacts`. Artifact paths are not tied to a particular directory layout. MatchBox can embed these bytes in standalone applications.

The [native module guide](https://github.com/ortus-boxlang/matchbox/blob/feature/native-modules/docs/native-plugins.md) covers artifact metadata and packaging. See [API documentation](https://docs.rs/matchbox-plugin-sdk) for the complete interface.

## Versioning and ownership

The SDK has its own Cargo version, independent of MatchBox releases. Binary compatibility comes from immutable ABI majors and named capabilities, not matching Cargo versions or Rust toolchains:

- `matchbox.values.v1`: call-scoped values, scalar/string operations, structs, opaque native objects, and errors.
- `matchbox.roots.v1`: optional retained VM values with explicit ownership and an RAII Rust wrapper.
- `matchbox.bytes.v1`: optional binary-value access. `require_bytes()` negotiates the immutable table; `Call::bytes()` copies binary data and `Call::bytes_new()` creates a host-owned copy. Raw `get` views last until the invocation ends; raw `new` copies before returning. NUL and non-UTF-8 bytes are preserved. Existing ABI-1/ValuesV1/RootsV1 layouts are unchanged.
- A newer SDK may request a capability an older host lacks; initialization fails clearly rather than guessing a layout.
- Existing capability layouts/signatures/semantics do not change. Add functionality through a new capability name/version.

Value handles, context pointers, and borrowed strings expire at the end of each invocation. Request `require_roots()` during initialization to obtain the immutable roots table; `Call::retain()` creates an owned `RetainedValue` for use between calls. Release needs no live invocation and remains valid after VM shutdown. Reads are restricted to the original VM and provider. Roots do not keep the VM alive; avoid strong cycles involving the owning native object.

`Call::object_data()` returns `NonNull<c_void>` after checking the host status and payload. Rust callers use `.cast::<T>().as_ref()` or `.as_mut()` inside `unsafe`; correct payload type, exclusive mutable access, and invocation-bounded lifetimes remain their responsibility. This changes the Rust helper's source API, not the `ValuesV1` C ABI or compatibility with existing compiled plugins.

All operations, including root destruction, are synchronous on the VM thread. Rust-owned payloads remain private to the plugin and are destroyed by its registered drop callback. Each side frees its own allocations. No Rust trait object, `String`, `Vec`, or VM/NaN-box layout crosses the ABI.

Use `guarded()` to catch Rust panics inside callbacks; never unwind across the C ABI, including initialization and destructors. Recovery requires `panic = "unwind"`; `panic = "abort"` terminates the process. Plugins are trusted native code, not sandboxed. No async callbacks, unload, or hot reload are provided.

## SDK development and publication

Inside a MatchBox checkout, use the workspace SDK directly. An external module can test an unpublished SDK without changing its versioned dependency:

```bash
cargo test --manifest-path native/Cargo.toml \
  --config 'patch.crates-io.matchbox-plugin-sdk.path="/absolute/path/to/matchbox/crates/matchbox-plugin-sdk"'
```

To release the SDK independently, from the MatchBox repository root:

```bash
cargo test -p matchbox-plugin-sdk
cargo publish -p matchbox-plugin-sdk --dry-run
cargo publish -p matchbox-plugin-sdk
```

Publishing requires an authenticated crates.io account (`cargo login`). Check the package contents and SDK version before upload; published versions are immutable. Use Cargo's normal version constraints in module repositories, keeping local development overrides out of published packages.

## License

Apache-2.0.
