# Interpreter loop investigation

Status: **single Unbound-local candidate implemented, validated and committed as `ef95ce4`: 16.29% fewer retired instructions.** Exploratory refreshed loop timing: 142 → 121 ms. P1 and the pre-measurement compiled-path refinement agree with the instruction outcome; original reference executable remains unchanged.

The user subsequently approved committing this investigation and merging the full tested stack into `develop`. This is repository finalization, not authorization for another improvement run. Large raw binaries/profiles remain in their existing ignored, hash-identified archives; the investigation reports and analysis scripts are versioned here.

This is an architectural investigation, NOT another autoresearch campaign. The previous campaign is exhausted and remains frozen. No implementation changes, new measurements, build-profile changes, installs or diagnostic builds are authorized merely by these documents.

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

Review the [retained result](11-unbound-local-result.md) and its source/tests. The three local-access sequences fall from 553 to a compiled-model 155 instructions; surrounding work grows by 44, giving a pre-measurement net prediction of 354/cycle. The measured million-batch reduction is 354,000,489 (16.29%). Exploratory timing is reported separately with baseline drift and raw samples. The one approved implementation/comparison is complete; broader context/operand changes are not automatically authorized. The original campaign and six-session debugger budget remain exhausted. No variant or profiling loop.

## Completion criteria

- A profile with an explicit region, event, sample counts/losses and symbol coverage.
- A source/native-code account of the actual hot paths, not cold alternatives.
- Major work classified as semantic requirement, representation/generalization overhead or implementation artifact.
- No double-counting in the budget; unknown costs and uncertainty remain visible.
- One reviewable design discussion grounded in evidence, with BoxLang contracts and alternatives stated. Implementation requires a subsequent explicit decision.
