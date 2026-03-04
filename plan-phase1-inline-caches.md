# Plan: Inline Caches (ICs)

**Objective:** Implement Monomorphic and Polymorphic Inline Caches to eliminate redundant shape lookups in hot execution paths.

**Prerequisite:** `plan-phase1-shapes.md` must be completed.

## Architectural Changes
1. **Cache Metadata:** A side-table in the `Chunk` or `VM` that maps instruction pointers (IP) to cache entries.
2. **IC Entry:**
   - `struct IcEntry { shape_id: ShapeId, index: usize }`
   - For Polymorphic ICs: `Vec<IcEntry>` (limited to 4 entries).

## Implementation Steps
1. **Infrastructure:** Add a `caches: Vec<Option<IcEntry>>` to `Chunk`, indexed by `OpCode` offset.
2. **Monomorphic Logic:**
   - In `OpMember`, check if `caches[ip]` matches the current object's `ShapeId`.
   - If yes: Return `data[index]` immediately (**Fast Path**).
   - If no: Perform full lookup, then update the cache with the new shape and index (**Slow Path**).
3. **Polymorphic Logic:** Upgrade the entry to store multiple shapes to handle code that processes different types of objects.
4. **Benchmarking:** Create a tight loop script to measure the performance delta.

## Benefits
- Near-native performance for property access in hot loops.
- Drastic reduction in VM overhead for dynamic objects.
