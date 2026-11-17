# Cache ownership and first-change design review

Status: **historical design proposal; subsequently approved candidate was tested, measured and rejected.** See [bounded result](08-cache-projection-result.md): +0.9664% retired instructions, implementation reverted. The review below records the rationale before that experiment, not a current recommendation to repeat it. Source is retained revision `139be94`. No new runtime samples, product changes or build changes were needed for this read-only review itself.

## What the existing profiles now establish

`map-handler-samples.py` and `handler-map.json` in the shared raw profile directory decode the actual native opcode jump table and follow direct branches from the six opcode classes in the repeated trace (ADD executes twice). Calls are not followed. Traversal stops at dispatch, returns and indirect jumps. Both sides of conditional branches are included, so this is conservative static reachability, not a dynamic native trace.

Of all 6,513 instruction samples:

| Native PC category | Samples | Share |
|---|---:|---:|
| Dispatch/header | 1,280 | 19.65% |
| FOR_LOOP_STEP body, including inlined local access | 774 | 11.88% |
| ADD bodies, both executions combined | 534 | 8.20% |
| GET_LOCAL body | 291 | 4.47% |
| SET_GLOBAL_POP body | 204 | 3.13% |
| GET_GLOBAL body | 175 | 2.69% |
| CONSTANT body | 27 | 0.41% |
| Shared handler tails / ambiguous ownership | 71 | 1.09% |
| Outlined helpers and other symbols | 3,157 | 48.47% |

These disjoint rows reconcile to 6,513. All previously unpartitioned inner-function samples now have a candidate handler or an explicitly shared label. "Unique" means unique among the six known repeated opcode classes, NOT proof of unique ownership among every opcode in the VM. Fixed entry/exit work and sampling skid remain caveats. Every sampled inner PC matches a disassembled instruction boundary.

This refines the previous 31.88% residual into about 30.79% candidate-specific bodies and 1.09% shared tails. Outlined helper ownership remains partly unresolved; native symbol coverage is not the same as a complete semantic cost bill. Run-to-run FOR_LOOP_STEP shares are 11.24–12.34%; ADD 7.28–8.76%. Do not interpret hundredths of a percent as precision.

Native anchors checked against source:
- GET_GLOBAL enters at `0x417d53`, borrows the frame chunk, calls cache_get at `0x417dd4`, checks variant/namespace, then indexes global values and resolves the fiber again before stack push.
- SET_GLOBAL_POP enters at `0x4195af`, calls cache_get at `0x41962b`, then independently resolves the fiber for pop and checks variant/namespace.
- Numeric ADD reaches `0x41c20c..0x41c28e`: conversion calls, operand retention/reloads, addition and tagging. ADD's 8.20% excludes numeric_value and stack helper self samples.
- FOR_LOOP_STEP enters at `0x40e909`. Float increment/tagging at `0x41cf2c..0x41cf69` is followed by local-binding/set machinery. Stack fallback at `0x41dc82..0x41dd24` repeatedly resolves fiber/frame/base before the write. Promoted bound lookup begins at `0x41dd24`. Its 11.88% is not the cost of a lone increment/compare.
- The inactive-JIT numeric back edge retains setup/checks: `0x41efcf..0x41f081` prepares loop/body state before testing the JIT option. This is mixed back-edge/JIT bookkeeping, not an isolated JIT-overhead estimate. No JIT execution occurred.

## Cache ownership: source-confirmed contracts

| Layer/path | Representation and lifetime | Relevant source |
|---|---|---|
| Reusable program | Arc<ChunkProgram>, immutable/shareable compiled data | `crates/matchbox-vm/src/vm/chunk.rs:40` |
| Chunk wrapper | Send + Sync metadata; runtime cache handle is Arc<Mutex<RuntimeCaches>> | `chunk.rs:93`, `tests/route_cache_tests.rs:198` |
| Script `VM::interpret` | Fresh runtime caches; program shared; execution chunk in Rc<RefCell<Chunk>> | `vm/mod.rs:3389` |
| Borrowed/route entry | Fresh frame-owned cache vector; does not populate source caches | `vm/mod.rs:3399`, `chunk.rs:411` |
| Promoted function metadata | Fresh cache state for this VM, not source VM slot/shape guards | `vm/mod.rs:9343` |
| Calls/recursion to VM-owned function | New execution chunk wrapper, same cache Arc when program matches | `chunk.rs:379` |
| Program replacement | Weak program identity check prevents sharing cache state for a different program | `chunk.rs:380` |
| Request/global restoration | Fresh global namespace generation invalidates old cached slot indices | `vm/request_scope.rs:16,78` |

