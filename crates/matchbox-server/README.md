# MatchBox web runtime

`WebrootEngine` is shared by the native HTTP server, external HTTP frontends and
the WASI HTTP runner. Web behavior belongs here rather than in each frontend.

## Request and response bridge

Templates can use:

- `getHTTPRequestData([includeBody=true])`: headers, method, protocol and optional
  content. Text media types return UTF-8 text; other media types return binary.
  Repeated reads use the same buffered body. Named `includeBody=false` omits it.
- `bx:header`: statusCode (100–599) or a header name/value. Repeated headers are
  preserved, including separate Set-Cookie fields. Names/values are validated;
  framing/connection headers are host-controlled and rejected.
- `bx:content`: type and reset (default true), with an optional component body.
  Reset clears the current output buffer; reset=false preserves it. `variable`
  accepts binary bytes or string-castable values; `file` reads a native file,
  infers its MIME type when unset, ignores prior output and optionally deletes
  it after a successful read. Both terminate the request and invoke onAbort.
- `location(URL[, addToken, statusCode=302])` and `bx:location`: validated Location
  header, status 301–399, then request abort. addToken is accepted but unused,
  matching current MiniServer.
- `abort;`, `bx:abort`, and `bx:abort type="page"`: request abort bypasses catch
  clauses, runs finally blocks, skips request-end and invokes onAbort. Page abort
  ends only the current template/include; showError on a request abort invokes
  onAbort followed by onError with a Custom exception.

Native template routes accept methods such as PUT, PATCH, DELETE and OPTIONS;
static resources retain GET/HEAD/POST handling. TRACE/TRACK are rejected. HEAD
retains the representation's Content-Length without sending a body; 204 and
304 responses do not send body bytes.

## Installed native modules

Native hosts can eagerly create `WebrootRuntime::from_project(project, home, config)`
before binding their HTTP listener. Defaults are CWD project discovery and explicit
BOXLANG_HOME or ~/.boxlang module lookup; hosts can supply home/config paths without
changing environment. No directories or JVM installation are created. Plain
`WebrootRuntime::default()` / `WebrootEngine::new()` remain explicitly unconfigured,
so embedded hosts/tests do not silently scan the host filesystem.

`matchbox-modules` now owns discovery/preparation shared with the CLI. Project
boxlang_modules, box.json local dependencies, matchbox.toml selections and home
modules use the existing name/dependency/enable/settings rules. Project selections
win over same-directory-name home packages. Configuration defaults to project
boxlang.json; an explicit configuration accepts only modules enabled/settings.
The MiniServer-compatible frontend wires configPath/BOXLANG_CONFIG/.boxlang.json
and serverHome precedence. Other runtime configuration and modulesDirectory remain
unsupported; home config/boxlang.json is not applied and emits a warning.

The VM's `LoadedModules` separates initialization from reverse-order unload;
`Application::run()` uses that same lifetime API. Descriptors configure once,
settings deep-merge, providers/BIFs/members/classes/components/interceptors stay
live across requests and GC. Modules load before Application.bx; application-end
runs before module unload. Failed startup cleans partial records and preserves
the original error. Background startup work is rejected before listener publication.

Runtime artifacts must be local, matching-target prebuilt packages. Hash/ABI/path/
cache checks remain unchanged; source-only Rust and remote artifact URLs fail
instead of compiling/downloading at server startup. Server dependencies no longer
enable JNI; the root CLI's explicitly selected JNI feature remains unchanged.
Modules can supply bounded Java facades, not arbitrary JVM classes/JAR support.

Hosts must protect `module_paths()` (including aliases) from direct HTTP serving.
Module public mappings/assets, nested module discovery and hot reload are not wired
here. Module BIFs/classes and component bodies execute with process privileges.

## Application.bx lifecycle

