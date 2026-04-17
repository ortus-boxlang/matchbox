use std::collections::HashMap;

pub type InternId = u32;

pub struct StringInterner {
    strings: Vec<String>,
    lookup: HashMap<String, InternId>,
}

impl StringInterner {
    pub fn new() -> Self {
        StringInterner {
            strings: Vec::new(),
            lookup: HashMap::new(),
        }
    }

    /// Intern a string with case-insensitive lookup while preserving the
    /// first-seen spelling for later resolution back to text.
    pub fn intern(&mut self, s: &str) -> InternId {
        let lowered = s.to_lowercase();
        if let Some(&id) = self.lookup.get(&lowered) {
            if let Some(existing) = self.strings.get_mut(id as usize) {
                if existing == &lowered && s != lowered {
                    *existing = s.to_string();
                }
            }
            return id;
        }
        let id = self.strings.len() as InternId;
        self.strings.push(s.to_string());
        self.lookup.insert(lowered, id);
        id
    }

    /// Resolve an InternId back to its preserved spelling.
    pub fn resolve(&self, id: InternId) -> &str {
        &self.strings[id as usize]
    }

    /// Read-only lookup (no insert). Returns None if the string was never interned.
    pub fn get_id(&self, s: &str) -> Option<InternId> {
        let lowered = s.to_lowercase();
        self.lookup.get(&lowered).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::StringInterner;

    #[test]
    fn preserves_first_seen_spelling_while_matching_case_insensitively() {
        let mut interner = StringInterner::new();

        let id1 = interner.intern("acceptAllDevices");
        let id2 = interner.intern("acceptalldevices");

        assert_eq!(id1, id2);
        assert_eq!(interner.resolve(id1), "acceptAllDevices");
        assert_eq!(interner.get_id("ACCEPTALLDEVICES"), Some(id1));
    }

    #[test]
    fn upgrades_lowercase_spelling_when_mixed_case_arrives_later() {
        let mut interner = StringInterner::new();

        let id1 = interner.intern("acceptalldevices");
        let id2 = interner.intern("acceptAllDevices");

        assert_eq!(id1, id2);
        assert_eq!(interner.resolve(id1), "acceptAllDevices");
    }
}
