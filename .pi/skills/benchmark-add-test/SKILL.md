---
name: benchmark-add-test
description: Add a faithful, licensed cross-runtime benchmark to MatchBox's standalone performance suite, with pinned upstream JavaScript, a shared BoxLang port, deterministic verification, and execution on MatchBox, Node.js, and JVM BoxLang. Use when adding or porting performance tests.
---

# Add a cross-runtime benchmark

The objective is a trustworthy evaluator, not a faster VM or a benchmark-specific
shortcut. No autoresearch activation or runtime optimization is authorized here.

## Read first

Resolve paths relative to this skill directory:

- `../../..` is the repository root. Run the commands below from there.
- Read `../../../benchmarks/runtime/README.md` completely.
- Inspect `../../../benchmarks/runtime/manifest.json`, both `harness.*` files,
  `run.py`, `test_runner.py`, and the existing Sieve JavaScript/BoxLang files under
  `../../../benchmarks/runtime/`.
- Read the chosen upstream source and helpers, its individual license/provenance,
  and AWFY's `docs/guidelines.md` and `docs/core-language.md` at the revision pinned
  in the manifest. The repository URL/revision are in that manifest; do not port
  from moving `master` or substitute website performance numbers.

## Select and preserve the workload

1. Clarify the requested benchmark and intended coverage. If unspecified, propose
   the next small AWFY case (for example Bounce, Permute, Towers or Storage) and
   agree its scope rather than importing the entire suite.
2. Inspect Git status and preserve existing work. New durable inputs belong under
   `benchmarks/runtime/`, not `.auto/`; campaign adapters remain separate.
3. Vendor original JavaScript and every required support file, unmodified, from
   the exact pinned upstream commit. Preserve headers and complete applicable
   license notices/attribution. AWFY has mixed provenance: do not assume Sieve's
   license applies to Richards, DeltaBlue, or another newly selected file.
4. Add SHA-256 entries for new upstream files to `manifest.json` without changing
   old hashes to excuse local edits. If upstream must be upgraded, make that an
   explicit, separately reviewed evaluator change with all affected provenance
   and verification updated together.
5. Write a faithful BoxLang port used **unchanged by both BoxLang engines**.
   Preserve algorithm, object structure, method dispatch, recursion, closures,
   allocation patterns and helper-library abstractions. Use native equivalents
   only for equivalent primitives (Sieve's array allocation/fill is an example),
   not algorithmic shortcuts. No hardcoded answers, native prime/JSON algorithms,
   flattened object graphs, caches of computed results, or engine-specific fast
   paths. Json must retain its handwritten parser, not `jsonDeserialize()`.

## Portability checklist

- Arrays: audit zero-based/one-based translation, boundaries, growth, fill and
  mutation. Sieve uses JS `flags[i - 1]` versus BoxLang `flags[i]`.
- Numbers: audit division, rounding, signed/unsigned bitwise operations, overflow,
  modulo and integer/floating behavior. Preserve deterministic PRNG sequences.
- Values/objects: audit null versus missing/undefined, Boolean coercion, identity
  versus structural equality, mutable references, method receivers and scope.
- Classes: use explicit package imports and qualified parent names. The runner
  sets its working directory to `benchmarks/runtime`; implicit sibling lookup
  is not reliably shared by the two engines. Follow the Sieve file layout.
- BIFs: prefer canonical BoxLang names, notably `jsonSerialize()`. CFML aliases
  such as `serializeJSON()` may require an absent JVM compatibility module.
- CLI: the BoxLang entry point supplies `bench` and `benchmarkName`, then includes
  the shared harness using the include path shown in `sieve.bxs`. Harness counts
  come from `cliGetArgs().options` using
  `--warmup=N --iterations=N`; JVM positional arrays can include the script path.
- Timing: reuse the harness. Do not call `getTickCount("nano")` and assume the
  unit is respected on MatchBox; its current implementation returns milliseconds.
- Missing language functionality: report the specific blocker. Do not silently
  replace the workload, weaken assertions or add benchmark-specialized VM code.
  Runtime compatibility fixes require separately agreed scope and reference tests.
  Consult JVM BoxLang's reference implementation before proposing such a fix.

## Register, verify and measure

1. Register the name, description, problem size, expected result, JavaScript path
   and BoxLang entry path in the manifest. The existing runner discovers entries;
   don't add a second orchestration framework. If a new case genuinely needs a
   different result/protocol shape, explicitly design/test that extension while
   retaining the same timing boundary and verification strength for existing cases.
2. Derive expected results from upstream's verifier/reference execution, not from
   whatever MatchBox happens to return. Cross-check with an independent small
   calculation where practical. Run/check verification on **every invocation**,
   including warmup, and allocate/reset state according to upstream semantics.
3. Add runner/contract tests relevant to the new case and failure modes. Keep
   expected constants independent of candidate implementations. Retain all passing
   Sieve/protocol tests and do not relax upstream integrity checks.
4. Run:

   ```bash
   python3 -m unittest discover -s benchmarks/runtime -p 'test_*.py' -v
   python3 benchmarks/runtime/run.py --list
   python3 benchmarks/runtime/run.py --benchmarks NEW_NAME \
     --samples 3 --warmup 100 --iterations 1000 \
     --output .auto/artifacts/NEW_NAME-validation.json
   ```

   First agree a smaller smoke-test policy if the new workload makes those counts
   expensive. Use identical counts across engines; increase counts explicitly if
   millisecond batches are zero/too short. Follow `../benchmark-run-suite/SKILL.md`
   for executable selection, builds, interpretation and time budgets.
5. Execute the existing suite as a regression check within the agreed budget.
   Passing Python tests alone is not proof the BoxLang port works. Require the
   actual expected result on all three engines, or clearly report the addition
   as blocked/incomplete; never mark an unexecuted runtime as passed.
6. Update `benchmarks/runtime/README.md` for new coverage/requirements and record
   generalizable lessons in these project-local skills. Keep `/benchmark-*`
   prompt templates thin delegates, not copies of the full workflow.

## Keep evaluation separate from optimization

Do not modify `crates/`, `src/`, build profiles, feature flags, dependencies or GC
settings to improve a benchmark while adding it. Do not use elapsed-time numbers
as correctness evidence. Freeze and review evaluator changes before any later
campaign; load `../matchbox-autoresearch/SKILL.md` before preparing that work.
No optimization loop, automatic commit or push is part of adding a benchmark.
