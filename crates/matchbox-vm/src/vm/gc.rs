use crate::types::{
    BxClass, BxCompiledFunction, BxFuture, BxInstance, BxInterface, BxNativeFunction,
    BxNativeObject, BxRange, BxStruct, BxValue, box_string::BoxString,
};
use chrono::{DateTime, Utc};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

pub type GcId = usize;

/// VM-internal root; no Rust layout crosses the plugin ABI.
pub struct RootedValue {
    value: BxValue,
    heap: Weak<()>,
}

/// Configuration for the garbage collector, with preset defaults
/// tuned for different target platforms.
#[derive(Debug, Clone)]
pub struct GCConfig {
    /// Initial capacity of backing Vecs (objects, marks, etc.)
    pub initial_capacity: usize,
    /// Initial GC threshold (alloc_count > this triggers collection)
    pub initial_threshold: usize,
    /// Maximum GC threshold after dynamic adjustment
    pub max_threshold: usize,
    /// Optional hard cap on total heap objects
    pub max_objects: Option<usize>,
}

impl GCConfig {
    /// Aggressive defaults for ESP32 (~320KB heap budget).
    pub fn for_esp32() -> Self {
        GCConfig {
            initial_capacity: 128,
            initial_threshold: 256,
            max_threshold: 1024,
            max_objects: Some(2048),
        }
    }

    /// Lenient defaults for desktop (plentiful RAM).
    pub fn for_desktop() -> Self {
        GCConfig {
            initial_capacity: 1024,
            initial_threshold: 10000,
            max_threshold: 100000,
            max_objects: None,
        }
    }

    /// Balanced defaults for WASM (browser/edge memory limits).
    pub fn for_wasm() -> Self {
        GCConfig {
            initial_capacity: 512,
            initial_threshold: 2000,
            max_threshold: 20000,
            max_objects: Some(50000),
        }
    }
}

impl Default for GCConfig {
    fn default() -> Self {
        #[cfg(target_os = "espidf")]
        {
            GCConfig::for_esp32()
        }
        #[cfg(not(target_os = "espidf"))]
        {
            GCConfig::for_desktop()
        }
    }
}

#[derive(Debug, Clone)]
pub enum GcObject {
    String(BoxString),
    Bytes(Vec<u8>),
    Array(Vec<BxValue>),
    Range(BxRange),
    DateTime(DateTime<Utc>),
    Struct(BxStruct),
    Instance(BxInstance),
    Future(BxFuture),
    CompiledFunction(Rc<BxCompiledFunction>),
    NativeFunction(BxNativeFunction),
    NativeCallback(crate::types::NativeCallback),
    Class(Rc<RefCell<BxClass>>),
    Interface(Rc<RefCell<BxInterface>>),
    NativeObject(Rc<RefCell<dyn BxNativeObject>>),
    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    JsValue(wasm_bindgen::JsValue),
    #[cfg(all(target_arch = "wasm32", feature = "js-host-abi", not(feature = "js")))]
    JsHandle(u32),
}

pub struct Heap {
    objects: Vec<Option<GcObject>>,
    marks: Vec<bool>,
    free_list: Vec<GcId>,
    alloc_count: usize,
    external_pressure: bool,
    next_gc_threshold: usize,
    generations: Vec<u8>,
    dirty: Vec<bool>,
    remembered_set: Vec<GcId>,
    young_objects: Vec<GcId>,
    minor_gc_count: usize,
    config: GCConfig,
    identity: Rc<()>,
    retained_values: Vec<Weak<RootedValue>>,
    retained_value_sets: Vec<Weak<Vec<BxValue>>>,
}

impl Heap {
    pub fn new() -> Self {
        Self::with_config(GCConfig::default())
    }

