# Handoff: Java façades and framework compatibility

## Decision and sequence

The approved sequence is:

1. Finish the module foundation in MatchBox (bounded foundation complete).
2. Build the optional **bx-matchbox-java** module (first slice started).
3. Hill-climb **TestBox**, then a minimal **ColdBox** application.

This document hands off milestones **2 and 3**. Milestone 2 has a working first slice; milestone 3 is not implemented. Do not start a general Java implementation, publish the SDK, or expand the image pilot as part of finishing milestone 1.

The goal is to run existing BoxLang framework APIs without a JVM. The shim implements a bounded set of Java-compatible interfaces using BoxLang and Rust; it does **not** execute Java bytecode, load arbitrary JARs, or promise full Java compatibility.

## Existing foundation and working copies

- MatchBox: `../matchbox-native-modules`, branch `feature/native-modules`.
- Current implementation checkpoint when this handoff was written: `d753fee`.
- Image pilot: `../bx-image-native-modules`, branch `feature/matchbox-native`; preserve the JVM backend.
- Canonical, read-only BoxLang reference: `../BoxLang`.
- Do not edit unrelated changes in the original MatchBox or reference worktrees.

Already implemented: precompiled native plugins; a stable capability-negotiated C ABI; embedded module/library bundles; verified extraction; retained VM values; runtime module lifecycle/settings; BIF/member annotations and invocation parity; live module/function/interceptor services; synchronous global custom interception.

At the original checkpoint above, descriptor-declared/annotated interceptors, module components, and dynamic/struct descriptor mappings were pending. Subsequent foundation slices implement those registrations, normal component bodies, and runtime private/public mappings with embedded BoxLang class lookup. The application envelope is now version 10; plugin ABI-1 libraries are unchanged.

**Milestone 1 is complete for the bounded synchronous module foundation.** The minimal module-owned Java class-provider boundary is now implemented separately from the embedded BoxLang catalog: runtime descriptor `classProviders` registrations route creation, imports, initialization, static references and property access through ordinary BoxLang/native values, with GC retention and owner cleanup. See `docs/native-plugins.md` for the implemented contract and explicit limitations. No plugin ABI/capability change was needed. Mutable lexical closure capture and BoxLang reentrant implicit-assignment scoping are now implemented through shared VM scopes, with JVM, GC, JIT-writeback, native-root, and source-free module contracts. Component non-local return/break/continue results and declaring-body scopes are now implemented, including source-free and GC contracts. As on the current JVM ASM backend, a forwarded break/continue exits the declaring function with null; components may consume these results instead. The VM retains its existing finally unwinding; the current JVM ASM snapshot skips finally on these generated component exits, so this is not claimed as backend parity. Live caller contexts, context-mediated named callbacks, active component states, and automatic module/function interception are now implemented and behavior-tested. The component flag/context/descriptor/service/source-loading audit is closed with explicit deferrals recorded in `docs/native-plugins.md#bounded-foundation-audit`: true output-policy flags, complete scope/descriptor reflection, other JVM subsystem event families, runtime source loading, and async contexts are not promised. Unsupported reflection/reload now fails instead of returning placeholders; module inventory and component ancestry expose actual runtime state. Full CFML/context scoping is not claimed. This document does not authorize SDK publication.

Important files:

- `crates/matchbox-modules/src/discovery.rs`, `crates/matchbox-modules/src/programs.rs`: discovery and module compilation.
- `crates/matchbox-vm/src/bundle.rs`: embedded application preparation and lifecycle.
- `crates/matchbox-vm/src/module_services.rs`: live records/services.
- `crates/matchbox-vm/src/plugins.rs`: native plugin loading and dispatch.
- `crates/matchbox-vm/src/vm/classes.rs`: module class-provider resolution and reference protocols.
- `crates/matchbox-vm/src/vm/scopes.rs`: shared local/argument capture, binding and JIT writeback.
- `crates/matchbox-plugin-sdk/`: independently versioned author SDK.
- `crates/matchbox-compiler/src/compiler/mod.rs`: imports and `new` lowering.
- `crates/matchbox-vm/src/bifs/mod.rs`: `createObject()` and existing native utilities.
- `tests/native_plugin_tests.rs`: end-to-end module/plugin contracts.

## Milestone 2: bx-matchbox-java

Project: `../bx-matchbox-java`, https://github.com/ortus-boxlang/bx-matchbox-java (branch `main`). Its first slice provides `java.lang.StringBuilder` creation/imports, default/string/integer-capacity construction, string append/chaining, UTF-16 length, and retained callbacks through an independent ABI-1 native provider. The same package uses real Java classes on JVM BoxLang. JVM/no-JNI/source-free bytecode/standalone contracts pass. This is a bounded starter, not completed TestBox compatibility or a general Java implementation; see that project's README for exact unsupported overloads/protocols and author-only SDK overrides. SDK publication and framework hill climbing remain deferred.

### Architecture boundary

```text
createObject("java", ...), new java:..., imported Java class references
                              |
                   MatchBox class resolution
                              |
                  module-registered class provider
                              |
                   BoxLang façade / Rust object
```

MatchBox owns resolution and the VM protocols needed to use the resulting values. The module owns the supported Java names and their behavior. Reuse the existing native-object, callback, rooting, and packaging machinery rather than introducing another object model.

Do not globally override `createObject()` or depend on interception of just that BIF. `new java:...` and imports now use the core resolver directly; `createObject()` uses that same resolution path. Class references, static members, initialization, and chaining must continue to share it. The JVM's ordinary Java creation path does not use `onCreateObjectRequest` as a universal hook.

