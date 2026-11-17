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

## Conditional Requests (ETag)

Static files such as `styles.css` are sent with an `ETag` header: a fingerprint of the file's contents (a SHA-256 hash in double quotes). Browsers use it to avoid downloading a file they already have:

1.  **First visit:** the server returns `200 OK` with the file and its `ETag`. The browser stores both.
2.  **Next visit, file unchanged:** the browser sends the tag back in an `If-None-Match` header. The tags match, so the server answers `304 Not Modified` with no body, and the browser reuses its cached copy.
3.  **File edited:** the contents have a new fingerprint, so the old tag no longer matches. The server sends the full file with its new `ETag`.

Try it with the server from the previous section running. The tag below is shortened, so copy the full `etag` value from your first response into the second. Your tag may also differ: on Windows, Git can check the file out with different line endings, which changes its bytes and therefore its hash.

```bash
# first request: 200 with an ETag
curl -i http://localhost:8080/styles.css
#   HTTP/1.1 200 OK
#   content-type: text/css
#   etag: "08c5d6d2...c1461"

# send the tag back: 304, empty body
curl -i -H 'If-None-Match: "08c5d6d2...c1461"' http://localhost:8080/styles.css
#   HTTP/1.1 304 Not Modified
#   etag: "08c5d6d2...c1461"

# templates are dynamic: no ETag, always 200
curl -i -H 'If-None-Match: *' http://localhost:8080/about.bxm
```

`If-None-Match` also accepts a comma-separated list of tags, weak tags (`W/"..."`), and `*`, which matches any existing file.

What does not get an ETag:

*   **Templates** (`.bxm` / `.bxs`), including pages reached through rewrites or folder index files. Their output can change on every request, so they always return `200`.
*   **Missing files** still return `404`, even with `If-None-Match: *`.
*   **`POST` requests** to static files keep their normal response.

`Last-Modified` / `If-Modified-Since` are not supported; only ETags are.
