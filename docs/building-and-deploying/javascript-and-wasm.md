# JavaScript + WebAssembly

MatchBox can compile BoxLang for the browser and Node.js via WebAssembly. There are three distinct modes, chosen to match different use-cases.

| Mode | Command | Use case |
| :--- | :--- | :--- |
| **JS Module (AOT)** | `--target js` | Ship a pre-compiled BoxLang app as an ES module |
| **WASM binary (AOT)** | `--target wasm` | Raw `.wasm` binary, brought your own loader |
| **Runtime (JIT-like)** | Build `pkg/` once, call `run_boxlang()` | Execute source strings at runtime in the browser |

---

## Mode 1: JavaScript ES Module (AOT)

This is the recommended way to ship a BoxLang application to the browser. MatchBox compiles your script to bytecode, embeds it in a WASM binary, and wraps it in a JavaScript ES module that bootstraps the runtime automatically.

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

### Use in Node.js

```js
import { greet } from './my_lib.js';

const message = await greet("Node");
console.log(message);
```

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

---

## Mode 2: Raw WASM Binary (AOT)

Use `--target wasm` when you want the raw `.wasm` file and full control over how it is loaded. This is useful for integration with non-standard JS runtimes, edge platforms, or when you are bundling the WASM via a tool like Webpack or Vite.

```bash
matchbox --target wasm my_app.bxs
# Produces: my_app.wasm
```

Load it manually using the standard WebAssembly API:

```js
const response = await fetch('./my_app.wasm');
const buffer   = await response.arrayBuffer();
const module   = await WebAssembly.instantiate(buffer, importObject);
```

Refer to the [MDN WebAssembly docs](https://developer.mozilla.org/en-US/docs/WebAssembly) for the full instantiation API.

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

When BoxLang code is running inside a browser WASM context, it can access the JavaScript environment through the `js` global:

```boxlang
// DOM access
title = js.document.title
js.document.getElementById("output").innerText = "Updated by BoxLang"

// Browser APIs
js.alert("Hello!")
js.console.log("Logged from BoxLang")

// Location
url = js.window.location.href
```

> `js.*` is only available in browser WASM. Using it in a native build throws a runtime error.

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
