---
name: debug-esp32-app
description: >-
  Debug MatchBox ESP32 applications. Covers monitor log collection, status and
  diagnostics routes, heap and reset inspection, coredump reading, and narrowing
  failures by subsystem (web routes, background fibers, HID, Wi‑Fi, memory).
applyTo: "**/*.bxs **/*.bxm **/matchbox-esp32-runner/**"
---

# MatchBox ESP32 App Debugging Agent Skill

## Overview

Debug ESP32 MatchBox applications by triangulating evidence from four sources: serial monitor logs, the `/__matchbox/status` endpoint, diagnostics data (reset reason, heap snapshots, boot counters), and coredumps. Use the debugging flow below to narrow failures to the correct subsystem.

## Prerequisite Skills

This skill assumes you can build, flash, and monitor the device. If not, load:

- **build-flash-esp32** — Build, flash, and monitor workflows
- **coredump-esp32** — Reading and interpreting ESP32 coredumps

## Debugging Flow

```
Symptom → Monitor Logs → Status Route → Diagnostics → Coredump → Root Cause
```

### Step 1: Identify the Symptom

| Symptom | Likely Subsystem | First Evidence Source |
|---------|-----------------|----------------------|
| Device reboots repeatedly | Memory / Watchdog | Monitor logs, diagnostics |
| Web page doesn't load | Wi‑Fi / Web server | Status route, monitor |
| Route returns error | VM / BoxLang code | Monitor logs, status route |
| Background task stops | Fiber / Memory | Monitor logs, coredump |
| No serial output | HID mode / Console | Status route |
| Slow responses | Memory pressure / GC | Heap info, diagnostics |
| Panic at runtime | VM / Native code | Coredump |

### Step 2: Collect Monitor Logs

Open the serial monitor and collect boot and error logs:

```bash
espflash monitor --chip esp32s3 --port /dev/ttyACM0
```

**Key log patterns:**

```
[matchbox] ESP32 bundled runner starting                ← boot started
[matchbox] strict profile = ..., tree-shake target = ...   ← build config
[matchbox] bundled features = ...                       ← enabled features
[matchbox] diagnostics boot=N reset=REASON ...          ← boot diagnostics
[matchbox] Executing Application.onApplicationStart ... ← App.bx loading
[matchbox] Application fiber scheduler started          ← runAsync tasks live
[matchbox] Embedded app server listening on ...         ← web server ready
[matchbox] Executing embedded route method=... path=... ← route hit
```

**Common error patterns:**

```
[matchbox] Failed to deserialize application bytecode   ← corrupt flash / rebuild
[matchbox] Application fiber scheduler error: ...       ← background task crashed
[matchbox] Platform services failed: ...                ← Wi‑Fi / web init failed
Embedded route execution failed: ...                    ← BoxLang error in handler
```

### Step 3: Check the Status Route

The ESP32 runner exposes a diagnostics endpoint:

```
GET /__matchbox/status
```

Returns JSON:

```json
{
  "ok": true,
  "hostname": "matchbox-esp32",
  "ip": "192.168.4.1",
  "features": "...",
  "heap": {
    "free": 123456,
    "largestInternal8bitBlock": 100000,
    "freeInternal8bit": 200000
  },
  "diagnostics": {
    "bootCount": 5,
    "eventCount": 3,
    "resetReason": "POWERON_RESET",
    "lastResetReason": "PANIC",
    "lastEvent": "route /status hit",
    "nvsAvailable": true,
    "nvsStatus": "ok",
    "heap": { ... },
    "hid": { ... }
  },
  "routes": [...]
}
```

**Key fields to inspect:**

| Field | What to check |
|-------|--------------|
| `heap.free` | Below 50KB → memory pressure, GC or reduce allocations |
| `heap.largestInternal8bitBlock` | Below 20KB → heap fragmentation |
| `diagnostics.resetReason` | `PANIC` or `INT_WDT` → check coredump |
| `diagnostics.bootCount` | Rapidly increasing → reboot loop |
| `diagnostics.lastEvent` | Shows last recorded app event |
| `routes` | Verify all expected routes are listed |

### Step 4: Interpret Diagnostics

The diagnostics module tracks device health across reboots (persisted in NVS).

