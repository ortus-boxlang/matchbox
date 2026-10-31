# Prebuilt native modules (desktop)

A module remains one BoxLang package: `box.json`, `ModuleConfig.bx`, shared `bifs/` and models, JVM JARs in `libs/`, and optional prebuilt MatchBox artifacts. JVM and MatchBox backends share the module identity and public BoxLang API, not Java/Rust object layouts.

## Artifact metadata

Add an optional `matchbox.artifacts` map to `box.json`, keyed by the output target triple:

```json
{
  "slug": "bx-image",
  "type": "boxlang-modules",
  "boxlang": { "moduleName": "image" },
  "matchbox": {
    "artifacts": {
      "x86_64-unknown-linux-gnu": {
        "path": "any/package/path/libimage.so",
        "sha256": "<64 lowercase hexadecimal characters>",
        "abi": 1,
        "libraries": [
          {
            "path": "vendor/codecs/libcodec.so.2",
            "sha256": "<64 lowercase hexadecimal characters>"
          }
        ]
      }
    }
  }
}
```

Specify exactly one `path` (inside the installed package) or `url` (HTTPS). URLs are resolved by the compiler/CLI, require its `bif-http` feature, and are limited to 256 MiB. Verify the hash before execution. Standalone executables and module-bearing `.bxb` artifacts contain the selected native bytes; they never download libraries at startup. Native source layout and artifact filenames are not prescribed. No target match is an error, not a request to compile the module locally.

Publish separate artifacts for OS, architecture, and libc. Authors must choose appropriate CPU and OS/libc baselines: a target triple alone cannot guarantee those. Unbundled dependencies must already be available on the host.

### Bundled shared libraries

Each target artifact may list `libraries`, including transitive dependencies, with exactly one package-contained `path` or HTTPS `url` and a SHA-256. An optional `filename` overrides the extracted basename for both the plugin and its libraries; remote dependent libraries **require** it. Names must be single filenames and unique within the target artifact (case-insensitive on Windows/macOS). Package paths can use any directory layout, including package-contained symlinks; their original basenames are preserved.

All selected files are embedded and verified before loading, then extracted together into one content-addressed directory. The cache identity includes every filename and hash, so changing a dependency creates a new directory even when the plugin bytes are unchanged. Declaration order does not matter. No environment search paths or runtime downloads are needed.

Use the OS loader's normal relative dependency mechanism:

- **Linux:** link the plugin **and each dependent library** with a `$ORIGIN` RUNPATH, e.g. `-Wl,-rpath,$ORIGIN`. Extracted names must match `DT_NEEDED`/SONAME names.
- **macOS:** use `@loader_path` dependency install names, or `@rpath` with appropriate loader-relative paths in each library.
- **Windows:** the loader searches the extracted DLL directory and standard safe directories using `LoadLibraryEx` flags, without changing process-wide search settings.

This uses the ordinary process loader namespace, not JVM-style sibling isolation. Incompatible dependencies need distinct SONAMEs/install names/DLL names. Libraries remain loaded with their provider for the process lifetime. Linux direct, bytecode, and standalone execution are tested; macOS/Windows execution validation remains pending.

## Runtime startup

Modules are discovered in project-local `boxlang_modules/` and in `BOXLANG_HOME/modules/` when that environment variable is set; project packages and explicit selections take precedence for the same directory name. Arbitrary packages can be selected with `--module` or explicit manifest paths.
`modulesDirectory` in the project runtime configuration adds directories to scan:
relative paths resolve against the startup project/CWD, and earlier entries win.
Project/explicit selections and default home packages precede these additional
folders. Missing/blank/non-directory entries are ignored; the native loader
requires an array of strings and rejects invalid types. Discovery reads metadata and compiles descriptors without executing lifecycle hooks. Module names honor `boxlang.moduleName`. Descriptor mappings are resolved at execution; dependency arrays and enabled discovery metadata currently require literals. Dependencies initialize first; missing dependencies and cycles fail. Disabled modules are omitted. Application overrides come from `boxlang.json` → `modules.<name>.settings`.

