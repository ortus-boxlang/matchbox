# BoxLang + Alpine.js Example

This example demonstrates how to define an **Alpine.js Data Component** using 100% BoxLang code.

## How it works

1.  **Direct Access**: The BoxLang script uses `js.Alpine.data()` to register a component.
2.  **Normalization**: The BoxLang Struct returned by the constructor is automatically converted into a reactive JavaScript object.
3.  **Closures**: The `increment` and `decrement` functions in BoxLang are wrapped as JS functions that Alpine can call.
4.  **Host Mutation**: When the HTML calls `@click="increment($data)"`, the BoxLang function receives a reference to the Alpine proxy and can update it directly.

## Building the Example

You must compile the BoxLang script into a JavaScript/WASM bundle using the MatchBox CLI:

```bash
matchbox --target js counter.bxs
```

This will produce:
- `counter.js`: The ES module entry point.
- `counter.wasm`: The WASM binary containing the BoxLang VM and your compiled code.

## Running the Example

Since browsers block WASM loading from `file://` URIs, you must serve this folder using a local web server:

```bash
# Using Python
python3 -m http.server 8000

# Using Node.js (npx)
npx serve .
```

Then open `http://localhost:8000` in your browser.
