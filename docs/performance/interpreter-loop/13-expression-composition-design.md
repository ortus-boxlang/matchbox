# Local-plus-constant expression composition: design/pricing D1

Status: **source/static design only; credible broader competitor, no product edit or net prediction**. 2026-10-30. Read [baseline/rules](next-candidates/README.md), [active context](12-active-context-design.md) and [selection](14-design-selection.md). Prior campaign/debugger budgets remain exhausted.

## 1. Focus and current mechanism

Replace repeated operand materialization/dispatch for a general local-plus-constant expression, not recognize the checksum, literal2, million-loop shape or numeric contents of i. Preserve the source algorithm, i/sum storage and exact arithmetic/coercion behavior.

Current verified conditional block: GET_LOCAL182 + CONSTANT241 + ADD257 = **680**, versus same-binary Node's historical late frame-load/immediate-add36. Of the680, three headers cost273; handler/helper remainder407. This is an envelope, not removable work. It is independent of sum's VM-global versus JS-frame storage difference.

Current compiler `compiler/mod.rs:1991–2015` eagerly lowers left then right, then chooses ADD_INT/ADD_FLOAT only for two literal operands, otherwise ADD. Identifier lowering at2150–2187 treats this/variables/import/private names specially before ordinary locals/globals. Runtime ADD4956–4998 supports array union, numeric strings and string fallback. numeric_value2367 handles numeric/GC-string conversion.

## 2. Canonical reference checked, compatibility limits explicit

Read-only reference revision **02b3218aaafc37977aa227d90df15f98cd4e9754**, with existing deleted .vscode files untouched. No pull, build or JVM run. Inspected runtime/operators/Plus.java and PlusTest.java.

Canonical Plus supports numeric-string coercion, integer/long overflow promotion, high-precision BigDecimal and set union. The tests exercise numbers, numeric strings, compound assignment and date/duration conversion. Current MatchBox ADD's array union, general string fallback and f64 representation are NOT demonstrated canonical equivalence. This performance proposal must preserve current MatchBox behavior and existing tests; it must not silently narrow types, change NaN/integer behavior, or smuggle a compatibility repair into the candidate. A future semantic change is a separate task.

## 3. Concrete expression scope C1

The smallest coherent source pattern is an expression whose existing lowering would be `GET_LOCAL slot; CONSTANT numeric_literal; ADD`. Candidate representation: a general local-plus-numeric-literal opcode, not a number-only interpreter. The LEFT VALUE remains dynamically typed, resolved by the same live local machinery and processed by the same generic addition semantics.

Why numeric literal rather than arbitrary Constant: closures/components need current capture, strings/arrays/classes need VM promotion/identity/rooting, and arbitrary right-side expressions have evaluation side effects. Limiting the initial compiler pattern to a numeric literal is a static source property, not a runtime assumption about i. Other expressions keep their existing legitimate opcodes; no compatibility/migration layer is proposed.

### Representation alternatives

- **C1a, constant-index operand:** two-word instruction: slot plus numeric constant index; retain legal constant access. Smaller new encoding and reuse of existing pool, but generic read_constant may consume most of the supposed operand-layer benefit. Direct cached access would overlap the deferred constant proposal and needs its certificate/lifetime proof.
- **C1b, boxed numeric-literal payload:** three words: slot plus two u32 words carrying exactly the BxValue::new_number result. This can eliminate repeated generic promotion lookup. Use the actual constructor's NaN normalization and preserve signed zero/infinity/rounding; do not introduce int-tagging or reassociation. Encoding/reconstructing words and code/input growth are introduced work. Compile-time construction is displaced work, not zero cost. No unaligned/endianness shortcut is assumed across native/Wasm/ESP32.

C1b is the more coherent layer-removal direction for pricing, not yet selected for implementation. Existing compiler lowering and runtime generic ADD remain the semantic authority; extracting a common add-values operation may introduce an outlined &mut VM helper/result/spill chain and must be billed.

### Observable contracts

1. Use the exact identifier-resolution decision that previously emitted GET_LOCAL. A naive resolve_local-first rewrite can bypass special this/variables/import/private behavior. Do not relocate sum or broaden Unbound eligibility.
2. Read the left operand before the literal/result, retaining live argument/local/scope aliases, late bindings, nulls, deletion fallback and ComponentBody behavior. Do not infer a local stack address from source spelling alone.
3. Preserve generic ADD result/fallback/error order. Constant numeric right is a proof about the RIGHT only; numeric-string and nonnumeric left cases remain valid. Both operands are not automatically numbers.
4. Intermediate values live in VM-rooted stack/frame locations across any call/poll that may collect, run callbacks, throw or suspend. Holding BxValue only in a Rust local does not root a pointer. Source numeric fast paths do not discharge the generic fallback proof.
5. Preserve GC/completion/interrupt/yield behavior and debugger source/IP/budget semantics, or explicitly obtain approval for a changed contract before relying on it for savings. A weight of3 on one counter is NOT three original observation boundaries.

## 4. Ranking-changing discovery: two headers are not free removals

The source header is not just a branch table. It checks/drains native completions (which can invoke code), fiber/frame/yield state, reload, periodic/prompt GC, trace/debugger and the interrupt path. collect_garbage roots the operand stack; completion callbacks can mutate scopes/values. Debugger budget can pause at an instruction boundary and reports executed counts (`web_runtime_tests.rs:870–896`).

