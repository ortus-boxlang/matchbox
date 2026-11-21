//! Classes and interfaces in compiled bytecode are shared, not copied.
//!
//! The compiler compiles each class file once and every `new`, `extends` or
//! interface reference to it holds the same [`Shared`] value. On the wire a
//! shared value is written in full the first time it is met (`Def`) and as
//! the index of that first copy afterwards (`Ref`), so a program stores each
//! class once however many places use it. Reading rebuilds the same sharing.
//!
//! Dedup needs a session: [`encode`] and [`decode`] open one around postcard.
//! Without a session every value is written in full, which any reader accepts.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A compiled class or interface held by reference; clones share it.
pub struct Shared<T>(Arc<T>);

impl<T> Shared<T> {
    /// Wraps a value so every clone of this handle points at the same value.
    pub fn new(value: T) -> Self {
        Shared(Arc::new(value))
    }

    /// A mutable reference, copying the value first when it is shared.
    pub fn make_mut(&mut self) -> &mut T
    where
        T: Clone,
    {
        Arc::make_mut(&mut self.0)
    }

    /// The value itself, copied when it is shared.
    pub fn into_inner(self) -> T
    where
        T: Clone,
    {
        Arc::unwrap_or_clone(self.0)
    }

    /// Both hold the very same value (not only equal ones).
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// The address of the shared value: equal for clones, an identity key.
    pub fn addr(&self) -> usize {
        Arc::as_ptr(&self.0) as *const () as usize
    }
}

impl<T> Clone for Shared<T> {
    /// Clones the handle, not the value: both handles hold the same compiled class.
    fn clone(&self) -> Self {
        Shared(Arc::clone(&self.0))
    }
}

impl<T> Deref for Shared<T> {
    type Target = T;
    /// Reads through the handle as if it were the compiled class itself.
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T: PartialEq> PartialEq for Shared<T> {
    /// Equal when the handles share a value, or when the values themselves are equal.
    fn eq(&self, other: &Self) -> bool {
        Shared::ptr_eq(self, other) || *self.0 == *other.0
    }
}

impl<T: fmt::Debug> fmt::Debug for Shared<T> {
    /// Formats the value, not the handle, so debug output is unchanged.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Default)]
struct WriteSession {
    /// value address -> index of its first (full) copy
    seen: HashMap<usize, u32>,
    /// keeps every written value alive so an address is never reused while the session runs
    keep: Vec<Box<dyn Any>>,
}

#[derive(Default)]
struct ReadSession {
    /// every value read in full, in the order it was written
    values: Vec<Box<dyn Any>>,
}

thread_local! {
    static WRITING: RefCell<Option<WriteSession>> = const { RefCell::new(None) };
    static READING: RefCell<Option<ReadSession>> = const { RefCell::new(None) };
}

#[derive(Serialize)]
enum WireOut<'a, T> {
    Def(&'a T),
    Ref(u32),
}

#[derive(Deserialize)]
enum WireIn<T> {
    Def(T),
    Ref(u32),
}

impl<T: Serialize + 'static> Serialize for Shared<T> {
    /// Writes a full definition the first time a value is seen and a reference afterwards.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let known = WRITING.with(|w| w.borrow().as_ref().and_then(|s| s.seen.get(&self.addr()).copied()));
        if let Some(index) = known {
            return WireOut::<T>::Ref(index).serialize(serializer);
        }
        // The index is taken after the body, so nested values number first; the reader does the same.
        let ok = WireOut::Def(&*self.0).serialize(serializer)?;
        WRITING.with(|w| {
            if let Some(s) = w.borrow_mut().as_mut() {
                let index = s.keep.len() as u32;
                s.seen.insert(self.addr(), index);
                s.keep.push(Box::new(self.clone()));
            }
        });
        Ok(ok)
    }
}

impl<'de, T: Deserialize<'de> + 'static> Deserialize<'de> for Shared<T> {
    /// Reads a full definition, or reuses the value a reference points at in the read session.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match WireIn::<T>::deserialize(deserializer)? {
            WireIn::Def(value) => {
                let shared = Shared::new(value);
                READING.with(|r| {
                    if let Some(s) = r.borrow_mut().as_mut() {
                        s.values.push(Box::new(shared.clone()));
                    }
                });
                Ok(shared)
            }
            WireIn::Ref(index) => READING.with(|r| {
                let r = r.borrow();
                let session = r
                    .as_ref()
                    .ok_or_else(|| serde::de::Error::custom("shared class reference outside a bytecode decode session"))?;
                session
                    .values
                    .get(index as usize)
                    .and_then(|v| v.downcast_ref::<Shared<T>>())
                    .cloned()
                    .ok_or_else(|| serde::de::Error::custom(format!("bad shared class reference {index}")))
            }),
        }
    }
}

/// Clears a session when it ends, also on a panic.
struct Close(fn());
impl Drop for Close {
    /// Ends the encode or decode session, including when the operation panics.
    fn drop(&mut self) {
        (self.0)()
    }
}

/// Serializes bytecode (a chunk, a bundle) with each shared class written once.
pub fn encode<T: Serialize>(value: &T) -> postcard::Result<Vec<u8>> {
    let outer = WRITING.with(|w| {
        let mut w = w.borrow_mut();
        if w.is_some() {
            return false;
        }
        *w = Some(WriteSession::default());
        true
    });
    let _close = outer.then(|| Close(|| WRITING.with(|w| *w.borrow_mut() = None)));
    postcard::to_stdvec(value)
}

/// Reads bytecode written by [`encode`].
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> postcard::Result<T> {
    let outer = READING.with(|r| {
        let mut r = r.borrow_mut();
        if r.is_some() {
            return false;
        }
        *r = Some(ReadSession::default());
        true
    });
    let _close = outer.then(|| Close(|| READING.with(|r| *r.borrow_mut() = None)));
    postcard::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Holder {
        a: Shared<String>,
        b: Shared<String>,
        nested: Vec<Shared<String>>,
    }

    /// Repeated handles are written once and decoded back as one shared value.
    #[test]
    fn a_shared_value_is_written_once_and_read_back_shared() {
        let s = Shared::new("x".repeat(1000));
        let h = Holder { a: s.clone(), b: s.clone(), nested: vec![s.clone(), Shared::new("other".into())] };
        let bytes = encode(&h).unwrap();
        assert!(bytes.len() < 1100, "the 1000-byte value was written more than once: {} bytes", bytes.len());
        let back: Holder = decode(&bytes).unwrap();
        assert_eq!(back, h);
        assert!(Shared::ptr_eq(&back.a, &back.b) && Shared::ptr_eq(&back.a, &back.nested[0]));
        assert!(!Shared::ptr_eq(&back.a, &back.nested[1]));
    }

    /// Plain postcard writes stay readable, with one full copy per handle.
    #[test]
    fn without_a_session_every_copy_is_full_and_still_reads() {
        let s = Shared::new("y".repeat(1000));
        let h = Holder { a: s.clone(), b: s.clone(), nested: vec![] };
        let bytes = postcard::to_stdvec(&h).unwrap();
        assert!(bytes.len() > 2000);
        let back: Holder = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(back, h);
        let back: Holder = decode(&bytes).unwrap();
        assert_eq!(back, h);
    }

    /// A reference needs the read session that `decode` opens.
    #[test]
    fn a_reference_without_a_session_is_an_error() {
        let s = Shared::new("z".to_string());
        let bytes = encode(&Holder { a: s.clone(), b: s, nested: vec![] }).unwrap();
        assert!(postcard::from_bytes::<Holder>(&bytes).is_err());
    }
}
