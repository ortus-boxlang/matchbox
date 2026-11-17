# Handoff: reuse frame-local promoted-constant access

Status: **deferred, credible, no implementation or net savings prediction**. Read [shared baseline/rules](README.md) first. Saved before deeper active-context/composition investigation, 2026-10-30. This document grants no run budget.

## Question and measured-path evidence

Can repeated noncapturing constant hits avoid generic closure-first/frame/chunk reconstruction, using the existing promotion certificate without changing constant semantics?

Source: `vm/mod.rs:4937` CONSTANT; `9274–9324` read_constant; `4560–4574` FOR_LOOP_STEP's promoted_ptr hit; frame reload initializes promoted_ptr around 4352; GC roots promoted_constants around 9634. Current CONSTANT costs **241/cycle including one 91-instruction header**. Its read_constant call subtree costs **101**, including **74 read_constant self**, constant-vector indexing and nested fiber/RefCell work. These are current conditional path costs, not 101 removable instructions or a complete candidate ceiling.

The current loop's CONSTANT loads 2; FOR_LOOP_STEP loads its limit via a simpler existing promoted-cache hit path. Do not recognize this literal/workload, force numeric constants globally, or change the source algorithm.

## Contracts needing proof

- read_constant deliberately checks CompiledFunction Closure/ComponentBody before cached return: a fresh function must capture the current receiver/scopes. Do NOT skip this from the observation that this workload uses numbers.
- Establish a maintained invariant that a populated promotion-cache slot cannot represent a capturing constant needing new capture. Audit every writer, resizing, frame creation/reload, program association and any code/constants mutation. A cache hit is a certificate only after that proof.
- Existing promoted_ptr refers to a frame field. Fiber/frame Vec relocation and nested callbacks can invalidate the address; distinguish the vector object pointer from its reallocatable element buffer. Cold fallback and any reentry must not reuse a stale pointer.
- Keep promoted BxValues rooted and preserve per-VM promotion/caches, program immutability and miss/error/index behavior. Strings, arrays, ordinary functions, lambdas, classes and interfaces need their existing ownership/identity behavior, not just numeric tests.
- Preserve closure scope exposure/invalidation, null values versus empty cache slots, and miss allocation/GC behavior. Never treat Some(null) as a miss.

## Credible replacements to compare, not implement automatically

1. Read a checked promoted slot first in read_constant, then retain the existing miss/capture logic. Avoids changing handler pointer organization, but may retain duplicate reconstruction and add miss work.
2. Generalize the existing frame-local promoted_ptr hit fragment to CONSTANT, retaining read_constant fallback. Potentially removes more state/borrow work but carries a stronger lifetime proof and caller-layout effects.
3. Let expression composition own constant retrieval; a standalone fast path may then be redundant. Do not implement a prerequisite unless it actually enables the chosen architecture and its incremental value is stated.

Price retained Option/bounds/load/return/push, setup/cache growth, fallback/capture work, spills/result representation and surrounding header/handler changes. Moving work before START is displaced work, not free elimination. Compiler effects are unpriced; no P1 or time prediction exists.

## Why deferred / next discriminating evidence

Active-context reuse overlaps the frame reconstruction; operand composition overlaps constant loading/dispatch/stack transfers. The 101 subtree is smaller than the whole expression envelope but safer/narrower is not automatically best. After another candidate lands, refresh the hot path before using these counts. Do not add a D saving to unchanged B/C predictions.

Next: source audit all promotion writes and capture classifications; diagram valid pointer lifetimes/miss reentry, then price both replacements above using current native fragments. A certificate failure or mutable-program counterexample changes feasibility, not merely the estimated percentage.

## Later approved validation / stop conditions

Test repeated closure/component capture with different scopes/receivers, null/numeric/string/array constants, per-VM promotion identity/isolation, nested calls and GC while constants remain live, frame/fiber relocation/reload, missing/out-of-range slots and non-JIT/JIT integration. Preserve existing compatibility behavior; inspect canonical BoxLang reference read-only before a semantic claim.

Verify generated hot hit and miss/capture paths BEFORE counters. Count caller/result preservation and surrounding changes, not just disappearance of borrow/helper symbols. Record original versioned net prediction before implementation; revise explicitly before outcomes if compilation changes it. Stop on an unproved certificate/lifetime or unsupported net ranking; no automatic variant. Separate approved instruction/time runs must retain unchanged workload/markers and raw failures.
