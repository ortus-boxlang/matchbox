# Area 4: instruction-budget synthesis

Status: **historical sampled budget, now supplemented by an executed native-path ledger.** Coordinator owns this document. Use [the native ledger](09-native-ledger.md) for current per-path accounting: sampled shares below are NOT exact instruction bills and differ materially for some helpers.

## Measured envelope

At one million iterations:

| Quantity | MatchBox | Node --jitless |
|---|---:|---:|
| All-thread retired instructions | 2172993829 | 254269661 |
| Batch-normalized instructions/iteration | 2172.993829 | 254.269661 |
| Loop time median | 142 ms | 14 ms |
| Continuing loop VM dispatches/bytecodes (diagnostic) | 7 | approximately 12 |

Gap: **1918.724168 batch-normalized instructions/iteration**. Ratio: 8.546x instructions, about 10.14x elapsed time. Do not call this an exact steady-state per-iteration bill: region boundary, initial test and cold caches contribute fixed work; Node varies by size. The control and size results are in the evidence index.

MatchBox's 2173/7 ~= 310 machine instructions per VM dispatch is a scale indicator, NOT measured cost of every handler. Dividing Node by twelve is equally not attribution. Bytecodes have different operands and semantic work.

## Frequency skeleton: source + trace, not machine-cost measurements

| Component | Steady-state frequency/iteration | Review owner | Native contribution |
|---|---:|---|---|
| Shared opcode dispatch/checks | 7 | Globals/dispatch | UNKNOWN |
| Cached global read | 1 | Globals/dispatch | UNKNOWN |
| Cached global write/pop | 1 | Globals/dispatch | UNKNOWN |
| Source-level mutex cache lookups within those two accesses | 2, included above | Globals/dispatch | UNKNOWN; do not add twice |
| Local helper read | 2: body + step | Locals | UNKNOWN |
| Local helper write | 1: step | Locals | UNKNOWN |
| Body promoted constant read | 1 | Arithmetic/constants | UNKNOWN |
| Generic numeric ADD | 2 | Arithmetic/constants | UNKNOWN |
| Bound read + numeric increment/compare | 1 | Arithmetic/constants | UNKNOWN |
| Inactive-JIT/back-edge/timeslice guards | 1 back edge | Globals/dispatch | UNKNOWN |
| GC/runtime work not already charged | measure if present | Coordinator | UNKNOWN |
| Fixed setup/initial-test/exit/markers | once/batch | Coordinator | UNKNOWN; not steady-state |

This table intentionally has no fabricated percentages. Source-confirmed branches may be optimized away, combined or outlined. Sampling self time, inclusive time, cycle shares and instruction shares are distinct units.

## Accounting rules

1. Keep measured totals and source-derived frequencies separate from attribution estimates.
2. Use disjoint buckets for the sum. Do not add inclusive dispatch samples to the same callees' self samples.
3. A source helper may inline into several handlers; attribute its instances once, retaining call-site identity.
4. Mark unidentified native addresses and profile-uncovered threads. Never distribute the residual by intuition.
5. Do not multiply CPU/cycle sampling percentages by the retired-instruction total to produce exact-looking instruction counts.
6. Any claimed instruction contribution needs a stated method, artifact, boundary, uncertainty and reconciliation with the total. If only approximate shares are supported, report those honestly.
7. The Node side requires its own evidence before claiming why its native path is cheaper. Native build/optimization differences and CommonJS storage context remain potential contributors.

## First measured sample distribution (not exact instruction costs)

Shared evidence: `benchmarks/instructions/results/interpreter-loop-profile-20261030T184742Z/`. Three accepted region-scoped instructions:u profiles, 6,513 samples, zero lost/throttled records. Original executable profiled; code/data-equivalent companion supplies function symbols. Ordinary sampling can skid; no precise_ip requested.

