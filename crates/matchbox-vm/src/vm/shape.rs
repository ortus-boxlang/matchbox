use rpds::{HashTrieMap, Vector};
use std::collections::HashMap;

/// Index of a layout in the [`ShapeRegistry`].
pub type ShapeId = u32;

/// A struct or instance layout: which slot each key lives in, and how each key is spelled.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    // Prefix layouts share HAMT nodes instead of retaining a full map copy per field.
    /// Case-folded key id -> slot.
    pub fields: HashTrieMap<u32, u32>,
    /// Slot -> interned key as written. Lookups fold case through `fields`; this keeps each
    /// layout's own spelling, so `{ mean: 1 }` stays `mean` after another layout used `Mean`.
    pub names: Vector<u32>,
    /// Layouts reached by adding one key, by (key id, spelling id).
    pub transitions: HashMap<(u32, u32), ShapeId>,
}

impl Shape {
    /// The empty layout.
    pub fn new() -> Self {
        Shape {
            fields: HashTrieMap::new(),
            names: Vector::new(),
            transitions: HashMap::new(),
        }
    }
}

/// Every layout the VM has created. Layouts are shared by all structs and instances that
/// add the same keys, spelled the same way, in the same order.
pub struct ShapeRegistry {
    // ponytail: layouts live for the VM lifetime; prune unused layouts if unique-layout churn warrants it.
    /// All layouts, indexed by [`ShapeId`]. Index 0 is the empty layout.
    pub shapes: Vec<Shape>,
}

impl ShapeRegistry {
    /// A registry holding only the empty layout.
    pub fn new() -> Self {
        // Initial empty shape at index 0
        ShapeRegistry {
            shapes: vec![Shape::new()],
        }
    }

    /// The empty layout every struct starts from.
    pub fn get_root(&self) -> ShapeId {
        0
    }

    /// The layout after adding `field_name_id`, spelled as the interned key itself.
    /// Use [`ShapeRegistry::transition_named`] when the written spelling is known.
    pub fn transition(&mut self, current_id: ShapeId, field_name_id: u32) -> ShapeId {
        self.transition_named(current_id, field_name_id, field_name_id)
    }

    /// Adds a field whose lookup id is `field_name_id` (case-folded) and whose spelling is
    /// the interned string `display_id`. Layouts that differ only in spelling are distinct.
    pub fn transition_named(&mut self, current_id: ShapeId, field_name_id: u32, display_id: u32) -> ShapeId {
        // 1. Check if transition already exists
        if let Some(&next_id) = self.shapes[current_id as usize]
            .transitions
            .get(&(field_name_id, display_id))
        {
            return next_id;
        }

        // 2. Keep layout IDs immutable for existing member/inline-cache guards.
        let current = &self.shapes[current_id as usize];
        let new_shape = Shape {
            fields: current.fields.insert(field_name_id, current.fields.size() as u32),
            names: current.names.push_back(display_id),
            transitions: HashMap::new(),
        };

        let new_id = self.shapes.len() as u32;
        self.shapes.push(new_shape);

        // 3. Record transition in the parent shape
        self.shapes[current_id as usize]
            .transitions
            .insert((field_name_id, display_id), new_id);

        new_id
    }

    /// The interned spelling of the field in `slot`.
    pub fn name(&self, shape_id: ShapeId, slot: u32) -> Option<u32> {
        self.shapes[shape_id as usize].names.get(slot as usize).copied()
    }

    /// The slot of `field_name_id` (case-folded) in `shape_id`, if the layout has it.
    pub fn get_index(&self, shape_id: ShapeId, field_name_id: u32) -> Option<u32> {
        self.shapes[shape_id as usize]
            .fields
            .get(&field_name_id)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::ShapeRegistry;

    #[test]
    fn shared_prefixes_preserve_indices_and_branch_identity() {
        let mut registry = ShapeRegistry::new();
        let first = registry.transition(0, 7);
        let other = registry.transition(0, 8);
        let reverse = registry.transition(other, 7);
        let mut wide = first;
        for key in 8..11300 {
            wide = registry.transition(wide, key);
        }
        assert_eq!(registry.transition(0, 7), first);
        assert_eq!(registry.get_index(0, 7), None);
        assert_eq!(registry.get_index(first, 7), Some(0));
        assert_eq!(registry.get_index(first, 8), None);
        assert_eq!(registry.get_index(reverse, 7), Some(1));
        assert_eq!(registry.get_index(wide, 7), Some(0));
        assert_eq!(registry.get_index(wide, 11299), Some(11292));
        assert_eq!(registry.name(wide, 0), Some(7));
    }

    #[test]
    fn spelling_is_part_of_the_layout() {
        let mut registry = ShapeRegistry::new();
        let lower = registry.transition_named(0, 7, 70);
        let upper = registry.transition_named(0, 7, 71);
        assert_ne!(lower, upper);
        assert_eq!(registry.transition_named(0, 7, 70), lower);
        assert_eq!(registry.get_index(lower, 7), Some(0));
        assert_eq!(registry.get_index(upper, 7), Some(0));
        assert_eq!(registry.name(lower, 0), Some(70));
        assert_eq!(registry.name(upper, 0), Some(71));
    }
}
