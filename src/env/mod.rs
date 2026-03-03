use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use crate::types::BxValue;

#[derive(Debug, Clone)]
pub struct Environment {
    parent: Option<Rc<RefCell<Environment>>>,
    values: HashMap<String, BxValue>,
}

impl Environment {
    pub fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Environment {
            parent: None,
            values: HashMap::new(),
        }))
    }

    pub fn new_with_parent(parent: Rc<RefCell<Environment>>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Environment {
            parent: Some(parent),
            values: HashMap::new(),
        }))
    }

    pub fn define(&mut self, name: String, value: BxValue) {
        self.values.insert(name.to_lowercase(), value);
    }

    pub fn assign(&mut self, name: &str, value: BxValue) -> Result<(), String> {
        let name_lower = name.to_lowercase();
        if self.values.contains_key(&name_lower) {
            self.values.insert(name_lower, value);
            Ok(())
        } else if let Some(parent) = &self.parent {
            parent.borrow_mut().assign(name, value)
        } else {
            // Implicit declaration in global scope in basic POC,
            // or we could throw an error. Let's implicitly define.
            self.values.insert(name_lower, value);
            Ok(())
        }
    }

    pub fn get(&self, name: &str) -> Option<BxValue> {
        let name_lower = name.to_lowercase();
        if let Some(val) = self.values.get(&name_lower) {
            Some(val.clone())
        } else if let Some(parent) = &self.parent {
            parent.borrow().get(name)
        } else {
            None
        }
    }
}
