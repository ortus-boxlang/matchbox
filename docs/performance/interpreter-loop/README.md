# Interpreter loop investigation

Historical merged result: **single Unbound-local candidate implemented, validated and committed as `ef95ce4`: 16.29% fewer retired instructions.** Exploratory refreshed loop timing: 142 → 121 ms. P1 and the pre-measurement compiled-path refinement agree with the instruction outcome; original reference executable remains unchanged.

The user subsequently approved committing this investigation and merging the full tested stack into `develop`. This is repository finalization, not authorization for another improvement run. Large raw binaries/profiles remain in their existing ignored, hash-identified archives; the investigation reports and analysis scripts are versioned here.

This is an architectural investigation, NOT another autoresearch campaign. The previous campaign is exhausted and remains frozen. No implementation changes, new measurements, build-profile changes, installs or diagnostic builds are authorized merely by these documents.

## Current follow-up: measured B1 and timed C3

A separately authorized 2026-10-30/31 follow-up refreshed both runtimes' instructions and separate loop timing, verified a current conditional native ledger, and compared two replacement designs. **One B1 candidate is implemented, contract-tested, statically native-verified and retained for review: 21.77% fewer scoped retired instructions, closing 25.31% of the fresh Node gap.** Separate unprofiled loop timing measured 122 → 85 ms median in three samples. The earlier Unbound result below remains historical evidence, not the current full cost ledger. B1 implementation/tests are committed as `57b6f18d0ad2ace67557cadccbb1b4daa5292219`. The user separately authorized committing the investigation documents and merging this validated stack into local `develop`. Repository finalization preserves the existing unpushed history and original measured binaries; it is not another optimization run or authorization to push.

- [Current baseline, artifact identities and future-run rules](next-candidates/README.md)
- [Active execution context: design/pricing D1](12-active-context-design.md)
- [Local-plus-constant composition: design/pricing D1](13-expression-composition-design.md)
- [Five-candidate handoff and selection gate, D1](14-design-selection.md)
- [B1 pricing and pre-implementation prediction, B1-P1 v1](15-b1-pricing-prediction.md) — preserved original conditional engineering range
- [B1 implementation/native validation and pre-outcome B1-P2 v1](16-b1-implementation-validation.md) — preserved pre-outcome mechanism/pricing record
- [B1 performance result and P1/P2 reconciliation](17-b1-performance-result.md) — latest measured instruction result; 396,000,386-instruction reduction, one retained candidate
- [Post-B1 guarded composition pricing, C3-P1 v1](18-c3-composition-pricing.md) — preserved source-only conditional 218–297/cycle reduction after replacement/growth allowances
- [C3 implementation/native validation and frozen pre-outcome P2](19-c3-implementation-validation.md) — 1012 test passes; static cycle 1423 → 1093, conditional 330/cycle reduction; preserved pre-outcome record
- [C3 timing result and retention decision](20-c3-timing-result.md) — fresh B1 92 → C3 73 ms loop median, 20.65% less time; **C3 instruction outcomes still unmeasured**, real noneligible-site tax
- Deferred handoffs: [header-only fiber reuse](next-candidates/header-fiber-reuse.md), [promoted constants](next-candidates/promoted-constants.md), [global-cache ownership/access](next-candidates/global-cache.md)

These scopes overlap and may subsume one another. Handoffs do not authorize implementation, renewed debugger/profiling budgets, measurements or an automatic sequence of experiments. Fresh/raw evidence remains in the identified dedicated worktree's ignored packets; documents alone do not transfer it.

## Read in order

1. [Brief and operating rules](00-brief.md)
2. [Evidence and reproduction](01-evidence.md)
3. [Shared profile: status and procedure](02-shared-profile.md)
4. Analysis handoffs, independent once the shared profile exists:
   - [Local access](03-locals.md)
   - [Arithmetic and constants](04-arithmetic-constants.md)
   - [Globals and dispatch](05-globals-dispatch.md)
5. [Instruction budget and synthesis](06-instruction-budget.md)
6. [Cache ownership and first-change design review](07-cache-design-review.md)
7. [Bounded cache-projection result: rejected](08-cache-projection-result.md)
8. [Evidence-driven executed native-path ledger](09-native-ledger.md) — current attribution and mechanism evidence
9. [Candidate selection and unbound-local replacement model](10-candidate-selection.md) — pre-implementation selection and prediction P1
10. [Unbound-local result: retained](11-unbound-local-result.md) — current implementation, counter/timing results, reconciliation and limits

## Operating model

One coordinator owns evidence provenance, serial profiling/measurements and the budget. Three reviewers may inspect source/assembly independently, writing only their assigned analysis document. Do not run concurrent benchmarks. Area 4 (bytecode-to-machine-work discrepancy) is the synthesis of areas 1-3, not an additional independent cost.

The current agent has no subagent-dispatch tool, so the initial code audits were performed sequentially. The handoffs are ready for separate agents/sessions; no parallel work is claimed.

Use no additional worktrees for read-only inspection. If diagnostic source changes or a symbolized rebuild become necessary, create ONE dedicated diagnostic worktree and separate build directory, document its exact commands and binary hashes, and never replace the retained reference artifact. Separate worktrees do not isolate CPU resources.

## Immediate next action

Review the [C3 timing result and retention decision](20-c3-timing-result.md). The separately authorized nine-process timing phase measured 92 → 73 ms loop medians against fresh B1, with all checksums correct; C3's whole-process median was 77.72 ms. The user then approved committing C3 and merging into local `develop`, preserving prior unpushed history and artifacts. C3 implementation/tests are committed as `ecc9074`. This repository-finalization approval is not a new measurement/build/optimization scope or authorization to push.

C3's static 1093-instruction ordinary cycle and 330/cycle reduction remain **conditional compiled predictions, not counter outcomes**. Its additional 45–65-instruction costs on several noneligible local-read paths and unknown broader workload impact remain explicit. B1's measured result and original P1/P2 remain unchanged; its 32-instruction/cycle causal residual and 386-instruction whole-batch delta residual are not resolved by C3 timings. Further affected-workload checks, instruction outcomes or implementation variants require a separate decision. The original campaign and six-session debugger budget remain exhausted.

## Completion criteria

- A profile with an explicit region, event, sample counts/losses and symbol coverage.
- A source/native-code account of the actual hot paths, not cold alternatives.
- Major work classified as semantic requirement, representation/generalization overhead or implementation artifact.
- No double-counting in the budget; unknown costs and uncertainty remain visible.
- One reviewable design discussion grounded in evidence, with BoxLang contracts and alternatives stated. Implementation requires a subsequent explicit decision.
