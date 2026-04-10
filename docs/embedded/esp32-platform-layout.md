# ESP32 Platform Layout

## Direction

The ESP32 runner should become a bundled embedded platform rather than a thin wrapper around the full MatchBox VM.

The platform should expose a strict BoxLang profile and compile in useful device capabilities behind feature flags.

## Bundled Features

The runner now defines platform features for:

- `platform-web`
- `platform-mdns`
- `platform-camera`
- `platform-bluetooth`
- `platform-pins`
- `platform-sdcard`
- `platform-printer`
- `psram`

`strict-profile` is enabled by default.

## Intended Architecture

1. `matchbox-embedded`
   - shared embedded route/request/response model
   - strict profile validation
   - future execution core and AOT-friendly contracts

2. `matchbox-esp32-runner`
   - bundled device platform
   - feature-gated hardware services
   - boot and system integration
   - strict built-in Wi-Fi and tiny HTTP control surface first
   - thin entrypoint into the embedded profile

## Tree-Shaking Direction

The deployment path should tree-shake in two places:

1. BoxLang level
   - compile only reachable handlers, services, and helper code for the embedded profile

2. Rust level
   - compile out unused bundled platform features with Cargo features and linker dead-code elimination

The long-term target is:

- strict embedded BoxLang subset
- merged file routing for `.bxm` and `.bxs`
- direct request scopes: `url`, `form`, `request`, `cgi`
- compile-time feature validation
- minimal bundled platform surface
- no general-purpose dynamic runtime on device