At execution, load native providers, instantiate descriptors, call `configure()`, read the descriptor's **variables-scope `settings`**, deep-merge application settings, and register BoxLang class BIFs using their `invoke()` method. After initialization, call `onLoad()` in dependency order. Call `onUnload()` in reverse order at normal shutdown, including application errors. This does not provide cleanup after process abort/crash.

`configure()` populates `settings`; its return value is ignored. Old build-time lifecycle execution and baked settings are removed. Class BIFs honor `@BoxBIF` aliases and `@BoxMember` declarations (also supported as class-header attributes). A string alias replaces the filename name; an alias array includes the filename name. Members retain their callable independently of global aliases and bind the receiver to `objectArgument`, or the first declared argument by default, for positional and named calls. The currently supported member targets are the VM's core scalar, array, struct, date, class, and callable types; JVM-specific targets fail explicitly. Parameterless `invoke()` methods retain positional receivers and extras in `arguments` under numeric keys, as on the JVM. Named member calls require a declared first parameter or an explicit `objectArgument`, which need not be declared. Class BIFs retain undeclared positional/named values and expose `__functionName` / `__isMemberExecution`. Explicit `objectArgument` positional dispatch maps only declared parameters and drops surplus values, matching JVM member dispatch. Doc comments remain documentation, not BIF/member registration annotations, matching the JVM. Synchronous global interception supports descriptor/custom-point handlers, descriptor-declared interceptor classes, and explicitly registered class instances, shared mutable event data, Boolean-true short-circuiting, and reverse-order cleanup. Declared methods with an `@interceptionPoint` annotation discover their own point; annotation presence matters even when its value is false. Documentation tags and inherited-only methods are not discovered, matching JVM metadata registration. Module `.bx`/`.bxs` class declarations are embedded with the application, so declared interceptors do not require their source package at execution. Module components support template tags and `bx:name` script syntax. Native objects can declare member-to-BIF mappings. A module relying on unsupported JVM APIs must provide an alternative backend.

### Native HTTP hosts

Discovery/preparation lives in the shared `matchbox-modules` crate rather than
private CLI files. `WebrootRuntime::from_project(project, home, config)` starts
modules on its VM-owning thread before serving; application-end precedes module
unload on orderly disposal. The module-aware web compiler defers packaged class
references to the VM catalog. Request namespaces retain module objects/settings
and clear inline caches without serializing them.

The HTTP startup path is offline: artifact URLs and source-only Rust packages fail.
Install native bytes in the package first; publisher trust and target/hash/ABI/cache
checks still apply. Runtime configuration supports module enabled/settings and
modulesDirectory. Nested packages under each module's modules/ directory are
discovered recursively; disabled ancestors suppress their subtree. Parent descriptor
constructors/class mappings are prepared before children; child configuration and
activation precede the parent, and children unload before their parent. Literal
this.modules per-child enabled/settings overrides are supported; global configuration
wins. Dynamic overrides remain explicitly unsupported, as do dynamic enabled/dependency
metadata. Top-level selections precede duplicate nested identities. Cyclic physical
paths fail instead of recursively walking symlinks forever. Hot reload remains unsupported.
LoadedModules::mappings and WebrootRuntime::module_mappings export resolved plain-data
mapping snapshots for HTTP hosts; only external mappings may be published. The native
frontend serves public mappings as confined mounts with its existing source/config
protection; ordinary aliases to package paths remain private. discover_module_installations reports all installation
roots and physical package paths (including disabled/shadowed packages) from the
same configuration snapshot; HTTP hosts must keep these paths private, including
through aliases. The existing discover_modules APIs use the same selection logic
without requiring hosts to maintain a second directory scanner.
The standalone server crate does not select JNI; the root CLI still controls its
own optional JNI feature.

### Descriptor mappings

