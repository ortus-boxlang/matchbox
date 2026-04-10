# Building for ESP32

MatchBox supports building and flashing BoxLang scripts directly to ESP32 microcontrollers. This is achieved by cross-compiling a specialized MatchBox runner for the Xtensa or RISC-V architectures and deploying your compiled bytecode to a dedicated flash partition.

## Current Web Runtime Support

ESP32 does not yet ship the full native app-server runtime from `matchbox-server`.

Today, the intended `web.server()` direction for ESP32 is the lean subset:

* route registration
* middleware definitions
* in-handler request/response helpers such as `event.renderJson()` and `event.renderHtml()`
* route params and request metadata helpers

The intended app shape is a route-driven script that avoids filesystem-backed features. A good ESP32-safe pattern today is:

```bx
import boxlang.web;

app = web.server();

app.get( "/", function( event, rc, prc ) {
    event.renderHtml( "<h1>Roastatron 3K</h1>" );
} );

app.get( "/status", function( event, rc, prc ) {
    event.renderJson( { "ok": true } );
} );

app.post( "/print", function( event, rc, prc ) {
    event.renderJson( { "queued": true } );
} );
```

This keeps the BoxLang surface aligned with the native server direction while avoiding the unsupported pieces below.

To opt into that build flavor, add `--esp32-web` when compiling for ESP32.

Without `--esp32-web`, `web.server()` usage is rejected at compile time for `--target esp32`.

Even with `--esp32-web`, the following app-server features currently fail at compile time:

* `app.listen()`
* `event.setView()` / `event.renderTemplate()`
* `app.middleware.buildStaticFiles( mount, dir )`
* webhook helpers such as `app.buildWebhook()` and `app.webhook(...)`
* cookie helpers
* session helpers

This is intentional. MatchBox now rejects those features early instead of producing firmware that implies support the ESP32 runner does not yet provide.

## Prerequisites

To build for ESP32, you must have the following installed on your development machine:

1.  **Rust ESP32 Toolchain**: Install using `espup`:
    ```bash
    cargo install espup
    espup install
    # Install the Rust Xtensa/RISC-V toolchains
    ```
2.  **ESP-IDF Environment**: Install a real ESP-IDF checkout and activate it before invoking MatchBox.
    MatchBox now prefers the activated ESP-IDF environment over the managed `esp-idf-sys` tool download path.
3.  **espflash**: For flashing the binary to the device. Version 3.3.0+ is required.
4.  **ldproxy**: Required by the ESP32 runner linker configuration.
    ```bash
    cargo install ldproxy
    ```
5.  **ESP-IDF Prerequisites**: Standard C build tools, Python, CMake, and Ninja (required for the `esp-idf-sys` crate).

### Required Shell Environment

Before using `--target esp32`, activate the ESP-IDF environment and switch the Rust toolchain:

```bash
source /path/to/esp-idf/export.sh
export RUSTUP_TOOLCHAIN=esp
```

Run MatchBox from that same shell. Do not layer other ESP export scripts on top of the activated ESP-IDF shell.

If these variables are not active, MatchBox's ESP32 runner build will fail.

### WSL Users (USB Access)
If you are using Windows Subsystem for Linux (WSL), you must "attach" your USB device to the Linux instance using `usbipd-win`. 

From a **Windows Administrator PowerShell**:
```powershell
usbipd list
usbipd attach --busid <BUSID> --auto-attach
```

## Building and Flashing

Use the `--target esp32` flag to trigger an ESP32 build. You should always specify your chip type via `--chip` (e.g., `esp32`, `esp32s3`, `esp32c3`).

### 1. Initial Setup (Full Flash)
The first time you flash a device, you must perform a "Full Flash." This installs the MatchBox Runner firmware and the custom partition table required for BoxLang.

```bash
matchbox app.bxs --target esp32 --chip esp32s3 --full-flash
```
*Note: `--full-flash` implicitly triggers the flash process.*

If no pre-built stub exists for your chip, MatchBox will fall back to building the ESP32 runner locally. At the time of writing, ESP32-S3 commonly takes this path.

The command must be run from a shell where the ESP-IDF environment has already been activated:

```bash
source /path/to/esp-idf/export.sh
export RUSTUP_TOOLCHAIN=esp
matchbox app.bxs --target esp32 --chip esp32s3 --full-flash
```

