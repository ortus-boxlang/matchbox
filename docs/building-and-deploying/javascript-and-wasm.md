# JavaScript + WebAssembly

MatchBox can compile BoxLang for browser-oriented WebAssembly workflows. There are three distinct modes, chosen to match different use-cases.

| Mode | Command | Use case |
| :--- | :--- | :--- |
| **JS Module (AOT)** | `--target js` | Ship a pre-compiled browser app/library as an ES module |
| **WASI binary (AOT)** | `--target wasm` / `--target wasi` | A `.wasm` module for WASI runtimes, not for browsers |
| **Runtime (JIT-like)** | Build `pkg/` once, call `run_boxlang()` | Execute source strings at runtime in the browser |

---

## Mode 1: JavaScript ES Module (AOT)

This is the recommended way to ship a BoxLang application to the browser today. MatchBox compiles your script to bytecode, embeds it in a WASM binary, and wraps it in a JavaScript ES module that bootstraps the runtime automatically.

### Compile

```bash
matchbox --target js my_lib.bxs
# Produces: my_lib.js  +  my_lib.wasm
```

### Use in HTML

```html
<!DOCTYPE html>
<html>
<head><title>BoxLang App</title></head>
<body>
<script type="module">
    import { greet, calculate } from './my_lib.js';

    const result = await greet("Developer");
    document.body.textContent = result;
</script>
</body>
</html>
```

### Current Runtime Contract

The generated module is browser-focused and currently assumes a real browser environment for the highest-usability path.

- Importing `./app.js` initializes the runtime automatically.
- Exported top-level BoxLang functions are available as ES module exports.
- The bundle also registers `window.MatchBox.modules["app"]`.
- `window.MatchBox.ready("app")` resolves when the module is ready.
- `window.MatchBox.State(moduleName, options)` creates a framework-agnostic state helper for state-snapshot style apps.

This browser contract is the supported path today. Non-browser runtimes may work for simple imports, but they are not the documented target surface yet.

### Exporting Functions

BoxLang functions defined at the top level of your script are automatically exported. Access modifiers have no effect on WASM exports — all functions are accessible:

```boxlang
// my_lib.bxs

function greet(name) {
    return "Hello, " & name & "!"
}

function calculate(a, b) {
    return a * b
}
```

### BIFs in WASM Builds

Native BIF groups are Cargo features of `matchbox_vm`, and a WASM build only contains the groups it was compiled with. The default feature set of a plain `js` build is empty, so groups such as `hash()` are absent unless the runtime is built with them.

