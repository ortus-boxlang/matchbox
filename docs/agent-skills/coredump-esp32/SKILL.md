---
name: coredump-esp32
description: >-
  Read and interpret ESP32 coredumps from MatchBox applications. Covers the
  espcoredump.py workflow, decoding the coredump partition, handling app SHA
  mismatch, and identifying Rust OOM, VM route execution, HTTP task, HID worker,
  and background fiber frames.
applyTo: "**/*.bxs **/*.bxm **/matchbox-esp32-runner/**"
---

# MatchBox ESP32 Coredump Debugging Agent Skill

## Overview

ESP32 MatchBox runner firmware enables coredump-to-flash via ESP-IDF. When the device crashes (panic, OOM, abort), the coredump is written to the dedicated `coredump` partition. Use `espcoredump.py` from the ESP-IDF tools to decode it.

## Coredump Configuration

The ESP32 runner enables coredump-to-flash in `sdkconfig.defaults`:

```
CONFIG_ESP_COREDUMP_ENABLE_TO_FLASH=y
CONFIG_ESP_COREDUMP_DATA_FORMAT_ELF=y
CONFIG_ESP_COREDUMP_CHECKSUM_CRC32=y
CONFIG_ESP_COREDUMP_MAX_TASKS_NUM=16
```

The `partitions.csv` reserves a 64K partition:

```
coredump, data, coredump, , 64K
```

## Prerequisites

- ESP-IDF environment activated (`source /path/to/esp-idf/export.sh`)
- The app ELF file that was flashed to the device (see build-flash-esp32 skill)
- Python (required by `espcoredump.py`)

## Workflow: Reading a Coredump

### 1. Read the coredump partition

```bash
# From an activated ESP-IDF shell
espcoredump.py info_corefile \
  --chip esp32s3 \
  --port /dev/ttyACM0 \
  crates/matchbox-esp32-runner/target/xtensa-esp32s3-espidf/release/matchbox-esp32-runner
```

This reads the coredump from flash via the serial port and decodes it against the app ELF.

### 2. Save coredump to file (for offline analysis)

```bash
# Dump the raw coredump partition to a file
espcoredump.py dump_corefile \
  --chip esp32s3 \
  --port /dev/ttyACM0 \
  -o coredump.elf \
  crates/matchbox-esp32-runner/target/xtensa-esp32s3-espidf/release/matchbox-esp32-runner
```

Then analyze later:

```bash
espcoredump.py info_corefile -m --core coredump.elf \
  crates/matchbox-esp32-runner/target/xtensa-esp32s3-espidf/release/matchbox-esp32-runner
```

(`-m` shows the backtrace of all tasks.)

### 3. Alternative: manual partition read

If `espcoredump.py` cannot connect, read the partition directly via `espflash`:

```bash
espflash read-bin 0x210000 0x10000 coredump.raw \
  --chip esp32s3 --port /dev/ttyACM0
```

(The coredump partition offset depends on the partition table layout. Verify with the `partitions.csv`.)

Then decode:

```bash
espcoredump.py info_corefile --core coredump.raw \
  crates/matchbox-esp32-runner/target/xtensa-esp32s3-espidf/release/matchbox-esp32-runner
```

## Understanding the Output

### App SHA Mismatch Warning

ESP-IDF embeds a build hash in the ELF. During iterative development, the flashed firmware and the ELF on disk will have different hashes.

```
WARNING: App (XX) and core dump (YY) have different SHA256!
```

**This is expected during development** if you changed and rebuilt the app between flash and coredump collection. The stack frame addresses are usually still valid as long as you use the ELF that was actually flashed. Keep the matching ELF or rebuild from the same source commit.

**Fix:** Rebuild and re-flash the runner before relying on coredump accuracy.

### Key Task Frames to Identify

Look for these functions in the backtrace to categorize the crash:

