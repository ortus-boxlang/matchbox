# MatchBox WebSocket Counter Example

This example demonstrates the routed app server plus SocketBox-style websocket listeners.

It serves an HTML page at `/`, opens a websocket connection to `/ws`, increments a shared counter when the button is clicked, and broadcasts the new count to every connected browser tab.

## Project Structure

* `app.bxs`: Main routed app plus websocket listener registration.
* `views/home.bxm`: HTML page with the client-side websocket code.

## Key Concepts

### 1. Listener Class Registration

WebSockets are enabled with a listener instance:

```boxlang
import boxlang.web;

listener = new ClickCounterSocket();
listener.configure();

app.enableWebSockets( "/ws", listener );
```

MatchBox snapshots the listener instance state when you register it, then restores that state inside the websocket runtime. This keeps preconfigured listener objects usable instead of forcing a zero-argument constructor.

### 2. Listener Lifecycle

The listener class uses the familiar SocketBox-style methods:

* `onConnect( channel )`
* `onMessage( message, channel )`
* `onClose( channel )`

### 3. Channel Helpers

The `channel` object currently supports:

* `sendMessage()` / `sendText()`
* `sendJson()`
* `sendBytes()`
* `broadcastMessage()` / `broadcastText()`
* `broadcastJson()`
* `broadcastBytes()`
* `close()`

This example uses `sendJson()` to initialize new connections and `broadcastJson()` to fan out counter changes.

## Running the Example

From the project root:

```bash
cargo run -p matchbox_server -- --app docs/examples/websocket_counter/app.bxs
```

Then open:

* `http://localhost:8099/`

Open the page in two tabs and click the button in either tab. Both tabs should update to the same counter value.
