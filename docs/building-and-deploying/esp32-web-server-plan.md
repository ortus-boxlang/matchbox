# Plan: MatchBox Embedded Web Server (ESP32)

This document outlines the strategy for implementing a lightweight, BoxLang-compatible web server for the ESP32 MatchBox runner.

## Core Objective
To provide a functional web runtime for ESP32 that can serve `.bxm` templates and handle HTTP requests (URL/Form scopes) without the overhead of a full-blown Rust web framework like Axum or Tokio.

## Architectural Approach

### 1. Leverage ESP-IDF Native Server
Instead of bundling a new HTTP server, we will use the `esp-idf-svc` crate, which provides a Rust interface to the high-performance, C-based HTTP server built into the ESP-IDF.

### 2. The Request Handler Bridge
A native Rust handler will be registered for specific URI patterns (e.g., `/*.bxm`). When a request is received:
1. **Request Parsing:** Extract query parameters and form data.
2. **Scope Mapping:** Convert request data into `matchbox_vm::types::BxStruct` objects for the `url` and `form` scopes.
3. **Bytecode Execution:** Look up the pre-compiled bytecode for the requested path and execute it within a MatchBox VM instance.
4. **Buffered Response:** Capture the VM's `output_buffer` and send it back via the ESP-IDF response stream.

## Implementation Phases

### Phase 1: ESP-IDF Integration
* Update `matchbox-esp32-runner` dependencies to include `esp-idf-svc` with the `http` feature.
* Scaffold a basic HTTP server initialization in `main.rs`.

### Phase 2: Scope Injection Logic
* Implement a `populate_embedded_scopes` function that translates `esp_idf_svc::http::server::Request` data into MatchBox `BxValue` types.
* Support `url`, `form`, and `cgi` (e.g., `remote_addr`, `method`).

### Phase 3: Template Execution
* Integrate the pre-compiled bytecode storage (already present in the runner) with the HTTP handler.
* Enable `output_buffer` in the VM for every request.

## Optimization for ESP32
* **Zero-Copy Parsing:** Use the ESP-IDF's internal buffers where possible.
* **Pre-Compiled Templates:** Templates are transpiled to bytecode on the dev machine, so the ESP32 only performs lightweight VM execution.
* **Selective Buffering:** For large responses, investigate streaming `writeOutput` directly to the ESP-IDF socket to save RAM.

## Future Possibilities
* **WebSocket Support:** Use the ESP-IDF's WebSocket implementation for real-time dashboards.
* **SPIFFS/LittleFS Integration:** Automatically serve `.bxm` files stored on the ESP32's internal flash.
