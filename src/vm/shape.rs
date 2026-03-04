use std::collections::HashMap;

pub type ShapeId = usize;

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub fields: HashMap<String, usize>,
    pub transitions: HashMap<String, ShapeId>,
}

impl Shape {
    pub fn new() -> Self {
        Shape {
            fields: HashMap::new(),
            transitions: HashMap::new(),
        }
    }
}

pub struct ShapeRegistry {
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

    pub fn transition(&mut self, current_id: ShapeId, field_name: &str) -> ShapeId {
        // 1. Check if transition already exists
        if let Some(&next_id) = self.shapes[current_id].transitions.get(field_name) {
            return next_id;
        }

        // 2. Create new shape based on current
        let mut new_shape = self.shapes[current_id].clone();
        new_shape.transitions.clear(); // Transitions are specific to the path taken
        
        let new_index = new_shape.fields.len();
        new_shape.fields.insert(field_name.to_string(), new_index);
        
        let new_id = self.shapes.len();
        self.shapes.push(new_shape);
        
        // 3. Record transition in the parent shape
        self.shapes[current_id].transitions.insert(field_name.to_string(), new_id);
        
        new_id
    }

    pub fn get_index(&self, shape_id: ShapeId, field_name: &str) -> Option<usize> {
        self.shapes[shape_id].fields.get(field_name).copied()
    }
}