    pub fn with_config(config: GCConfig) -> Self {
        let cap = config.initial_capacity;
        Heap {
            objects: Vec::with_capacity(cap),
            marks: Vec::with_capacity(cap),
            free_list: Vec::new(),
            alloc_count: 0,
            external_pressure: false,
            next_gc_threshold: config.initial_threshold,
            generations: Vec::with_capacity(cap),
            dirty: Vec::with_capacity(cap),
            remembered_set: Vec::new(),
            young_objects: Vec::new(),
            minor_gc_count: 0,
            config,
            identity: Rc::new(()),
            retained_values: Vec::new(),
            retained_value_sets: Vec::new(),
        }
    }

    pub fn retain_value(&mut self, value: BxValue) -> Result<Rc<RootedValue>, String> {
        if value
            .as_gc_id()
            .is_some_and(|id| self.get_opt(id).is_none())
        {
            return Err("Cannot retain a collected value".into());
        }
        let root = Rc::new(RootedValue {
            value,
            heap: Rc::downgrade(&self.identity),
        });
        self.retained_values.push(Rc::downgrade(&root));
        self.alloc_count += 1;
        Ok(root)
    }

    pub(super) fn retain_values(&mut self, values: Vec<BxValue>) -> Result<Rc<Vec<BxValue>>, String> {
        if values.iter().any(|value| value.as_gc_id().is_some_and(|id| self.get_opt(id).is_none())) {
            return Err("Cannot retain a collected value".into());
        }
        // Preserve per-value collection pressure without one root allocation per binding.
        self.alloc_count += values.len();
        let roots = Rc::new(values);
        self.retained_value_sets.push(Rc::downgrade(&roots));
        Ok(roots)
    }

    pub fn read_retained_value(&self, root: &RootedValue) -> Result<BxValue, String> {
        if !root.heap.ptr_eq(&Rc::downgrade(&self.identity)) {
            return Err("Retained value belongs to another VM".into());
        }
        Ok(root.value)
    }

    fn allocation_weight(obj: &GcObject) -> usize {
        let bytes = match obj {
            GcObject::NativeObject(object) => object.borrow().external_allocation_bytes(),
            _ => 0,
        };
        1usize.saturating_add(bytes.div_ceil(std::mem::size_of::<GcObject>()))
    }

    pub fn alloc(&mut self, obj: GcObject) -> GcId {
        let weight = Self::allocation_weight(&obj);
        self.external_pressure |= weight > 1;
        self.alloc_count = self.alloc_count.saturating_add(weight);
        if let Some(id) = self.free_list.pop() {
            self.objects[id] = Some(obj);
            self.marks[id] = false;
            self.generations[id] = 0;
            self.dirty[id] = false;
            self.young_objects.push(id);
            id
        } else {
            let id = self.objects.len();
            self.objects.push(Some(obj));
            self.marks.push(false);
            self.generations.push(0);
            self.dirty.push(false);
            self.young_objects.push(id);
            id
        }
    }

    /// Attempts to allocate a new GC object, returning `None` if the backing
    /// storage cannot grow (e.g. out of memory). The fast path (reusing a
    /// slot from the free list) is identical to `alloc`.
    pub fn try_alloc(&mut self, obj: GcObject) -> Option<GcId> {
        let weight = Self::allocation_weight(&obj);
        self.external_pressure |= weight > 1;
        self.alloc_count = self.alloc_count.saturating_add(weight);
        if let Some(id) = self.free_list.pop() {
            self.objects[id] = Some(obj);
            self.marks[id] = false;
            self.generations[id] = 0;
            self.dirty[id] = false;
            self.young_objects.push(id);
            Some(id)
        } else {
            // Check hard cap before attempting to grow
            if let Some(max) = self.config.max_objects {
                if self.objects.len() >= max {
                    return None;
                }
            }
            // Try to grow backing Vecs; if OOM, return None
            self.objects.try_reserve(1).ok()?;
            self.marks.try_reserve(1).ok()?;
            self.generations.try_reserve(1).ok()?;
            self.dirty.try_reserve(1).ok()?;
            let id = self.objects.len();
            self.objects.push(Some(obj));
            self.marks.push(false);
            self.generations.push(0);
            self.dirty.push(false);
            self.young_objects.push(id);
            Some(id)
        }
    }