| Group (feature) | BIFs | In the browser |
| :--- | :--- | :--- |
| `bif-crypto` | `hash`, `hash40`, `hmac`, `generateSecretKey`, `generatePBKDFKey`, `encrypt`, `decrypt` | Compiles for `wasm32-unknown-unknown` with `js` (pure Rust digests and ciphers; random bytes come from the `js` feature's `getrandom` backend). Not part of the default WASM build: add `bif-crypto` to the features of the runtime you ship, for example `cargo build --target wasm32-unknown-unknown --features js,bif-crypto`. |
| `bif-http` | `http()` | Works in the browser, backed by `fetch`. See [`http()` in the browser](#http-in-the-browser) for the options it supports. |
| `bif-io`, `bif-cli`, `bif-jni`, `bif-zip`, `bif-datasource` | filesystem, terminal, Java, archive and database BIFs | Not available in the browser. |

A hosted playground that builds the runtime with default features off therefore has no `hash()` or `http()`: this is a build choice, not a runtime limitation, and each can be offered by enabling `bif-crypto` or `bif-http` in that build.

---

## Mode 2: WASI Binary (AOT)

`--target wasm` (the same as `--target wasi`) produces a `.wasm` module built for WASI preview1 runtimes such as Wasmtime or Node's `node:wasi`. It is not a browser module: for the browser use Mode 1 (`--target js`) or Mode 3.

```bash
matchbox --target wasm my_app.bxs
# Produces: my_app.wasm
```

See [WASM container deployment](wasm-container.md) for running it.

---

## Mode 3: Runtime Mode (Dynamic Execution)

In runtime mode you ship the MatchBox engine itself (`pkg/`) and execute BoxLang source code dynamically at run time — similar to a JIT. This is useful for:

- Allowing user-provided BoxLang scripts in your application.
- Interactive BoxLang playgrounds.
- Server-side rendering on a WASM-capable edge runtime.

### HTML Integration

```html
<script type="module">
    import init, { run_boxlang } from './pkg/matchbox.js';

    await init();   // load and compile the WASM runtime

    run_boxlang(`
        name = "Browser"
        println("Hello from BoxLang running in #name#!")
    `);
</script>
```

### Persistent VM (calling functions by name)

For apps that need to call multiple BoxLang functions efficiently, use the persistent `BoxLangVM` instance rather than re-initialising on every call:

```js
import init, { BoxLangVM } from './pkg/matchbox.js';

await init();

const vm = new BoxLangVM();

// Load a script once  
vm.load_source(`
    function add(a, b) { return a + b }
    function greet(name) { return "Hello, " & name }
`);

// Call functions by name as many times as you like
const sum     = vm.call("add",   [10, 20]);
const message = vm.call("greet", ["BoxLang"]);

console.log(sum, message);
```

---

## JavaScript Interop from BoxLang

When BoxLang code is running inside the browser JS target, it can access a browser bridge through the `js` global:

```boxlang
// DOM access
title = js.document.title
js.document.getElementById("output").innerText = "Updated by BoxLang"

// Browser APIs
js.alert("Hello!")
js.console.log("Logged from BoxLang")

// Location
url = js.location.href
```

The important current rule is:

- Plain JS values cross the boundary as BoxLang values: strings, booleans, null, numbers, arrays, and plain objects.
- Browser host objects stay as JS handles: DOM nodes, `document`, `window`, functions, and other browser-native objects.

That keeps state/data interop predictable while preserving access to real browser objects.

> `js.*` is a browser-only surface. Using it in a native build throws a runtime error.

### Network requests with `fetch`

`js.fetch(...)` works like the browser function. A JS Promise has `.get()`, which waits for the Promise and pauses only the BoxLang call that used it. The page and other calls keep running.

```boxlang
function getJson(url) {
    var res = js.fetch(url).get()
    return res.json().get()
}

function postJson(url, token, payload) {
    var res = js.fetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/json", "Authorization": "Bearer " & token },
        body: serializeJSON(payload)
    }).get()
    return res.json().get()
}
```

A network error throws, so `try`/`catch` works as usual.

### Reading a stream

Read a response body chunk by chunk to show output as it arrives. `new js.TextDecoder()` turns the bytes into text:

```boxlang
function streamText(url) {
    var res = js.fetch(url).get()
    var reader = res.body.getReader()
    var decoder = new js.TextDecoder()
    var all = ""
    while (true) {
        var chunk = reader.read().get()
        if (chunk.done) {
            break
        }
        all &= decoder.decode(chunk.value)
    }
    return all
}
```

### `new js.X(...)`

`new js.TextDecoder()`, `new js.URL(address)` and `new js.a.b.Thing(1)` construct JavaScript objects. A class path that starts with `js.` always means the browser global, never a BoxLang class file, so do not name a class folder `js`. Named and spread arguments are not supported for these constructors.

### Event handlers

A BoxLang function can be an event listener. It keeps running in the VM that created it, even when several MatchBox modules or several instances of one module are on the page.

```boxlang
function wire(host) {
    var input = js.document.createElement("input")
    var btn = js.document.createElement("button")
    var log = js.document.createElement("div")
    btn.textContent = "send"
    btn.addEventListener("click", function (e) {
        log.textContent = "you said: " & input.value
    })
    host.shadowRoot.append(input)
    host.shadowRoot.append(btn)
    host.shadowRoot.append(log)
}
```

### Web components

Exported functions are ordinary `async` functions, so a custom element can call them from its lifecycle methods:

```js
import * as mod from './widget.js';

class BxWidget extends HTMLElement {
    constructor() { super(); this.attachShadow({ mode: 'open' }); }
    async connectedCallback() { await mod.wire(this); }
}
customElements.define('bx-widget', BxWidget);
```

Strings such as `"<ul>"` can sit on their own line in a script, so building markup in BoxLang works.

### Concurrency

Every call to an exported function runs as its own fiber in one VM. Calls can overlap: two streams read at the same time finish in about the time of one, and an event handler runs while a stream is in flight. A call that is waiting on `.get()` does not block the others. Each exported function returns a Promise that resolves with the result or rejects with the error.

`BoxLangVM` also has `call(name, args)`, which is the same as an exported function, and the lower level `start(name, args)`, `poll(handle)` and `pump()` for hosts that drive the VM themselves.

### `http()` in the browser

With the `bif-http` feature, `http()` uses `fetch` and returns the same result as native MatchBox: call `.get()` and deserialize the JSON.

```boxlang
function status(url) {
    var response = jsonDeserialize(http({ url: url, method: "POST", headers: { "Content-Type": "application/json" }, body: "{}" }).get())
    return response.status
}
```

The result has `status`, `headers` (lowercase names, each value an array), `body` and `file_content`. A 4xx or 5xx status returns normally. A network failure or timeout throws.

| Option | In the browser |
| :--- | :--- |
| `url`, `method`, `headers`, `body` | Supported. |
| `timeout` | Supported, in seconds, default `30`, `0` disables it. |
| `redirect` | Only `true`. `redirect: false` throws, because `fetch` hides the redirect response. |
| `path` | Throws. The browser cannot write files. |
| `connectionTimeout`, `noProxy`, `ipv4Only` | Throw. The browser does not expose them. |

The browser's own rules still apply: the server must allow the request with CORS, and `headers` only lists the response headers that CORS exposes. `http()` returns the whole body, so for streaming use `js.fetch` as shown above.

---

## Serving Locally for Development

Browsers block WASM file loading over `file://`. Serve your project with any local HTTP server:

```bash
npx serve .
# or
python3 -m http.server 8080
```

Then open `http://localhost:8080` in your browser.

---

## Production Deployment

The WASM output files (`*.js` + `*.wasm`) can be deployed to any static hosting service:

- **CDN** — Upload to S3, Cloudflare R2, or a similar object store.
- **Vercel / Netlify** — Drop the files into your project's output directory.
- **Edge Workers** — The raw WASM binary can be loaded directly into Cloudflare Workers or similar runtimes. See [WASM Container](wasm-container.md) for details.

Ensure your server sets the correct `Content-Type` for `.wasm` files:

```
Content-Type: application/wasm
```

Most modern hosts handle this automatically.