A naive fused opcode with one poll instead of three eliminates **182 current header instructions gross**, but also coalesces these boundaries. This pass has not established that change as contract-equivalent or authorized it. Existing FOR_LOOP_STEP fusion is not a blanket exemption for future polling changes.

There are three materially different designs:

| Design | Work retained / added | Decision |
|---|---|---|
| One new opcode, one header/poll | Drops two old observation opportunities | NOT accepted under the conservative unchanged-contract scope; no saving credited as free. |
| One opcode with resumable internal observation phases | Keep required polls; root operands and record exact restart/IP state for GC/yield/callback/throw/debugger pauses | Coherent in principle, but resume-state/phase branches, stack state and consumer work are unpriced. A local implementation detail becomes runtime state if suspension is possible. |
| Guarded composition, ordinary semantics when unsafe to combine | Check completion/GC/debug/trace/interrupt conditions and preserve legal restart targets/consumer decoding | Guard costs and races need proof; an interrupt may arrive between checks. Avoid bytecode-with-hidden-entry-points or a second micro-interpreter as a stopgap. Not selected by this pass. |

No claim is made that fusing bytecodes can NEVER preserve BoxLang or that every old compiler bytecode count is an eternal language guarantee. The unresolved question is what MatchBox's current polling/debugger/host contracts require and what the equivalent replacement costs. That must be decided explicitly, not inferred from a large dispatch subtotal.

If exact old boundary semantics are required, a phase-aware design can reduce fetch/operand overhead but retains much of the surrounding checks and may require state/root storage. If a different logical-instruction policy is acceptable, it needs a stated contract/validation decision. This uncertainty is large enough to change C versus B's ranking.

## 5. Bytecode/JIT/tooling surface

At minimum audit/update together if C is ever approved:
- compiler identifier/binary lowering and branch/source-line bookkeeping;
- opcode encoding/names/width and serialized/embedded bytecode consumers;
- interpreter shared ADD operation and generic slow/error/root behavior;
- JIT numeric body parsing/translation at jit.rs943–1000 and1527–1580;
- JIT parameter-hint width walker at1807; eligibility/call-origin/stack-depth analysis at1879–2060; hot-function translation2292–2573;
- construct entry scanning/translation at jit/construct.rs127–140 and200–223;
- FOR_LOOP_STEP body slicing/backedge/source-IP handling and debugger/trace reporting.

Some JIT analyses iterate raw words or infer stack effects from the old opcodes. Merely teaching the interpreter a new multiword opcode while those default to width1 or reject it can misdecode literal bits or silently remove previously supported JIT behavior. The measured harness's inactive JIT does not authorize that regression. Tests must cover both newly supported decoding and existing eligibility, guard/deopt/OSR contracts. Do not add a compatibility layer for obsolete bytecodes; change current consumers coherently.

## 6. Explicit replacement bill, not a prediction

For C1b, a deliberately conservative accounting skeleton keeps the OLD GET_LOCAL and ADD body envelopes and all CONSTANT caller/push work except its101-instruction read_constant subtree:

`existing = headers273 + GET_LOCAL body91 + CONSTANT body150 + ADD body166 = 680`

`replacement skeleton = header91 + local91 + constant caller/push49 + add166 + L_literal + R_observation/resume + G_codegen + D_displaced`

`nominal net = 283 − L_literal − R_observation/resume − G_codegen − D_displaced`

Here49 =150−101 retains even potentially redundant old caller work as a reservation for replacement glue, not a literal executable design. **283 is gross removed header+subtree work**, NOT a whole-candidate savings prediction. L/R/G/D are unpriced. The skeleton retains operand-stack materialization and full generic arithmetic; a fuller operand redesign might eliminate more, but that is UNKNOWN rather than zero or additive free savings.

If original headers were retained in full to preserve boundaries, this skeleton's gross becomes only101 before literal/replacement work. Partial poll sharing may recover some of182, but claiming it requires the phase/guard proof above. Removing stack interfaces, numeric conversion of the known right, and shared state reuse overlap with B/D and with each other. No independent sum of opportunities is valid.

Zero new work for283 would be15.56% of current baseline /18.09% of fresh gap, not an achievable bound or timing prediction. There is no defensible finite net interval yet. Eagerly compiling a new opcode to discover R/G would be a diagnostic, not an already-priced optimization.

## 7. Validation plan and stop conditions

Before any future outcomes: numbers/fractions/large sums/signed zero/NaN/infinities; numeric strings/non-numeric/null/bool/array/struct left values preserving current behavior; bound/unbound/Shared/Stack locals and aliases/deletion; special identifier/import/class contexts; callback scope mutation at each original boundary; GC/OOM with intermediate values reachable only through the evaluation stack; forced interrupt/yield/debugger budgets and line stepping; compiler branch/exception/source mapping; JIT eligibility/OSR/deopt/construction consumers and cross-target word decoding.

Verify compiled mechanism including every retained/new poll and guard, literal decode, generic helper/result/spill work and surrounding blocks BEFORE counters. Stop if preserving boundaries needs an unpriced state machine, if a general value path is narrowed, if JIT consumers regress, or if the explicit budget ends. No automatic fusion variant.

## Decision at D1

C1 remains credible and may have a larger ultimate effect than B1. It is NOT priced by deleting two dispatch rows. Its observation/resume policy and consumer surface are materially less resolved than B1's checked borrow design. Prefer B1 as the next bounded mechanism-cost question under unchanged contracts, but do not describe C's unknown benefit as zero or claim B's net dominance. No new P1 or implementation is authorized by this report.