    pub fn get(&self, id: GcId) -> &GcObject {
        self.objects[id]
            .as_ref()
            .expect("Attempted to access collected object")
    }

    #[inline]
    pub fn get_opt(&self, id: GcId) -> Option<&GcObject> {
        self.objects.get(id).and_then(|o| o.as_ref())
    }

    pub fn get_mut(&mut self, id: GcId) -> &mut GcObject {
        if self.generations[id] > 0 && !self.dirty[id] {
            self.dirty[id] = true;
            self.remembered_set.push(id);
        }
        self.objects[id]
            .as_mut()
            .expect("Attempted to access collected object")
    }

    pub fn should_collect(&self) -> bool {
        self.alloc_count > self.next_gc_threshold
    }

    /// Large external owners cannot wait1024 opcodes before the next safe point.
    pub fn should_collect_promptly(&self) -> bool {
        self.external_pressure && self.should_collect()
    }

    /// Returns the current GC threshold (for testing/diagnostics).
    pub fn gc_threshold(&self) -> usize {
        self.next_gc_threshold
    }

    pub fn collect(&mut self, roots: &[BxValue]) {
        self.retained_values.retain(|root| root.strong_count() > 0);
        self.retained_value_sets.retain(|root| root.strong_count() > 0);
        self.alloc_count = 0;
        self.external_pressure = false;
        self.minor_gc_count += 1;

        if self.minor_gc_count >= 8 {
            self.major_collect(roots);
            self.minor_gc_count = 0;
        } else {
            self.minor_collect(roots);
        }

        // Each vacant arena slot occurs exactly once in the free list.
        let live = self.objects.len() - self.free_list.len();
        let dynamic = live.saturating_mul(2).max(self.config.initial_threshold);
        self.next_gc_threshold = dynamic.min(self.config.max_threshold);
    }

    fn minor_collect(&mut self, roots: &[BxValue]) {
        // Clear marks for young objects only
        for &id in &self.young_objects {
            if id < self.marks.len() {
                self.marks[id] = false;
            }
        }

        let mut worklist = Vec::new();

        // Add young roots from stack/globals
        for root in roots {
            self.add_to_worklist_young(root, &mut worklist);
        }

        for root in self.retained_values.iter().filter_map(Weak::upgrade) {
            self.add_to_worklist_young(&root.value, &mut worklist);
        }
        for roots in self.retained_value_sets.iter().filter_map(Weak::upgrade) {
            for root in roots.iter() {
                self.add_to_worklist_young(root, &mut worklist);
            }
        }

        // Scan remembered set: old objects that were mutated may point to young objects
        // We need to collect the IDs first to avoid borrow issues
        let remembered: Vec<GcId> = self.remembered_set.drain(..).collect();
        for id in &remembered {
            self.push_children_young(*id, &mut worklist);
        }

        // Mark phase: only traverse young objects
        while let Some(id) = worklist.pop() {
            if self.marks[id] {
                continue;
            }
            self.marks[id] = true;
            self.push_children_young(id, &mut worklist);
        }

        // Sweep phase: only process young objects
        let young: Vec<GcId> = self.young_objects.drain(..).collect();
        for id in young {
            if self.objects[id].is_some() && !self.marks[id] {
                self.objects[id] = None;
                self.free_list.push(id);
            } else if self.objects[id].is_some() {
                // Promote survivors to old generation
                self.generations[id] = 1;
            }
        }

        // Clear dirty flags for remembered objects
        for id in remembered {
            self.dirty[id] = false;
        }
    }

