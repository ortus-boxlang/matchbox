# MatchBox Web Server Example

This example demonstrates the built-in MatchBox high-performance web runtime. It includes support for BoxLang Markup (`.bxm`), static asset serving, and automatic scope injection (URL, Form, Cookie, Session).

## Project Structure

*   `index.bxm`: The home page, demonstrating **Session** persistence and **Dynamic Output**.
*   `about.bxm`: An informational page showing **CGI Scope** and simple loops.
*   `contact.bxm`: A page demonstrating **URL Scope** and **Conditional Logic** (`<bx:if>`).
*   `styles.css`: A static CSS file served automatically by the runtime.

## How it Works

### 1. BXM Transpilation
MatchBox includes a specialized markup transpiler that converts HTML-like templates into standard BoxLang bytecode. For example:
```html
<bx:output>Hello #user.name#!</bx:output>
```
Is converted into:
```boxlang
writeOutput("Hello ");
writeOutput(user.name);
writeOutput("!");
```

### 2. Built-in Scopes
The runtime automatically populates and injects the following global scopes into every request:
*   `url`: Populated from query string parameters.
*   `form`: Populated from POST request bodies.
*   `cookie`: Access to browser cookies (including `MBX_SESSION_ID`).
*   `session`: A persistent, in-memory struct that survives across requests.
*   `cgi`: Server environment variables.

### 3. Native Performance
The web server is built on **Axum** and **Tokio**, and the MatchBox VM executes the compiled bytecode at near-native speeds.

## Running the Example

1.  From the project root, run the following command:
    ```bash
    cargo run -p matchbox_server -- --port 8080 --webroot docs/examples/web_server
    ```
2.  Open your browser to `http://localhost:8080`.
3.  Visit `http://localhost:8080/contact.bxm?name=MatchBox` to see URL parameter handling.
4.  Refresh the home page to see the `session.visitCount` increment.