The nearest `Application.bx` between the selected template's directory and its
mount root supplies a fresh listener instance per request. Its pseudo-constructor
runs, but its `init()` method is not called. Request scopes are available before
construction. Same-name applications share live BoxLang values, including arrays,
closures and instances, not serialized snapshots. Unnamed descriptors use their
source identity.

Implemented hooks:

- `onApplicationStart()` once per application lifetime; failed startup is retried.
- `onRequestStart(target)`; null/no return continues, false skips the request.
- `onRequest(target)` override; otherwise execute the template in the listener's
  variables/method context. Dynamic includes preserve that context and the caller.
- `onRequestEnd(target)`, including after false start; unhandled errors skip it.
- `onError(error, event)` with native message/type/detail fields and an empty event.
- `onMissingTemplate(target)` for the primary template; true handles it, false
  returns 404 without calling request-end.
- `onAbort(target)` for request aborts, redirects and file/variable content.
  Aborts in onError are ignored; a constructor abort has no listener hooks yet.
- `onApplicationEnd(applicationScope)` at expiry or orderly runtime disposal.

`this.name` and `this.applicationTimeout` are applied. Timeout defaults to zero
(no expiry) and uses MatchBox's seconds-valued `createTimeSpan()` result. Native
idle cleanup runs about once a second; WASI checks expiry on subsequent requests.
Only an explicit runtime drop can run shutdown hooks in WASI; component teardown
or process kill does not promise a hook.

Native hosts keep a VM on a dedicated thread. Requests are serialized within a
shared runtime; slow application code blocks its other template requests. WASI
uses thread-local VM ownership. No heap values cross threads. Detached fibers
or native futures cannot outlive a request: they fail explicitly, remain safely
rooted until native senders finish, and cannot execute in a later namespace.
Dynamic includes are capped at 16 to protect the interpreter stack; static
include compilation retains the compiler's existing behavior.

## Host API

`handle(request)` remains available for hosts without connection metadata.
HTTP hosts should use `handle_with_context(request, WebrootContext { ... })` to
supply the actual protocol, peer address/port, listener port, raw query string,
original URI, scheme, and optional public script name. The resolved asset path
stays in `request.path`; alias/rewrite metadata is separate. CGI also exposes
incoming headers through `http_*` keys. Proxy trust decisions belong to the host,
not to incoming X-Forwarded-* values automatically.

Use a shared `WebrootRuntime` with `WebrootEngine::with_runtime()` for mounts
that must share named applications. `WebrootEngine::new()` owns a separate
runtime. Asset stores must be Send + Sync and can implement `source_path()` to
supply a confined physical filename for native compilation and relative imports.
The native frontend shares one runtime across its aliases.

The VM's `with_isolated_globals()` restores bindings/output/control state on
success, errors and panics. Escaping heap values require `retain_value()` roots.
Global inline caches are cleared and unreachable request objects collected at
the boundary. Web BIFs resolve the active request rather than capturing an old
request if a callable is retained in application state.

`WebrootResponse.headers` is now an ordered `Vec<(String, String)>`, not a map.
Adapters must append these fields instead of inserting/replacing them. Do not
construct a response with a default content type before appending the runtime's
headers. The native and WASI adapters are updated together.

The WASI HTTP API does not provide a connection's protocol version or peer
address; those fields are empty there. Authority, scheme, URI, query, method and
body are preserved where available.

Component body-result callbacks are shared across native/WASM in the VM's
`register_component_runtime()`, which also remains the module runtime's path.
Unknown web component names still delegate to the existing component dispatcher.

## Scope and checks