`this.mapping` is evaluated after descriptor construction and before `configure()`. A nonblank string creates `/bxModules/<value>/` and `bxModules.<value>`. A struct accepts `name` (blank/missing defaults to the module name), `usePrefix` (default true), `external` (default false), and `path` (relative to the package root, or absolute). Struct invocation paths remain `bxModules.<name>` even with `usePrefix:false`, matching the JVM. Mapping names have leading/trailing slashes; defaulted struct names are written back to the descriptor.

`this.publicMapping` defaults to `/bxModules/<module-name>/public/`, pointing to the package's `public/` directory. A nonblank string appends that folder to the resolved private mapping name but resolves its path against the package root. Struct definitions use the same rules as private mappings, with `external:true` by default. `moduleRecord.asStruct()` exposes both resolved definitions and the invocation path. Changing descriptor mapping fields in `configure()` does not rebind registered mappings.

Package `.bx`/`.bxs` class declarations are embedded under private identities, then bound to the resolved mapping paths at execution, before `configure()`. Surrounding script statements are not executed when loading those declarations. Constructors and imported class references work after removing the source package; named/default `init()` arguments, null-returning initializers, and static method calls/references use the shared VM invocation machinery. Only classes contained in the packaged module are embedded—mapping metadata can describe other paths, but classes from those paths are not bundled or loaded automatically. Implicit constructor arguments and constructor spreads fail explicitly until supported.

Module-root `build/`, `dist/`, and `target/` directories are authoring outputs, not class sources: their contents are neither parsed nor embedded. This lets development checkouts retain generated packages and malformed test fixtures without loading them as module classes. These names remain valid below source directories such as `models/build/`. Non-BIF `.bxs` scripts are conservatively screened for class declarations using the lexer; comments, string contents, member names and struct keys do not turn them into class sources. Candidate class scripts still undergo normal syntax validation; surrounding script statements remain unexecuted.

This is BoxLang package-class resolution, distinct from the class-provider boundary below. Public HTTP mapped mounts can serve native templates/static assets. Arbitrary application/runtime source mappings and complete BoxLang static scopes remain separate work.

### Module-owned Java class providers

After `configure()`, MatchBox registers the descriptor's variables-scope `classProviders` array. Each entry has an exact, case-sensitive `java:` class name and an ordinary zero-argument callable. Resolver prefixes and import aliases are case-insensitive. Duplicate names and invalid registrations fail; no wildcard provider or global `createObject()` replacement is involved.

```boxlang
function configure() {
    // The JVM backend need not register a MatchBox provider.
    if (boxRuntime.getFunctionService().hasGlobalFunction("myNativeClassReference")) {
        classProviders = [{
            name: "java:org.example.Counter",
            provider: myNativeClassReference
        }];
    }
}
```

The callable returns a native object, BoxLang class reference, or BoxLang instance representing the supported class. A returned BoxLang class runs its pseudo-constructor, but not `init()`, to create the façade. Native providers can use existing ABI-1 BIFs and object-to-BIF method mappings; no SDK capability was added. Factories run only during resolution at execution, never during packaging. Registrations retain their callable independently of mutable aliases and appear in `moduleRecord.classProviders` / `asStruct()`.

`createObject("java", name)`, `new java:...`, explicit Java imports, unprefixed imports of registered names, imported-class calls, and stored class references share resolution. Unprefixed resolution prefers an embedded BoxLang class. Creation returns an uninitialized reference; `init()` and `new` invoke the provided constructor. Successful initialization retains the returned instance and forwards its methods; failed/null initialization does not replace an existing instance. Static calls, `::` member syntax, bound method references, dot/index field access, and chaining use existing VM values and dispatch. BoxLang façade fields are writable; native class fields are currently read-only. References and initialized instances survive GC, including retained method references and nested callbacks.

Providers remain available through `onUnload()`, then unregister by module ownership, including partial startup and failing unload. Existing references remain usable. With any module-owned Java names registered, missing names fail without falling back to a JVM. Without those registrations, builds with the existing optional `bif-jni` feature can use the bounded JNI backend; builds without it fail explicitly. The nonfunctional `java.io.File` placeholder was removed.