| Crash Context | Key Frames |
|---|---|
| **Rust OOM** | `esp_idf_svc::heap_alloc_failed`, `alloc::alloc::handle_alloc_error`, `alloc::alloc::oom` |
| **VM panic** | `matchbox_vm::vm::VM::run_fiber`, `matchbox_vm::vm::VM::interpret` |
| **VM route execution** | `matchbox_embedded::route_handler`, `matchbox_embedded::web::handle_request` |
| **HTTP task crash** | `esp_idf_svc::http::server`, `httpd_uri_handler_wrapper` |
| **HID worker crash** | `matchbox_esp32_runner::hid::hid_task`, `tinyusb_hid_task` |
| **Background fiber crash** | `matchbox_vm::fiber::Fiber::run`, `matchbox_embedded::fiber::run_background` |
| **Wi-Fi panic** | `esp_wifi_internal`, `wifi_driver_task` |
| **Watchdog timeout** | `task_wdt`, `esp_task_wdt`, `abort()` with no explicit panic message |

### Reading the Panic Line

Look for the panic message and location in the output. Example from a VM panic:

```
PanicHookInfo {
    payload: Any { .. },
    location: Location {
        file: "crates/matchbox-vm/src/vm/mod.rs",
        line: 2697,
        column: 74,
    },
}
```

This tells you the exact Rust source line that panicked. Cross-reference with the source code to understand what operation failed (unwrap on None, division by zero, assertion, etc.).

## Stack Frame Categories

### VM Frames

When the VM panics during BoxLang execution, the Rust frames show the VM call chain:

```
matchbox_vm::vm::VM::run_fiber           ← executing a BoxLang fiber
matchbox_vm::vm::VM::run_all             ← running all fibers
matchbox_vm::vm::VM::interpret_chunk_shared  ← interpreting bytecode
matchbox_vm::vm::VM::interpret           ← top-level interpreter entry
```

### HTTP / Web Request Frames

If a web request handler crashes:

```
httpd_uri_handler_wrapper                 ← ESP-IDF HTTP server dispatching
matchbox_esp32_runner::web::handle_request  ← MatchBox request handler
matchbox_embedded::web::route_handler      ← route dispatch
matchbox_vm::vm::VM::run_fiber             ← handler fiber execution
```

### HID Frames

If HID processing crashes:

```
tinyusb_hid_task                           ← TinyUSB HID task
matchbox_esp32_runner::hid::hid_task       ← MatchBox HID worker
matchbox_vm::vm::VM::run_fiber             ← HID fiber execution
```

## Diagnostics Before Crash

The diagnostics module logs at boot:

```
[matchbox] diagnostics boot=N reset=REASON nvs=true nvs_status='ok' last_event='...'
```

If the device reboots after a crash, the diagnostics record the reset reason. Common reset reasons:
- `POWERON_RESET` — power cycle or initial boot
- `PANIC` — software panic (coredump available)
- `INT_WDT` — interrupt watchdog timeout
- `TASK_WDT` — task watchdog timeout
- `DEEPSLEEP_RESET` — woke from deep sleep
- `BROWNOUT_RESET` — brownout detected (check power supply)

## Reporting Checklist

When reporting a crash, include:

- [ ] **Reset reason** from monitor output or diagnostics
- [ ] **Full panic message** if printed before coredump
- [ ] **Coredump backtrace** (all tasks with `-m`)
- [ ] **App SHA** from the flashed ELF
- [ ] **MatchBox version** (commit hash or build date from ELF: `readelf -p .comment` or `git rev-parse --short HEAD`)
- [ ] **BoxLang source** that triggered the crash (if known)
- [ ] **Heap state**: free heap, min free heap at time of crash
- [ ] **ESP-IDF version** used to build the runner

## Recovery After Crash

1. Read the coredump (see workflow above)
2. Power-cycle the device to clear the panic state
3. Re-flash the app bytecode: `matchbox --target esp32 --chip esp32s3 --flash app.bxs`
4. If the device does not boot after coredump, re-run full flash: `matchbox --target esp32 --chip esp32s3 --full-flash app.bxs`

## Related Skills

- **build-flash-esp32** — Building and flashing MatchBox ESP32 applications
- **debug-esp32-app** — Debugging ESP32 MatchBox applications  
- **build-esp32-app** — Authoring MatchBox ESP32 applications
