# JIT Compilation

MatchBox includes a multi-tier JIT compiler built on [Cranelift](https://cranelift.dev/). It profiles hot code at runtime and compiles it to native machine code, eliminating interpreter dispatch overhead for the loops and functions that matter most.

---

## Where JIT is Available

| Build target | JIT active? | Reason |
| :--- | :---: | :--- |
| Native CLI (`matchbox script.bxs`) | Yes | Enabled by default in the native CLI binary |
| Native runner stubs (cross-deployed) | No | Stubs are built without the `jit` feature to keep their embedded size down |
| WASM (browser / container) | No | Cranelift does not target WASM; the browser's own JIT runs the WASM module |
| ESP32 / embedded | No | The `matchbox-esp32-runner` crate is workspace-excluded and has no Cranelift dependency |

The JIT is compiled in via the `jit` Cargo feature, which is included in the default feature set of the `matchbox` CLI crate. The `matchbox_vm` library crate keeps `jit` opt-in (default off) so that consumers — the ESP32 runner, WASM runner, and native runner stubs — remain lean unless they explicitly request it.

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

The entire function body is compiled to a native `fn(locals_ptr, heap_ptr, out_val_ptr) -> status` function. Supported operations:

| Category | Opcodes |
| :--- | :--- |
| Locals | `GET_LOCAL`, `SET_LOCAL`, `SET_LOCAL_POP` |
| Constants | Numeric literals |
| Arithmetic | `ADD`, `SUB`, `MUL`, `DIV`, `MOD` (float and int variants) |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=` |
| Boolean | `NOT` |
| Control flow | `JUMP`, `JUMP_IF_FALSE` (forward jumps) |
| Stack | `POP`, `DUP` |
| Calls | `CALL` (direct calls to other JIT-compiled functions) |
| Strings | `STRING_CONCAT` (`&` operator, via `jit_concat` runtime helper) |
| Return | `RETURN` |

If any unsupported opcode is present, the function is silently skipped — it continues running in the interpreter with no correctness impact. If a type guard fails at runtime (e.g. a variable expected to be numeric turns out to be a heap pointer), the compiled frame **deopts**: execution transfers back to the interpreter at the current bytecode position and the function is not recompiled.

---

## What Is Not Supported in Tier 4

The following categories cause a function to remain interpreted:

- **Global variables** — `GET_GLOBAL`, `SET_GLOBAL`, `DEFINE_GLOBAL`, `GET_PRIVATE`, `SET_PRIVATE`
- **I/O** — `PRINT`, `PRINTLN`
- **Object / collection creation** — `NEW`, `ARRAY`, `STRUCT`
- **Property and index access** — `MEMBER`, `SET_MEMBER`, `INC_MEMBER`, `INDEX`, `SET_INDEX`
- **Dynamic calls** — `INVOKE`, `INVOKE_NAMED`, `CALL_NAMED`
- **Loops inside functions** — `FOR_LOOP_STEP`, `LOOP`, `ITER_NEXT`
- **Exception handling** — `PUSH_HANDLER`, `POP_HANDLER`, `THROW`
- **Multi-word compare-jumps** — `LOCAL_COMPARE_JUMP`, `GLOBAL_COMPARE_JUMP`, `COMPARE_JUMP`, `LOCAL_JUMP_IF_NE_CONST`
- **Miscellaneous** — `INC`, `DEC`, `INC_LOCAL`, `INC_GLOBAL`, `SWAP`, `OVER`, `JUMP_IF_NULL`

In practice, Tier 4 is most effective for pure computational functions: numeric kernels, string-building loops, recursive math, and hot helper functions that operate only on their arguments and local variables.

---

## Opting Out

If you need to build the CLI without JIT (e.g. to reduce binary size for a constrained native target):

```bash
cargo build --release --no-default-features \
  --features "bif-io,bif-jni,bif-crypto,bif-cli"
```

---

## Future Directions

### JIT in native runner stubs

The `build.rs` feature-forwarding block (lines 58–64) explicitly lists which features are passed to `matchbox-runner` stub builds. Adding `"jit"` there would give deployed native runner stubs the same JIT benefits as the CLI's local interpreter. The trade-off is binary size: Cranelift adds roughly 10–20 MB per stub, which multiplies across targets in cross-compile mode. When that trade-off becomes acceptable, the change is a one-liner in `build.rs`:

```rust
if env::var("CARGO_FEATURE_JIT").is_ok() { features.push("jit"); }
```

### Wider Tier-4 opcode coverage

The most impactful missing opcodes for real-world functions are globals and property access. Both are achievable with runtime helpers following the same pattern as `jit_concat` and `jit_ic_member_fallback`. The main constraint is the deopt story: any operation that can throw or have side effects needs a clean deopt path back to the interpreter.

### String constant support in Tier-4

Currently, `CONSTANT` only accepts numeric literals in compiled functions. String constants require the GC heap at JIT-compile time, which isn't available. One path forward: a `jit_intern_const` helper that lazily allocates the string object on first call and caches its GC id, making string constants safe to use from compiled frames.

### Tier-4 for embedded loops

Functions containing `FOR_LOOP_STEP` are currently rejected by `fn_is_translatable`. Supporting them would require the Tier-2 loop compiler to operate within Tier-4's SSA frame, essentially merging the two tiers. The deopt model is the main complexity: a loop inside a compiled function that deopts mid-iteration needs to restore both the function's locals and the loop counter.
