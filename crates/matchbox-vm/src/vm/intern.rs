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

    /// Intern a string: preserve the first casing seen for presentation,
    /// but use its lowercase form for deduplication (case-insensitivity).
    pub fn intern(&mut self, s: &str) -> InternId {
        let lowered = s.to_lowercase();
        if let Some(&id) = self.lookup.get(&lowered) {
            return id;
        }
        let id = self.strings.len() as InternId;
        self.strings.push(s.to_string());
        self.lookup.insert(lowered, id);
        id
    }

    /// Resolve an InternId back to its original casing string.
    pub fn resolve(&self, id: InternId) -> &str {
        &self.strings[id as usize]
    }

    /// Read-only lookup (no insert). Returns None if the string was never interned.
    pub fn get_id(&self, s: &str) -> Option<InternId> {
        let lowered = s.to_lowercase();
        self.lookup.get(&lowered).copied()
    }
}
