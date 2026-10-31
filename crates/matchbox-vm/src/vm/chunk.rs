use super::opcode::op;
use crate::types::Constant;
use crate::types::box_string::BoxString;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, Weak};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[cfg(not(target_os = "espidf"))]
pub type RuntimeCaches = Vec<Option<IcEntry>>;

#[cfg(target_os = "espidf")]
pub type RuntimeCaches = Vec<(usize, IcEntry)>;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum ConstantKey {
    Number(u64),
    String(BoxString),
    Boolean(bool),
    Null,
    StringArray(Vec<String>),
    Function(String),
}

impl ConstantKey {
    fn from_constant(c: &Constant) -> Option<Self> {
        match c {
            Constant::Number(f) => Some(ConstantKey::Number(f.to_bits())),
            Constant::String(s) => Some(ConstantKey::String(s.clone())),
            Constant::Boolean(b) => Some(ConstantKey::Boolean(*b)),
            Constant::Null => Some(ConstantKey::Null),
            Constant::StringArray(v) => Some(ConstantKey::StringArray(v.clone())),
            Constant::CompiledFunction(f) => Some(ConstantKey::Function(f.name.clone())),
            Constant::Class(_) | Constant::Interface(_) => None,
        }
    }
}

/// Immutable program data shared across all executions of a compiled chunk.
///
/// On memory-constrained targets such as the ESP32-S3, route tables keep a
/// single `Arc<ChunkProgram>` per route and create only a lightweight mutable
/// [`ChunkRuntime`] per request. This avoids duplicating bytecode, literal
/// constants, line tables, filename and source text for every request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkProgram {
    pub code: Vec<u32>,
    pub constants: Vec<Constant>,
    pub lines: Vec<u32>,
    pub filename: String,
    pub source: String,
}

impl Default for ChunkProgram {
    fn default() -> Self {
        ChunkProgram {
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
            filename: String::new(),
            source: String::new(),
        }
    }
}

impl ChunkProgram {
    pub fn new(filename: &str) -> Self {
        ChunkProgram {
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
            filename: filename.to_string(),
            source: String::new(),
        }
    }

    /// Create a new sub-program sharing the parent filename.
    pub fn new_sub_program(&self) -> Self {
        ChunkProgram {
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
            filename: self.filename.clone(),
            source: String::new(),
        }
    }
}

/// Mutable cache state, shared by calls to the same VM-owned function.
///
/// Script/route entry points and constant promotion create independent state.
/// On ESP32 the caches use a compact sparse representation.
#[derive(Debug, Clone)]
pub struct ChunkRuntime {
    pub caches: Arc<Mutex<RuntimeCaches>>,
    constant_map: HashMap<ConstantKey, u32>,
    cache_program: Option<Weak<ChunkProgram>>,
}

impl PartialEq for ChunkRuntime {
    fn eq(&self, other: &Self) -> bool {
        let caches_equal = Arc::ptr_eq(&self.caches, &other.caches) || {
            let caches = self.caches.lock().unwrap().clone();
            caches == *other.caches.lock().unwrap()
        };
        caches_equal && self.constant_map == other.constant_map
    }
}

impl Default for ChunkRuntime {
    fn default() -> Self {
        ChunkRuntime {
            caches: Arc::new(Mutex::new(Vec::new())),
            constant_map: HashMap::new(),
            cache_program: None,
        }
    }
}

impl ChunkRuntime {
    #[inline]
    fn emit_cache_slot(&mut self) {
        #[cfg(not(target_os = "espidf"))]
        self.caches.lock().unwrap().push(None);
    }

    pub fn ensure_caches(&mut self, _code_len: usize) {
        #[cfg(not(target_os = "espidf"))]
        {
            let mut caches = self.caches.lock().unwrap();
            if caches.len() < _code_len {
                caches.resize(_code_len, None);
            }
        }
    }

    pub fn cache_get(&self, ip: usize) -> Option<IcEntry> {
        Self::get_from(&self.caches.lock().unwrap(), ip)
    }