Keep the compatibility module optional. Loading it must not start/download a JVM or invoke Cargo. Unsupported classes/methods must report clear errors; never return placeholders or pretend futures.

Any native SDK extension must be an additive, versioned capability. Published ABI-1 layouts and existing plugin binaries must remain valid. SDK publication is explicitly deferred; use the workspace SDK and local Cargo overrides for development.

### First vertical slice

Implement `java.lang.StringBuilder` as one complete class façade:

- Class lookup through the supported creation/import forms.
- `.init(...)`, `.append(...)`, `.toString()`, and chaining.
- Defined constructor/default/error behavior, verified against JVM BoxLang.
- Correct object lifetime across GC and nested callbacks.
- Direct execution, bytecode, and standalone execution after deleting package/source files.

The existing Rust-backed `StringBuilderObject` is a reuse candidate, not an automatic drop-in. Check Java's zero-based, end-exclusive and UTF-16 string semantics before sharing index-based operations.

Next, add only classes encountered while starting TestBox, likely `System`, file/path utilities, and ordered collections. Do not bulk-implement `java.*`.

### Semantic traps

- A Java class reference is not the same thing as an initialized instance.
- Static members and properties matter: `System.out.println(...)` is not just a constructor call.
- Collections require correct indexing, ordering, key sensitivity, iteration and membership semantics. An ordinary BoxLang struct is not automatically a `TreeMap`.
- Nulls, identity, casts, overloads, integer precision/overflow and exception behavior need explicit contracts.
- Current VM values/retained roots are thread-affine. Rust workers must not directly access them; threading needs a real VM execution/context design.
- Unknown JARs, engine internals, object serialization, Hibernate and coverage agents are not promised by this milestone.

## Milestone 3: framework-led hill climbing

### Choose TestBox first

Start with a synchronous CLI path, which is smaller than ColdBox's HTTP/application lifecycle. Pin a framework release and its dependency versions; record whether the exercised source is BoxLang or CFML compatibility code.

Initial acceptance gate:

1. Discover and execute one passing spec and one failing spec.
2. Produce correct assertions/results and process exit status.
3. Run without a JVM and without framework-specific runtime hacks.
4. Preserve the same observable framework API on JVM BoxLang.

Then grow through reporters, discovery, fixtures/lifecycle, metadata and MockBox. Java façades alone will not supply dynamic mixins, generated mock code, runtime class loading, or missing language semantics.

For each failure, classify it as:

- Module infrastructure.
- Java façade behavior.
- General BoxLang/CFML language or runtime compatibility.

Fix the shared boundary. Do not recognize framework filenames, rewrite their source heuristically, or accumulate fake service adapters. Initially use unmodified framework packages as the acceptance target; discuss any proposed upstream portability change before editing framework code.

### ColdBox gate

After the TestBox baseline, boot a minimal ColdBox application and serve one route. Expand through application/request lifecycle, mappings, WireBox, logging, caching and rendering as execution requires.

Executors, futures, proxies, scheduling, cancellation and concurrent collections are a separate substantial increment. Framework constructors can touch these APIs even when an application does not explicitly use async features; fail honestly until supported.

### Local reconnaissance (not exhaustive)

Copies previously inspected:

- TestBox 7.0.0+19: `/home/jacob/dev/jbeers/agentctl/testbox`.
- TestBox 6.5.0+18: `/home/jacob/dev/ortus-boxlang/mvm/testbox`.
- ColdBox 8.0.5+33: `/home/jacob/dev/jbeers/care2game/coldbox`.

Observed TestBox dependencies include `java.io.File`, `StringBuilder`, `System`, `TreeMap`, `Thread`, `Paths`, and (version 7 streaming) `ConcurrentLinkedQueue`; also `javacast`, class-name introspection, `cbstreams`, `cbMockData` and `globber`.

ColdBox includes engine/runtime detection, application mappings/custom-tag updates, Java concurrency, futures, functional proxies and class introspection. This inventory is a starting point, not a compatibility guarantee.

## Verification and checkpoints

Use behavior-focused red–green slices: one JVM-verified contract, one failing MatchBox test, the smallest shared implementation, then regressions. Preserve runnable checkpoints on feature branches.

```bash
cd ../matchbox-native-modules
export CARGO_TARGET_DIR=/home/jacob/dev/ortus-boxlang/matchbox/target
cargo test --test native_plugin_tests
cargo test
cargo test -p matchbox_compiler -p matchbox_vm -p matchbox-plugin-sdk
cargo clippy --lib -p matchbox -p matchbox_compiler -p matchbox_vm
```

The foundation exit run passed the full root suite (546 compatibility transfers, 26 explicit exclusions; 116 integration and 51 native-plugin contracts), compiler/VM/SDK unit/doc tests, and library Clippy (existing warnings remain). All 51 native-plugin contracts also passed with JNI disabled. Direct, source-free bytecode/standalone, nested callback, retained-context/GC and failure-unwind contracts pass. The existing precompiled image plugin is unchanged and its no-JNI contract still prints `bx-image native contract: OK`. JVM probes verified module lifecycle ordering, function argument/result/error interception, contexts, and real module inventory. Three reflection/reload transfers are now honest exclusions rather than placeholder-based passes. Re-run these gates after changing the foundation.

Verify unchanged image-plugin loading when changing VM/SDK boundaries. Consumers must continue to need neither Rust nor Cargo, the SDK, startup downloads, or a JVM.

Successful next checkpoints are **one real façade**, **one real TestBox suite**, and **one real ColdBox route**—not exhaustive module APIs, a Java emulator, or a release-automation project.