| Disjoint native sample bucket | Share | Interpretation |
|---|---:|---|
| Shared dispatch/header PC range | 19.65% | Frame/yield/GC/decode/jump-table work, excluding outlined callees |
| FOR_LOOP_STEP body self PCs | 11.88% | Includes inline local read/write and back-edge machinery; excludes outlined helpers |
| ADD body self PCs | 8.20% | Both ADD executions combined; excludes numeric/stack helpers |
| GET_LOCAL body self PCs | 4.47% | Inline local-access machinery; excludes outlined helpers |
| SET_GLOBAL_POP body self PCs | 3.13% | Inline write/pop handler work; excludes called helpers |
| GET_GLOBAL body self PCs | 2.69% | Inline read handler work; excludes called helpers |
| CONSTANT body self PCs | 0.41% | Caller work; read_constant is separate below |
| Shared handler-tail self PCs | 1.09% | Kept ambiguous; no speculative caller assignment |
| Chunk::cache_get self | 9.03% | Confirmed hit path retains lock call and 80-byte entry copy; excludes called helpers |
| Fiber indexing helper self | 6.40% | Actual outlined bounds/address helper; callers' argument setup excluded |
| Operand stack pop/push self | 6.91% | Container helpers, not caller state lookup |
| read_constant self | 5.05% | Constant-hit machinery; excludes caller work |
| numeric_value self | 3.61% | Does NOT include all ADD handler work |
| Local binding/Option helpers self | about 7.18% | slot_binding/or_else/get/and_then; excludes inline locals and fiber indexing |
| RefCell borrow, mutex/poison helpers, other indexing | remainder | Shared helpers require caller/native ownership checking |

The first eight rows sum (within rounding) to run_fiber_inner's 51.53%; never add that symbol total again. These are instruction-SAMPLE shares, not exact event bills or removable fractions. No percentages from CPU-cycle profiles were converted into instruction counts.

Follow-up method: `map-handler-samples.py` decodes native opcode entry points from the actual jump table and conservatively follows direct branches among the six repeated opcode classes (ADD executes twice), stopping at dispatch/returns/indirect jumps and not following calls. Unique means unique among those candidate classes, not every opcode in the VM. Untaken cold branches are included in reachability; this is not a dynamic native trace. The 71 samples reachable from multiple classes remain shared. All 3,356 samples inside the inner function reconcile to the buckets; 3,157 samples remain outside the inner function, mostly in the already identified helpers. Per-run variation and source/native anchors are in the [cache design review](07-cache-design-review.md).

This explains substantial native scaffolding behind the small bytecode count, but it does not yet explain every instruction or assign all cross-runtime difference to individual design choices. A fourth independent cost is not added for synthesis.

Native Node profiles (diagnostic --perf-basic-prof) concentrate in JumpLoop/Add/Inc/TestLessThan/AddSmi handlers. Percentages inside different runtimes are not absolute cost comparisons. Million-bound ExtraWide prefixes also mean the n=2 diagnostic's twelve-bytecode structure is not an exact million-run decoder-dispatch count. This caveat does not remove the observed retired-instruction gap.

## Classify major findings

For each materially supported contribution, classify:
- **Semantic work:** required observable BoxLang behavior for this operation.
- **Representation/generalization work:** supporting capabilities not used here, whose absence is not represented cheaply/safely.
- **Implementation artifact:** redundant work, avoidable ownership/synchronization, code-layout/codegen effects, etc.
- **Unknown:** evidence insufficient; give next falsifying observation.

A lock or guard is not automatically an implementation artifact. Its purpose and ownership contract must first be understood. A profile hotspot is not automatically a safe optimization target.

## Decisions deferred until synthesis

Do not yet choose a replacement interpreter, new frame model, lazy scope materialization, bytecode changes, lock removal or build-profile changes. Candidate design directions must explain the measured major costs and preserve contracts beyond this script. No workload-specific recognition or weakened tests.

## Current conclusion

The [executed native-path ledger](09-native-ledger.md) now accounts for 2173 logical instructions on four continuing reference iterations, closely matching the independent hardware envelope. It records 49 outlined fiber-index calls; shared header plus its own fiber callees is a 588-instruction subtotal. Corresponding Node ordinary paths are 205/230/247 instructions across three states, with an explicit ~3.3% million-batch model residual rather than assigning all untraced work to GC. Prior sampled helper shares should not be used as cost bills.

The rejected projected-cache candidate is now reconciled: accessor/helper work increases by 26 instructions while caller work drops by five, yielding +21/cycle, matching its measured million-batch regression to a 145-instruction residual. No optimization is retained and no new variant was tried. A possible scoped-header-borrow design has a conditional 154-instruction removal opportunity before replacement overhead; the complete net-prediction/design gate is not yet satisfied.
