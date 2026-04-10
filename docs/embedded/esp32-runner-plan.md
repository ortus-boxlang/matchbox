# ESP32 Runner Plan

## Goal

Create an embedded MatchBox execution profile for ESP32 that preserves the BoxLang authoring model while dropping the parts of the current runtime that are too dynamic and memory-heavy for microcontrollers.

The immediate target is:

- Wi-Fi connect and device identity
- small HTTP control surface
- camera / BLE / printer orchestration from BoxLang
- low and predictable RAM use

## Non-Goals

The ESP32 profile should not try to match the full native or JVM runtime surface.

Out of scope for the embedded profile:

- general-purpose dynamic web framework behavior
- template rendering and static file serving
- sessions, cookies, webhooks, websockets
- dynamic module loading on device
- repeated app interpretation per request
- large binary payloads moving through generic VM values

## Runtime Split

The MatchBox repo should keep two execution personalities:

1. `matchbox-vm`
   - full dynamic VM
   - native / wasm oriented
   - highest compatibility target for the JVM runtime

2. `matchbox-embedded`
   - constrained embedded execution core
   - low-allocation request handling
   - typed and profile-validated module contracts
   - optimized for ESP32-class devices

The compiler frontend should stay shared. The backend/runtime strategy should split by target profile.

## ESP32 Runner Responsibilities

`matchbox-esp32-runner` should become an integration crate around `matchbox-embedded`, not a place that re-implements BoxLang semantics ad hoc.

It should own:

- boot and task startup
- Wi-Fi lifecycle
- optional mDNS / hostname advertisement
- embedded HTTP server startup
- device-level logging and diagnostics
- hardware service registration
- flash / partition bootstrapping

It should not own:

- generic language execution semantics
- route table data model
- request / response BoxLang abstractions
- embedded profile validation rules

Those belong in `matchbox-embedded`.

## Embedded Contract

The embedded profile should preserve the BoxLang feel but narrow the contract:

- file-routed apps only
- typed public APIs
- stable object shapes
- native handles for heavy resources
- lean request scopes only
- compile-time rejection for unsupported features

Examples of preserved concepts:

- classes / components / services
- route handlers and compile-time templates
- functions, loops, conditionals, arrays, structs
- native modules accessed through BoxLang objects

Examples of constrained concepts:

- dynamic method lookup
- arbitrary long-lived object shape mutation
- broad reflection features
- large byte arrays as normal app-level values
- framework-style request context objects such as `event`, `rc`, and `prc`

## File Routing Contract

The embedded profile should use one merged file-routing model.

Recommended shape:

- `app/index.bxm` -> `GET /`
- `app/status.bxm` -> `GET /status`
- `app/print.post.bxs` -> `POST /print`
- `app/printer/[id].bxm` -> `GET /printer/:id`
- `app/printer/[id].delete.bxs` -> `DELETE /printer/:id`

Rules:

- `.bxm` files compile to extensionless `GET` routes
- `.bxs` files can use method suffixes such as `.get`, `.post`, `.put`, `.patch`, `.delete`
- `index` maps to the containing directory root
- bracket placeholders such as `[id]` map to named URL params

## Request Scope Contract

The embedded profile should expose direct BoxLang scopes instead of framework request objects:

- `url`
- `form`
- `request`
- `cgi`

Route params should be merged into `url`, with route placeholders winning over query-string collisions.

## Module Model

Modules remain composable, but the embedded profile requires a stricter ABI.

Module requirements:

- typed inputs and outputs
- stable BoxLang-facing contracts across targets where possible
- native-backed handles for large resources
- no expectation of broad runtime reflection

Embedded-native modules should own:

- camera frame buffers
- image conversion buffers
- BLE connection state
- Wi-Fi / device state

BoxLang should orchestrate those resources instead of holding large copies of them.

## Migration Phases

### Phase 1: Scaffold and Boundaries

- add `matchbox-embedded` crate
- define embedded profile types and responsibilities
- stop adding more logic directly to the ESP32 runner template

### Phase 2: Request / Route Core

- move file-route parsing / matching into `matchbox-embedded`
- define request / response / scope structs
- define an embedded app lifecycle

### Phase 3: Execution Core

- add a constrained execution engine for embedded handlers
- remove repeated runtime rebuilds
- support one-time boot and request reuse

### Phase 4: Service Handles

- define native handle model for camera, imaging, BLE, printer resources
- adapt ESP32 modules to the embedded contract

### Phase 5: Compiler / Validation

- formalize the `embedded` profile validator
- reject unsupported language and runtime features clearly
- prepare for stricter typing and future AOT work

## Near-Term Implementation Priorities

1. Create `matchbox-embedded` and move the route/request/response model there.
2. Make `matchbox-esp32-runner` depend on that crate.
3. Stop using the full runtime model for embedded web dispatch.
4. Add separate internal-RAM and PSRAM diagnostics to the runner.
5. Make ESP32 modules target the embedded module contract.

## AOT Direction

The embedded crate should be designed so it can later support:

- stricter typed execution
- precomputed route tables
- AOT-friendly module bindings

It should not assume the current bytecode VM is the final execution model for embedded targets.