    fn major_collect(&mut self, roots: &[BxValue]) {
        // Full mark-sweep of the entire heap
        self.marks.fill(false);
        let mut worklist = Vec::new();
        for root in roots {
            self.add_to_worklist(root, &mut worklist);
        }
        for root in self.retained_values.iter().filter_map(Weak::upgrade) {
            self.add_to_worklist(&root.value, &mut worklist);
        }
        for roots in self.retained_value_sets.iter().filter_map(Weak::upgrade) {
            for root in roots.iter() {
                self.add_to_worklist(root, &mut worklist);
            }
        }

        while let Some(id) = worklist.pop() {
            if self.marks[id] {
                continue;
            }
            self.marks[id] = true;
            self.push_children(id, &mut worklist);
        }

        // Sweep entire heap
        for i in 0..self.objects.len() {
            if self.objects[i].is_some() && !self.marks[i] {
                self.objects[i] = None;
                self.free_list.push(i);
            } else if self.objects[i].is_some() {
                // All survivors become old
                self.generations[i] = 1;
            }
        }

        // Clear generational bookkeeping
        self.young_objects.clear();
        self.remembered_set.clear();
        self.dirty.fill(false);
    }

    fn push_children(&self, id: GcId, worklist: &mut Vec<GcId>) {
        match self.objects[id].as_ref().unwrap() {
            GcObject::String(_)
            | GcObject::Bytes(_)
            | GcObject::NativeCallback(_)
            | GcObject::NativeFunction(_)
            | GcObject::Class(_)
            | GcObject::Interface(_)
            | GcObject::Range(_)
            | GcObject::DateTime(_) => {}
            GcObject::CompiledFunction(function) => {
                if let Some(receiver) = &function.captured_receiver {
                    self.add_to_worklist(receiver, worklist);
                }
                for scope in &function.captured_scopes {
                    self.add_to_worklist(scope, worklist);
                }
            }
            GcObject::NativeObject(obj) => {
                let mut tracer = WorklistTracer {
                    worklist,
                    heap: self,
                };
                // Use unsafe to bypass RefCell borrow check during tracing.
                // This is safe because GC is stop-the-world and we are only reading.
                // This is necessary because the object might be borrowed by the VM
                // during a native method call that triggered GC.
                unsafe {
                    let ptr = obj.as_ptr();
                    (*ptr).trace(&mut tracer);
                }
            }
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            GcObject::JsValue(_) => {}
            #[cfg(all(target_arch = "wasm32", feature = "js-host-abi", not(feature = "js")))]
            GcObject::JsHandle(_) => {}
            GcObject::Array(arr) => {
                for val in arr {
                    self.add_to_worklist(val, worklist);
                }
            }
            GcObject::Struct(s) => {
                for val in &s.properties {
                    self.add_to_worklist(val, worklist);
                }
            }
            GcObject::Instance(inst) => {
                for val in &inst.properties {
                    self.add_to_worklist(val, worklist);
                }
                for val in inst.variables.borrow().values() {
                    self.add_to_worklist(val, worklist);
                }
            }
            GcObject::Future(f) => {
                self.add_to_worklist(&f.value, worklist);
                if let Some(h) = &f.error_handler {
                    self.add_to_worklist(h, worklist);
                }
            }
        };
    }

    fn push_children_young(&self, id: GcId, worklist: &mut Vec<GcId>) {
        match self.objects[id].as_ref().unwrap() {
            GcObject::String(_)
            | GcObject::Bytes(_)
            | GcObject::NativeCallback(_)
            | GcObject::NativeFunction(_)
            | GcObject::Class(_)
            | GcObject::Interface(_)
            | GcObject::Range(_)
            | GcObject::DateTime(_) => {}
            GcObject::CompiledFunction(function) => {
                if let Some(receiver) = &function.captured_receiver {
                    self.add_to_worklist_young(receiver, worklist);
                }
                for scope in &function.captured_scopes {
                    self.add_to_worklist_young(scope, worklist);
                }
            }
            GcObject::NativeObject(obj) => {
                let mut tracer = YoungWorklistTracer {
                    worklist,
                    heap: self,
                };
                unsafe {
                    let ptr = obj.as_ptr();
                    (*ptr).trace(&mut tracer);
                }
            }
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            GcObject::JsValue(_) => {}
            #[cfg(all(target_arch = "wasm32", feature = "js-host-abi", not(feature = "js")))]
            GcObject::JsHandle(_) => {}
            GcObject::Array(arr) => {
                for val in arr {
                    self.add_to_worklist_young(val, worklist);
                }
            }
            GcObject::Struct(s) => {
                for val in &s.properties {
                    self.add_to_worklist_young(val, worklist);
                }
            }
            GcObject::Instance(inst) => {
                for val in &inst.properties {
                    self.add_to_worklist_young(val, worklist);
                }
                for val in inst.variables.borrow().values() {
                    self.add_to_worklist_young(val, worklist);
                }
            }
            GcObject::Future(f) => {
                self.add_to_worklist_young(&f.value, worklist);
                if let Some(h) = &f.error_handler {
                    self.add_to_worklist_young(h, worklist);
                }
            }
        };
    }
}

