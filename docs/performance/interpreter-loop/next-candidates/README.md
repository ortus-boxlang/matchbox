# Deferred candidate handoffs — 2026-10-30

These are three credible candidates deferred while **active execution context** and **local-plus-constant expression composition** receive deeper design/pricing. Deferral is not rejection, a numeric ranking, or permission to implement all five in sequence.

- [Header-only fiber acquisition reuse](header-fiber-reuse.md)
- [Promoted-constant access reuse](promoted-constants.md)
- [Global-cache ownership/access redesign](global-cache.md)

## Shared baseline and evidence

Source: local develop **582473adb6eb643f35ec1fe2e21da37520fb3ad0**, including Unbound locals. Dedicated worktree: `/home/jacob/dev/ortus-boxlang/matchbox-checksum-baseline-20261030T2203Z`, branch `investigation/checksum-baseline-20261030T2203Z`. Never reset local develop to an older remote branch.

Fresh measurements, three native fresh processes/runtime/metric, one million iterations, no warmup:

| Runtime | Aggregate retired user instructions, median / MAD | Separate loop ms, median / MAD |
|---|---:|---:|
| Current MatchBox interpreter | 1818993839 / 131 | 118 / 0 (raw 118,118,132) |
| Node v22.23.3 --jitless | 254279176 / 1532 | 13 / 1 (raw 12,13,16) |

Fresh gap: **1564714663** instructions/batch. These are comparison denominators, not promises of achievable parity. Timing has millisecond resolution; no instruction-to-time prediction.

Current executable SHA-256: `abe03005c7464ecb3b396d8844a06e6692522ce5c36a5700e1c43a0dd6a73d11`. Built from current source with examples/instruction_interpreter.rs, real compiler/tree-shaken prelude and JIT-inactive assertions; release opt-level=z/LTO/one codegen unit/panic abort/default features, symbols retained. Node SHA-256: `fde6a4bf8d0562f7751d1a2d6cb9b417c4cfe107bbcb0aa3e9a24e125e348f48`.

Ignored packets under that dedicated worktree's `benchmarks/instructions/results/`:
- `baseline-20261030T220339Z/`: exact sources, build/executable provenance, raw instruction/time evidence, summary and hashes.
- `static-ledger-20261030T221311Z/`: current conditional native model, verified selected main instructions/helper closure/dispatch table, Node identity, disjoint ledger, failures and limitations. Start at README.md, ledger-summary.json, model-verification-v3.json and current-model-v3.json.

The verified current **conditional ordinary-cycle model is 1819**, not a new execution trace. Fresh MatchBox median is 6161 below 1819×1m. Selected branch assumptions exclude boundary/periodic/runtime paths; Node's approximate regime model leaves ~3.30% of its fresh aggregate unassigned. Old pre-Unbound traces are historical: do not reuse their addresses, 49 fiber calls, 553 local-access envelope or candidate ranking without current verification. The preserved old autoresearch target is intentionally pre-Unbound.

## Rules for a future run

1. Read AGENTS.md, the evidence-driven-improvement skill, measurement skill/docs if measurements are authorized, and investigation 09–11 for history. Read the relevant handoff below. Preserve existing work/artifacts and verify current executable identity, not just checkout revision.
2. Re-establish the current ledger if another candidate has landed. These opportunities overlap; **do not add their gross counts or predicted savings**. Revisit selection against active-context/composition work and no change.
3. Preserve the direct source loop, checksum 500001500000, iteration count, scopes/storage and arithmetic. MatchBox sum is a VM global; Node CommonJS sum/i are wrapper-frame locals. Do not hide the difference with wrappers/storage relocation.
4. Obtain an explicit scope/time/run budget. These handoffs grant no runtime measurements, implementation edits, commits, pushes, installs or optimization loop. Prior debugger/campaign budgets remain exhausted, not reset.
5. Price eliminated, retained, introduced and displaced work; record a versioned net prediction BEFORE implementation. Unknown benefit is not zero. Component cost/gross removal is not net savings. Baseline reduction and comparator-gap closure have different denominators.
6. After separate implementation approval, preserve original prediction, test contracts, verify compiled mechanism and surrounding work BEFORE outcome counters, explicitly version revisions, then measure instructions and time separately within an approved run budget. Reconcile both unexpectedly small and large results. Never automatically start the next candidate.

Raw ignored evidence is not carried by Git; transferring this handoff does not transfer binaries/packets. Check artifacts/hashes before relying on them.