### Flash Permissions

On Linux, the build may succeed but the flash step can still fail if your user cannot open the serial device (for example `/dev/ttyACM0`).

Recommended fix:

```bash
groups
sudo usermod -aG dialout $USER
# or use the serial device group used by your distro, such as `uucp`
```

Then log out and back in before retrying.

Avoid rerunning the entire `matchbox ... --full-flash` command with `sudo`, because that restarts the full build in a root environment. If you only need elevated access for flashing, build as your normal user first and then flash the produced ELF with `espflash`.

### 2. Fast Deployment (Default)
Once the Runner is on the device, you only need to update the BoxLang bytecode. This takes ~1 second and does not require re-flashing the firmware.

```bash
matchbox app.bxs --target esp32 --chip esp32s3 --flash
```

If the script uses the embedded routed web subset, opt in explicitly:

```bash
matchbox app.bxs --target esp32 --chip esp32s3 --esp32-web --full-flash
```

## Watch Mode (Live Coding)

MatchBox features a built-in watch mode that provides a "Hot Reload" experience for physical hardware.

```bash
matchbox app.bxs --target esp32 --chip esp32s3 --watch
```

**What Watch Mode does:**
1.  **Initial Flash**: Performs a fast-deploy of your script.
2.  **Integrated Monitor**: Automatically opens `espflash monitor` and performs a hardware reset.
3.  **Auto-Update**: Watches your directory for `.bxs` changes. Upon save, it kills the monitor, flashes the new bytecode in 1s, and restarts the monitor/reset cycle.

## How it Works

1.  **Compilation**: Your `.bxs` script is compiled into `.bxb` bytecode using the **Postcard** serialization format, ensuring 64-bit to 32-bit architecture compatibility.
2.  **Partitioning**: MatchBox uses a custom partition table (`partitions.csv`) that reserves a 1MB `storage` partition at offset `0x110000` for bytecode.
3.  **Runtime**: The ESP32 Runner starts a dedicated FreeRTOS task with a **48KB stack** to host the MatchBox VM.
4.  **Environment Awareness**: The BoxLang `server` scope is automatically populated with hardware information (e.g., `server.os.arch` will return `xtensa` or `riscv`).

## App Server Roadmap

The embedded app-server plan is to keep the BoxLang programming model aligned with the native routed server, but ship it in capability tiers:

* ESP32 first: routed handlers and middleware
* later: websockets
* not planned for the first embedded slice: filesystem-backed static assets, template rendering from disk, native webhook helpers, and session-heavy server features

## Memory and Performance

*   **SRAM**: ESP32 devices have limited memory (usually 520KB). The VM is configured with a large stack to prevent overflows, but you should still be mindful of creating massive arrays.
*   **Flash**: The VM and runtime add roughly 800KB - 1.2MB to the firmware size. The bytecode is stored separately in the 1MB `storage` partition.

## Native Hardware Access

Standard BoxLang BIFs (Built-in Functions) like `println` are mapped to the ESP32's serial console. To access hardware pins (GPIO, I2C, WiFi), you can use **Native Fusion** or use a BoxLang module that provides hardware wrappers.

## Why MatchBox Prefers `fromenv`

MatchBox now instructs the ESP32 runner build to use `ESP_IDF_TOOLS_INSTALL_DIR=fromenv`. This keeps the
CLI aligned with the contributor's installed ESP-IDF environment and avoids brittle per-project tool downloads
that can break on newer host distributions.

## Troubleshooting

### Missing `ldproxy`

If the runner fails with `linker ldproxy not found`, install it on your host machine:

```bash
cargo install ldproxy
```

### Stale ESP32 Runner Build Artifacts

If you change ESP-IDF versions, chip targets, or shell environments and keep seeing stale CMake or toolchain errors, wipe the runner build output and try again:

```bash
rm -rf crates/matchbox-esp32-runner/target
rm -rf target/esp32_stubs
```

### Manual Flash Fallback

If MatchBox successfully produces an ELF but cannot open the serial device, you can flash it directly:

```bash
espflash flash \
  --chip esp32s3 \
  --port /dev/ttyACM0 \
  --partition-table crates/matchbox-esp32-runner/partitions.csv \
  app.elf
```
