# Plan: Hidden Classes (Shapes)

**Objective:** Transition `Struct` and `Instance` property storage from `HashMap` to a Shape-based system to reduce memory overhead and enable future optimizations.

## Architectural Changes
1. **Shape Struct:** Represents a specific set of properties and their mapping to indices in a data vector.
   - `fields: HashMap<String, usize>`
   - `transitions: HashMap<String, ShapeId>` (for adding new properties)
2. **Shape Registry:** A centralized manager in the VM that deduplicates shapes.
3. **Refactored BxValue:**
   - `Struct(ShapeId, Vec<BxValue>)`
   - `Instance(ShapeId, Vec<BxValue>)`

## Implementation Steps
1. **Define Core Types:** Implement `Shape` and `ShapeRegistry` in `src/vm/shape.rs`.
2. **Refactor Types:** Update `BxValue` variants in `src/types/mod.rs`.
3. **Update VM Logic:**
   - Modify `OpMember` to lookup the index via the object's Shape.
   - Modify `OpSetMember` to handle shape transitions when a new property is added.
4. **Validation:** Ensure all existing struct and class tests pass with the new storage model.

## Benefits
- Reduced memory usage per object (no individual HashMaps).
- faster property lookups (single hash check vs map traversal).
- Prerequisite for Inline Caches (ICs).
