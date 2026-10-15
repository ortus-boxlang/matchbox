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

## Host API

`handle(request)` remains available for hosts without connection metadata.
HTTP hosts should use `handle_with_context(request, WebrootContext { ... })` to
supply the actual protocol, peer address/port, listener port, raw query string,
original URI, scheme, and optional public script name. The resolved asset path
stays in `request.path`; alias/rewrite metadata is separate. CGI also exposes
incoming headers through `http_*` keys. Proxy trust decisions belong to the host,
not to incoming X-Forwarded-* values automatically.

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

This is the HTTP bridge, **not** full MiniServer runtime parity. Application.bx
lifecycle/module discovery, MiniServer-equivalent sessions, redirects/aborts,
file/variable content, cookie-scope response writes, multipart parsing, and
SocketBox remain follow-up work. Existing process-local string sessions are
unchanged and unsuitable for unbounded production traffic. CFML and `.bx`
remote class execution are not release requirements for the native frontend.

References: BoxLang MiniServer's exchange implementation and BoxLang web-support's
Header, Content, GetHTTPRequestData and CGI scope implementations.

```sh
cargo test -p matchbox_server
cargo test --test host_lifecycle_tests
cargo check -p matchbox_wasi_http_runner --target wasm32-wasip2
```

`tests/webroot_http.rs` exercises the compiler/VM/runtime together.
`tests/http_transport.rs` launches the actual HTTP executable and sends requests
on a TCP socket, including HTTP/1.0 and repeated response headers.