| Diagnostic | Meaning |
|-----------|---------|
| `bootCount` | Number of boots since flash. Spikes suggest reboot loops. |
| `resetReason` | `POWERON_RESET` = normal power-on. `PANIC` = software crash. `TASK_WDT` = task hung. `BROWNOUT_RESET` = check power. |
| `lastResetReason` | Reason for the *previous* reboot (persisted). |
| `nvsAvailable` | `false` → NVS partition issues, no persistence. |

### Step 5: Narrow by Subsystem

#### Web Route Failures

1. Check `GET /__matchbox/status` — is the server running?
2. Check monitor logs for `Executing embedded route` — is the route hit?
3. Check error message: `Embedded route execution failed: ...` — BoxLang error
4. Test with `curl`: `curl http://<ip>/your-route`

#### Wi‑Fi Issues

1. Does the status endpoint respond? → Wi‑Fi works
2. Check monitor logs for Wi‑Fi connect messages
3. Try connecting as station: `esp32WifiStation("ssid", "password")` in `onApplicationStart()`
4. If using AP mode, check the device broadcasts the configured SSID

#### Background Fiber (runAsync) Issues

1. Check monitor for `Application fiber scheduler` messages
2. If `Application fiber scheduler error: ...` → fiber crashed
3. Ensure `runAsync` callbacks have `sleep()` calls to yield
4. Check coredump for `matchbox_vm::fiber::Fiber::run` frames

#### Memory Pressure

1. Check `heap.free` on `/__matchbox/status` — consistently low?
2. Check `heap.largestInternal8bitBlock` — fragmented?
3. Reduce BoxLang allocations: smaller responses, clear temporary arrays
4. Enable GC: call garbage collection between requests is automatic
5. Consider adding `application.esp32.web.port` sleep between heavy routes

#### HID Mode (No Serial Output)

When USB HID is active, the serial console may be unavailable:

1. **Use the status route** as your primary debug tool
2. **Record diagnostic events** in BoxLang: route hit logs, state changes
3. **Check `/__matchbox/status`** for diagnostics snapshot including HID state
4. If you need serial back: temporarily disable HID in the build config and re-flash

### Step 6: Read the Coredump (if PANIC reset)

If `resetReason` is `PANIC`, the device wrote a coredump. See the **coredump-esp32** skill for the full workflow.

Quick reference:

```bash
espcoredump.py info_corefile \
  --chip esp32s3 \
  --port /dev/ttyACM0 \
  crates/matchbox-esp32-runner/target/xtensa-esp32s3-espidf/release/matchbox-esp32-runner
```

## Common Issues & Fixes

### "Embedded route execution failed"

**Cause:** BoxLang error in the handler (undefined variable, type mismatch, runtime error).

**Fix:**
1. Read the error message in monitor output
2. Check the handler `.bxs` or `.bxm` for the line referenced
3. Verify expected scopes (`url`, `form`, `cgi`) are used correctly

### Reboot Loop

**Cause:** Crash during `onApplicationStart()`, memory allocation failure, or watchdog.

**Fix:**
1. Check `resetReason` on status route
2. Read coredump for `PANIC` reason
3. Simplify `onApplicationStart()` temporarily, re-flash, then add back incrementally

### "Failed to deserialize application bytecode"

**Cause:** Bytecode on flash is corrupt or from a different build.

**Fix:**
```bash
matchbox --target esp32 --chip esp32s3 --full-flash app.bxs
```

### "Platform services failed"

**Cause:** Wi‑Fi or web server initialization failed.

**Fix:**
1. Check monitor for more specific error
2. Verify Wi‑Fi credentials in `application.esp32.wifi`
3. Check no other service is using port 80

## Reporting Checklist

When reporting an ESP32 app bug, include:

- [ ] **Reset reason** from `/__matchbox/status` diagnostics
- [ ] **Monitor output** from boot to failure
- [ ] **Heap snapshot** from status route
- [ ] **Coredump backtrace** (if PANIC reset)
- [ ] **Application.bx** and the failing route source
- [ ] **MatchBox version** (commit hash)
- [ ] **Steps to reproduce**: specific request, state, conditions

## Related Skills

- **build-flash-esp32** — Building, flashing, and monitoring
- **coredump-esp32** — Reading and interpreting coredumps
- **build-esp32-app** — Authoring ESP32 applications
