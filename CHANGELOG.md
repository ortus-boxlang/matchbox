# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Multi-target Native Fusion (Hybrid builds for Native, WASM, and JS).
- Dynamic JNI reflection bridge for Java interoperability.
- Persistent `BoxLangVM` for WASM with dynamic `call()` support.
- Automated JavaScript module generation via `--target js`.
- Member method delegation to BIFs (e.g., `"foo".ucase()`).
- High-performance integration testing macro (in-process execution).
- GitHub Actions for automated multi-platform Release and Snapshot builds.
- Tracing Mark-and-Sweep Garbage Collector.
- Hidden Classes (Shapes) and Monomorphic Inline Caches for performance.

### Changed
- Renamed project from `bx-rust` to `MatchBox`.
- Refactored binary into a library/binary hybrid.

### Fixed
- Object lifetime issues in JNI bridge for release builds.
- Mac runner pool assignment errors in GitHub Actions.
