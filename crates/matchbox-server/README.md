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
  Reset clears the current output buffer; reset=false preserves it. File and
  variable attributes are not implemented and fail explicitly.

Native template routes accept methods such as PUT, PATCH, DELETE and OPTIONS;
static resources retain GET/HEAD/POST handling. TRACE/TRACK are rejected. HEAD
retains the representation's Content-Length without sending a body; 204 and
304 responses do not send body bytes.

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

This is **not** full MiniServer runtime parity. Module discovery, Application.bxm,
full application configuration/output modifiers, abort hooks, session lifecycle,
redirects/aborts, file/variable content, cookie-scope writes, multipart and
SocketBox remain follow-up work. Dynamic include failures do not yet carry the
full JVM missing-include exception taxonomy. Descriptor lookup stops at each
mount, without falling back from an alias to the primary webroot's descriptor.

Application.bx defaults to sessions disabled. `sessionManagement=true` fails
explicitly until application-isolated sessions exist; it must not silently use
the legacy session map. Sites without a descriptor retain the previous
process-local string sessions, still without expiry/size bounds. Persistent VM
symbol/shape tables also lack a bounded eviction policy. This is not recommended
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
`tests/http_transport.rs` launches the actual HTTP executable and sends requests
on a TCP socket, including HTTP/1.0 and repeated response headers.
