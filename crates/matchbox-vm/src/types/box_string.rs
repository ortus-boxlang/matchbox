use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use serde::{Serialize, Serializer, Deserialize, Deserializer};

pub const SSO_CAPACITY: usize = 22;

#[derive(Clone, Debug)]
pub struct RopeData {
    pub len: u32,
    pub left: BoxString,
    pub right: BoxString,
}

#[derive(Clone, Debug)]
pub enum StringRepr {
    Inline { len: u8, buf: [u8; SSO_CAPACITY] },
    Flat(Arc<str>),
    Rope(Arc<RopeData>),
}

#[derive(Clone, Debug)]
pub struct BoxString {
    repr: StringRepr,
}

impl BoxString {
    pub fn new(s: &str) -> Self {
        let len = s.len();
        if len <= SSO_CAPACITY {
            let mut buf = [0; SSO_CAPACITY];
            buf[..len].copy_from_slice(s.as_bytes());
            BoxString {
                repr: StringRepr::Inline { len: len as u8, buf },
            }
        } else {
            BoxString {
                repr: StringRepr::Flat(Arc::from(s)),
            }
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match &self.repr {
            StringRepr::Inline { len, .. } => *len as usize,
            StringRepr::Flat(s) => s.len(),
            StringRepr::Rope(rope) => rope.len as usize,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn depth(&self) -> usize {
        match &self.repr {
            StringRepr::Inline { .. } | StringRepr::Flat(_) => 0,
            StringRepr::Rope(rope) => 1 + std::cmp::max(rope.left.depth(), rope.right.depth()),
        }
    }

    pub fn concat(&self, other: &BoxString) -> BoxString {
        let total_len = self.len() + other.len();
        if total_len <= SSO_CAPACITY {
            let mut buf = [0; SSO_CAPACITY];
            self.write_to_slice(&mut buf[..self.len()]);
            other.write_to_slice(&mut buf[self.len()..total_len]);
            BoxString {
                repr: StringRepr::Inline { len: total_len as u8, buf },
            }
        } else {
            BoxString {
                repr: StringRepr::Rope(Arc::new(RopeData {
                    len: total_len as u32,
                    left: self.clone(),
                    right: other.clone(),
                })),
            }
        }
    }

    fn write_to_slice(&self, dest: &mut [u8]) {
        match &self.repr {
            StringRepr::Inline { len, buf } => {
                dest.copy_from_slice(&buf[..*len as usize]);
            }
            StringRepr::Flat(s) => {
                dest.copy_from_slice(s.as_bytes());
            }
            StringRepr::Rope(rope) => {
                let left_len = rope.left.len();
                rope.left.write_to_slice(&mut dest[..left_len]);
                rope.right.write_to_slice(&mut dest[left_len..]);
            }
        }
    }

    pub fn flatten(&mut self) -> &str {
        if let StringRepr::Rope(rope) = &self.repr {
            let mut s = String::with_capacity(rope.len as usize);
            self.append_to_string(&mut s);
            self.repr = StringRepr::Flat(Arc::from(s));
        }

        match &self.repr {
            StringRepr::Inline { len, buf } => {
                unsafe { std::str::from_utf8_unchecked(&buf[..*len as usize]) }
            }
            StringRepr::Flat(s) => s,
            StringRepr::Rope(_) => unreachable!(),
        }
    }

    pub fn as_flat_str(&mut self) -> &str {
        self.flatten()
    }

    fn append_to_string(&self, dest: &mut String) {
        match &self.repr {
            StringRepr::Inline { len, buf } => {
                dest.push_str(unsafe { std::str::from_utf8_unchecked(&buf[..*len as usize]) });
            }
            StringRepr::Flat(s) => {
                dest.push_str(s);
            }
            StringRepr::Rope(rope) => {
                rope.left.append_to_string(dest);
                rope.right.append_to_string(dest);
            }
        }
    }
}

// ------------------------------------------------------------------------
// Traits
// ------------------------------------------------------------------------

impl Hash for BoxString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash_bytes(state);
    }
}

impl BoxString {
    fn hash_bytes<H: Hasher>(&self, state: &mut H) {
        match &self.repr {
            StringRepr::Inline { len, buf } => {
                state.write(&buf[..*len as usize]);
            }
            StringRepr::Flat(s) => {
                state.write(s.as_bytes());
            }
            StringRepr::Rope(rope) => {
                rope.left.hash_bytes(state);
                rope.right.hash_bytes(state);
            }
        }
    }
}

impl PartialEq for BoxString {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        
        let mut iter1 = BoxStringChunkIter::new(self);
        let mut iter2 = BoxStringChunkIter::new(other);
        
        let mut chunk1 = iter1.next().unwrap_or(&[]);
        let mut chunk2 = iter2.next().unwrap_or(&[]);
        
        while !chunk1.is_empty() && !chunk2.is_empty() {
            let min_len = chunk1.len().min(chunk2.len());
            if chunk1[..min_len] != chunk2[..min_len] {
                return false;
            }
            chunk1 = &chunk1[min_len..];
            if chunk1.is_empty() {
                chunk1 = iter1.next().unwrap_or(&[]);
            }
            chunk2 = &chunk2[min_len..];
            if chunk2.is_empty() {
                chunk2 = iter2.next().unwrap_or(&[]);
            }
        }
        
