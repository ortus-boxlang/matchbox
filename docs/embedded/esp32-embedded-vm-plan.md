# ESP32 Embedded VM Plan

## Why This Exists

The current ESP32 path proved that:

- embedded routing and storage-loaded app artifacts are workable
- simple `.bxs` and `.bxm` routes can run on-device
- the general MatchBox VM execution model remains too expensive for heavy ESP32 workflows
- direct use of existing module objects and futures is still too costly for camera, bitmap, and BLE print paths

This branch changes direction.

The goal is no longer "make `matchbox-vm` fit ESP32 well enough."
The goal is:

- keep the BoxLang parser and compiler frontend
- define a separate embedded execution runtime for ESP32
- make the runtime intentionally ESP32-shaped
- expose only a small set of native-backed BoxLang primitives

## Product Goal

Allow BoxLang developers to deploy useful ESP32 device apps while accepting an embedded-specific programming model.

That means:

- BoxLang remains the authoring language
- ESP32 developers must learn the embedded runtime ergonomics
- hardware-heavy work stays native
- the runtime is allowed to diverge from the main MatchBox VM where required

## Non-Goals

This runtime does not try to provide:

- high compatibility with the full MatchBox VM execution model
- broad support for existing MatchBox native modules
- a general-purpose dynamic web framework
- a "desktop/server BoxLang on a microcontroller" illusion

## Guiding Constraints

- No per-request VM creation.
- No route bytecode deserialization on each request.
- No module/future/object-heavy path for camera, bitmap, BLE, GPIO, or storage.
- Keep large binary buffers out of BoxLang values wherever possible.
- Favor native handles, native services, and compact structs over rich dynamic objects.
- Preserve shared parser/compiler code when it helps. Fork runtime behavior aggressively when it does not.

## Target Architecture

### 1. Shared Frontend

Keep using shared code for:

- BoxLang parser
- BXM parser
- semantic analysis where practical
- embedded route discovery
- embedded compile pipeline

### 2. ESP32 Embedded Backend

Build a separate embedded runtime contract that owns:

- route loading
- request scope binding
- native BIF registration
- response generation
- native resource lifecycle

This runtime should not depend on the current `matchbox-vm` request execution path for heavy workflows.

### 3. Native ESP32 Platform Layer

Implement direct ESP32-native services inside the embedded runtime or closely-owned crates:

- camera capture
- bitmap conversion
- BLE printer communication
- Wi-Fi
- mDNS
- GPIO / pins
- SD card / storage

These should be exposed as a small number of explicit BIFs or native intrinsics.

## BoxLang Contract

The embedded contract should be intentionally narrow.

### Request Model

Supported request scopes:

- `url`
- `form`
- `request`
- `cgi`

Avoid `event`, `rc`, and `prc`.

### Routing Model

Keep the file-routed embedded model:

- `app/index.bxs` -> `GET /`
- `app/status.bxs` -> `GET /status`
- `app/hello/[name].bxm` -> `GET /hello/:name`
- `app/print.post.bxs` -> `POST /print`

### Runtime Shape

The embedded runtime should support:

- small route handlers
- compile-time templates
- simple structs, arrays, strings, conditionals, loops
- direct native BIF calls

It should reject or avoid:

- rich native object graphs
- futures/promises for local device operations
- module-heavy dynamic loading
- broad class instantiation during request execution

## Native BIF Direction

Heavy device workflows should be exposed as direct ESP32 BIFs.

Examples:

- `esp32CameraCapture()`
- `esp32BitmapFromJpeg(bytes)`
- `esp32BlePrintBytes(payload)`
- `esp32CaptureAndPrint()`

These should return compact BoxLang structs, not large native-object wrappers.

Preferred result shapes:

- dimensions
- format
- byte counts
- device name / id
- success or error data

Avoid returning large image byte arrays unless the caller explicitly asks for them and the runtime can afford it.

## Deployment Model

Long-term target:

- prebuilt ESP32 runner firmware
- storage-loaded app artifact
- app updates without Rust rebuilds
- app deploys without full firmware reflashing

This plan should continue preserving that direction.

## Implementation Phases

### Phase 1: Stabilize Branch Direction

- remove dependency on module-heavy ESP32 paths for core device workflows
- keep current bundled runner boot, routing, and artifact loading
- add direct native BIFs for camera and print workflow
- validate `/print` through the native BIF path

### Phase 2: Define Embedded Runtime Boundary

- identify which parts of `matchbox-vm` remain usable
- identify what must move into a dedicated embedded executor
- document the minimum executable BoxLang subset for ESP32

### Phase 3: Separate Embedded Execution Core

- introduce a dedicated embedded execution layer or crate
- keep route execution small and long-lived
- bind request scopes without rebuilding general runtime state
- support direct BIF dispatch as the primary native interface

### Phase 4: Strip Main-VM Assumptions

- remove per-request execution patterns that only exist due to general VM expectations
- stop depending on module object/future semantics for embedded platform features
- move embedded-only optimizations behind explicit code boundaries

### Phase 5: Board Profiles

- add board-specific config for XIAO ESP32S3 Sense and similar boards
- add PSRAM profiles
- add camera pin presets
- keep `box.json` overrides

## Immediate Work Queue

1. Get the new direct ESP32-native `/print` path compiling and running.
2. Remove unnecessary dependency on the existing module object model in the request path.
3. Decide whether the next executable step is:
   - a smaller adapter around `matchbox-vm`
   - or a true embedded executor crate.
4. Keep comments in shared VM code marking:
   - legacy execution path
   - embedded borrowed execution path
   so later unification stays possible.

## Branch Policy

This branch is allowed to:

- make hard embedded-specific tradeoffs
- introduce an embedded-only runtime contract
- diverge from the main VM where memory or predictability requires it

This branch should avoid:

- dragging desktop/server assumptions into the embedded runtime
- forcing the main VM to adopt ESP32-specific semantics prematurely