This is **not** full MiniServer runtime parity. Module public/source mappings,
nested discovery, Application.bxm, full application configuration/output modifiers,
external session-cache providers and SocketBox remain
follow-up work. Native HTTP hosts can call multipart::parse after enforcing their
body limit: repeated fields/arrays and private temporary files populate form.
Files are removed at request completion, not deferred until VM garbage collection.
Multipart parsing is native-only and accepts UTF-8 text, at most 1000 parts and
RFC-length boundaries. FileUpload/FileUploadAll support field selection, metadata,
extension/MIME filters, strict handling and error/skip/overwrite/makeunique conflicts.
Original filenames must be portable basenames. Destination writes are staged and
committed atomically; errors never truncate an existing file. Unknown MIME uses
application/octet-stream. attributes/mode are explicitly unsupported. Relative
non-dot destinations resolve under the OS temp directory; dot-relative paths use
the current source. Full runtime security configuration remains pending; listener
allowedFileOperationExtensions/disallowedFileOperationExtensions are respected. apply_proxy_headers is an explicit opt-in for hosts behind
proxies that overwrite X-Forwarded-*; it never infers trust from header presence. Dynamic include failures do not yet carry the
full JVM missing-include exception taxonomy. Descriptor lookup stops at each
mount, without falling back from an alias to the primary webroot's descriptor.

Native hosts can use `static_files::static_file_response()` after authorizing a
filesystem path. It streams with 64 KiB read chunks, Last-Modified/date preconditions,
single byte ranges, If-Range and HEAD lengths. POST retains MiniServer's static
behavior. ETags and multipart ranges are not provided. The native frontend uses
this helper after its existing path/source/config checks, including aliases and
static welcome files. This is not a path-confinement API.

Template/content responses remain buffered. Binary content with reset=false is
emitted before the retained text buffer (matching current MiniServer). Native
file paths are relative to the current source, mount-relative for /paths, or
existing absolute filesystem paths. Custom application/alias mappings are not
applied. Files require a filesystem-backed mount; embedded/WASI asset files are
not supported by this operation. File reads are fully buffered; deleteFile runs
after a successful read but before network delivery, not after client receipt.
Trusted templates can deliberately serve files outside the public root, just as
with fileRead(); never pass unvalidated user paths. Read/delete errors propagate.

Native finally blocks always run on abort. MiniServer 1.16's ASM backend skips
finally when the same try also has catches; its Java backend runs finally. We
preserve cleanup rather than reproducing that ASM defect. Abort types other than
request/page fail explicitly; exit-tag/template/loop are not implemented.

Sessions default off. `sessionManagement=true` enables live typed native/memory
state with application isolation and sharing across same-name mounts. jsessionid,
sessionCookie/setClientCookies, start/end/expiry, rotate/invalidate/startTime are
supported. Session-end precedes application-end; failed starts retry and preserve
the cookie on a redacted 500 response. JEE/external cache storage is rejected.
The obsolete string/MBX_SESSION_ID path is removed. Cookies support validated
scope assignments and bx:cookie, including repeated response fields. Persistent
VM symbol/shape tables and session entry counts lack a bounded eviction policy. This is not recommended
for internet-facing production traffic. CFML and `.bx` remote class execution
are excluded from the native frontend's release requirements.

References: BoxLang MiniServer's exchange implementation and BoxLang web-support's
Header, Content, GetHTTPRequestData and CGI scope implementations.

```sh
cargo test -p matchbox_server
cargo test --test host_lifecycle_tests --test request_scope_tests
cargo check -p matchbox_wasi_http_runner --target wasm32-wasip2
```

`tests/webroot_http.rs` and `tests/webroot_application.rs` exercise the
compiler/VM/runtime, live state, lifecycle, cleanup, alias sharing and concurrency.
`tests/webroot_modules.rs` covers persistent catalogs/classes/members/components,
GC/source removal, native artifact Java facades, startup failure cleanup and offline
rejection. The CLI's existing native-plugin regression suite exercises the same loader.
`tests/webroot_response.rs` covers typed abort unwinding, page includes, binary/
file responses, deletion validation, constructor redirects and hook ordering.
`tests/http_transport.rs` launches the actual HTTP executable and sends requests
on a TCP socket, including HTTP/1.0 and repeated response headers.
