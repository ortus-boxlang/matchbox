# Analysis handoff 2: arithmetic and constants

Status: **historical source/native handoff, supplemented by an executed native-path ledger.** See [current accounting](09-native-ledger.md): both ADD bodies have 134 self instructions, numeric helper self work is 88, and body constant helpers 80 per traced cycle; caller/stack/state work is separate. Node's corresponding state-dependent paths are now traced. Sample shares below are not exact instruction bills.

## Scope and question

Explain the machine work behind the two numeric ADDs and numeric constant accesses actually taken. Do not count the entire generic handler as executed, or assume arithmetic is the bottleneck because it is the visible source operation.

Read the shared brief/evidence/profile handoff. Source checked at 139be94.

## Source-confirmed path

- `crates/matchbox-compiler/src/compiler/mod.rs`, binary '+' compilation around 1993: ADD_INT/ADD_FLOAT selected only for two syntactic numeric literals. i+2 and sum+term select ADD.
- `crates/matchbox-vm/src/vm/mod.rs`, ADD around 4953: pop two values, check GC-pointer pair for array-union applicability, then call numeric_value for both values. Numeric path pushes BxValue::new_number(a+b).
- `vm/mod.rs`, numeric_value around 2367: is_number/as_number is the path here. String parsing and array iteration are not executed for these operands.
- `crates/matchbox-vm/src/types/mod.rs`: NaN-box value tags and conversions. Numeric BxValue construction is not itself a GC heap-object allocation.
- `vm/mod.rs`, read_constant around 9273: inspect constant kind for closure semantics even before promoted-value cache lookup; on a hit return cached BxValue. On miss clone/promote/cache, growing root promoted_constants as needed.
- `vm/mod.rs`, promote_constant around 9324: numeric constant becomes boxed number, not a heap object.
- `vm/mod.rs`, FOR_LOOP_STEP around 4545: numeric increment/read/write; bound obtained through promoted_ptr cache fast path, otherwise read_constant. Numeric comparison and JIT/timeslice guards surround the back edge.

## Source-derived steady-state frequencies

Per continuing iteration:
- **two generic ADDs**, each with two numeric operands.
- **one CONSTANT dispatch** for 2, normally a promoted-constant cache hit after first use.
- **one bound access** inside FOR_LOOP_STEP, normally through its direct promoted-constant cache read. Do NOT charge full read_constant to this bound access each iteration.
- one numeric increment/compare in FOR_LOOP_STEP.

The two-iteration diagnostic uses the same constant index for bound=2 and body literal=2. The measured million case has a separate 1,000,000 bound constant, so inspect the actual marked source/bytecode as needed; do not assume identical constant-pool indexes.

## Comparator evidence

V8's diagnostic uses `AddSmi 2` for the immediate term and `Add r0` for accumulation. It uses accumulator/register operands instead of MatchBox's operand stack. These are bytecode observations, not proof of native handler costs or static typing.

Node's normalized hardware counts rise between 100k and 1m. Numeric representation and GC may matter, but are **unproven explanations**. A concrete diagnosis requires additional V8 evidence in separate unmeasured processes; do not infer a transition point solely from the mathematical sum.

## Reviewer assignment

- Match profile/assembly to the hot numeric paths, conversions, pushes/pops and constant cache accesses.
- Determine whether repeated type/GC-pointer checks, helper calls, RefCell checks or native code layout materially contribute.
- Separate cold constant promotion from repeated cache-hit work.
- Explain any compiler-eliminated operations and any uncertainty from inlining/LTO.
- Report semantic purpose: dynamic numeric/string/array behavior, closure constant promotion, per-VM identity and GC rooting.

Propose focused diagnostics before collecting them. No opcode fusion, coercion changes, literal-only shortcuts, removed capabilities or optimization edits in this review.

## Overlap / accounting ownership

Own ADD bodies and body CONSTANT work. Coordinate FOR_LOOP_STEP increment/limit cost with area 1; its local reads/writes remain area 1, JIT eligibility/timeslice/dispatch work area 3. No double-counting enclosing inclusive samples and helper self samples.

Follow-up native direct-branch mapping puts both ADD executions' body PCs at 8.20% of all instruction samples, separate from numeric_value's 3.61% and the shared stack helpers. CONSTANT caller body is 0.41%, separate from read_constant's 5.05%. FOR_LOOP_STEP's 11.88% mixes local access, increment/tagging, bound lookup/comparison and back-edge/JIT guards; it is not pure arithmetic. These candidate-specific PC labels come from conservative reachability, not a dynamic native trace. See [method and native anchors](07-cache-design-review.md).

## Shared-profile update

Accepted instruction profiles show read_constant 5.05% and numeric_value 3.61% SELF shares. These exclude caller setup, stack operations, type/result handling and inlined code within run_fiber_inner; do not conclude that total ADD costs only 3.61%.

Native numeric result block at 0x41c20c..0x41c28e in matched reference code calls numeric_value twice, retains operands across calls, performs another fiber-index helper call, reloads operands, then executes addsd and number tagging. Actual floating-point arithmetic is surrounded by general helper/state work. Annotated native disassembly and sampled PCs are in the shared profile artifacts.

Node instruction samples identify AddHandler and AddSmiHandler, supporting execution of interpreter handlers rather than JIT-generated loop code. At the million bound, ExtraWide/LdaSmiExtraWide handlers also appear: the n=2 diagnostic's roughly twelve bytecodes are not an exact count of million-bound decoder dispatches. Prefix encoding differences must remain visible. Node size-dependent total changes still require a dedicated diagnosis; a few GC symbols in the profile do not establish the full cause.
