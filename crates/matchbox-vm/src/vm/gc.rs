use crate::types::{BxValue, BxStruct, BxInstance, BxFuture, BxCompiledFunction, BxClass, BxInterface, BxNativeFunction, BxNativeObject, box_string::BoxString};
use std::rc::Rc;
use std::cell::RefCell;

pub type GcId = usize;

#[derive(Debug, Clone)]
pub enum GcObject {
    String(BoxString),
    Array(Vec<BxValue>),
    Struct(BxStruct),
    Instance(BxInstance),
    Future(BxFuture),
    CompiledFunction(Rc<BxCompiledFunction>),
    NativeFunction(BxNativeFunction),
    Class(Rc<RefCell<BxClass>>),
    Interface(Rc<RefCell<BxInterface>>),
    NativeObject(Rc<RefCell<dyn BxNativeObject>>),
    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    JsValue(wasm_bindgen::JsValue),
    #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
    JsHandle(u32),
}

pub struct Heap {
    objects: Vec<Option<GcObject>>,
    marks: Vec<bool>,
    free_list: Vec<GcId>,
    alloc_count: usize,
    next_gc_threshold: usize,
    generations: Vec<u8>,
    dirty: Vec<bool>,
    remembered_set: Vec<GcId>,
    young_objects: Vec<GcId>,
    minor_gc_count: usize,
}

impl Heap {
    pub fn new() -> Self {
        Heap {
            objects: Vec::with_capacity(1024),
            marks: Vec::with_capacity(1024),
            free_list: Vec::new(),
            alloc_count: 0,
            next_gc_threshold: 1000,
            generations: Vec::with_capacity(1024),
            dirty: Vec::with_capacity(1024),
            remembered_set: Vec::new(),
            young_objects: Vec::new(),
            minor_gc_count: 0,
        }
    }

    pub fn alloc(&mut self, obj: GcObject) -> GcId {
        self.alloc_count += 1;
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

    pub fn get(&self, id: GcId) -> &GcObject {
        self.objects[id].as_ref().expect("Attempted to access collected object")
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
        self.objects[id].as_mut().expect("Attempted to access collected object")
    }

    pub fn should_collect(&self) -> bool {
        self.alloc_count > self.next_gc_threshold
    }

    pub fn collect(&mut self, roots: &[BxValue]) {
        self.alloc_count = 0;
        self.minor_gc_count += 1;

        if self.minor_gc_count >= 8 {
            self.major_collect(roots);
            self.minor_gc_count = 0;
        } else {
            self.minor_collect(roots);
        }

        let live = self.objects.iter().filter(|o| o.is_some()).count();
        self.next_gc_threshold = live.saturating_mul(2).max(1000);
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

        // Scan remembered set: old objects that were mutated may point to young objects
        // We need to collect the IDs first to avoid borrow issues
        let remembered: Vec<GcId> = self.remembered_set.drain(..).collect();
        for id in &remembered {
            self.push_children_young(*id, &mut worklist);
        }

        // Mark phase: only traverse young objects
        while let Some(id) = worklist.pop() {
            if self.marks[id] { continue; }
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

        while let Some(id) = worklist.pop() {
            if self.marks[id] { continue; }
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
            GcObject::String(_) | GcObject::NativeFunction(_) | GcObject::Class(_) | GcObject::Interface(_) | GcObject::CompiledFunction(_) => {}
            GcObject::NativeObject(obj) => {
                let mut tracer = WorklistTracer { worklist, heap: self };
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
            #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
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
            GcObject::String(_) | GcObject::NativeFunction(_) | GcObject::Class(_) | GcObject::Interface(_) | GcObject::CompiledFunction(_) => {}
            GcObject::NativeObject(obj) => {
                let mut tracer = YoungWorklistTracer { worklist, heap: self };
                unsafe {
                    let ptr = obj.as_ptr();
                    (*ptr).trace(&mut tracer);
                }
            }
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            GcObject::JsValue(_) => {}
            #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
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
            if id < self.objects.len()
                && self.objects[id].is_some()
                && self.generations[id] == 0
            {
                worklist.push(id);
            }
        }
    }
}
