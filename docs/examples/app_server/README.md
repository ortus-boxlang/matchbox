# MatchBox App Server Example

This example demonstrates the routed MatchBox app server API built around `web.server()` and ColdBox-style handlers:

```boxlang
function( event, rc, prc ) { ... }
```

App-server scripts should import the namespace explicitly:

```boxlang
import boxlang.web;
```

It is a different product from the static/BXM webroot server. Use this app server when you want to build:

* JSON APIs
* lightweight microservices
* middleware pipelines
* signed webhook receivers
* server-rendered endpoints with explicit template rendering
* apps that need explicit static asset mounts inside a routed server

This example is for the native server runtime. ESP32 builds currently support only the lean routed direction and reject heavier features like `app.listen()`, templates, static asset mounts, webhooks, cookies, and sessions at compile time.

For a websocket-focused example, see [websocket_counter](/home/jacob/dev/ortus-boxlang/matchbox/docs/examples/websocket_counter/README.md).

## Project Structure

* `app.bxs`: Main routed application.
* `public/site.css`: Static asset served through app middleware.
* `views/home.bxm`: Template rendered by `event.setView()`.

## Key Concepts

### 1. The Handler Shape

Each route handler receives three arguments:

* `event`: A native request/response object with ColdBox-style helpers.
* `rc`: The public request collection. Route params, query params, form fields, and top-level JSON object keys are merged here.
* `prc`: The private request collection for passing internal state across middleware and handlers.

### 2. Middleware

Middleware uses a fourth `next` argument:

```boxlang
app.use( function( event, rc, prc, next ) {
    prc.requestStarted = true;
    next.run();
} );
```

Middleware can mutate `rc`, `prc`, `session`, headers, cookies, status codes, and response bodies.

### 3. Templates

Routes can render templates explicitly:

```boxlang
event.setView( "views/home.bxm", {
    "title": "MatchBox"
} );
```

Templates receive:

* `event`
* `rc`
* `prc`
* `session`
* `viewArgs`

### 4. Static Files Middleware

Static assets can be mounted explicitly inside the app server:

```boxlang
app.use( app.middleware.buildStaticFiles( "/assets", "public" ) );
```

This serves files from the `public` directory at `/assets/...`. The directory is resolved relative to the app script root, missing files fall through to the normal app routing path, and traversal outside the mounted directory is blocked.

### 5. Webhook Builder

Webhook endpoints can be declared with a fluent builder:

```boxlang
app.webhook(
    app.buildWebhook()
        .path( "/webhooks/stripe" )
        .secret( env.STRIPE_SECRET )
        .signatureHeader( "stripe-signature" )
        .prefix( "sha256=" )
        .timestampHeader( "stripe-timestamp" )
        .toleranceSeconds( 300 )
        .replayHeader( "stripe-event-id" )
        .replayTtlSeconds( 300 ),
    function( event, rc, prc ) {
        event.renderText( "ok" );
    }
);
```

## Application lifetime and modules

The native routed host evaluates the app script once, before publishing its listener. Routes and middleware are startup configuration; attempts to register them during a request fail. Restart after editing the app script.

Installed modules use the existing native module loader: project `boxlang_modules`, explicit `BOXLANG_HOME/modules`, and project/runtime `modulesDirectory` configuration. `--config` selects the module configuration file. Module settings, private mappings and lifecycle hooks are applied before the script runs. Native artifacts must already be installed; startup does not compile or download providers. Package installation/source paths remain private even if a static mount aliases them. Mount only a dedicated public/assets directory, not application source or credentials.

HTTP handlers and configured WebSocket listeners share one thread-owned VM. Declare shared mutable structs or instances at startup. Request bindings are isolated and collected between calls; do not retain `event`, `rc`, `prc` or session data in application state. Modules unload at orderly runtime disposal.

## Background work without blocking HTTP

Use a separate worker script for slow commands rather than detaching a request fiber. `app.worker(scriptPath[, onComplete])` starts a supervised module-aware VM with its own module instances and application-lifetime state. Only JSON-compatible data crosses the boundary; closures, HTTP contexts and live BoxLang objects do not.

For example, `worker.bxs`:

```boxlang
function onJob(required struct payload) {
    // Run bounded work here; this is not the HTTP VM.
    return { value: payload.value + 1 };
}
```