This is a resolution/protocol boundary, not a Java implementation. The module owns class names, overloads, methods, fields, identity and other Java behavior. Java named constructor/method arguments, constructor spreads, JAR/class-loader arguments, unsupported members, and null constructor results fail explicitly. Implicit default construction for instance methods on uninitialized references and complete Java reflection/static scopes are not implemented. The optional [bx-matchbox-java](https://github.com/ortus-boxlang/bx-matchbox-java) module now has a bounded `java.lang.StringBuilder` starter with a shared JVM/native package and verified no-JNI/source-free execution. Other Java classes/framework compatibility and SDK publication remain deferred.

### Injected references and startup cleanup

Descriptors and class BIFs receive a live `moduleRecord`, `boxRuntime`, and `log` in their variables scope, after construction and before `configure()`/`invoke()`. BIFs also receive `functionService`; descriptors and BIFs receive `interceptorService` and `componentService`. The record exposes name, source-package path, private/public mappings, invocation path, dependencies, descriptor metadata, merged settings, class BIF/member registrations, `moduleConfig`, and activation state; `asStruct()`, `isEnabled()`, and `isActivated()` are supported. `getModuleList()` / `getModuleInfo(module)` return record snapshots sharing live settings, with an empty struct for unknown modules. Settings remain the same mutable struct visible to the descriptor and `getModuleSettings()`. Before configuration, absent `settings`, `interceptors`, `customInterceptionPoints`, and `classProviders` variables receive fresh empty defaults; unscoped configuration assignments therefore update those descriptor variables.

The core adapters currently support `boxRuntime.getFunctionService()` / `getModuleService()` / `getInterceptorService()` / `getComponentService()`, interceptor-service `register()` / `unregister()` / `announce()` / `newAndRegister(clazz, properties, name, moduleRecord)` and interception-point queries/registration, function-service `hasGlobalFunction()` / `getGlobalFunctionNames()` for registered native and class BIFs (not ordinary UDFs), module-service `getModuleRecord()` / `hasModule()` / `getRegistry()`, and single-message logger methods. Unsupported service calls fail explicitly; this is not a complete JVM service API. Component-service `hasComponent()` and `getComponentNames()` expose registered module components. Cache, async, scheduler, and datasource-service injection remain pending. `boxAnnounce(state, data, poolname)` dispatches synchronously and returns true; only the global pool is supported. Event handlers receive a thread-affine execution context retaining the caller's live local/argument scopes. `getBoxContext()` preserves context identity during a call; component bodies reuse their declaring context. Supported methods are `getVisibleScopes()` (current local/arguments), `getScopeNearby(name)`, `invokeFunction(callable[, arrayOrStructArguments])`, `getComponents()`, `findClosestComponent(name[, offset])`, and `writeToBuffer(value[, force])`. Retained contexts and their callbacks survive GC. This is not full JVM `IBoxContext` reflection, root/lexical-scope enumeration, or request/thread context support. Unknown methods fail explicitly.

Built-in `preModuleLoad`, `postModuleLoad`, `preModuleUnload`, `postModuleUnload`, `preFunctionInvoke`, `postFunctionInvoke`, and `onFunctionException` points are registered and emitted automatically. Module events expose live `moduleRecord`/`moduleName` data; function events expose context, complete mutable arguments, function, and name, with mutable results after successful calls and exception data after uncaught failures. Defaults are installed before pre-function events. Automatic events do not recursively observe their interceptor dispatch; explicit nested announcements still run. This protects BoxLang handlers from the JVM's recursive function-event dispatch and is not a claim of identical observer-stack behavior. Other JVM subsystem/BIF/template event families remain outside this slice.

Descriptor `configure()` can declare `interceptors = [{class: "interceptors.Probe", properties: {step: 2}}]`. Relative class names resolve under the module invocation path. Each entry receives default properties, a default `<qualified-class>@<module>` name, and its constructed `interceptor`. Instances receive variables-scope `name`, shared `properties`, `log`, `boxRuntime`, `interceptorService`, and `moduleRecord` before their optional `configure()`, then register before descriptor `onLoad()`. Interceptors remain registered during `onUnload()` and are unregistered afterward, including partial startup failures. Service announcement supports reentrant dispatch.

Activation is marked only after `onLoad()` succeeds. Reverse-order cleanup includes partially configured modules when startup fails, continues if an `onUnload()` hook fails, and preserves the original startup/application error.

## Module components

Top-level `components/*.bx` classes register their `invoke()` method. `@BoxComponent` uses the same filename/string/array alias rules as `@BoxBIF`. Invocation supplies named `context`, `attributes`, `body`, and `executionState` arguments, so declaration order does not matter. The live record exposes registered aliases under `moduleRecord.components`.

`@AllowsBody(true)` permits a body; `@RequiresBody(true)` both permits and requires one. Self-closing tags still supply an empty body callback, matching JVM behavior. Unknown tags and invalid body usage fail explicitly rather than silently skipping content.

```boxlang
@AllowsBody(true)
class {
    function invoke(context, attributes, body, executionState) {
        var buffer = newBuffer();
        var result = processBody(context, body, buffer);
        writeOutput("[" & buffer.toString() & "]");
        return result;
    }
}
```

Components receive the supported BIF services plus `getName()`, `newBuffer()`, `newBuilder()`, and `processBody(context, body, buffer)` helpers. `newBuffer()` captures and appends body output; a builder/non-buffer argument uses the current output context, as on the JVM. Nested captures restore their enclosing buffer, bodies may run repeatedly, and body errors restore output handling and preserve the original error. Body results support `isEarlyExit()`, `isReturn()`, `isBreak([label])`, `isContinue([label])`, `resultType()`, `returnValue()`, and `label()`. Non-local returns preserve their value through nested components and GC; components can inspect, consume, or forward break/continue results. Body-local loops still handle their own break/continue. A forwarded early exit reaches the declaring function (break/continue return null, matching the current JVM ASM backend), not a guessed caller loop.

`allowsBody` / `requiresBody` are enforced. The flag assessment leaves `ignoreEnableOutputOnly=true` / `autoEvaluateBodyExpressions=true` explicitly unsupported: they depend on JVM output-only/template-expression policies, not merely body dispatch. Their literal metadata is rejected during packaging without running descriptors. False/default flags, explicit output, captured builders/buffers, and body completion are supported. Component bodies reuse their declaring local/argument scopes, rather than introducing a function scope; root body assignments likewise update the declaring script context. Anonymous-function closures retain shared mutable declaring local and argument scopes, including nested callbacks and later declarations. Captures survive their declaring call, GC, and native retained roots; JIT loop writes remain visible through those scopes. Function assignments update existing local/argument/captured/class variables; new BoxLang function variables are local, so reentrant methods do not overwrite one another. `->` lambdas only search their own local/argument scopes. These are BoxLang function rules, not complete CFML/context parity. The context is the same live execution context used by interception. Its component stack is nearest-first, exposes live execution-state structs, and unwinds on success, failure, and early exits. Module components remain registered until VM destruction (the JVM likewise does not currently unregister components on module unload).

## Bounded foundation audit

Milestone 1 covers synchronous packaged modules, not an exhaustive JVM runtime API. The audited boundaries are:

| Boundary | Implemented contract | Explicitly deferred |
| --- | --- | --- |
| Descriptor/records | Runtime construction/configuration/settings/mappings, dependency ordering, activation/unload/error cleanup, real module inventory | Hot reload; arbitrary runtime package discovery |
| Invocation/scopes | Named/default/surplus arguments, module BIF/member metadata, shared mutable closures, declaring component bodies, lambdas isolated from captures | Complete CFML scoping, static scopes and reflection |
| Context/callbacks | Stable identities, live current local/argument scopes, callable-value array/struct dispatch, active component ancestry, output buffers, retained-context GC | Complete lexical/root/class/request scope inventories; source/template loading and thread contexts |
| Services | The runtime/module/function/interceptor/component/logger adapters listed above; unsupported methods throw | Cache/datasource/scheduler/async/executor adapters; complete descriptor metadata |
| Events | Custom announcements and the seven automatic module/function points listed above, mutable data/results, exception propagation | BIF/instance/class/template/source/configuration/application/request event families |
| Components | Aliases, script/template invocation, body validation/control/results, captures, caller context and execution-state cleanup | Source-backed CFML custom tags; the two true output-policy flags described above |
| Class resolution | Embedded packaged BoxLang catalogs plus exact module-owned Java providers across creation/import/static/property/init protocols | Arbitrary JARs/Java bytecode, full JVM reflection and runtime source mappings |

`getFunctionList()` / `getComponentList()` require descriptor reflection that is not implemented; they now throw rather than reporting an empty successful inventory. Use the supported service name queries. `boxModuleReload()` likewise throws because native libraries/modules have process lifetime. `getBaseTagList()` exposes current module-component ancestry; `getBaseTagData()` does not fabricate data for unsupported source-backed custom tags.

Three compatibility transfer tests formerly passed only because reflection/reload returned placeholders. They are now explicitly ignored with unsupported-feature reasons; the missing-custom-tag fixture now verifies the canonical JVM error instead of accepting null. These exclusions are not compatibility successes. Unrelated general-language BIFs and complete framework compatibility are not certified by this module audit.

Direct execution, source-free `.bxb` and standalone execution, retained roots/contexts and repeated GC, nested callbacks, no-JNI execution, and the unchanged image-provider contract are the exit gates. Plugin ABI-1 remains unchanged; envelope v11 requires rebuilding older development applications.

## Native SDK / ABI

Build a Rust `cdylib` with the `matchbox-plugin-sdk = "0.1"` Cargo dependency from crates.io (the SDK source lives in `crates/matchbox-plugin-sdk`). The SDK is independently versioned and used only by module authors; ForgeBox distributes the actual module and its prebuilt artifacts. Consumers need neither Cargo nor the SDK. Export `matchbox_plugin_init_v1`. The host supplies a C-layout registration interface and named capability lookup. The first capability is `matchbox.values.v1`: numbers, strings, booleans/null, structs, opaque plugin objects, and error reporting. BIF metadata declares names, aliases, argument names, required arguments, and scalar defaults. Named arguments are case-insensitive; unknown/duplicate arguments fail.

The SDK depends only on Rust's standard library and does not link MatchBox's VM. No `String`, `Vec`, `Rc`, `BxVM`, NaN-box layout, or Rust trait-object vtable crosses the ABI. SDK helpers wrap the C interface for Rust authors.

Compatibility rules:

- ABI major and each named capability's layout/signature/semantics are immutable.
- Add new operations through new capabilities; retain old capabilities for existing binaries.
- Unknown/missing required capabilities cause explicit initialization failures.
- Value handles and borrowed strings/context pointers last **one invocation**. The optional `matchbox.roots.v1` capability provides separate persistent roots; it does not extend the lifetime of invocation handles.
- Plugin payloads are opaque, plugin-owned pointers, scoped to the provider instance and type name. Ownership transfers to the host only when object creation succeeds. The host invokes the plugin's destructor when the object is collected.
- Host and plugin each free their own allocations. Initialized libraries remain loaded for the process lifetime, including when plugin state/destructors outlive a VM.
- All operations are synchronous on the VM thread. No async, unload, or hot reload.
- Catch panics inside Rust callbacks using `guarded`; never unwind across C ABI callbacks. Recovery needs `panic = "unwind"`; aborting panics terminate the process.

Plugins are trusted arbitrary native code, not sandboxed. SHA-256 pins content; it does not establish publisher trust. Cache permissions prevent other users from substituting code; same-user compromise is outside this trust model.

## Binary VM values

`matchbox.bytes.v1` is an additive capability with `get` and `new` operations. Binary data is not passed through UTF-8 strings. `get` exposes a call-scoped borrowed byte view; `new` copies plugin bytes before returning. SDK authors negotiate with `require_bytes()` and use `Call::bytes()` / `Call::bytes_new()`. Hosts lacking this capability fail initialization explicitly; existing ValuesV1/RootsV1 tables and ABI major 1 remain unchanged.

The optional [`bx-matchbox-std`](../modules/bx-matchbox-std/README.md) package starts incremental standard-library migration with raw standard streams, TCP, timeouts, buffered exact/delimited reads, and strict UTF-8 decoding. It is explicitly selectable and embeddable, not automatically baked into every runtime. Parser inspection remains a core API; see [AST inspection](ast-inspection.md).

## Retained VM values

`matchbox.roots.v1` is an additive capability; `HostV1`, `ValuesV1`, and the ABI major are unchanged, so existing prebuilt plugins need no rebuild. Plugins requiring roots request `require_roots(host, context)` during initialization; older hosts report the missing capability explicitly.

The capability has three operations: retain an invocation value as an opaque host-owned root, resolve a root into a new invocation value, and release the root. The SDK wraps them with RAII:

```rust
// roots: &'static RootsV1, obtained during plugin initialization
let saved = call.retain(roots, call.argument(0)?)?;
// Move saved into plugin-owned state. During a later invocation:
let value = saved.value(&later_call)?;
// Explicit release, or let the owning Rust payload's destructor do it:
drop(saved);
```

Roots keep the value and its ordinary GC-reachable children alive through minor and major collection, including a callback's captured receiver and lexical scopes. Root reads validate both VM and provider identity. A root does **not** keep its VM alive: reads using another VM fail, but release needs no invocation context and remains safe after VM shutdown.

All root operations, including destruction, must remain on the VM thread. The Rust wrapper is neither `Send` nor `Sync` and cannot be cloned; retaining the same value again creates an independent root. Raw C root pointers must be live, host-issued allocations and released exactly once. Strong roots cannot break cycles: do not retain a value that refers back to its own native holder unless the plugin provides an explicit release path.

## Packaging / cache

```bash
matchbox --module /path/to/package app.bxs
matchbox --build --module /path/to/package --output app.bxb app.bxs
matchbox --target native --module /path/to/package --output app app.bxs
```

Native libraries are extracted into a private content-addressed cache by target/library-group hash. Each file is published atomically across concurrent launches; every selected cached file is verified before loading. Tampered entries fail rather than being silently trusted. Set `MATCHBOX_NATIVE_CACHE` to choose the directory. Otherwise use the platform's user cache directory (or a process-specific temporary directory when no home/cache directory is available).

Source-based Native Fusion remains available with `--target native`. A source-only Rust module cannot be executed directly by the CLI; publish a prebuilt artifact or use Native Fusion. Native module envelopes currently target desktop execution, not ESP32 or WebAssembly. The module envelope format is version 11; rebuild application bundles made with the earlier development format. This does not change the native plugin ABI or require rebuilding plugin libraries.

## Checks

```bash
cargo test --test native_plugin_tests
cargo test -p matchbox_vm
cargo test -p matchbox-plugin-sdk
```

The native fixture tests module-owned Java resolution, creation/import/static/property protocols, provider validation and cleanup, class/reference/initialized-instance GC retention, runtime lifecycle, shared class BIFs, named/default arguments, object collection, structured result lifetime, retained values/callbacks, mutable lexical capture and release, reentrant method/body scopes, source-free closure execution, VM/provider isolation, root release during GC and after VM shutdown, caught plugin panics, missing capabilities, envelope packaging, transitive shared-library packaging without the source package, dependency-aware cache identity, cache reuse, concurrent extraction, staging-name collisions, and artifact rejection. The separate `bx-image` pilot runs an identical BoxLang contract against its existing JVM JAR and native provider.
