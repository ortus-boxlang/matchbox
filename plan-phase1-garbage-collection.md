# Plan: Garbage Collection (Mark-and-Sweep)

**Objective:** Replace Rust's `Rc` with a tracing Mark-and-Sweep Garbage Collector to support reference cycles and centralize memory management.

## Architectural Changes
1. **The Heap:** A central arena owned by the `VM` that manages all dynamic allocations.
2. **Managed Pointer:** `BxValue` will store a `GcId` (index) instead of an `Rc`.
3. **Roots:** The VM must track all "root" objects (current stacks, globals, and pending futures).

## Implementation Steps
1. **Define Heap:** Create a `Heap` struct that stores objects in a `Vec` or specialized arena.
2. **Mark Phase:** Implement recursive traversal starting from roots. Every reached object gets a `marked` flag.
3. **Sweep Phase:** Iterate through the entire heap. Delete unmarked objects and reset flags for the next cycle.
4. **Refactor Codebase:** 
   - Remove `Rc<RefCell<T>>` from all types in `src/types/mod.rs`.
   - Update every VM instruction to request/access objects via the `Heap`.
5. **Cycle Detection:** Verify that cyclic structures (e.g., `s = {}; s.self = s;`) are correctly reclaimed when the root is dropped.

## Benefits
- Elimination of memory leaks caused by reference cycles.
- Improved performance for object assignments (no atomic ref-count updates).
- More predictable memory footprint for large-scale applications.
