use rpds::HashTrieMap;
use std::collections::HashMap;

pub type ShapeId = u32;

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    // Prefix layouts share HAMT nodes instead of retaining a full map copy per field.
    pub fields: HashTrieMap<u32, u32>,
    pub transitions: HashMap<u32, ShapeId>,
}

impl Shape {
    pub fn new() -> Self {
        Shape {
            fields: HashTrieMap::new(),
            transitions: HashMap::new(),
        }
    }
}

pub struct ShapeRegistry {
    // ponytail: layouts live for the VM lifetime; prune unused layouts if unique-layout churn warrants it.
    pub shapes: Vec<Shape>,
}

impl ShapeRegistry {
    pub fn new() -> Self {
        // Initial empty shape at index 0
        ShapeRegistry {
            shapes: vec![Shape::new()],
        }
    }

    pub fn get_root(&self) -> ShapeId {
        0
    }

    pub fn transition(&mut self, current_id: ShapeId, field_name_id: u32) -> ShapeId {
        // 1. Check if transition already exists
        if let Some(&next_id) = self.shapes[current_id as usize]
            .transitions
            .get(&field_name_id)
        {
            return next_id;
        }

        // 2. Keep layout IDs immutable for existing member/inline-cache guards.
        let fields = &self.shapes[current_id as usize].fields;
        let new_shape = Shape {
            fields: fields.insert(field_name_id, fields.size() as u32),
            transitions: HashMap::new(),
        };

        let new_id = self.shapes.len() as u32;
        self.shapes.push(new_shape);

        // 3. Record transition in the parent shape
        self.shapes[current_id as usize]
            .transitions
            .insert(field_name_id, new_id);

        new_id
    }

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
    }
}
