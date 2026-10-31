# HTML Encoding

`encodeForHTML()` turns untrusted text into text that is safe to place between HTML tags. Markup in the input is shown on the page as text instead of being interpreted by the browser.

```boxlang
encodeForHTML( string, canonicalize = false )
```

`htmlEditFormat()` is an alias for the same function. Output matches BoxLang JVM: the single quote (`'`) is not encoded, and `canonicalize` does not change the result.

## Runnable Example

Save this as `index.bxm` in an empty folder named `encoding-demo`:

```html
<bx:script>comment = "<script>alert('xss')</script> & <b>bold</b>";</bx:script>
<bx:output><p>#encodeForHTML( comment )#</p></bx:output>
```

From the MatchBox project root, serve the folder:

```bash
cargo run -p matchbox_server -- --port 8080 --webroot encoding-demo
```

Open `http://localhost:8080`. The page shows the comment as plain text, `<script>alert('xss')</script> & <b>bold</b>`: no alert runs and nothing is bold.

## Which Encoder?

`encodeForHTML()` is only safe for text between tags, such as `<p>#value#</p>`. Other places in a page have different rules and need their own encoders, which MatchBox does not provide yet:

| Where the value goes | Encoder needed |
| :--- | :--- |
| Attribute value, such as `<a title="#value#">` | `encodeForHTMLAttribute()` |
| JavaScript, such as `<script>` blocks or `onclick` | `encodeForJavaScript()` |
| CSS, such as `<style>` blocks | `encodeForCSS()` |
