# bx-matchbox-std

An optional MatchBox standard-library module. It can be selected, installed, embedded in an application, or omitted. It is **not** an LSP package. Standard-library migration is incremental: existing prelude/native BIFs remain in core until a tested replacement moves here; new stream APIs have only this implementation.

The first backend is prebuilt for **x86_64-unknown-linux-gnu**, plugin ABI **1**, requiring the additive **`matchbox.bytes.v1`** capability. No JVM backend, Windows/macOS validation, async executor, or dynamic module unloading is claimed.

## Author build

From the MatchBox repository root:

```sh
python3 modules/bx-matchbox-std/native/package.py
# Optional SDK author override:
python3 modules/bx-matchbox-std/native/package.py --sdk-path /path/to/matchbox-plugin-sdk
```

Outputs `build/module/` and `build/bx-matchbox-std.zip` inside this module. The ZIP contains shared BoxLang BIFs, descriptor, license, prebuilt provider and SHA-256 metadata—not Rust sources or Cargo dependencies. No publishing is performed.

Consumers install that ZIP or select its built directory:

```sh
matchbox --module /path/to/bx-matchbox-std/build/module app.bxs
```

Consumers need no Cargo, SDK, JVM, Python, or startup downloads. Embedded providers extract to MatchBox's private verified native cache; a compatible Linux/glibc host and writable cache are required.

## BoxLang API

```boxlang
listener = tcpListen(port=0, host="127.0.0.1");
address = listener.address(); // { host, port }, including the actual ephemeral port
client = tcpConnect(host=address.host, port=address.port, timeoutMs=5000);
server = listener.accept(timeoutMs=1000);
client.writeAll(charsetDecode("hello" & chr(10), "utf-8"));
line = server.readUntil(separator=charsetDecode(chr(10), "utf-8"), maxBytes=8192, timeoutMs=500);
println(utf8Decode(line));
client.close(); server.close(); listener.close();
```

- `tcpListen(port=0, host="127.0.0.1")`: a listener with `accept(timeoutMs=-1)`, `address()`, `close()`.
- `tcpConnect(host, port, timeoutMs=5000)`: a duplex stream. OS DNS lookup is synchronous and excluded from the connection timeout.
- `stdInput()` / `stdOutput()`: byte streams backed by owned duplicate descriptors. Closing them does not close process stdin/stdout. Do not mix raw input with independently buffered `cliRead()`.
- `stream.read(maxBytes=8192, timeoutMs=-1)`: short binary reads; maximum request 1 MiB. **Null is timeout; empty binary is EOF.** Zero-byte requests are rejected.
- `stream.readUntil(separator, maxBytes=8192, timeoutMs=-1)`: includes the binary separator; retains partial data on timeout. EOF after partial data and exceeding the limit throw. Maximum limit 1 MiB.
- `stream.readExactly(size, timeoutMs=-1)`: exact binary read, retaining partial bytes across timeouts; premature EOF throws. Size must be 0–64 MiB.
- `stream.writeAll(data, timeoutMs=5000)`: binary only; returns total bytes written. Errors/timeouts throw, including the partial count when available. A failed write may have transmitted a prefix: close/reestablish application framing instead of blindly retrying the entire payload.
- `utf8Decode(data)`: strict decoding; invalid UTF-8 throws rather than replacing bytes.
- `close()`: idempotent; subsequent I/O throws. GC also drops owned resources. Resources and calls are VM-thread-affine.

Read/accept timeouts: `-1` waits indefinitely, `0` polls without waiting, positive milliseconds bound the operation. Already-buffered complete reads return immediately. Protocols, encoding policy, Content-Length framing and JSON-RPC are deliberately BoxLang/application concerns.

## Checks

```sh
cargo test --manifest-path modules/bx-matchbox-std/native/Cargo.toml \
  --config 'patch.crates-io.matchbox-plugin-sdk.path="/absolute/path/to/crates/matchbox-plugin-sdk"'
matchbox --no-shaking --module modules/bx-matchbox-std/build/module \
  modules/bx-matchbox-std/tests/streams.bxs
```

`bx-lint/tests/smoke.py` checks real stdio/TCP, fragmented input across timeouts, strict UTF-8, malformed framing, reconnects and reloads. `bx-lint/tests/build.py` stages copies, compiles both application formats, removes source/packages and executes with fresh caches and no Cargo/BoxLang launcher on PATH. The runtime and standalone runner are built without JNI.