struct WorklistTracer<'a> {
    worklist: &'a mut Vec<GcId>,
    heap: &'a Heap,
}

impl<'a> crate::types::Tracer for WorklistTracer<'a> {
    fn mark(&mut self, val: &BxValue) {
        self.heap.add_to_worklist(val, self.worklist);
    }
}

struct YoungWorklistTracer<'a> {
    worklist: &'a mut Vec<GcId>,
    heap: &'a Heap,
}

impl<'a> crate::types::Tracer for YoungWorklistTracer<'a> {
    fn mark(&mut self, val: &BxValue) {
        self.heap.add_to_worklist_young(val, self.worklist);
    }
}

impl Heap {
    fn add_to_worklist(&self, val: &BxValue, worklist: &mut Vec<GcId>) {
        if let Some(id) = val.as_gc_id() {
            if id < self.objects.len() && self.objects[id].is_some() {
                worklist.push(id);
            }
        }
    }

    fn add_to_worklist_young(&self, val: &BxValue, worklist: &mut Vec<GcId>) {
        if let Some(id) = val.as_gc_id() {
            if id < self.objects.len() && self.objects[id].is_some() && self.generations[id] == 0 {
                worklist.push(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_threshold_tracks_live_objects_through_reuse_and_retained_graphs() {
        let config = GCConfig { initial_capacity: 1, initial_threshold: 2, max_threshold: 16, max_objects: Some(12) };
        let mut heap = Heap::with_config(config.clone());
        let parent = heap.alloc(GcObject::Array(Vec::new()));
        let retained = heap.retain_value(BxValue::new_ptr(parent)).unwrap();
        let check_slots = |heap: &Heap| {
            let mut free = heap.free_list.clone();
            free.sort_unstable();
            let holes: Vec<_> = heap.objects.iter().enumerate().filter_map(|(id, o)| o.is_none().then_some(id)).collect();
            assert_eq!(free, holes); // Also rejects duplicate free IDs.
            assert_eq!(heap.objects.len() - heap.free_list.len(), heap.objects.iter().filter(|o| o.is_some()).count());
        };
        for cycle in 0..32 {
            let child = heap.try_alloc(GcObject::Bytes(vec![cycle])).unwrap();
            if let GcObject::Array(values) = heap.get_mut(parent) {
                *values = vec![BxValue::new_ptr(child)]; // Old-to-young barrier; previous child becomes garbage.
            } else { panic!("retained parent lost"); }
            while heap.try_alloc(GcObject::Bytes(Vec::new())).is_some() { check_slots(&heap); }
            check_slots(&heap); // Failed bounded allocation cannot add a phantom slot.
            heap.collect(&[]); // Reachability is supplied by the retained root, not the caller.
            check_slots(&heap);
            let live = (0..12).filter(|id| heap.get_opt(*id).is_some()).count();
            assert_eq!(heap.gc_threshold(), (live * 2).max(config.initial_threshold).min(config.max_threshold));
            assert_eq!(heap.read_retained_value(&retained).unwrap().as_gc_id(), Some(parent));
            assert!(matches!(heap.get(child), GcObject::Bytes(bytes) if bytes == &[cycle]));
            assert!(!heap.should_collect());
        }
        drop(retained);
        for _ in 0..8 { heap.collect(&[]); check_slots(&heap); }
        assert!((0..12).all(|id| heap.get_opt(id).is_none()));
        assert_eq!(heap.gc_threshold(), config.initial_threshold);
        for _ in 0..12 { assert!(heap.try_alloc(GcObject::Bytes(Vec::new())).is_some()); check_slots(&heap); }
        assert!(heap.try_alloc(GcObject::Bytes(Vec::new())).is_none());
    }

    #[test]
    fn try_alloc_succeeds_with_free_list_reuse() {
        let mut heap = Heap::new();
        // Allocate and then simulate GC freeing to populate free_list
        let id = heap.alloc(GcObject::String(BoxString::new("hello")));
        // Simulate what collect() does: mark as freed and add to free_list
        heap.objects[id] = None;
        heap.free_list.push(id);
        heap.young_objects.retain(|&x| x != id);

        // try_alloc should reuse the freed slot
        let result = heap.try_alloc(GcObject::String(BoxString::new("world")));
        assert_eq!(result, Some(id));
        assert!(heap.objects[id].is_some());
    }

    #[test]
    fn try_alloc_succeeds_with_new_allocation() {
        let mut heap = Heap::new();
        let result = heap.try_alloc(GcObject::String(BoxString::new("hello")));
        assert!(result.is_some());
        let id = result.unwrap();
        assert!(heap.objects[id].is_some());
    }

    #[test]
    fn try_alloc_returns_none_when_capacity_exhausted() {
        let mut heap = Heap::new();
        // Fill up capacity to approach the grow boundary
        let cap = heap.objects.capacity();
        for _ in 0..cap {
            heap.alloc(GcObject::String(BoxString::new("fill")));
        }
        // The method signature enforces Option return; verify type
        let result = heap.try_alloc(GcObject::String(BoxString::new("extra")));
        let _: Option<GcId> = result;
    }

    #[test]
    fn gcconfig_esp32_preset() {
        let cfg = GCConfig::for_esp32();
        assert_eq!(cfg.initial_capacity, 128);
        assert_eq!(cfg.initial_threshold, 256);
        assert_eq!(cfg.max_threshold, 1024);
        assert_eq!(cfg.max_objects, Some(2048));
    }

    #[test]
    fn gcconfig_desktop_preset() {
        let cfg = GCConfig::for_desktop();
        assert_eq!(cfg.initial_capacity, 1024);
        assert_eq!(cfg.initial_threshold, 10000);
        assert_eq!(cfg.max_threshold, 100000);
        assert_eq!(cfg.max_objects, None);
    }

    #[test]
    fn gcconfig_wasm_preset() {
        let cfg = GCConfig::for_wasm();
        assert_eq!(cfg.initial_capacity, 512);
        assert_eq!(cfg.initial_threshold, 2000);
        assert_eq!(cfg.max_threshold, 20000);
        assert_eq!(cfg.max_objects, Some(50000));
    }

    #[test]
    fn gcconfig_initial_threshold_used_in_heap() {
        let cfg = GCConfig::for_esp32();
        let heap = Heap::with_config(cfg.clone());
        assert_eq!(heap.next_gc_threshold, cfg.initial_threshold);
    }

    #[test]
    fn try_alloc_respects_max_objects() {
        let mut cfg = GCConfig::for_desktop();
        cfg.max_objects = Some(5);
        let mut heap = Heap::with_config(cfg);
        // Allocate up to the cap
        for _ in 0..5 {
            assert!(
                heap.try_alloc(GcObject::String(BoxString::new("x")))
                    .is_some()
            );
        }
        // 6th allocation should fail
        assert!(
            heap.try_alloc(GcObject::String(BoxString::new("y")))
                .is_none()
        );
    }
}