    fn get_from(caches: &RuntimeCaches, ip: usize) -> Option<IcEntry> {
        #[cfg(not(target_os = "espidf"))]
        {
            caches.get(ip).and_then(|entry| entry.clone())
        }

        #[cfg(target_os = "espidf")]
        {
            caches
                .iter()
                .find_map(|(cached_ip, entry)| (*cached_ip == ip).then(|| entry.clone()))
        }
    }

    pub fn cache_set(&mut self, ip: usize, entry: IcEntry) {
        Self::set_in(&mut self.caches.lock().unwrap(), ip, entry);
    }

    fn set_in(caches: &mut RuntimeCaches, ip: usize, entry: IcEntry) {
        #[cfg(not(target_os = "espidf"))]
        {
            if caches.len() <= ip {
                caches.resize(ip + 1, None);
            }
            caches[ip] = Some(entry);
        }

        #[cfg(target_os = "espidf")]
        {
            if let Some((_, existing)) = caches.iter_mut().find(|(cached_ip, _)| *cached_ip == ip) {
                *existing = entry;
            } else {
                caches.push((ip, entry));
            }
        }
    }

    pub fn cache_add_shape(&mut self, ip: usize, shape_id: usize, index: usize) {
        if let Some(entry) = Self::shape_entry(self.cache_get(ip), shape_id, index) {
            self.cache_set(ip, entry);
        }
    }

    fn shape_entry(cached: Option<IcEntry>, shape_id: usize, index: usize) -> Option<IcEntry> {
        match cached {
            None => Some(IcEntry::Monomorphic { shape_id, index }),
            Some(IcEntry::Monomorphic {
                shape_id: cached_shape,
                index: cached_index,
            }) => {
                if cached_shape == shape_id {
                    return None;
                }
                let mut entries = [(0, 0); 4];
                entries[0] = (cached_shape, cached_index);
                entries[1] = (shape_id, index);
                Some(IcEntry::Polymorphic { entries, count: 2 })
            }
            Some(IcEntry::Polymorphic { mut entries, count }) => {
                if entries[..count]
                    .iter()
                    .any(|(cached_shape, _)| *cached_shape == shape_id)
                {
                    return None;
                }
                if count < entries.len() {
                    entries[count] = (shape_id, index);
                    Some(IcEntry::Polymorphic { entries, count: count + 1 })
                } else {
                    Some(IcEntry::Megamorphic)
                }
            }
            Some(IcEntry::Megamorphic | IcEntry::Global { .. }) => None
        }
    }

    pub fn cache_slice(&self, start: usize, len: usize) -> Vec<Option<IcEntry>> {
        (start..start + len).map(|ip| self.cache_get(ip)).collect()
    }
}

/// A compiled chunk of BoxLang bytecode.
///
/// Historically `Chunk` contained both immutable program data and mutable
/// runtime caches. It now wraps a shared [`ChunkProgram`] plus a local
/// [`ChunkRuntime`]. The default constructor creates a unique program, which
/// preserves the existing API for compilation and direct execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub program: Arc<ChunkProgram>,
    #[serde(skip)]
    pub runtime: ChunkRuntime,
    // Frame clones copy this vector; promotion/function metadata uses only ChunkRuntime.
    #[serde(skip)]
    owned_route_caches: Option<RuntimeCaches>,
}

impl Default for Chunk {
    fn default() -> Self {
        Chunk {
            program: Arc::new(ChunkProgram::default()),
            runtime: ChunkRuntime::default(),
            owned_route_caches: None,
        }
    }
}

impl Chunk {
    #[inline]
    fn emit_cache_slot(&mut self) {
        self.runtime.emit_cache_slot();
    }

    pub fn new(filename: &str) -> Self {
        Chunk {
            program: Arc::new(ChunkProgram::new(filename)),
            runtime: ChunkRuntime::default(),
            owned_route_caches: None,
        }
    }

