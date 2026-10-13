# HTTP Requests

Native MatchBox includes `http()` when built with the `bif-http` feature (enabled by default). Requests run in-process; no curl executable or shell is needed. This also works in [standalone native binaries](building-and-deploying/native-builds.md).

## Make a Request

`http()` returns a future. Call `.get()` to wait for completion, then deserialize its JSON result:

```boxlang
response = jsonDeserialize(http({
    url: "https://example.com/",
    timeout: 10
}).get());
println(response.status);
println(response.body);
```

The response contains:

| Field | Meaning |
| :--- | :--- |
| `status` | Numeric HTTP status, such as `200` or `404`. |
| `headers` | Response headers as a struct of lowercase names to arrays of string values. |
| `body` | Response text, when not downloading to a file. |
| `file_content` | The same response text as `body`. |
| `file_path` | Destination path, when downloading to a file instead of returning text. |

For a JSON API, deserialize `response.body` separately to obtain the API's data. HTTP 4xx/5xx statuses are returned normally; check `status` before using the body or downloaded file.

### Response Headers

Every response, including 4xx/5xx responses and downloads, includes `headers`. Header names are lowercase, and every value is an array, even when the header appeared once. Repeated headers such as `Set-Cookie` keep each value separately, in the order received. Header bytes that are not valid UTF-8 are decoded with replacement characters (`�`).

```boxlang
response = jsonDeserialize(http({ url: "https://example.com/" }).get());
println(response.headers["content-type"][1]);

if (structKeyExists(response.headers, "set-cookie")) {
    for (cookie in response.headers["set-cookie"]) {
        println(cookie);
    }
}
```

With `redirect: true`, `headers` belongs to the final response. With `redirect: false`, it belongs to the original 3xx response, so its `location` header names the redirect target. Headers from intermediate redirects are not kept.

## Request Options

Pass a struct with these case-insensitive keys:

| Option | Default | Behavior |
| :--- | :--- | :--- |
| `url` | Required | HTTP or HTTPS URL. |
| `method` | `"GET"` | `GET`, `POST`, `PUT`, or `DELETE`; case-insensitive. |
| `headers` | None | Struct of header names and values. |
| `body` | None | Request body as text. Serialize JSON explicitly when needed. |
| `path` | None | Write the response body to this file instead of returning text. |
| `timeout` | `30` | Total network request deadline in **seconds**, including connection, redirects, and response-body reads/downloads. `0` disables this deadline. |
| `connectionTimeout` | `0` | Connection deadline in **seconds**, including DNS, TCP, and TLS setup. `0` means no separate connection deadline; `timeout` still applies. |
| `redirect` | `true` | Follow at most 10 redirects. `false` returns the original 3xx response without requesting its target. |
| `noProxy` | `false` | `true` bypasses system/environment proxies for this request only. Otherwise normal proxy settings apply. |
| `ipv4Only` | `false` | `true` restricts outgoing connections to IPv4, including connections made after redirects. Otherwise normal IPv4/IPv6 selection applies. |

Both deadlines accept non-negative finite numbers, including fractional seconds. Negative, non-numeric, infinite, NaN, or out-of-range durations are rejected. The three boolean controls require actual `true`/`false` values, not strings or numbers. Explicit `null` is invalid for these five controls; omit a key to use its default.

The total deadline is **not** an inactivity timeout: receiving another chunk does not restart it. If both deadlines are enabled, whichever expires first ends the request.

`ipv4Only` controls this machine's connections. When using a proxy, the proxy chooses its own upstream route. Combine `ipv4Only: true` with `noProxy: true` when you need a direct IPv4 connection.

## Probe a Private Service

With `apiKey` loaded from your application's configuration:

```boxlang
try {
    response = jsonDeserialize(http({
        url: "http://private-agent:8642/api/sessions?limit=1",
        headers: { Authorization: "Bearer " & apiKey },
        timeout: 5,
        connectionTimeout: 2,
        redirect: false,
        noProxy: true,
        ipv4Only: true
    }).get());
    println("HTTP status: " & response.status);
} catch (any error) {
    println("Probe failed: " & error.message);
}
```

Headers stay in-process rather than being passed to a child command's arguments. Avoid logging request structs, credentials, or sensitive response bodies. HTTPS certificate verification remains enabled; these options do not disable TLS checks.

## Downloads and Failures

```boxlang
response = jsonDeserialize(http({
    url: "https://example.com/archive.zip",
    path: "archive.zip.tmp",
    timeout: 60
}).get());
```

A download overwrites an existing destination once response headers arrive. An interrupted download can leave a partial file. Use a temporary destination, check the status and any required checksum, and only then move it into place.

Invalid control values throw immediately from `http()`, before a connection is opened or the destination is touched. Transport and file errors reject the future and throw when `.get()` is called. Timeouts report `HTTP request timed out`. Transport diagnostics omit the request URL and do not include header values or request bodies.

The positional forms remain available with default controls: `http(url)`, `http(url, method)`, and `http(url, method, path)`. Use the struct form to configure headers, body, or request controls.

## Runtime Compatibility

This page describes **native MatchBox**, not the JVM HTTP client or browser fetch. MatchBox's request struct, future, and JSON result are distinct from JVM BoxLang's fluent HTTP client. The `timeout`, `connectionTimeout`, and `redirect` names follow BoxLang terminology; `noProxy` and `ipv4Only` are native MatchBox extensions. Do not assume these examples are drop-in JVM code.

Successful HTTP future completion is not implemented for WASM/browser builds. These options do not add WASM HTTP support. See [Differences from BoxLang](differences-from-boxlang.md).
