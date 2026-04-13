# WASM Browser — Todo App

A minimal browser Todo application written in BoxLang and compiled to WebAssembly.
BoxLang owns the app state and state transitions. The HTML shell wires that
state into Alpine using the generated `window.MatchBox.State(...)` helper.

## Project Layout

```
wasm_browser/
├── todo.bxs    ← BoxLang source (state + DOM rendering logic)
└── index.html  ← HTML shell (imports the compiled ES module)
```

## How It Works

`matchbox --target js` compiles `todo.bxs` to an **ES module** (`todo.js` + `todo.wasm`).

- **BoxLang owns the state** and returns plain snapshot structs for the UI.
- **The generated browser contract owns readiness and module lookup** through `window.MatchBox`.
- **The page owns framework wiring** by creating an Alpine store from `window.MatchBox.State("todo", ...)`.

## Build

You need the `matchbox` binary and `wasm-pack` / the WASM target available.

```bash
cd docs/examples/wasm_browser
matchbox --target js todo.bxs
```

This produces two files alongside your source:

```
todo.js    ← ES module wrapper (import this in HTML)
todo.wasm  ← WebAssembly binary (loaded automatically by todo.js)
```

## Run Locally

Browsers block WASM loading over `file://` for security reasons. Serve the
directory with any local HTTP server:

```bash
# Option A — Node.js (npx, no install required)
npx serve .

# Option B — Python
python3 -m http.server 8080

# Option C — any other static file server
```

Then open the printed URL (usually `http://localhost:3000` or
`http://localhost:8080`) in your browser.

## Features

- Add a task by typing in the input and pressing **Enter** or clicking **Add**.
- Click a task text to toggle it **done / undone**.
- Click **✕** to remove a task.

## Deploy to Production

The compiled output (`todo.js` + `todo.wasm` + `index.html`) is a fully static
bundle. Upload it to any static host:

```bash
# Vercel
vercel deploy

# Netlify drag-and-drop
# Upload the wasm_browser/ folder to app.netlify.com/drop
```

Ensure your server sets the correct MIME type for `.wasm` files:

```
Content-Type: application/wasm
```

Most modern hosts (Netlify, Vercel, Cloudflare Pages) handle this automatically.

## Key Concepts

| Concept | Where to look |
|---|---|
| BoxLang state/actions | `todo.bxs` |
| Generated browser namespace | `todo.js` → `window.MatchBox.modules["todo"]` |
| Readiness signal | `window.MatchBox.ready("todo")` |
| Generic state helper | `window.MatchBox.State("todo", ...)` in `index.html` |
| Alpine store wiring | `index.html` |