Consequences:
- A blanket Mutex → RefCell substitution in ChunkRuntime breaks the explicit Send + Sync contract. The single-threaded VM does not make all reusable metadata thread-confined.
- Route-owned direct-vector access already exists. Turning every cache into a per-frame vector would discard sharing across function calls/recursion, and could add cloning/allocation. It is not a drop-in fix.
- Namespace checks are correctness guards, not optional overhead. Slot indices refer to a VM/global layout, not universally to the immutable program.
- Global cached reads need only an index and namespace, but cache_get returns the entire IcEntry enum. The largest alternative contains four shape/index pairs. The compiled global-hit path copies 80 bytes because it uses this general result representation.

## Recommended first candidate: project a global-slot hit under the existing lock

Proposed behavior, not an implementation:

`cached_global_index(ip, current_namespace) -> Option<usize>`

Inspect the cache entry **by reference** while holding the same mutex (or using the existing route-owned vector). Return only the index when the entry is Global and namespace matches; otherwise return None. Do not return a reference into unlocked storage. Keep current bounds checks and miss/refill behavior.

Apply the same global-slot accessor to all five global IC consumers: INC_GLOBAL, GLOBAL_COMPARE_JUMP, GET_GLOBAL, SET_GLOBAL and SET_GLOBAL_POP. Retain general entry retrieval for actual shape-cache/JIT inspection consumers; it is a different operation, not a compatibility fallback.

Why this first:
- It tests the concrete mismatch between a small query and a wide result without changing cache ownership, bytecode semantics, sharing or concurrency contracts.
- It can avoid materializing/copying the full enum and checking it again at the caller. Native output must confirm that it actually does.
- It does NOT remove mutex/poisoning, repeated fiber/frame lookups, scope fallback, dispatch or stack machinery. Its measured benefit may be modest.
- cache_get's 9.03% self share is not a copy-only cost or a predicted saving. Even hypothetically removing that whole share leaves most of the 8.55x discrepancy. This is a bounded validation of one mechanism, not a complete architectural answer.

Alternatives deferred:
- **VM-owned runtime cache layer separate from shareable compiled metadata:** potentially removes synchronization from execution, but needs a coherent model for function identity, recursion sharing, program replacement and request namespaces. Worth a later architectural review; not justified as the first implementation step by this one loop alone.
- **Direct cached frame/local execution state:** supported by repeated native indexing and scope machinery, but crosses yielding, frame changes, scope materialization and GC pointer validity. Larger semantic surface than a projected cache read.
- **No change:** appropriate if the candidate does not improve retired instructions or complicates the design. No loop of repeated speculative tuning is authorized.

## Validation gate for a subsequently approved implementation

1. Unit coverage for missing cache slot, empty/wrong variant, matching/mismatched namespace, both route-owned and mutex-backed caches, and unchanged cache contents. Test sparse ESP32 logic at the source/configuration level or explicitly record cross-target validation limits.
2. Existing route-cache tests unchanged: thread sharing and serialization; independent VMs/global layouts; retained closures across namespaces and GC; shape polymorphism; errors and all route entrypoints.
3. Existing host lifecycle tests unchanged, especially source cache immutability and repeated route execution with shared constants. Cover function cache sharing/recursion and program identity changes.
4. Ordinary relevant VM/global tests and regression suite. No weakened tests or changed BoxLang contracts.
5. Separate candidate build/provenance; preserve reference. Inspect native global-hit path for absence of the 80-byte copy and retention of lock/namespace guards. A Rust source edit alone is not proof.
6. Sequential unchanged region-count comparison, three fresh processes per executable, exact checksum, matched features/toolchain/profile; timing remains a separate metric. No autoresearch reset or broad sweep. Define this bounded experiment explicitly before running it.

## Review decision

The existing data is sufficient to discuss this first candidate. More sampling of the same workload is not currently the bottleneck. The larger architectural finding is distributed state/representation overhead, not one disastrous arithmetic instruction. Approval of a specific implementation/validation scope is the next decision; nothing has been optimized here.
