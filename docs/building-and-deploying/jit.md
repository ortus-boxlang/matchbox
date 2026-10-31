# JIT Compilation

MatchBox includes a multi-tier JIT compiler built on [Cranelift](https://cranelift.dev/). It profiles hot code at runtime and compiles it to native machine code, eliminating interpreter dispatch overhead for the loops and functions that matter most.

---

## Where JIT is Available

| Build target | JIT active? | Reason |
| :--- | :---: | :--- |
| Native CLI (`matchbox script.bxs`) | Yes | Enabled by default in the native CLI binary |
| Desktop native runner stubs | Opt-in | The CLI forwards its `jit` feature; runners initialize Cranelift before plain or module-envelope execution |
| WASM (browser / container) | No | Cranelift does not target WASM; the browser's own JIT runs the WASM module |
| ESP32 / embedded | No | The `matchbox-esp32-runner` crate is workspace-excluded and has no Cranelift dependency |

The JIT is compiled in via the `jit` Cargo feature, which is included in the default feature set of the `matchbox` CLI crate. The `matchbox_vm` library and desktop runner keep `jit` opt-in (default off). JIT-enabled CLI builds forward the feature only to desktop runner stubs, never WASM/WASI/ESP32. Plain bytecode and module envelopes initialize the same JIT policy. Cached desktop stubs record their feature set; Cargo watches these shared feature sidecars so switching target directories reruns validation before embedding a stub. Builds with differing feature sets must still be serialized. `python3 tests/stub_features.py` checks JIT→non-JIT→JIT builds and successful source-free native function/loop execution.

For a no-JNI JIT-enabled author build:

```bash
cargo build --no-default-features --features 'bif-io,bif-cli,bif-crypto,bif-http,jit' --target-dir target/lint-jit
python3 tests/jit_runner.py target/lint-jit/debug/matchbox
```

The process check removes source/module copies, verifies numeric results, and requires actual Cranelift compilation/execution of compiler-generated numeric/default/named-argument leaves, binary-search/counting function loops, and an empty-loop control, in source, plain/module `.bxb`, and standalone paths. Set `MATCHBOX_JIT_TRACE=1` to log initialization and hot functions rejected by the translatability check; compiled-function messages include names. Trace output goes to stderr; a first-successful-execution message distinguishes native execution from compilation followed only by deopts. Enabling the feature alone is not evidence that application functions compiled.

---

## The Four Tiers

The JIT uses a tiered strategy: cheap profiling runs first, and compilation only fires once a site has proven itself hot.

### Tier 1 — Empty loop elimination

**Trigger:** A `for` loop whose body is empty, after ≥ 10 000 iterations.

The entire loop is replaced by a single `select` instruction: the counter jumps straight to its final value in one native call. No iteration actually occurs.

### Tier 2 — Numeric loop body

**Trigger:** A `for` loop with a numeric-only body, after ≥ 10 000 cumulative iterations (counter persists across fiber scheduler quanta).

The loop body is translated to Cranelift SSA IR. Local variables live in registers throughout; only the final values are written back to the VM's locals array on exit. Inline-cached struct member reads (monomorphic or bi-morphic) are also supported.

### Tier 3 — Array iteration

**Trigger:** A `for-in` loop over a float-only array, after ≥ 5 000 cumulative iterations.

The array data pointer and length are passed directly to the compiled loop, bypassing the iterator protocol entirely.

### Tier 4 — Hot function compilation

**Trigger:** A function body that passes a translatability check, after ≥ 100 calls.

Compilation starts at the body immediately after `FUNCTION_ENTER`, not at the function's prologue. Ordinary VM frames still bind positional/named/default arguments, initialize scopes and run entry interception; compiled results use ordinary return/exit interception. All call paths that execute this marker are covered. Debugger stepping stays interpreted.

The body-entry subset is readonly, forward-only leaves, plus bounded terminal record returns:

| Category | Opcodes |
| :--- | :--- |
| Parameters | `GET_LOCAL` within declared arity, including the arguments-scope slot |
| Constants | Number, Boolean, null |
| Arithmetic | `ADD`, `SUBTRACT`, `MULTIPLY`, `DIVIDE` and float variants |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=` |
| Boolean | `NOT`, Boolean-guarded conditions |
| Readonly collections | Guarded `INDEX` on arrays/plain structs and lowercase constant `MEMBER` on plain structs |
| Core calls | Proven `int`, `isStruct`, `structKeyExists` calls within one expression block |
| Control flow | `JUMP`, `JUMP_IF_FALSE` (forward jumps) |
| Stack | `POP`, `DUP` |
| Return | `RETURN` |
| Literal records | Terminal `STRUCT; RETURN`, at most two literal string keys with parameter values |

Arithmetic/comparison guards require non-NaN floats. Integers, coercions, non-Boolean truthiness, zero divisors and NaN-producing arithmetic safely **deopt to body entry**, without replaying defaults/interception or losing scope mutations. For small records, machine code loads parameter values into a fixed two-slot result buffer; the epilogue performs terminal allocation through the same `literal_struct` VM service as the interpreter. Key interning/presentation and shared object identity are preserved. Inputs remain rooted in live frame scopes; heap allocation does not collect, and ordinary return handling roots the result. No allocations or side effects precede a possible deopt. Each core call requires the original native callee identity through the ordinary private-scope resolver at every native entry. Intrinsic references cannot escape as values or through branch spills. Struct index keys must already be strings; coercions, custom callees and unsupported receivers deopt. Reads observe current values/layouts, null versus missing and case-sensitive keys. General writes, calls and allocation remain interpreted; the old direct compiled-callee path and relaxed coercion helpers were removed because they bypassed these semantics. Program ownership guards prevent stale function/address reuse across GC; JIT state remains VM-local.

### Bounded function-loop regions

A second entry point operates at the **first ordinary `LOOP` backedge** after
VM declarations and one interpreted iteration. It shares the function compiler;
it does not recognize/reimplement a particular BoxLang helper. After 100 such
invocations, it can compile the remaining region with:

- Numeric `SET_LOCAL`/`SET_LOCAL_POP` to already-bound named locals, not parameters.
- Guarded `INDEX` reads from arrays/plain structs and `MEMBER` reads from plain structs.
  Member source names must already be lowercase, so compilation of an untaken
  branch cannot upgrade key presentation. Folded/exact IDs handle case-sensitive
  records; missing keys, instances and proxies deopt.
- Proven `int`, `isStruct` and `structKeyExists` calls within an expression block. Each native
  entry uses the **same private-scope resolver as the VM** and requires the core
  native function's identity. Captured/rebound/custom callees stay interpreted;
  native truncation preserves the current MatchBox BIF, not claimed full JVM
  negative-number/BigDecimal parity.
- Backedges to region entry, forward branches and numeric returns. Nested loop
  targets, multiword compare-jumps and general calls remain unsupported.

A fixed **32-slot** checkpoint stages numeric writes in the VM stack, with no
heap writes, calls or collection during the kernel. Guards discard the entire
attempt and resume the OSR entry, without replaying earlier declarations/events
or the first interpreted iteration. Reading this frame's declaring scope through
an alias deopts: its dictionary must not observe uncommitted stack writes.
Completion commits named locals and uses ordinary return/exit interception.
After **1024 native backedges**, progress is committed and control returns to the
VM scheduler/GC before resuming at region entry. Parameters, captures and frame
roots stay intact. Debugger execution stays interpreted.

Only one profiling/guard attempt is made per invocation, except after budget
yields; unsupported loops do not pay profiling on every interpreted iteration.
Cache keys include the immutable program owner, region entry, slot count and
writable mask. Interner IDs/machine code remain VM-local. Public checks cover
scope aliases, late guard rollback/errors, shadowed intrinsics, defaults/named
calls, GC, scheduler chunks, cross-VM copied metadata and 402 pre/post event pairs.

This enables bx-lint's actual `bxAstTokenIndex` binary-search body. In a bounded
Linux standalone probe, 10,000 parser-token searches improved **254→87ms**, while
the no-loop control stayed **50ms**. Whole-LSP open/edit medians moved only
**768→750ms / 759→748ms**, with overlapping ranges and slower completion; this is
not a general analyzer speed guarantee or justification for arbitrary expansion.

### Append-only construction loops

A separate bounded loop entry compiles **the actual compiler-generated loop**,
including its branches, indexed reads, `ascii` calls, `arrayAppend` operations and
calls to proven parameter-only record constructors. There is no native analyzer
or text-mapping BIF. It shares the Tier-4 SSA compiler and activates after 10,000
cumulative backedges. Fused `LOCAL_JUMP_IF_NE_CONST` branches are lowered with
rebased targets; named numeric `INC_LOCAL` is supported. An invocation-owned
negative feasibility latch skips repeated scans of the last unsupported loop;
it stores only an instruction offset, never GC values or shared-program IDs.

Construction calls require fresh ordinary callee resolution on every native
entry. Core BIF identity and immutable record-body ownership are guarded. Record
constructors have at most two parameters/fields, lowercase literal keys and
non-NaN float arguments; null/defaults, custom callees and unsupported values
remain interpreted. Function interception and debugger stepping disable this
entry. Early returns, nonlocal exits, nested loops, general calls, arbitrary heap
writes and collection constructors remain unsupported.

Array appends use the normal VM write barrier. Each transaction checkpoints at
most 32 append targets and 32 named slots. A late guard failure truncates the
appended tails and restores staged locals before interpreter replay; it never
repeats previously committed chunks. Record allocation uses `literal_struct`;
allocation does not collect or invoke callbacks, and output arrays/frame values
root results before the next VM safepoint. No borrowed object/array pointer is
retained across allocation. After 1024 backedges, progress is committed and the
ordinary scheduler/GC runs before re-entry. No GC IDs enter shared program or
compiled call metadata.

```bash
python3 tests/jit_construct.py target/lint-jit/debug/matchbox ../bx-lint
```

This actual-process check extracts the real BoxLang `bxTextMap` loop and requires
native execution in source, source-free bytecode and standalone paths. It checks
supplementary Unicode, UTF-8 continuation slots, UTF-16 positions, CRLF, EOF,
independent output, late append rollback/errors, null/default replay, early
returns, dynamic intrinsic/helper rebinding and named arguments. It is a bounded
construction subset, not whole-program compilation.

---

## Optional Parameter Type Hints

Existing BoxLang parameter declarations (`numeric`, `boolean`, `array`, `struct`,
`string`) are retained as immutable `FUNCTION_ENTER` metadata. Recognized hints
are checked after ordinary defaults/events and at every native re-entry. A mismatch
falls back to the interpreter; declarations do not authorize unchecked casts or
change existing dynamic argument behavior. Unknown types and `any` are ignored.

Guarded numeric parameters, numeric constants and non-NaN arithmetic results feed
SSA facts, avoiding redundant numeric guards. Parameters remain readonly; hints
prove neither record layouts nor element types. Return/local declarations, coercion
contracts, structural/generic types and a static/gradual type checker are not added.
This is an optimizer prototype using existing syntax, not a complete type system.

`tests/jit_runner.py` proves real source-free typed/readonly compilation and
execution, plus wrong-hint fallback, dynamic callees, branch-returned intrinsic
references, live layouts, nulls, Unicode/case-sensitive keys, index errors and GC.
In the bounded bx-lint experiment, JIT changes improved real-file edit medians
about 3–4%; two added annotations produced no consistent additional gain. Frames,
allocation and most traversal still remain interpreted.

---

## What Is Not Supported in Tier 4

The following categories cause a function to remain interpreted:

- **Global variables** — `GET_GLOBAL`, `SET_GLOBAL`, `DEFINE_GLOBAL`, `SET_PRIVATE`, and `GET_PRIVATE` except proven guarded core intrinsics
- **I/O** — `PRINT`, `PRINTLN`
- **Object / collection creation** — `NEW`, `ARRAY`, and `STRUCT` outside the bounded terminal-record pattern
- **General property/index operations** — writes, instances/proxies and key coercions; readonly array/plain-struct reads use guards
- **General calls** — all except proven core `int`, `isStruct`, `structKeyExists`; `INVOKE`, `INVOKE_NAMED`, `CALL_NAMED` remain interpreted
- **Declarations/parameter mutation** — `BIND_LOCAL` and writes outside the staged named-local subset
- **Strings/remainder/integer specializations** — `STRING_CONCAT`, `MODULO`, `ADD_INT`, `SUB_INT`, `MUL_INT`
- **General function loops** — `FOR_LOOP_STEP`, `ITER_NEXT`, nested `LOOP` targets
- **Exception handling** — `PUSH_HANDLER`, `POP_HANDLER`, `THROW`
- **Multi-word compare-jumps** — `LOCAL_COMPARE_JUMP`, `GLOBAL_COMPARE_JUMP`, `COMPARE_JUMP`, `LOCAL_JUMP_IF_NE_CONST`
- **Miscellaneous** — `INC`, `DEC`, `INC_LOCAL`, `INC_GLOBAL`, `SWAP`, `OVER`, `JUMP_IF_NULL`

This bounded subset enables numeric parameter-only helpers, bx-lint's `bxPosition`/`bxRange` and its nonallocating binary-search loop and readonly `bxGet`, not general functions or recursion. The append-only entry additionally enables bx-lint's `bxTextMap` construction loop. Compiled machine code and its immutable program/key metadata live until the VM is dropped; memory and application latency must still be measured.

---

## Opting Out

If you need to build the CLI without JIT (e.g. to reduce binary size for a constrained native target):

```bash
cargo build --release --no-default-features \
  --features "bif-io,bif-jni,bif-crypto,bif-cli"
```

---

## Future Directions

### Measure eligibility, not just activation

A historical bounded bx-lint LSP experiment, before body-entry compilation, initialized Cranelift and reached the hot-function threshold in 35 helpers, but compiled none: all observed candidates were rejected for unsupported bytecode. Opening a 200-function script remained about 1.7 seconds, with or without JIT. Even the current pure-math `jit_hot_fn.bxs` functions were rejected in that build; existing output-only tests do not prove Tier-4 activation. The current runner check above additionally proves real compiler-function compilation after the body-entry fix. These are build/workload observations, not a general JIT speed guarantee.

The tested Linux bx-lint standalone grew from 5,821,860 to 7,607,060 bytes; actual linked size depends on target, features and optimization. Do not assume a fixed Cranelift size penalty.

### Wider Tier-4 opcode coverage

The most impactful missing opcodes for real-world functions are globals and property access. Runtime helpers such as `jit_ic_member_fallback` illustrate guarded reads. The main constraint is the deopt story: any operation that can throw or have side effects needs a clean deopt path back to the interpreter.

### String constant support in Tier-4

General string values remain unsupported. Adding them requires GC-rooted VM values and safepoints; embedding unguarded GC IDs in machine code is not safe.

### Tier-4 for embedded loops

Functions containing `FOR_LOOP_STEP` are currently rejected by `fn_is_translatable`. Supporting them would require the Tier-2 loop compiler to operate within Tier-4's SSA frame, essentially merging the two tiers. The deopt model is the main complexity: a loop inside a compiled function that deopts mid-iteration needs to restore both the function's locals and the loop counter.