    /// Create a new sub-chunk.
    pub fn new_sub_chunk(&self) -> Self {
        Chunk {
            program: Arc::new(self.program.new_sub_program()),
            runtime: ChunkRuntime::default(),
            owned_route_caches: None,
        }
    }

    /// Emit a zero-operand instruction (1 word).
    #[inline]
    pub fn emit0(&mut self, opcode: u8, line: u32) {
        Arc::make_mut(&mut self.program).code.push(opcode as u32);
        Arc::make_mut(&mut self.program).lines.push(line);
        self.emit_cache_slot();
    }

    /// Emit a single-operand instruction (1 word).
    /// Operand `a` occupies bits [31:8]; must fit in 24 bits for correct decoding.
    #[inline]
    pub fn emit1(&mut self, opcode: u8, a: u32, line: u32) {
        Arc::make_mut(&mut self.program)
            .code
            .push((opcode as u32) | (a << 8));
        Arc::make_mut(&mut self.program).lines.push(line);
        self.emit_cache_slot();
    }

    /// Emit a two-operand instruction (2 words).
    /// `a` in word0 bits [31:8], `b` in word1 (full 32 bits).
    #[inline]
    pub fn emit2(&mut self, opcode: u8, a: u32, b: u32, line: u32) {
        Arc::make_mut(&mut self.program)
            .code
            .push((opcode as u32) | (a << 8));
        Arc::make_mut(&mut self.program).lines.push(line);
        self.emit_cache_slot();
        Arc::make_mut(&mut self.program).code.push(b);
        Arc::make_mut(&mut self.program).lines.push(line);
        self.emit_cache_slot();
    }

    /// Emit a three-operand instruction (3 words).
    /// `a` in word0 bits [31:8], `b` in word1, `c` in word2 (each full 32 bits except a which is 24-bit).
    #[inline]
    pub fn emit3(&mut self, opcode: u8, a: u32, b: u32, c: u32, line: u32) {
        Arc::make_mut(&mut self.program)
            .code
            .push((opcode as u32) | (a << 8));
        Arc::make_mut(&mut self.program).lines.push(line);
        self.emit_cache_slot();
        Arc::make_mut(&mut self.program).code.push(b);
        Arc::make_mut(&mut self.program).lines.push(line);
        self.emit_cache_slot();
        Arc::make_mut(&mut self.program).code.push(c);
        Arc::make_mut(&mut self.program).lines.push(line);
        self.emit_cache_slot();
    }

    /// Emit ITER_NEXT: 3 words.
    /// word0 = ITER_NEXT | (collection_slot << 8)
    /// word1 = cursor_slot | (has_index << 31)
    /// word2 = exit_offset (placeholder 0, back-patched later)
    #[inline]
    pub fn emit_iter_next(&mut self, collection: u32, cursor: u32, has_index: bool, line: u32) {
        let word1 = cursor | if has_index { 0x8000_0000u32 } else { 0 };
        self.emit3(op::ITER_NEXT, collection, word1, 0, line);
    }

    pub fn add_constant(&mut self, value: Constant) -> u32 {
        if let Some(key) = ConstantKey::from_constant(&value) {
            if let Some(&idx) = self.runtime.constant_map.get(&key) {
                return idx;
            }
            let idx = self.program.constants.len() as u32;
            Arc::make_mut(&mut self.program).constants.push(value);
            self.runtime.constant_map.insert(key, idx);
            idx
        } else {
            Arc::make_mut(&mut self.program).constants.push(value);
            (self.program.constants.len() - 1) as u32
        }
    }

    pub fn ensure_caches(&mut self) {
        if let Some(_caches) = &mut self.owned_route_caches {
            #[cfg(not(target_os = "espidf"))]
            _caches.resize(_caches.len().max(self.program.code.len()), None);
        } else {
            self.runtime.ensure_caches(self.program.code.len());
        }
    }