Register and use it in the app script:

```boxlang
import boxlang.web;
app = web.server();
state = { latest: {} };
worker = app.worker("worker.bxs", function(record) {
    state.latest = record;
    // This callback runs in the HTTP/WebSocket VM and can publish updates.
});
app.post("/jobs", function(event, rc, prc) {
    id = worker.submit({ value: 41 });
    event.setHTTPStatus(id == "" ? 409 : 202);
    event.renderJson({ operationId: id });
});
app.get("/jobs/latest", function(event, rc, prc) {
    event.renderJson(worker.status());
});
app.listen({ host: "127.0.0.1", port: 8090 });
```

These routes are illustrative, **not an authenticated control API**. Add authentication, Host/Origin policy, CSRF protection, validation and application-specific confirmations before exposing side effects.

Each worker accepts one active job; `submit` returns an empty string while busy, without queuing another operation. Accepted submissions return a random ID. `status()` and the completion callback expose the latest `{ id, status, result, error }` record (`idle`, `running`, `succeeded`, `failed`). No history or durable job storage is provided. Errors are sanitized; catch known application errors inside `onJob` if a safe structured outcome is needed. Failure does not prove that an external side effect was rolled back. Never automatically replay an uncertain job.

Submissions during startup or shutdown are rejected. On orderly disposal, accepted jobs finish and worker modules unload before parent modules. The native routed executable handles Ctrl-C and SIGTERM by stopping admission, closing WebSockets and draining work. Abrupt termination/crash cannot guarantee cleanup or that external commands stopped. Give external commands their own deadlines; shutdown can wait for them. Stopping this server does not undo external resources.

## Periodic callbacks and request security helpers

`app.every(milliseconds, callback)` registers a startup-only timer in the shared routed VM. Intervals range from 100 ms to 24 hours, with at most eight timers. Callbacks are serialized with HTTP/WebSocket handlers and missed ticks are coalesced. Submit slow work to a managed worker instead of blocking a timer.

`event.getHTTPContent(true, true)` decodes strict JSON: duplicate keys and trailing documents are rejected, and object keys retain exact casing in case-sensitive structs. Use bracket access such as `body["name"]` for exact wire keys; validate an explicit field allowlist. The default single-argument decoder retains ordinary BoxLang struct semantics.

`event.getHTTPHeaderCount(name)` counts repeated header fields before header-value flattening. Reject ambiguous authority headers in application middleware. `web.secureEquals(actual, expected)` compares SHA-256 digests in constant time; use it for process-local capabilities, sessions and CSRF tokens instead of ordinary case-insensitive language equality.

## Authorization and bounds

Application middleware runs **before** a WebSocket upgrade, using the original request headers. A middleware denial is returned as HTTP rather than accepting a socket and closing it later. Enforce authentication and Host/Origin there; `onConnect` is too late for handshake authorization. Connection admission is bounded to 16 per listener.

Native routed request bodies are limited to 16 KiB, including streamed/chunked bodies. The runtime queue holds at most 32 messages; overloaded HTTP dispatch returns 503. WebSocket messages/frames are capped at 64 KiB, outbound queues at 16 messages, and blocked sends at ten seconds. Workers are limited to eight per application, with 64 KiB JSON input and 1 MiB JSON results. Worker failure/outcome records are memory-only and submissions are never retried by the host.

The routed host and its managed workers are native-server facilities, not a promise of WASI/ESP32 parity or complete JVM executor semantics. The separate webroot runtime has its own request/application lifecycle. Treat native modules as trusted operator-authority code, not a sandbox. This is not certification for internet-facing multi-tenant hosting.

## Running the Example

From the project root:

```bash
cargo run -p matchbox_server -- --app docs/examples/app_server/app.bxs
```

Then visit:

* `http://localhost:8090/`
* `http://localhost:8090/api/hello/MatchBox`
* `http://localhost:8090/assets/site.css`

## What The Example Covers

* app-level middleware
* route groups
* cookies
* in-memory session state
* JSON responses
* static asset serving with `app.middleware.buildStaticFiles()`
* template rendering with `setView()`
* signed webhook route registration
* for websockets, see the dedicated websocket counter example
