---
name: build-flash-esp32
description: >-
  Build, flash, and monitor MatchBox BoxLang applications on ESP32 microcontrollers
  (ESP32-S3, ESP32-C3, ESP32). Covers environment setup, full-flash, fast-deploy,
  watch mode, monitor troubleshooting, and common failure modes.
applyTo: "**/*.bxs **/*.bxm **/matchbox-esp32-runner/**"
---

# MatchBox ESP32 Build & Flash Agent Skill

## Overview

Build and deploy BoxLang (`.bxs`) scripts to ESP32 microcontrollers using MatchBox's `--target esp32` pipeline. The workflow compiles BoxLang to bytecode, bundles it with the ESP32 runner, flashes via `espflash`, and supports iterative development with fast bytecode-only deploys.

## Prerequisites

### Required Tools

| Tool | Install | Notes |
|------|---------|-------|
| Rust ESP32 toolchain | `cargo install espup && espup install` | Installs Xtensa/RISC-V Rust toolchains |
| ESP-IDF | Clone esp-idf and run `install.sh` | MatchBox prefers `fromenv` mode |
| `espflash` 3.3.0+ | `cargo install espflash` | Flashing the device |
| `ldproxy` | `cargo install ldproxy` | ESP32 runner linker |
| C build tools, Python, CMake, Ninja | System package manager | Required by `esp-idf-sys` |

### Shell Environment

```bash
# Activate ESP-IDF before any MatchBox ESP32 command
source /path/to/esp-idf/export.sh

# Select the esp Rust toolchain
export RUSTUP_TOOLCHAIN=esp
```

Run all MatchBox ESP32 commands from this same shell session. Do not layer other ESP export scripts on top.

### Serial Device Permissions (Linux)

If flash fails with permission denied on `/dev/ttyACM0` (or similar):

```bash
# Add your user to the dialout group
sudo usermod -aG dialout $USER
# Log out and back in for the change to take effect
```

Alternatively, identify your distro's serial group (`uucp`, `dialout`, etc.):

```bash
ls -la /dev/ttyACM0
```

### WSL USB Passthrough

If using WSL2, attach the USB device from an **Administrator PowerShell** on Windows:

```powershell
usbipd list
usbipd attach --busid <BUSID> --auto-attach
```

## Build & Flash Workflows

### 1. Initial Full Flash

Required the **first time** flashing a device. Installs the runner firmware + custom partition table.

```bash
matchbox --target esp32 --chip esp32s3 --full-flash app.bxs
```

If your script uses the embedded web server subset, add `--esp32-web`:

```bash
matchbox --target esp32 --chip esp32s3 --esp32-web --full-flash app.bxs
```

**Chip aliases:** `esp32`, `esp32s3`, `esp32c3`

### 2. Fast Deploy (Bytecode-Only)

After the runner is flashed, update only the BoxLang bytecode (~1 second):

```bash
matchbox --target esp32 --chip esp32s3 --flash app.bxs
```

### 3. Watch Mode (Live Coding)

Watches for `.bxs` file changes, auto-redeploys bytecode, and restarts the monitor:

```bash
matchbox --target esp32 --chip esp32s3 --watch app.bxs
```

What watch mode does:
1. Initial fast-deploy of the script
2. Opens `espflash monitor` with hardware reset
3. On file save: kills monitor, re-flashes bytecode, restarts monitor + reset

## Manual Flash Fallback

If MatchBox builds the ELF but can't open the serial device, flash manually:

```bash
espflash flash \
  --chip esp32s3 \
  --port /dev/ttyACM0 \
  --partition-table crates/matchbox-esp32-runner/partitions.csv \
  app.elf
```

## Monitoring

After flashing, open the serial monitor:

```bash
espflash monitor --chip esp32s3 --port /dev/ttyACM0
```

Or use `matchbox` watch mode which manages monitor lifecycle automatically.

### HID Mode Note

When USB HID is active, the serial console may be disabled (CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG affects console behavior). Use status routes or diagnostics BIFs for debug output when in HID mode.

## Troubleshooting

### `ldproxy` not found

```bash
cargo install ldproxy
```

### Stale build artifacts after changing ESP-IDF or chip target

```bash
rm -rf crates/matchbox-esp32-runner/target
rm -rf target/esp32_stubs
```

Then retry the full flash.

### Monitor already running

```bash
# Find and kill the existing monitor process
pkill -f espflash.monitor
# or
lsof /dev/ttyACM0
```

### Serial device busy

If another process (monitor, serial terminal, IDE) holds the port:

```bash
lsof /dev/ttyACM0
# Kill the owning process or close the other application
```

### Device not responding after flash

Press the **EN/RST** button on the ESP32 board, or use `espflash reset`:

```bash
espflash reset --port /dev/ttyACM0
```

## Verification Checklist

After following this skill, confirm:

- [ ] ESP-IDF environment activates without errors
- [ ] `espup`-managed Rust toolchain reports correct Xtensa/RISC-V targets
- [ ] `ldproxy` is installed and on PATH
- [ ] Serial device is writable by your user (no `sudo` needed)
- [ ] `matchbox --target esp32 --chip esp32s3 --full-flash` succeeds
- [ ] Monitor output shows MatchBox boot and script execution
- [ ] Fast deploy (`--flash`) completes in ~1 second
- [ ] Watch mode detects file changes and auto-redeploys
- [ ] Manual flash fallback via `espflash flash` works if automated flash fails

## Related Skills

- **coredump-esp32** — Reading ESP32 coredumps from MatchBox apps
- **debug-esp32-app** — Debugging ESP32 MatchBox applications
- **build-esp32-app** — Authoring MatchBox ESP32 applications