        chunk1.is_empty() && chunk2.is_empty()
    }
}

impl Eq for BoxString {}

struct BoxStringChunkIter<'a> {
    stack: Vec<&'a BoxString>,
}

impl<'a> BoxStringChunkIter<'a> {
    fn new(s: &'a BoxString) -> Self {
        Self { stack: vec![s] }
    }
}

impl<'a> Iterator for BoxStringChunkIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node) = self.stack.pop() {
            match &node.repr {
                StringRepr::Inline { len, buf } => {
                    if *len > 0 {
                        return Some(&buf[..*len as usize]);
                    }
                }
                StringRepr::Flat(s) => {
                    if !s.is_empty() {
                        return Some(s.as_bytes());
                    }
                }
                StringRepr::Rope(rope) => {
                    self.stack.push(&rope.right);
                    self.stack.push(&rope.left);
                }
            }
        }
        None
    }
}

impl fmt::Display for BoxString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            StringRepr::Inline { len, buf } => {
                let s = unsafe { std::str::from_utf8_unchecked(&buf[..*len as usize]) };
                write!(f, "{}", s)
            }
            StringRepr::Flat(s) => write!(f, "{}", s),
            StringRepr::Rope(rope) => {
                write!(f, "{}", rope.left)?;
                write!(f, "{}", rope.right)
            }
        }
    }
}

impl Serialize for BoxString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for BoxString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(BoxString::new(&s))
    }
}

// ------------------------------------------------------------------------
// Tests
// ------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;

    fn calculate_hash<T: Hash>(t: &T) -> u64 {
        let mut s = DefaultHasher::new();
        t.hash(&mut s);
        s.finish()
    }

    #[test]
    fn test_size() {
        assert!(std::mem::size_of::<BoxString>() <= 32, "BoxString size is strictly optimal");
    }

    #[test]
    fn test_new_inline() {
        let s = BoxString::new("hello");
        assert!(matches!(s.repr, StringRepr::Inline { .. }));
        assert_eq!(s.len(), 5);
        assert_eq!(s.to_string(), "hello");
        assert_eq!(s.depth(), 0);
    }

    #[test]
    fn test_new_flat() {
        let s = BoxString::new("this string is definitely longer than 22 characters");
        assert!(matches!(s.repr, StringRepr::Flat(_)));
        assert_eq!(s.len(), 51);
        assert_eq!(s.to_string(), "this string is definitely longer than 22 characters");
        assert_eq!(s.depth(), 0);
    }

    #[test]
    fn test_concat_inline() {
        let a = BoxString::new("hello");
        let b = BoxString::new(" world");
        let c = a.concat(&b);
        assert!(matches!(c.repr, StringRepr::Inline { .. }));
        assert_eq!(c.len(), 11);
        assert_eq!(c.to_string(), "hello world");
        assert_eq!(c.depth(), 0);
    }

    #[test]
    fn test_concat_rope() {
        let a = BoxString::new("this is a somewhat long string ");
        let b = BoxString::new("that will definitely create a rope when concatenated.");
        let c = a.concat(&b);
        assert!(matches!(c.repr, StringRepr::Rope(_)));
        assert_eq!(c.len(), a.len() + b.len());
        assert_eq!(c.to_string(), "this is a somewhat long string that will definitely create a rope when concatenated.");
        assert_eq!(c.depth(), 1);
    }

    #[test]
    fn test_flatten() {
        let a = BoxString::new("hello ");
        let b = BoxString::new("world ");
        let c = BoxString::new("this is a long string to force rope.");
        let mut rope = a.concat(&b).concat(&c);
        assert!(matches!(rope.repr, StringRepr::Rope(_)));
        
        let flat_str = rope.flatten();
        assert_eq!(flat_str, "hello world this is a long string to force rope.");
        assert!(matches!(rope.repr, StringRepr::Flat(_)));
    }

    #[test]
    fn test_eq_mixed_variants() {
        let long_part1 = "a".repeat(15);
        let long_part2 = "b".repeat(15);
        let rope1 = BoxString::new(&long_part1).concat(&BoxString::new(&long_part2));
        let flat1 = BoxString::new(&format!("{}{}", long_part1, long_part2));
        
        assert!(matches!(rope1.repr, StringRepr::Rope(_)));
        assert!(matches!(flat1.repr, StringRepr::Flat(_)));
        assert_eq!(rope1, flat1);
        
        let flat_diff = BoxString::new(&format!("{}X{}", long_part1, long_part2));
        assert_ne!(rope1, flat_diff);
    }

    #[test]
    fn test_hash_mixed_variants() {
        let long_part1 = "a".repeat(15);
        let long_part2 = "b".repeat(15);
        let rope1 = BoxString::new(&long_part1).concat(&BoxString::new(&long_part2));
        let flat1 = BoxString::new(&format!("{}{}", long_part1, long_part2));
        
        assert_eq!(calculate_hash(&rope1), calculate_hash(&flat1));
    }
}