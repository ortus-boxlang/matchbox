# Analysis handoff 3: globals and dispatch

Status: **historical source/native handoff, supplemented by an executed native-path ledger.** See [current accounting](09-native-ledger.md): header self work 462 plus its fiber callees 126 = 588 instructions/cycle; cache/access/lock/poison helper chain 170. The rejected projection candidate is now reconciled at +21/cycle. Sample shares below are observations, not cost bills.

## Scope and question

Explain cached global reads/writes, repeated dispatch bookkeeping and the fused back edge. Distinguish a global cache HIT from a free register access; distinguish repeated guards from startup-only setup.

Read brief/evidence/profile prerequisites. Source checked at 139be94.

## Source-confirmed global path

- `crates/matchbox-vm/src/vm/mod.rs`, GET_GLOBAL around 5502, SET_GLOBAL_POP around 5560: borrow frame.chunk, read instruction-site cache, match Global entry and namespace; on hit read/write global_values[index]. Miss path reads/interns name, consults global_names, and populates cache.
- `vm/mod.rs`, insert_global_interned around 2889: initial sum definition updates/appends global slot. This setup precedes the counted loop.
- `crates/matchbox-vm/src/vm/chunk.rs`, Chunk::cache_get around 354: choose owned-route cache or shared runtime cache. The measured interpret path uses ordinary runtime caches, not fresh_route_chunk owned caches.
- `vm/chunk.rs`, ChunkRuntime::cache_get around 136: **caches.lock().unwrap()**, then get_from indexes the cache and clones the entry. Storage is **Arc<Mutex<RuntimeCaches>>**. Even the warmed source-level cache-hit path traverses this access; do not describe it as only a direct global_values read.

Per iteration: one GET_GLOBAL and one SET_GLOBAL_POP, hence **two source-level mutex-protected cache reads** after warmup/fill. Normal hits do not intern names or populate cache every iteration. Uncontended lock does not imply kernel blocking; native optimizer behavior and actual atomic/helper work require assembly evidence. Do NOT attribute a percentage from source alone.

The Arc field's presence does not mean Arc is cloned each cache access. Identify actual reference-count operations before claiming them. Cloning the IcEntry enum may or may not involve copying its maximum payload after optimization; inspect native code rather than assuming.

## Source-confirmed dispatch/back-edge path

- `vm/mod.rs`, run_fiber_inner around 4269 and quantum loop around 4316: native-completion empty condition; fiber/frame/yield checks; frame-state reload when changed; code bounds; JIT-feature local-pointer setup; GC/safepoint checks; optional debugger checks; opcode fetch/decode; optional trace; interrupt check; match dispatch.
- Repeated loop executes **seven dispatches**, not twelve. Code/cache/headroom setup inside frame_changed is not paid seven times every iteration.
- Existing feature-dependent compiled code matters even with vm.jit=None. Do not claim JIT compiled execution.
- FOR_LOOP_STEP around 4545: local read/write and bound comparison, then numeric/body eligibility branches. Its generic-body JIT block checks inactive JIT/active-loop state before falling back to interpreter back edge. With jit=None, profile state is not updated via JitState; actual surviving branches depend on code generation.
- Single-fiber deadline is None: no timeslice clock call inside the loop. Scheduler clock reads before execution are separate from repeated dispatch cost.

## Comparator context

Node's standard CommonJS wrapper keeps sum and i in local registers, unlike MatchBox's VM-global sum. Need to understand this difference, not erase it by silently changing the benchmark. A separately approved storage-matched comparison could clarify its impact but is not yet run.

V8 has its own interpreter checks and runtime overhead. Do not assume the twelve bytecodes are twelve cost-free jumps, or that MatchBox's seven dispatches prove dispatch cannot dominate.

## Reviewer assignment

- Identify global-cache hot native path, lock/unlock/enum-copy/refcell/namespace/index work that actually survives optimization.
- Locate repeated dispatch checks and their frequencies; establish which are outside the hot loop.
- Measure self versus inclusive hot samples and annotate assembly where possible.
- Separate inactive-JIT eligibility overhead from actual JIT work (none) and local/number work owned by other reviews.
- Explain why shared cache ownership/synchronization exists before suggesting removing it. Check reuse across calls, independent VMs, route entry and concurrent metadata ownership.

No lock replacement, feature removal, runtime-check suppression or profile tuning during investigation. Propose diagnostics to coordinator.

## Overlap / accounting ownership

Own shared dispatch, global-slot/cache access and back-edge non-local/non-arithmetic bookkeeping. Delegate helper-local work to area 1 and arithmetic/constants to area 2. Area 4 combines disjoint work; inclusive callgraph percentages cannot simply be added.

## Shared-profile/native update

Accepted instruction-sample SELF shares: Chunk::cache_get 9.03%, Mutex::lock 2.61%, shared MutexGuard drop helper 2.47%, plus poisoning helpers. Vec<BxFiber>::index is 6.40% across callers, not solely global access. Caller setup/inlined work remains in run_fiber_inner.

Matched native cache_get 0x3fb732..0x3fb7ff calls Mutex::lock at 0x3fb785 and, on a populated entry, executes rep movsb with rcx=0x50 at 0x3fb7b2. Thus the **80-byte cached-entry copy really survives compilation**, including on global hits. Two hits/iteration mean 160 bytes of copy payload, not necessarily main-memory traffic. Tail jump at 0x3fb7c9 enters guard drop. The regex generic name on that merged drop symbol does NOT mean regex executes in this loop.

Inspected shared dispatch/header native range 0x40c2df..0x40ca1d contains 1,280/6,513 instruction samples = 19.65% of all samples. It is a SUBSET of run_fiber_inner's 51.53% self share; do not add both. It includes frame/yield/GC/decode/jump-table work; bounds-helper callees are accounted outside the range. The outlined fiber-index routine at 0x406a1a retains bounds compare, multiply/address formation and return, and is called repeatedly by the header/handlers. This is concrete code-generation evidence, not just source-size inference.

The previous 31.88% residual is now partitioned by conservative native direct-branch reachability into about 30.79% candidate-specific handler bodies and 1.09% shared tails. GET_GLOBAL/SET_GLOBAL_POP bodies account for 2.69%/3.13%, excluding their helpers. Keep cache-get self separate from mutex and shared-helper self shares; do not infer that eliminating a cache lock alone removes all global-access cost.

Ownership audit: Chunk is explicitly tested as Send + Sync; script entry and constant promotion reset caches, function calls share VM-owned cache state when program identity matches, routes already use fresh direct-vector caches, and global namespace generations invalidate slot indices across restoration. Thus a global Mutex-to-RefCell substitution is not sound as a representation change. See [cache ownership/design review](07-cache-design-review.md) for source anchors, preserved contracts and the narrower proposed global-slot projection under the existing lock. No implementation change has been made.
