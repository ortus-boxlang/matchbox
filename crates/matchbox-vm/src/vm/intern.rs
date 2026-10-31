use std::collections::HashMap;

pub type InternId = u32;

pub struct StringInterner {
    strings: Vec<String>,
    lookup: HashMap<String, InternId>,
    exact_lookup: HashMap<String, InternId>,
}

impl StringInterner {
    pub fn new() -> Self {
        StringInterner {
            strings: Vec::new(),
            lookup: HashMap::new(),
            exact_lookup: HashMap::new(),
        }
    }

    /// Intern a string: preserve the first casing seen for presentation,
    /// but use its lowercase form for deduplication (case-insensitivity).
    pub fn intern(&mut self, s: &str) -> InternId {
        // Direct hits are canonical lowercase keys and cannot upgrade presentation.
        if let Some(&id) = self.lookup.get(s) {
            return id;
        }
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

    /// Intern a key without case folding. Case-sensitive structs use this
    /// path so distinct spellings can occupy distinct shape fields. Exact IDs
    /// must stay separate from folded IDs whose presentation can be upgraded.
    pub fn intern_case_sensitive(&mut self, s: &str) -> InternId {
        if let Some(&id) = self.exact_lookup.get(s) {
            return id;
        }

        let id = self.strings.len() as InternId;
        self.strings.push(s.to_string());
        self.exact_lookup.insert(s.to_string(), id);
        id
    }

    /// Resolve an InternId back to its original casing string.
    pub fn resolve(&self, id: InternId) -> &str {
        &self.strings[id as usize]
    }

    /// Read-only lookup (no insert). Returns None if the string was never interned.
    pub fn get_id(&self, s: &str) -> Option<InternId> {
        self.lookup.get(s).copied().or_else(|| {
            // Already-folded ASCII misses cannot match a different lowercase key.
            if s.bytes().all(|byte| byte.is_ascii() && !byte.is_ascii_uppercase()) {
                return None;
            }
            self.lookup.get(&s.to_lowercase()).copied()
        })
    }

    pub fn get_exact_id(&self, s: &str) -> Option<InternId> {
        self.exact_lookup.get(s).copied()
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

    #[test]
    fn case_sensitive_interning_keeps_distinct_spellings() {
        let mut interner = StringInterner::new();

        let upper = interner.intern_case_sensitive("Name");
        let lower = interner.intern_case_sensitive("name");

        assert_ne!(upper, lower);
        assert_eq!(interner.resolve(upper), "Name");
        assert_eq!(interner.resolve(lower), "name");
        assert_eq!(interner.get_exact_id("Name"), Some(upper));
        assert_eq!(interner.get_exact_id("name"), Some(lower));
    }
}