    pub fn cache_get(&self, ip: usize) -> Option<IcEntry> {
        match &self.owned_route_caches {
            Some(caches) => ChunkRuntime::get_from(caches, ip),
            None => self.runtime.cache_get(ip),
        }
    }

    pub fn cache_set(&mut self, ip: usize, entry: IcEntry) {
        match &mut self.owned_route_caches {
            Some(caches) => ChunkRuntime::set_in(caches, ip, entry),
            None => self.runtime.cache_set(ip, entry),
        }
    }

    pub fn cache_add_shape(&mut self, ip: usize, shape_id: usize, index: usize) {
        if let Some(entry) = ChunkRuntime::shape_entry(self.cache_get(ip), shape_id, index) {
            self.cache_set(ip, entry);
        }
    }

    pub fn cache_slice(&self, start: usize, len: usize) -> Vec<Option<IcEntry>> {
        (start..start + len).map(|ip| self.cache_get(ip)).collect()
    }

    /// Share VM-owned function cache state, not per-invocation scopes/constants.
    pub(super) fn execution_chunk(&self) -> Rc<RefCell<Chunk>> {
        let same_program = self.runtime.cache_program.as_ref().is_none_or(|program| {
            program
                .upgrade()
                .is_some_and(|program| Arc::ptr_eq(&program, &self.program))
        });
        Rc::new(RefCell::new(Chunk {
            program: Arc::clone(&self.program),
            runtime: ChunkRuntime {
                caches: if same_program {
                    Arc::clone(&self.runtime.caches)
                } else {
                    Arc::new(Mutex::new(Vec::new()))
                },
                constant_map: HashMap::new(),
                cache_program: Some(Arc::downgrade(&self.program)),
            },
            owned_route_caches: None,
        }))
    }

    pub fn clone_without_runtime_caches(&self) -> Self {
        Chunk {
            program: Arc::clone(&self.program),
            runtime: ChunkRuntime {
                cache_program: Some(Arc::downgrade(&self.program)),
                ..ChunkRuntime::default()
            },
            owned_route_caches: None,
        }
    }

    /// Route frames are not function metadata; their caches die with the frame.
    pub(super) fn fresh_route_chunk(&self) -> Self {
        let mut chunk = self.clone_without_runtime_caches();
        chunk.owned_route_caches = Some(Vec::new());
        chunk
    }

    pub fn reconstruct_functions(&mut self) {
        // NO-OP in the split-program model.
    }

    // Legacy field accessors to minimize churn in the rest of the codebase.
    // These forward to the shared program data.
    pub fn code(&self) -> &Vec<u32> {
        &self.program.code
    }

    pub fn constants(&self) -> &Vec<Constant> {
        &self.program.constants
    }

    pub fn lines(&self) -> &Vec<u32> {
        &self.program.lines
    }

    pub fn filename(&self) -> &String {
        &self.program.filename
    }

    pub fn source(&self) -> &String {
        &self.program.source
    }

    pub fn code_mut(&mut self) -> &mut Vec<u32> {
        &mut Arc::make_mut(&mut self.program).code
    }

    pub fn constants_mut(&mut self) -> &mut Vec<Constant> {
        &mut Arc::make_mut(&mut self.program).constants
    }

    pub fn lines_mut(&mut self) -> &mut Vec<u32> {
        &mut Arc::make_mut(&mut self.program).lines
    }

    pub fn set_filename(&mut self, filename: String) {
        Arc::make_mut(&mut self.program).filename = filename;
    }

    pub fn set_source(&mut self, source: String) {
        Arc::make_mut(&mut self.program).source = source;
    }
}

impl std::ops::Deref for Chunk {
    type Target = ChunkProgram;

    fn deref(&self) -> &Self::Target {
        &self.program
    }
}

impl std::ops::DerefMut for Chunk {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.program)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IcEntry {
    Monomorphic {
        shape_id: usize,
        index: usize,
    },
    Polymorphic {
        entries: [(usize, usize); 4],
        count: usize,
    },
    Megamorphic,
    Global {
        index: usize,
        namespace: u64,
    },
}
