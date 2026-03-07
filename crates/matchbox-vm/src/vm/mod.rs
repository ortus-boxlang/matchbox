pub mod chunk;
pub mod opcode;
pub mod gc;
pub mod shape;

use crate::types::{BxValue, BxCompiledFunction, BxClass, BxInstance, BxFuture, FutureStatus, Constant, BxVM, BxStruct, BxNativeObject, BxNativeFunction, box_string::BoxString};
use self::chunk::{Chunk, IcEntry};
use self::opcode::OpCode;
use self::gc::{Heap, GcObject};
use self::shape::ShapeRegistry;
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::{Instant, Duration};
use std::vec;

#[cfg(all(target_arch = "wasm32", feature = "js"))]
use wasm_bindgen::prelude::*;
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use js_sys::{Array, Function, Reflect};

pub struct CallFrame {
    pub function: Rc<BxCompiledFunction>,
    pub ip: usize,
    pub stack_base: usize,
    pub receiver: Option<BxValue>,
    pub handlers: Vec<usize>,
}

pub struct BxFiber {
    pub stack: Vec<BxValue>,
    pub frames: Vec<CallFrame>,
    pub future_id: usize,
    pub wait_until: Option<Instant>,
    pub yield_requested: bool,
}

pub struct VM {
    pub fibers: Vec<BxFiber>,
    pub global_names: HashMap<String, usize>,
    pub global_values: Vec<BxValue>,
    pub current_fiber_idx: Option<usize>,
    pub shapes: ShapeRegistry,
    pub heap: Heap,
}

impl BxVM for VM {
    fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>) -> BxValue {
        self.spawn(func, args)
    }

    fn spawn_by_value(&mut self, func: &BxValue, args: Vec<BxValue>) -> Result<BxValue, String> {
        if let Some(id) = func.as_gc_id() {
            let obj = self.heap.get(id);
            if let GcObject::CompiledFunction(f) = obj {
                let f = Rc::clone(f);
                Ok(self.spawn(f, args))
            } else {
                Err("Value is not a callable function".to_string())
            }
        } else {
            Err("Value is not a callable function".to_string())
        }
    }

    fn call_function_by_value(&mut self, func: &BxValue, args: Vec<BxValue>) -> Result<BxValue, String> {
        self.call_function_value(*func, args).map_err(|e| e.to_string())
    }

    fn yield_fiber(&mut self) {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].yield_requested = true;
        }
    }

    fn sleep(&mut self, ms: u64) {
        if let Some(idx) = self.current_fiber_idx {
            let until = Instant::now() + Duration::from_millis(ms);
            self.fibers[idx].wait_until = Some(until);
            self.fibers[idx].yield_requested = true;
        }
    }

    fn get_root_shape(&self) -> u32 {
        self.shapes.get_root()
    }

    fn get_shape_index(&self, shape_id: u32, field_name: &str) -> Option<u32> {
        self.shapes.get_index(shape_id, field_name)
    }

    fn array_len(&self, id: usize) -> usize {
        if let GcObject::Array(arr) = self.heap.get(id) {
            arr.len()
        } else { 0 }
    }

    fn array_push(&mut self, id: usize, val: BxValue) {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            arr.push(val);
        }
    }

    fn array_get(&self, id: usize, idx: usize) -> BxValue {
        if let GcObject::Array(arr) = self.heap.get(id) {
            arr.get(idx).copied().unwrap_or(BxValue::new_null())
        } else { BxValue::new_null() }
    }

    fn array_new(&mut self) -> usize {
        self.heap.alloc(GcObject::Array(Vec::new()))
    }

    fn struct_len(&self, id: usize) -> usize {
        if let GcObject::Struct(s) = self.heap.get(id) {
            s.properties.len()
        } else { 0 }
    }

    fn struct_new(&mut self) -> usize {
        self.heap.alloc(GcObject::Struct(BxStruct {
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
        }))
    }

    fn struct_get_shape(&self, id: usize) -> u32 {
        if let GcObject::Struct(s) = self.heap.get(id) {
            s.shape_id
        } else { 0 }
    }

    fn future_on_error(&mut self, id: usize, handler: BxValue) {
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            f.error_handler = Some(handler);
        }
    }

    fn native_object_new(&mut self, obj: Rc<RefCell<dyn BxNativeObject>>) -> usize {
        self.heap.alloc(GcObject::NativeObject(obj))
    }

    fn string_new(&mut self, s: String) -> usize {
        self.heap.alloc(GcObject::String(BoxString::new(&s)))
    }

    fn to_string(&self, val: BxValue) -> String {
        self.to_string_internal(val)
    }

    fn to_box_string(&self, val: BxValue) -> BoxString {
        if let Some(id) = val.as_gc_id() {
            if let GcObject::String(s) = self.heap.get(id) {
                return s.clone();
            }
        }
        BoxString::new(&self.to_string_internal(val))
    }
}

impl VM {
    fn to_string_internal(&self, val: BxValue) -> String {
        if val.is_number() {
            val.as_number().to_string()
        } else if val.is_int() {
            val.as_int().to_string()
        } else if val.is_bool() {
            val.as_bool().to_string()
        } else if val.is_null() {
            "null".to_string()
        } else if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => s.to_string(),
                GcObject::Array(_) => format!("<array id:{}>", id),
                GcObject::Struct(_) => format!("<struct id:{}>", id),
                GcObject::Instance(inst) => format!("<instance of {}>", inst.class.borrow().name),
                GcObject::Future(_) => format!("<future id:{}>", id),
                GcObject::CompiledFunction(f) => format!("<function {}>", f.name),
                GcObject::NativeFunction(_) => "<native function>".to_string(),
                GcObject::Class(c) => format!("<class {}>", c.borrow().name),
                GcObject::Interface(i) => format!("<interface {}>", i.borrow().name),
                GcObject::NativeObject(o) => format!("<native object {:?}>", o.borrow()),
                #[cfg(all(target_arch = "wasm32", feature = "js"))]
                GcObject::JsValue(js) => format!("<js value {:?}>", js),
            }
        } else {
            "<invalid>".to_string()
        }
    }

    fn is_equal(&self, a: BxValue, b: BxValue) -> bool {
        if a == b { return true; }
        if let (Some(id_a), Some(id_b)) = (a.as_gc_id(), b.as_gc_id()) {
            match (self.heap.get(id_a), self.heap.get(id_b)) {
                (GcObject::String(s1), GcObject::String(s2)) => s1 == s2,
                _ => false,
            }
        } else {
            false
        }
    }

    pub fn new() -> Self {
        Self::new_with_bifs(HashMap::new())
    }

    pub fn new_with_bifs(external_bifs: HashMap<String, BxNativeFunction>) -> Self {
        let mut vm = VM {
            fibers: Vec::new(),
            global_names: HashMap::new(),
            global_values: Vec::new(),
            current_fiber_idx: None,
            shapes: ShapeRegistry::new(),
            heap: Heap::new(),
        };

        #[cfg(all(target_arch = "wasm32", feature = "js"))]
        {
            if let Some(window) = web_sys::window() {
                let id = vm.heap.alloc(GcObject::JsValue(window.into()));
                vm.insert_global("js".to_string(), BxValue::new_ptr(id));
            }
        }

        // Register standard BIFs
        for (name, func) in crate::bifs::register_all() {
            let id = vm.heap.alloc(GcObject::NativeFunction(func));
            vm.insert_global(name, BxValue::new_ptr(id));
        }

        // Register external/plugin BIFs
        for (name, func) in external_bifs {
            let id = vm.heap.alloc(GcObject::NativeFunction(func));
            vm.insert_global(name, BxValue::new_ptr(id));
        }

        vm
    }

    pub fn insert_global(&mut self, name: String, val: BxValue) {
        let name_lower = name.to_lowercase();
        if let Some(&idx) = self.global_names.get(&name_lower) {
            self.global_values[idx] = val;
        } else {
            let idx = self.global_values.len();
            self.global_names.insert(name_lower, idx);
            self.global_values.push(val);
        }
    }

    pub fn get_global(&self, name: &str) -> Option<BxValue> {
        let name_lower = name.to_lowercase();
        self.global_names.get(&name_lower).map(|&idx| self.global_values[idx])
    }


    fn resolve_member_method(&self, receiver: &BxValue, method_name: &str) -> Option<String> {
        let name = method_name.to_lowercase();
        if receiver.is_number() {
            return match name.as_str() {
                "abs" => Some("abs".to_string()),
                "round" => Some("round".to_string()),
                _ => None,
            };
        }

        if let Some(id) = receiver.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(_) => match name.as_str() {
                    "len" | "length" => Some("len".to_string()),
                    "ucase" | "touppercase" => Some("ucase".to_string()),
                    "lcase" | "tolowercase" => Some("lcase".to_string()),
                    _ => None,
                },
                GcObject::Array(_) => match name.as_str() {
                    "len" | "length" | "count" => Some("len".to_string()),
                    "append" | "add" => Some("arrayappend".to_string()),
                    "each" => Some("arrayeach".to_string()),
                    "map" => Some("arraymap".to_string()),
                    "reduce" => Some("arrayreduce".to_string()),
                    "filter" => Some("arrayfilter".to_string()),
                    "tolist" => Some("arraytolist".to_string()),
                    _ => None,
                },
                GcObject::Struct(_) => match name.as_str() {
                    "len" | "count" => Some("len".to_string()),
                    "exists" | "keyexists" => Some("structkeyexists".to_string()),
                    "each" => Some("structeach".to_string()),
                    _ => None,
                },
                GcObject::Future(_) => match name.as_str() {
                    "onerror" => Some("futureonerror".to_string()),
                    _ => None,
                },
                _ => None,
            }
        } else {
            None
        }
    }

    fn resolve_method(&self, class: Rc<RefCell<BxClass>>, method_name: &str) -> Option<Rc<BxCompiledFunction>> {
        let mut current_class = class;
        loop {
            let class_ref = current_class.borrow();
            if let Some(method) = class_ref.methods.get(method_name) {
                return Some(Rc::clone(method));
            }
            
            if let Some(parent_name) = &class_ref.extends {
                if let Some(val) = self.get_global(parent_name) {
                    if let Some(id) = val.as_gc_id() {
                        if let GcObject::Class(parent_class) = self.heap.get(id) {
                            let next_class = Rc::clone(parent_class);
                            drop(class_ref); // release borrow
                            current_class = next_class;
                            continue;
                        }
                    }
                }
            }
            return None;
        }
    }

    pub fn interpret(&mut self, mut chunk: Chunk) -> Result<BxValue> {
        chunk.ensure_caches();
        let constant_count = chunk.constants.len();
        let function = Rc::new(BxCompiledFunction {
            name: "script".to_string(),
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            chunk: Rc::new(RefCell::new(chunk)),
            promoted_constants: RefCell::new(vec![None; constant_count]),
        });
        
        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
        }));

        let fiber = BxFiber {
            stack: Vec::with_capacity(256),
            frames: vec![CallFrame {
                function,
                ip: 0,
                stack_base: 0,
                receiver: None,
                handlers: Vec::new(),
            }],
            future_id,
            wait_until: None,
            yield_requested: false,
        };
        
        self.fibers.push(fiber);
        let res = self.run_all();
        self.current_fiber_idx = None;
        res
    }

    pub fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>) -> BxValue {
        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
        }));

        let mut stack = Vec::with_capacity(256);
        for arg in args {
            stack.push(arg);
        }

        let fiber = BxFiber {
            stack,
            frames: vec![CallFrame {
                function: func,
                ip: 0,
                stack_base: 0,
                receiver: None,
                handlers: Vec::new(),
            }],
            future_id,
            wait_until: None,
            yield_requested: false,
        };

        self.fibers.push(fiber);
        BxValue::new_ptr(future_id)
    }

    fn run_all(&mut self) -> Result<BxValue> {
        let mut last_result = BxValue::new_null();
        
        while !self.fibers.is_empty() {
            let mut i = 0;
            let mut all_waiting = true;
            while i < self.fibers.len() {
                let now = Instant::now();
                if let Some(until) = self.fibers[i].wait_until {
                    if now < until {
                        i += 1;
                        continue;
                    } else {
                        self.fibers[i].wait_until = None;
                    }
                }
                
                all_waiting = false;
                self.current_fiber_idx = Some(i);
                match self.run_fiber(i, 100) {
                    Ok(Some(result)) => {
                        let fiber = self.fibers.remove(i);
                        if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                            f.value = result;
                            f.status = FutureStatus::Completed;
                        }
                        last_result = result;
                        continue;
                    }
                    Ok(None) => {
                        i += 1;
                    }
                    Err(e) => {
                        let fiber = self.fibers.remove(i);
                        let mut handler = None;
                        if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                            f.status = FutureStatus::Failed(e.to_string());
                            handler = f.error_handler;
                        }
                        
                        if let Some(h) = handler {
                            self.spawn_error_handler(h, e.to_string());
                            continue;
                        }

                        if self.fibers.is_empty() {
                            return Err(e);
                        } else {
                            eprintln!("\n[Async Task Error] {}", e);
                        }
                    }
                }
                self.current_fiber_idx = None;
            }
            
            if all_waiting && !self.fibers.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }

            // Periodically collect garbage
            if self.heap.should_collect() {
                self.collect_garbage();
            }
        }
        
        Ok(last_result)
    }

    fn run_fiber(&mut self, fiber_idx: usize, quantum: usize) -> Result<Option<BxValue>> {
        for _ in 0..quantum {
            if fiber_idx >= self.fibers.len() {
                return Ok(None);
            }
            if self.fibers[fiber_idx].frames.is_empty() {
                return Ok(Some(BxValue::new_null()));
            }
            if self.fibers[fiber_idx].yield_requested {
                self.fibers[fiber_idx].yield_requested = false;
                return Ok(None);
            }

            let (instruction, ip_at_start) = {
                let fiber = &self.fibers[fiber_idx];
                let frame = fiber.frames.last().unwrap();
                let chunk = frame.function.chunk.borrow();
                if frame.ip >= chunk.code.len() {
                    return Ok(Some(BxValue::new_null()));
                }
                (chunk.code[frame.ip].clone(), frame.ip)
            };
            
            self.fibers[fiber_idx].frames.last_mut().unwrap().ip += 1;

            match instruction {
                // --- Hot Loop / Specialized Opcodes ---
                OpCode::OpIncLocal(slot) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let val = self.fibers[fiber_idx].stack[base + slot as usize];
                    if val.is_number() {
                        self.fibers[fiber_idx].stack[base + slot as usize] = BxValue::new_number(val.as_number() + 1.0);
                    } else if val.is_int() {
                        self.fibers[fiber_idx].stack[base + slot as usize] = BxValue::new_int(val.as_int() + 1);
                    } else {
                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                        continue;
                    }
                }
                OpCode::OpLocalCompareJump(slot, const_idx, offset) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let val = self.fibers[fiber_idx].stack[base + slot as usize];
                    let limit = self.read_constant(fiber_idx, const_idx as usize);
                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= offset as usize;
                        }
                    } else if val.is_int() && limit.is_int() {
                        if val.as_int() < limit.as_int() {
                            self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= offset as usize;
                        }
                    }
                }
                OpCode::OpCompareJump(const_idx, offset) => {
                    let limit = self.read_constant(fiber_idx, const_idx as usize);
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    
                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= offset as usize;
                        }
                    } else {
                        self.throw_error(fiber_idx, "OpCompareJump expects numeric operands")?;
                        continue;
                    }
                }
                OpCode::OpIncGlobal(idx) => {
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.function.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    if let Some(IcEntry::Global { index }) = ic {
                        let val = self.global_values[index];
                        if val.is_number() {
                            self.global_values[index] = BxValue::new_number(val.as_number() + 1.0);
                        } else {
                            self.throw_error(fiber_idx, "Operand of increment must be a number")?;
                            continue;
                        }
                    } else {
                        // Slow path: resolve global and update IC
                        let name = self.read_string_constant(fiber_idx, idx as usize);
                        let name_lower = name.to_lowercase();
                        if let Some(&global_idx) = self.global_names.get(&name_lower) {
                            let val = self.global_values[global_idx];
                            if val.is_number() {
                                self.global_values[global_idx] = BxValue::new_number(val.as_number() + 1.0);
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.function.chunk.borrow_mut();
                                chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                            } else {
                                self.throw_error(fiber_idx, "Operand of increment must be a number")?;
                                continue;
                            }
                        } else {
                            self.throw_error(fiber_idx, &format!("Global {} not found", name))?;
                            continue;
                        }
                    }
                }
                OpCode::OpGlobalCompareJump(name_idx, const_idx, offset) => {
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.function.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    let val = if let Some(IcEntry::Global { index }) = ic {
                        self.global_values[index]
                    } else {
                        let name = self.read_string_constant(fiber_idx, name_idx as usize);
                        let name_lower = name.to_lowercase();
                        if let Some(&global_idx) = self.global_names.get(&name_lower) {
                            let v = self.global_values[global_idx];
                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.function.chunk.borrow_mut();
                            chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                            v
                        } else {
                            BxValue::new_null()
                        }
                    };

                    let limit = self.read_constant(fiber_idx, const_idx as usize);
                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= offset as usize;
                        }
                    }
                }

                // --- Basic Hot Opcodes ---
                OpCode::OpGetLocal(slot) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let val = self.fibers[fiber_idx].stack[base + slot as usize];
                    self.fibers[fiber_idx].stack.push(val);
                }
                OpCode::OpSetLocal(slot) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    self.fibers[fiber_idx].stack[base + slot as usize] = val;
                }
                OpCode::OpSetLocalPop(slot) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack[base + slot as usize] = val;
                }
                OpCode::OpConstant(idx) => {
                    let constant = self.read_constant(fiber_idx, idx as usize);
                    self.fibers[fiber_idx].stack.push(constant);
                }
                OpCode::OpAddInt => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_int(a.as_number() as i32 + b.as_number() as i32));
                }
                OpCode::OpAddFloat => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() + b.as_number()));
                }
                OpCode::OpAdd => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() + b.as_number()));
                    } else {
                        let a_s = self.to_box_string(a);
                        let b_s = self.to_box_string(b);
                        let res_id = self.heap.alloc(GcObject::String(a_s.concat(&b_s)));
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(res_id));
                    }
                }
                OpCode::OpSubtract => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() - b.as_number()));
                    } else {
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        continue;
                    }
                }
                OpCode::OpSubInt => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_int(a.as_number() as i32 - b.as_number() as i32));
                }
                OpCode::OpSubFloat => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() - b.as_number()));
                }
                OpCode::OpMultiply => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() * b.as_number()));
                    } else {
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        continue;
                    }
                }
                OpCode::OpMulInt => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_int(a.as_number() as i32 * b.as_number() as i32));
                }
                OpCode::OpMulFloat => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() * b.as_number()));
                }
                OpCode::OpDivide => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        let b_n = b.as_number();
                        if b_n == 0.0 { self.throw_error(fiber_idx, "Division by zero")?; continue; }
                        else { self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() / b_n)); }
                    } else {
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        continue;
                    }
                }
                OpCode::OpDivFloat => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() / b.as_number()));
                }
                OpCode::OpPop => {
                    self.fibers[fiber_idx].stack.pop();
                }
                OpCode::OpJumpIfFalse(offset) => {
                    if !self.is_truthy(*self.fibers[fiber_idx].stack.last().unwrap()) {
                        self.fibers[fiber_idx].frames.last_mut().unwrap().ip += offset as usize;
                    }
                }
                OpCode::OpJump(offset) => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().ip += offset as usize;
                }
                OpCode::OpLoop(offset) => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= offset as usize;
                }
                OpCode::OpReturn => {
                    let fiber = &mut self.fibers[fiber_idx];
                    let frame = fiber.frames.pop().unwrap();
                    let result = if fiber.stack.len() > frame.stack_base {
                        fiber.stack.pop().unwrap()
                    } else {
                        BxValue::new_null()
                    };
                    
                    if fiber.frames.is_empty() {
                        return Ok(Some(result));
                    }
                    
                    fiber.stack.truncate(frame.stack_base);
                    
                    if frame.function.name.ends_with(".constructor") {
                        let instance = fiber.stack.pop().unwrap();
                        fiber.stack.push(instance);
                    } else {
                        // For regular function calls, the function itself was at stack_base - 1
                        if frame.stack_base > 0 {
                            fiber.stack.pop();
                        }
                        fiber.stack.push(result);
                    }
                }

                // --- Global / Scope Opcodes ---
                OpCode::OpGetGlobal(idx) => {
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.function.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    if let Some(IcEntry::Global { index }) = ic {
                        let val = self.global_values[index];
                        self.fibers[fiber_idx].stack.push(val);
                    } else {
                        let name = self.read_string_constant(fiber_idx, idx as usize);
                        let name_lower = name.to_lowercase();
                        if let Some(&global_idx) = self.global_names.get(&name_lower) {
                            let val = self.global_values[global_idx];
                            self.fibers[fiber_idx].stack.push(val);
                            
                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.function.chunk.borrow_mut();
                            chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                        } else {
                            self.fibers[fiber_idx].stack.push(BxValue::new_null()); 
                        }
                    }
                }
                OpCode::OpSetGlobal(idx) => {
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.function.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    let val = *self.fibers[fiber_idx].stack.last().unwrap();

                    if let Some(IcEntry::Global { index }) = ic {
                        self.global_values[index] = val;
                    } else {
                        let name = self.read_string_constant(fiber_idx, idx as usize);
                        let name_lower = name.to_lowercase();
                        if let Some(&global_idx) = self.global_names.get(&name_lower) {
                            self.global_values[global_idx] = val;
                            
                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.function.chunk.borrow_mut();
                            chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                        } else {
                            self.insert_global(name.clone(), val);
                            if let Some(&global_idx) = self.global_names.get(&name_lower) {
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.function.chunk.borrow_mut();
                                chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                            }
                        }
                    }
                }
                OpCode::OpSetGlobalPop(idx) => {
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.function.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    let val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(IcEntry::Global { index }) = ic {
                        self.global_values[index] = val;
                    } else {
                        let name = self.read_string_constant(fiber_idx, idx as usize);
                        let name_lower = name.to_lowercase();
                        if let Some(&global_idx) = self.global_names.get(&name_lower) {
                            self.global_values[global_idx] = val;
                            
                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.function.chunk.borrow_mut();
                            chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                        } else {
                            self.insert_global(name.clone(), val);
                            if let Some(&global_idx) = self.global_names.get(&name_lower) {
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.function.chunk.borrow_mut();
                                chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                            }
                        }
                    }
                }
                OpCode::OpDefineGlobal(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx as usize);
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.insert_global(name, val);
                }
                OpCode::OpGetPrivate(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx as usize).to_lowercase();
                    let val = if let Some(receiver) = self.fibers[fiber_idx].frames.last().unwrap().receiver {
                        if let Some(id) = receiver.as_gc_id() {
                            if name == "this" {
                                Some(receiver)
                            } else if name == "variables" {
                                if let GcObject::Instance(inst) = self.heap.get(id) {
                                    let _vars = Rc::clone(&inst.variables);
                                    // Should we return the actual variables scope as a struct/native object?
                                    // For now just return a virtual struct that points to it.
                                    Some(BxValue::new_ptr(self.heap.alloc(GcObject::Struct(BxStruct {
                                        shape_id: self.shapes.get_root(),
                                        properties: Vec::new(),
                                    }))))
                                } else { None }
                            } else {
                                if let GcObject::Instance(inst) = self.heap.get(id) {
                                    let val = inst.variables.borrow().get(&name).copied().unwrap_or(BxValue::new_null());
                                    Some(val)
                                } else { None }
                            }
                        } else { None }
                    } else {
                        None
                    };

                    if let Some(v) = val {
                        self.fibers[fiber_idx].stack.push(v);
                    } else {
                        self.throw_error(fiber_idx, &format!("'variables' scope only available in classes. Tried to access '{}'", name))?;
                        continue;
                    }
                }
                OpCode::OpSetPrivate(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx as usize).to_lowercase();
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    if let Some(receiver) = self.fibers[fiber_idx].frames.last().unwrap().receiver {
                        if let Some(id) = receiver.as_gc_id() {
                            if let GcObject::Instance(inst) = self.heap.get_mut(id) {
                                inst.variables.borrow_mut().insert(name, val);
                            }
                        }
                    } else {
                        self.throw_error(fiber_idx, "'variables' scope only available in classes.")?;
                        continue;
                    }
                }

                // --- Stack Manipulation ---
                OpCode::OpDup => {
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    self.fibers[fiber_idx].stack.push(val);
                }
                OpCode::OpSwap => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(b);
                    self.fibers[fiber_idx].stack.push(a);
                }
                OpCode::OpOver => {
                    let val = self.fibers[fiber_idx].stack[self.fibers[fiber_idx].stack.len() - 2];
                    self.fibers[fiber_idx].stack.push(val);
                }
                OpCode::OpInc => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if val.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(val.as_number() + 1.0));
                    } else if val.is_int() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_int(val.as_int() + 1));
                    } else {
                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                        continue;
                    }
                }
                OpCode::OpDec => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if val.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(val.as_number() - 1.0));
                    } else if val.is_int() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_int(val.as_int() - 1));
                    } else {
                        self.throw_error(fiber_idx, "Decrement operand must be a number")?;
                        continue;
                    }
                }

                // --- Data Structures ---
                OpCode::OpArray(count) => {
                    let mut items = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        items.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    items.reverse();
                    let id = self.heap.alloc(GcObject::Array(items));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                }
                OpCode::OpStruct(count) => {
                    let mut shape_id = self.shapes.get_root();
                    let mut props = Vec::with_capacity(count as usize);
                    
                    let mut kv_pairs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        let value = self.fibers[fiber_idx].stack.pop().unwrap();
                        let key_val = self.fibers[fiber_idx].stack.pop().unwrap();
                        let key = self.to_string(key_val).to_lowercase();
                        kv_pairs.push((key, value));
                    }
                    kv_pairs.reverse(); 

                    for (key, value) in kv_pairs {
                        shape_id = self.shapes.transition(shape_id, &key);
                        props.push(value);
                    }

                    let id = self.heap.alloc(GcObject::Struct(BxStruct { shape_id, properties: props }));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                }
                OpCode::OpIndex => {
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let Some(id) = base_val.as_gc_id() {
                        match self.heap.get(id) {
                            GcObject::Array(arr) => {
                                if index_val.is_number() || index_val.is_int() {
                                    let idx = if index_val.is_int() { index_val.as_int() as usize } else { index_val.as_number() as usize };
                                    if idx < 1 || idx > arr.len() {
                                        self.throw_error(fiber_idx, &format!("Array index out of bounds: {}", idx))?;
                                        continue;
                                    } else {
                                        self.fibers[fiber_idx].stack.push(arr[idx - 1]);
                                    }
                                } else {
                                    self.throw_error(fiber_idx, "Array index must be a number")?;
                                    continue;
                                }
                            }
                            GcObject::Struct(s) => {
                                let key = self.to_string(index_val).to_lowercase();
                                if let Some(idx) = self.shapes.get_index(s.shape_id, &key) {
                                    self.fibers[fiber_idx].stack.push(s.properties[idx as usize]);
                                } else {
                                    self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                }
                            }
                            _ => { self.throw_error(fiber_idx, "Invalid access: base must be array or struct")?; continue; }
                        }
                    } else {
                        self.throw_error(fiber_idx, "Invalid access: base must be array or struct")?;
                        continue;
                    }
                }
                OpCode::OpSetIndex => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    
                    if let Some(id) = base_val.as_gc_id() {
                        let key = if !index_val.is_number() && !index_val.is_int() {
                            Some(self.to_string(index_val).to_lowercase())
                        } else {
                            None
                        };

                        match self.heap.get_mut(id) {
                            GcObject::Array(arr) => {
                                if index_val.is_number() || index_val.is_int() {
                                    let idx = if index_val.is_int() { index_val.as_int() as usize } else { index_val.as_number() as usize };
                                    if idx < 1 || idx > arr.len() {
                                        self.throw_error(fiber_idx, &format!("Array index out of bounds: {}", idx))?;
                                        continue;
                                    } else {
                                        arr[idx - 1] = val;
                                        self.fibers[fiber_idx].stack.push(val);
                                    }
                                } else {
                                    self.throw_error(fiber_idx, "Array index must be a number")?;
                                    continue;
                                }
                            }
                            GcObject::Struct(s) => {
                                let key = key.unwrap();
                                if let Some(idx) = self.shapes.get_index(s.shape_id, &key) {
                                    s.properties[idx as usize] = val;
                                } else {
                                    s.shape_id = self.shapes.transition(s.shape_id, &key);
                                    s.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::Instance(inst) => {
                                let key = key.unwrap();
                                if let Some(idx) = self.shapes.get_index(inst.shape_id, &key) {
                                    inst.properties[idx as usize] = val;
                                } else {
                                    inst.shape_id = self.shapes.transition(inst.shape_id, &key);
                                    inst.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => { self.throw_error(fiber_idx, "Invalid indexed assignment")?; continue; }
                        }
                    } else {
                        self.throw_error(fiber_idx, "Invalid indexed assignment")?;
                        continue;
                    }
                }
                OpCode::OpMember(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx as usize).to_lowercase();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    
                    if let Some(id) = base_val.as_gc_id() {
                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        if let GcObject::JsValue(js) = self.heap.get(id) {
                            let js = js.clone();
                            let prop = JsValue::from_str(&name);
                            match Reflect::get(&js, &prop) {
                                Ok(val) => {
                                    let bx_val = self.js_to_bx(val);
                                    self.fibers[fiber_idx].stack.push(bx_val);
                                }
                                Err(_) => self.fibers[fiber_idx].stack.push(BxValue::new_null()),
                            }
                            continue;
                        }

                        match self.heap.get(id) {
                            GcObject::Struct(s) => {
                                let shape_id = s.shape_id;
                                let properties_ptr = &s.properties as *const Vec<BxValue>;

                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.function.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                    if cached_shape == shape_id as usize {
                                        let val = unsafe { &*properties_ptr }[index as usize];
                                        self.fibers[fiber_idx].stack.push(val);
                                        continue;
                                    }
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.function.chunk.borrow_mut();
                                        chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                    }
                                    let val = unsafe { &*properties_ptr }[idx as usize];
                                    self.fibers[fiber_idx].stack.push(val);
                                } else {
                                    self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                }
                            }
                            GcObject::Instance(inst) => {
                                let shape_id = inst.shape_id;
                                let properties_ptr = &inst.properties as *const Vec<BxValue>;
                                let class = Rc::clone(&inst.class);

                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.function.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                    if cached_shape == shape_id as usize {
                                        let val = unsafe { &*properties_ptr }[index as usize];
                                        self.fibers[fiber_idx].stack.push(val);
                                        continue;
                                    }
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.function.chunk.borrow_mut();
                                        chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                    }
                                    let val = unsafe { &*properties_ptr }[idx as usize];
                                    self.fibers[fiber_idx].stack.push(val);
                                } else if let Some(method) = self.resolve_method(Rc::clone(&class), &name) {
                                    let m_id = self.heap.alloc(GcObject::CompiledFunction(method));
                                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(m_id));
                                } else {
                                    self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                }
                            }
                            GcObject::NativeObject(obj) => {
                                let val = obj.borrow().get_property(&name);
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => { self.throw_error(fiber_idx, "Member access only supported on structs, instances, JS objects, and native objects")?; continue; }
                        }
                    } else {
                        self.throw_error(fiber_idx, "Member access only supported on structs, instances, JS objects, and native objects")?;
                        continue;
                    }
                }
                OpCode::OpSetMember(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx as usize).to_lowercase();
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    
                    if let Some(id) = base_val.as_gc_id() {
                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        if let GcObject::JsValue(js) = self.heap.get(id) {
                            let js = js.clone();
                            let prop = JsValue::from_str(&name);
                            let js_val = self.bx_to_js(&val);
                            Reflect::set(&js, &prop, &js_val).ok();
                            self.fibers[fiber_idx].stack.push(val);
                            continue;
                        }

                        match self.heap.get_mut(id) {
                            GcObject::Struct(s) => {
                                let shape_id = s.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.function.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                    if cached_shape == shape_id as usize {
                                        s.properties[index as usize] = val;
                                        self.fibers[fiber_idx].stack.push(val);
                                        continue;
                                    }
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.function.chunk.borrow_mut();
                                        chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                    }
                                    s.properties[idx as usize] = val;
                                } else {
                                    s.shape_id = self.shapes.transition(shape_id, &name);
                                    s.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::Instance(inst) => {
                                let shape_id = inst.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.function.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                    if cached_shape == shape_id as usize {
                                        inst.properties[index as usize] = val;
                                        self.fibers[fiber_idx].stack.push(val);
                                        continue;
                                    }
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.function.chunk.borrow_mut();
                                        chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                    }
                                    inst.properties[idx as usize] = val;
                                } else {
                                    inst.shape_id = self.shapes.transition(shape_id, &name);
                                    inst.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::NativeObject(obj) => {
                                obj.borrow_mut().set_property(&name, val);
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => { self.throw_error(fiber_idx, "Member assignment only supported on structs, instances, JS objects, and native objects")?; continue; }
                        }
                    } else {
                        self.throw_error(fiber_idx, "Member assignment only supported on structs, instances, JS objects, and native objects")?;
                        continue;
                    }
                }
                OpCode::OpIncMember(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx as usize).to_lowercase();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(id) = base_val.as_gc_id() {
                        match self.heap.get_mut(id) {
                            GcObject::Struct(s) => {
                                let shape_id = s.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.function.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                let index = if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                    if cached_shape == shape_id as usize { Some(index as usize) } else { None }
                                } else { None };

                                if let Some(idx) = index.or_else(|| self.shapes.get_index(shape_id, &name).map(|i| i as usize)) {
                                    let old_val = s.properties[idx];
                                    if old_val.is_number() {
                                        let new_val = BxValue::new_number(old_val.as_number() + 1.0);
                                        s.properties[idx] = new_val;
                                        self.fibers[fiber_idx].stack.push(new_val);
                                        
                                        if index.is_none() {
                                            let fiber = &self.fibers[fiber_idx];
                                            let frame = fiber.frames.last().unwrap();
                                            let mut chunk = frame.function.chunk.borrow_mut();
                                            chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                        }
                                    } else {
                                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                                        continue;
                                    }
                                } else {
                                    self.throw_error(fiber_idx, &format!("Member {} not found", name))?;
                                    continue;
                                }
                            }
                            GcObject::Instance(inst) => {
                                let shape_id = inst.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.function.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                let index = if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                    if cached_shape == shape_id as usize { Some(index as usize) } else { None }
                                } else { None };

                                if let Some(idx) = index.or_else(|| self.shapes.get_index(shape_id, &name).map(|i| i as usize)) {
                                    let old_val = inst.properties[idx];
                                    if old_val.is_number() {
                                        let new_val = BxValue::new_number(old_val.as_number() + 1.0);
                                        inst.properties[idx] = new_val;
                                        self.fibers[fiber_idx].stack.push(new_val);
                                        
                                        if index.is_none() {
                                            let fiber = &self.fibers[fiber_idx];
                                            let frame = fiber.frames.last().unwrap();
                                            let mut chunk = frame.function.chunk.borrow_mut();
                                            chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                        }
                                    } else {
                                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                                        continue;
                                    }
                                } else {
                                    self.throw_error(fiber_idx, &format!("Member {} not found", name))?;
                                    continue;
                                }
                            }
                            _ => { 
                                self.throw_error(fiber_idx, "Fused increment only supported on structs and instances for now")?; 
                                continue; 
                            }
                        }
                    } else {
                        self.throw_error(fiber_idx, "Member access only supported on objects")?;
                        continue;
                    }
                }
                OpCode::OpStringConcat => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a_s = self.to_box_string(a);
                    let b_s = self.to_box_string(b);
                    let res_id = self.heap.alloc(GcObject::String(a_s.concat(&b_s)));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(res_id));
                }

                // --- Calls / Invocations ---
                OpCode::OpCall(arg_count) => {
                    self.execute_call(fiber_idx, arg_count as usize, None)?;
                }
                OpCode::OpCallNamed(total_count, names_idx) => {
                    let names = match self.read_constant(fiber_idx, names_idx as usize) {
                        v if v.is_ptr() => {
                            if let GcObject::Array(arr) = self.heap.get(v.as_gc_id().unwrap()) {
                                arr.iter().map(|v| self.to_string(*v)).collect::<Vec<_>>()
                            } else {
                                bail!("Internal VM error: names constant is not a StringArray")
                            }
                        }
                        _ => bail!("Internal VM error: names constant is not a StringArray"),
                    };
                    self.execute_call(fiber_idx, total_count as usize, Some(names))?;
                }
                OpCode::OpInvoke(idx, arg_count) => {
                    let name = self.read_string_constant(fiber_idx, idx as usize).to_lowercase();
                    self.execute_invoke(fiber_idx, name, arg_count as usize, None, ip_at_start)?;
                }
                OpCode::OpInvokeNamed(name_idx, total_count, names_idx) => {
                    let name = self.read_string_constant(fiber_idx, name_idx as usize).to_lowercase();
                    let names = match self.read_constant(fiber_idx, names_idx as usize) {
                        v if v.is_ptr() => {
                            if let GcObject::Array(arr) = self.heap.get(v.as_gc_id().unwrap()) {
                                arr.iter().map(|v| self.to_string(*v)).collect::<Vec<_>>()
                            } else {
                                bail!("Internal VM error: names constant is not a StringArray")
                            }
                        }
                        _ => bail!("Internal VM error: names constant is not a StringArray"),
                    };
                    self.execute_invoke(fiber_idx, name, total_count as usize, Some(names), ip_at_start)?;
                }
                OpCode::OpNew(arg_count) => {
                    let class_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count as usize;
                    let class_val = self.fibers[fiber_idx].stack[class_idx];
                    if let Some(id) = class_val.as_gc_id() {
                        let class = if let GcObject::Class(c) = self.heap.get(id) {
                            Some(Rc::clone(c))
                        } else { None };

                        if let Some(class) = class {
                            let variables_scope = Rc::new(RefCell::new(HashMap::new()));
                            
                            let inst_id = self.heap.alloc(GcObject::Instance(BxInstance {
                                class: Rc::clone(&class),
                                shape_id: self.shapes.get_root(),
                                properties: Vec::new(),
                                variables: variables_scope.clone(),
                            }));
                            
                            let instance_val = BxValue::new_ptr(inst_id);
                            self.fibers[fiber_idx].stack[class_idx] = instance_val;

                            let frame = CallFrame {
                                function: Rc::clone(&class.borrow().constructor),
                                ip: 0,
                                stack_base: class_idx + 1 + arg_count as usize,
                                receiver: Some(instance_val),
                                handlers: Vec::new(),
                            };
                            self.fibers[fiber_idx].frames.push(frame);
                        } else {
                            self.throw_error(fiber_idx, "Can only instantiate classes.")?;
                            continue;
                        }
                    } else {
                        self.throw_error(fiber_idx, "Can only instantiate classes.")?;
                        continue;
                    }
                }

                // --- Comparison ---
                OpCode::OpEqual => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let res = self.is_equal(a, b);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(res));
                }
                OpCode::OpNotEqual => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let res = self.is_equal(a, b);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(!res));
                }
                OpCode::OpLess => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(a.as_number() < b.as_number()));
                    } else {
                        self.throw_error(fiber_idx, "Comparison only supported for numbers currently")?;
                        continue;
                    }
                }
                OpCode::OpLessEqual => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(a.as_number() <= b.as_number()));
                    } else {
                        self.throw_error(fiber_idx, "Comparison only supported for numbers currently")?;
                        continue;
                    }
                }
                OpCode::OpGreater => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(a.as_number() > b.as_number()));
                    } else {
                        self.throw_error(fiber_idx, "Comparison only supported for numbers currently")?;
                        continue;
                    }
                }
                OpCode::OpGreaterEqual => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(a.as_number() >= b.as_number()));
                    } else {
                        self.throw_error(fiber_idx, "Comparison only supported for numbers currently")?;
                        continue;
                    }
                }

                // --- Control Flow / Misc ---
                OpCode::OpIterNext(collection_slot, cursor_slot, offset, push_index) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let collection_idx = base + collection_slot as usize;
                    let cursor_idx = base + cursor_slot as usize;
                    
                    let (is_done, next_val, next_idx) = {
                        let cursor_val = if self.fibers[fiber_idx].stack[cursor_idx].is_number() {
                            self.fibers[fiber_idx].stack[cursor_idx].as_number() as usize
                        } else if self.fibers[fiber_idx].stack[cursor_idx].is_int() {
                            self.fibers[fiber_idx].stack[cursor_idx].as_int() as usize
                        } else {
                            bail!("Internal VM error: iterator cursor is not a number")
                        };
                        
                        let collection = self.fibers[fiber_idx].stack[collection_idx];
                        if let Some(id) = collection.as_gc_id() {
                            match self.heap.get(id) {
                                GcObject::Array(arr) => {
                                    if cursor_val < arr.len() {
                                        (false, Some(arr[cursor_val]), Some(BxValue::new_number(cursor_val as f64 + 1.0)))
                                    } else {
                                        (true, None, None)
                                    }
                                }
                                GcObject::Struct(s) => {
                                    let keys = {
                                        let mut k = Vec::new();
                                        let shape = &self.shapes.shapes[s.shape_id as usize];
                                        for key in shape.fields.keys() {
                                            k.push(key.clone());
                                        }
                                        k.sort();
                                        k
                                    };
                                    if cursor_val < keys.len() {
                                        let key = &keys[cursor_val];
                                        let idx = self.shapes.get_index(s.shape_id, key).unwrap();
                                        let val = s.properties[idx as usize];
                                        let key_id = self.heap.alloc(GcObject::String(BoxString::new(key)));                                        (false, Some(BxValue::new_ptr(key_id)), Some(val))
                                    } else {
                                        (true, None, None)
                                    }
                                }
                                _ => { 
                                    self.throw_error(fiber_idx, "Iteration only supported for arrays and structs")?;
                                    (true, None, None)
                                }
                            }
                        } else {
                            self.throw_error(fiber_idx, "Iteration only supported for arrays and structs")?;
                            (true, None, None)
                        }
                    };

                    if is_done {
                        self.fibers[fiber_idx].frames.last_mut().unwrap().ip += offset as usize;
                    } else {
                        let current_cursor = self.fibers[fiber_idx].stack[cursor_idx];
                        let next_cursor_val = if current_cursor.is_int() { BxValue::new_int(current_cursor.as_int() + 1) } else { BxValue::new_number(current_cursor.as_number() + 1.0) };
                        self.fibers[fiber_idx].stack[cursor_idx] = next_cursor_val;
                        self.fibers[fiber_idx].stack.push(next_val.unwrap());
                        if push_index {
                            self.fibers[fiber_idx].stack.push(next_idx.unwrap());
                        }
                    }
                }
                OpCode::OpLocalJumpIfNeConst(slot, const_idx, offset) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let val = self.fibers[fiber_idx].stack[base + slot as usize];
                    let constant = self.read_constant(fiber_idx, const_idx as usize);
                    if val != constant {
                        self.fibers[fiber_idx].frames.last_mut().unwrap().ip += offset as usize;
                    }
                }
                OpCode::OpPushHandler(offset) => {
                    let target_ip = self.fibers[fiber_idx].frames.last().unwrap().ip + offset as usize;
                    self.fibers[fiber_idx].frames.last_mut().unwrap().handlers.push(target_ip);
                }
                OpCode::OpPopHandler => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().handlers.pop();
                }
                OpCode::OpThrow => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.throw_value(fiber_idx, val)?;
                }
                OpCode::OpPrint(count) => {
                    let mut args = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    let out = args.iter().map(|a| self.to_string(*a)).collect::<Vec<_>>().join(" ");
                    print!("{}", out);
                }
                OpCode::OpPrintln(count) => {
                    let mut args = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    let out = args.iter().map(|a| self.to_string(*a)).collect::<Vec<_>>().join(" ");
                    println!("{}", out);
                }
            }
        }
        Ok(None)
    }

    fn throw_error(&mut self, fiber_idx: usize, msg: &str) -> Result<()> {
        let msg_id = self.heap.alloc(GcObject::String(BoxString::new(msg)));
        let val = BxValue::new_ptr(msg_id);
        self.throw_value(fiber_idx, val)
    }

    fn throw_value(&mut self, fiber_idx: usize, val: BxValue) -> Result<()> {
        let mut line = 0;
        let mut filename = "unknown".to_string();
        let mut source_snippet = String::new();

        if !self.fibers[fiber_idx].frames.is_empty() {
            let frame = self.fibers[fiber_idx].frames.last().unwrap();
            let chunk = frame.function.chunk.borrow();
            filename = chunk.filename.clone();
            if frame.ip > 0 && frame.ip <= chunk.lines.len() {
                line = chunk.lines[frame.ip - 1];
                
                // Extract source snippet
                if !chunk.source.is_empty() && line > 0 {
                    let lines: Vec<&str> = chunk.source.lines().collect();
                    if line as usize <= lines.len() {
                        let code_line = lines[line as usize - 1].trim();
                        source_snippet = format!("\n\n  |  {}\n  |  {}", line, code_line);
                    }
                }
            }
        }

        while !self.fibers[fiber_idx].frames.is_empty() {
            let frame_idx = self.fibers[fiber_idx].frames.len() - 1;
            if !self.fibers[fiber_idx].frames[frame_idx].handlers.is_empty() {
                let handler_ip = self.fibers[fiber_idx].frames[frame_idx].handlers.pop().unwrap();
                let stack_base = self.fibers[fiber_idx].frames[frame_idx].stack_base;
                self.fibers[fiber_idx].frames[frame_idx].ip = handler_ip;
                
                self.fibers[fiber_idx].stack.truncate(stack_base);
                self.fibers[fiber_idx].stack.push(val);
                return Ok(());
            }
            self.fibers[fiber_idx].frames.pop();
        }
        let val_str = self.to_string(val);
        bail!("VM Runtime Error: {}{}\n(at {} line {})", val_str, source_snippet, filename, line);
    }

    pub fn call_function(&mut self, name: &str, args: Vec<BxValue>) -> Result<BxValue> {
        if let Some(f) = self.get_global(name) {
            return self.call_function_value(f, args);
        }
        anyhow::bail!("Function {} not found", name)
    }

    pub fn call_function_value(&mut self, func: BxValue, args: Vec<BxValue>) -> Result<BxValue> {
        if let Some(id) = func.as_gc_id() {
            match self.heap.get(id) {
                GcObject::CompiledFunction(f) => {
                    let f = Rc::clone(f);
                    if args.len() < f.min_arity as usize || args.len() > f.arity as usize {
                        anyhow::bail!("Expected {}-{} arguments but got {}", f.min_arity, f.arity, args.len());
                    }
                    
                    let future_id = self.heap.alloc(GcObject::Future(BxFuture {
                        value: BxValue::new_null(),
                        status: FutureStatus::Pending,
                        error_handler: None,
                    }));

                    let mut stack = Vec::with_capacity(f.arity as usize + 1);
                    stack.push(func); // function itself at base
                    for arg in args {
                        stack.push(arg);
                    }
                    while stack.len() < (f.arity + 1) as usize {
                        stack.push(BxValue::new_null());
                    }

                    let fiber = BxFiber {
                        stack,
                        frames: vec![CallFrame {
                            function: f,
                            ip: 0,
                            stack_base: 1,
                            receiver: None,
                            handlers: Vec::new(),
                        }],
                        future_id,
                        wait_until: None,
                        yield_requested: false,
                    };
                    self.fibers.push(fiber);
                    let fiber_idx = self.fibers.len() - 1;
                    self.current_fiber_idx = Some(fiber_idx);
                    let res = self.run_fiber(fiber_idx, 1000000);
                    self.current_fiber_idx = None;
                    
                    match res? {
                        Some(val) => Ok(val),
                        None => Ok(BxValue::new_null()),
                    }
                }
                GcObject::NativeFunction(func) => {
                    let func = *func;
                    func(self, &args).map_err(|e| anyhow::anyhow!(e))
                }
                _ => anyhow::bail!("Value is not a callable function"),
            }
        } else {
            anyhow::bail!("Value is not a callable function")
        }
    }

    fn is_truthy(&self, val: BxValue) -> bool {
        if val.is_bool() {
            val.as_bool()
        } else if val.is_number() {
            val.as_number() != 0.0
        } else if val.is_int() {
            val.as_int() != 0
        } else if val.is_null() {
            false
        } else if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => !s.is_empty() && s.to_string().to_lowercase() != "false",
                _ => true,
            }
        } else {
            false
        }
    }

    fn reorder_arguments(&self, args: Vec<BxValue>, names: Vec<String>, params: &[String]) -> Vec<BxValue> {
        let mut final_args = vec![BxValue::new_null(); params.len()];
        let mut positional_args = Vec::new();
        let mut named_args = Vec::new();

        for (i, arg_val) in args.into_iter().enumerate() {
            if i < names.len() && !names[i].is_empty() {
                named_args.push((names[i].to_lowercase(), arg_val));
            } else {
                positional_args.push(arg_val);
            }
        }

        // 1. Fill positional args
        for (i, arg_val) in positional_args.into_iter().enumerate() {
            if i < final_args.len() {
                final_args[i] = arg_val;
            }
        }

        // 2. Fill named args
        for (name, arg_val) in named_args {
            if let Some(param_idx) = params.iter().position(|p| p.to_lowercase() == name) {
                final_args[param_idx] = arg_val;
            }
        }
        final_args
    }

    fn spawn_error_handler(&mut self, handler: BxValue, error_msg: String) {
        let err_id = self.heap.alloc(GcObject::String(BoxString::new(&error_msg)));
        let err_val = BxValue::new_ptr(err_id);
        
        if let Some(id) = handler.as_gc_id() {
            match self.heap.get(id) {
                GcObject::CompiledFunction(f) => {
                    self.spawn(Rc::clone(f), vec![err_val]);
                }
                GcObject::NativeFunction(f) => {
                    let f = *f;
                    let _ = f(self, &[err_val]);
                }
                _ => {}
            }
        }
    }

    fn execute_call(&mut self, fiber_idx: usize, arg_count: usize, names: Option<Vec<String>>) -> Result<()> {
        let func_val = self.fibers[fiber_idx].stack[self.fibers[fiber_idx].stack.len() - 1 - arg_count];
        
        if let Some(id) = func_val.as_gc_id() {
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            if let GcObject::JsValue(js) = self.heap.get(id) {
                let js = js.clone();
                if let Ok(func) = js.clone().dyn_into::<Function>() {
                    let js_args = Array::new();
                    let mut args = Vec::new();
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    for arg in args {
                        js_args.push(&self.bx_to_js(&arg));
                    }
                    self.fibers[fiber_idx].stack.pop(); // Pop the function
                    match Reflect::apply(&func, &JsValue::UNDEFINED, &js_args) {
                        Ok(val) => {
                            let bx_val = self.js_to_bx(val);
                            self.fibers[fiber_idx].stack.push(bx_val);
                            return Ok(());
                        }
                        Err(e) => return self.throw_error(fiber_idx, &format!("JS Error: {:?}", e)),
                    }
                } else {
                    return self.throw_error(fiber_idx, "Can only call JS functions.");
                }
            }

            match self.heap.get(id) {
                GcObject::CompiledFunction(func) => {
                    let func = Rc::clone(func);
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    // Don't pop function yet, it's used as marker

                    let final_args = if let Some(names_list) = names {
                        self.reorder_arguments(args, names_list, &func.params)
                    } else {
                        let mut a = args;
                        for _ in 0..(func.arity as usize - arg_count) {
                            a.push(BxValue::new_null());
                        }
                        a
                    };

                    // Stack: ... [func] [arg1] [arg2] ...
                    // Function is already at len() - 1 - arg_count.
                    // We popped args, now we push final_args back.
                    for arg in final_args {
                        self.fibers[fiber_idx].stack.push(arg);
                    }

                    let mut frame = CallFrame {
                        function: Rc::clone(&func),
                        ip: 0,
                        stack_base: 0,
                        receiver: self.fibers[fiber_idx].frames.last().unwrap().receiver,
                        handlers: Vec::new(),
                    };
                    // Let's be consistent: stack_base is where first arg is. Function is at stack_base - 1.
                    frame.stack_base = self.fibers[fiber_idx].stack.len() - func.arity as usize;
                    self.fibers[fiber_idx].frames.push(frame);
                    Ok(())
                }
                GcObject::NativeFunction(func) => {
                    let func = *func;
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    self.fibers[fiber_idx].stack.pop(); // Pop the function object
                    
                    match func(self, &args) {
                        Ok(val) => {
                            self.fibers[fiber_idx].stack.push(val);
                            Ok(())
                        }
                        Err(e) => self.throw_error(fiber_idx, &e),
                    }
                }
                _ => self.throw_error(fiber_idx, "Can only call functions."),
            }
        } else {
            self.throw_error(fiber_idx, "Can only call functions.")
        }
    }

    fn execute_invoke(&mut self, fiber_idx: usize, name: String, arg_count: usize, names: Option<Vec<String>>, ip_at_start: usize) -> Result<()> {
        let receiver_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count as usize;
        let receiver_val = self.fibers[fiber_idx].stack[receiver_idx];
        
        if let Some(id) = receiver_val.as_gc_id() {
            #[cfg(all(target_arch = "wasm32", feature = "js"))]
            if let GcObject::JsValue(js) = self.heap.get(id) {
                let js = js.clone();
                let prop = JsValue::from_str(&name);
                match Reflect::get(&js, &prop) {
                    Ok(val) => {
                        if let Ok(func) = val.clone().dyn_into::<Function>() {
                            let js_args = Array::new();
                            let mut args = Vec::new();
                            for _ in 0..arg_count {
                                args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                            }
                            args.reverse();
                            for arg in args {
                                js_args.push(&self.bx_to_js(&arg));
                            }
                            self.fibers[fiber_idx].stack.pop(); // Pop the receiver
                            match Reflect::apply(&func, &js, &js_args) {
                                Ok(val) => {
                                    let bx_val = self.js_to_bx(val);
                                    self.fibers[fiber_idx].stack.push(bx_val);
                                    return Ok(());
                                }
                                Err(e) => return self.throw_error(fiber_idx, &format!("JS Error: {:?}", e)),
                            }
                        }
                    }
                    Err(_) => {}
                }
            }

            match self.heap.get(id) {
                GcObject::Future(f) => {
                    let (status, value) = (f.status.clone(), f.value);

                    if name == "get" {
                        match status {
                            FutureStatus::Pending => {
                                self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= 1;
                                self.fibers[fiber_idx].yield_requested = true;
                                return Ok(());
                            }
                            FutureStatus::Completed => {
                                for _ in 0..arg_count { self.fibers[fiber_idx].stack.pop(); }
                                self.fibers[fiber_idx].stack.pop();
                                self.fibers[fiber_idx].stack.push(value);
                                return Ok(());
                            }
                            FutureStatus::Failed(e) => {
                                return self.throw_error(fiber_idx, &format!("Async operation failed: {}", e));
                            }
                        }
                    } else if let Some(bif_name) = self.resolve_member_method(&receiver_val, &name) {
                        return self.execute_bif_call(fiber_idx, bif_name, arg_count, receiver_val);
                    }
                }
                GcObject::NativeObject(obj) => {
                    let obj = Rc::clone(obj);
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    self.fibers[fiber_idx].stack.pop(); // receiver

                    let mut obj_borrow = obj.borrow_mut();
                    match obj_borrow.call_method(self, &name, &args) {
                        Ok(res) => {
                            self.fibers[fiber_idx].stack.push(res);
                            return Ok(());
                        }
                        Err(e) => {
                            drop(obj_borrow);
                            return self.throw_error(fiber_idx, &e);
                        }
                    }
                }
                GcObject::Instance(inst) => {
                    let shape_id = inst.shape_id;
                    let class = Rc::clone(&inst.class);

                    let ic = {
                        let fiber = &self.fibers[fiber_idx];
                        let frame = fiber.frames.last().unwrap();
                        let chunk = frame.function.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    let method = if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                        if cached_shape == shape_id as usize {
                            let method_val = inst.properties[index as usize];
                            if let Some(m_id) = method_val.as_gc_id() {
                                if let GcObject::CompiledFunction(f) = self.heap.get(m_id) {
                                    Some(Rc::clone(f))
                                } else { None }
                            } else { None }
                        } else { None }
                    } else { None };

                    let method = if method.is_none() {
                        if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                            let method_val = inst.properties[idx as usize];
                            if let Some(m_id) = method_val.as_gc_id() {
                                if let GcObject::CompiledFunction(f) = self.heap.get(m_id) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.function.chunk.borrow_mut();
                                        chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                    }
                                    Some(Rc::clone(f))
                                } else { None }
                            } else { None }
                        } else if let Some(f) = self.resolve_method(Rc::clone(&class), &name) {
                            Some(f)
                        } else { None }
                    } else { method };
                    
                    if let Some(func) = method {
                        let mut args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        args.reverse();
                        // Pop receiver, but we'll push it back as the first element of stack for the frame
                        self.fibers[fiber_idx].stack.pop(); 

                        let final_args = if let Some(names_list) = names {
                            self.reorder_arguments(args, names_list, &func.params)
                        } else {
                            let mut a = args;
                            for _ in 0..(func.arity as usize - arg_count) {
                                a.push(BxValue::new_null());
                            }
                            a
                        };
                        
                        // Receiver should be available to the frame. In Matchbox, we often put it in CallFrame.receiver.
                        // But local variables slot 0 might also be receiver in some conventions.
                        // Let's stick to CallFrame.receiver and push arguments.
                        self.fibers[fiber_idx].stack.push(receiver_val);

                        for arg in final_args { self.fibers[fiber_idx].stack.push(arg); }

                        let frame = CallFrame {
                            function: func.clone(),
                            ip: 0,
                            stack_base: self.fibers[fiber_idx].stack.len() - func.arity as usize,
                            receiver: Some(receiver_val),
                            handlers: Vec::new(),
                        };
                        self.fibers[fiber_idx].frames.push(frame);
                        return Ok(());
                    } else if let Some(on_missing) = self.resolve_method(Rc::clone(&class), "onmissingmethod") {
                        let mut original_args = Vec::with_capacity(arg_count);
                        for _ in 0..arg_count {
                            original_args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        original_args.reverse();
                        self.fibers[fiber_idx].stack.pop(); // receiver
                        let args_array_id = self.heap.alloc(GcObject::Array(original_args));
                        let name_id = self.heap.alloc(GcObject::String(BoxString::new(&name)));

                        self.fibers[fiber_idx].stack.push(receiver_val); // receiver at base
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(name_id));
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(args_array_id));

                        let mut frame = CallFrame {
                            function: on_missing.clone(),
                            ip: 0,
                            stack_base: self.fibers[fiber_idx].stack.len() - 2,
                            receiver: Some(receiver_val),
                            handlers: Vec::new(),
                        };
                        
                        for _ in 0..(on_missing.arity - 2) {
                            self.fibers[fiber_idx].stack.push(BxValue::new_null());
                        }
                        frame.stack_base = self.fibers[fiber_idx].stack.len() - on_missing.arity as usize;

                        self.fibers[fiber_idx].frames.push(frame);
                        return Ok(());
                    }
                }
                _ => {}
            }
        }

        // handle primitives and fallback BIFs
        if let Some(bif_name) = self.resolve_member_method(&receiver_val, &name) {
            return self.execute_bif_call(fiber_idx, bif_name, arg_count, receiver_val);
        }

        self.throw_error(fiber_idx, &format!("Method {} not found on {}.", name, receiver_val))
    }

    fn execute_bif_call(&mut self, fiber_idx: usize, bif_name: String, arg_count: usize, receiver: BxValue) -> Result<()> {
        if let Some(bif_val) = self.get_global(&bif_name) {
            if let Some(bif_id) = bif_val.as_gc_id() {
                if let GcObject::NativeFunction(bif) = self.heap.get(bif_id) {
                    let bif = *bif;
                    let mut args = Vec::with_capacity(arg_count + 1);
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    self.fibers[fiber_idx].stack.pop(); // receiver
                    
                    let mut final_args = vec![receiver];
                    final_args.extend(args);
                    
                    match bif(self, &final_args) {
                        Ok(res) => {
                            self.fibers[fiber_idx].stack.push(res);
                            return Ok(());
                        }
                        Err(e) => return self.throw_error(fiber_idx, &e),
                    }
                }
            }
        }
        self.throw_error(fiber_idx, &format!("BIF {} not found.", bif_name))
    }

    fn read_constant(&mut self, fiber_idx: usize, idx: usize) -> BxValue {
        let val = {
            let fiber = &self.fibers[fiber_idx];
            let frame = fiber.frames.last().unwrap();
            let function = &frame.function;
            
            let mut promoted = function.promoted_constants.borrow_mut();
            if promoted.len() <= idx {
                let chunk_len = function.chunk.borrow().constants.len();
                promoted.resize(chunk_len, None);
            }
            promoted[idx]
        };

        if let Some(v) = val {
            return v;
        }

        let constant = {
            let fiber = &self.fibers[fiber_idx];
            let frame = fiber.frames.last().unwrap();
            let chunk = frame.function.chunk.borrow();
            chunk.constants[idx].clone()
        };

        let promoted = self.promote_constant(constant);
        
        {
            let fiber = &self.fibers[fiber_idx];
            let frame = fiber.frames.last().unwrap();
            frame.function.promoted_constants.borrow_mut()[idx] = Some(promoted);
        }
        
        promoted
    }

    fn promote_constant(&mut self, constant: Constant) -> BxValue {
        match constant {
            Constant::Number(n) => BxValue::new_number(n),
            Constant::Boolean(b) => BxValue::new_bool(b),
            Constant::Null => BxValue::new_null(),
            Constant::String(s) => BxValue::new_ptr(self.heap.alloc(GcObject::String(s))),
            Constant::StringArray(arr) => {
                let mut values = Vec::with_capacity(arr.len());
                for s in arr {
                    let id = self.heap.alloc(GcObject::String(BoxString::new(&s)));
                    values.push(BxValue::new_ptr(id));
                }
                let id = self.heap.alloc(GcObject::Array(values));
                BxValue::new_ptr(id)
            }
            Constant::CompiledFunction(f) => {
                let mut f = f;
                let count = f.chunk.borrow().constants.len();
                f.promoted_constants = RefCell::new(vec![None; count]);
                BxValue::new_ptr(self.heap.alloc(GcObject::CompiledFunction(Rc::new(f))))
            }
            Constant::Class(c) => BxValue::new_ptr(self.heap.alloc(GcObject::Class(Rc::new(RefCell::new(c))))),
            Constant::Interface(i) => BxValue::new_ptr(self.heap.alloc(GcObject::Interface(Rc::new(RefCell::new(i))))),
        }
    }

    fn read_string_constant(&mut self, fiber_idx: usize, idx: usize) -> String {
        let val = self.read_constant(fiber_idx, idx);
        if let Some(id) = val.as_gc_id() {
            if let GcObject::String(s) = self.heap.get(id) {
                return s.to_string();
            }
        }
        panic!("Constant at index {} is not a string: {:?}", idx, val)
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub fn bx_to_js(&self, val: &BxValue) -> JsValue {
        if val.is_number() {
            JsValue::from_f64(val.as_number())
        } else if val.is_int() {
            JsValue::from_f64(val.as_int() as f64)
        } else if val.is_bool() {
            JsValue::from_bool(val.as_bool())
        } else if val.is_null() {
            JsValue::NULL
        } else if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => {
                    let mut s_flat = s.clone();
                    JsValue::from_str(s_flat.flatten())
                }
                GcObject::Array(arr) => {
                    let js_arr = Array::new();
                    for item in arr {
                        js_arr.push(&self.bx_to_js(item));
                    }
                    js_arr.into()
                }
                GcObject::Struct(s) => {
                    let js_obj = js_sys::Object::new();
                    let shape = &self.shapes.shapes[s.shape_id as usize];
                    for (k, &idx) in shape.fields.iter() {
                        Reflect::set(&js_obj, &JsValue::from_str(k), &self.bx_to_js(&s.properties[idx])).ok();
                    }
                    js_obj.into()
                }
                GcObject::JsValue(js) => js.clone(),
                _ => JsValue::UNDEFINED,
            }
        } else {
            JsValue::UNDEFINED
        }
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub fn js_to_bx(&mut self, val: JsValue) -> BxValue {
        if val.is_string() {
            let id = self.heap.alloc(GcObject::String(BoxString::new(&val.as_string().unwrap())));
            BxValue::new_ptr(id)
        } else if let Some(n) = val.as_f64() {
            BxValue::new_number(n)
        } else if let Some(b) = val.as_bool() {
            BxValue::new_bool(b)
        } else if val.is_null() {
            BxValue::new_null()
        } else if Array::is_array(&val) {
            let js_arr: Array = val.into();
            let mut bx_arr = Vec::new();
            for i in 0..js_arr.length() {
                bx_arr.push(self.js_to_bx(js_arr.get(i)));
            }
            BxValue::new_ptr(self.heap.alloc(GcObject::Array(bx_arr)))
        } else if val.is_instance_of::<js_sys::Promise>() {
            let promise: js_sys::Promise = val.into();
            BxValue::new_ptr(self.heap.alloc(GcObject::JsValue(promise.into())))
        } else {
            BxValue::new_ptr(self.heap.alloc(GcObject::JsValue(val)))
        }
    }

    fn collect_garbage(&mut self) {
        let mut roots = Vec::new();
        // 1. Fiber stacks and frames
        for fiber in &self.fibers {
            roots.extend(fiber.stack.iter().cloned());
            for frame in &fiber.frames {
                if let Some(recv) = &frame.receiver {
                    roots.push(*recv);
                }
            }
            roots.push(BxValue::new_ptr(fiber.future_id));
        }
        // 2. Globals
        roots.extend(self.global_values.iter().cloned());

        self.heap.collect(&roots);
    }

    pub fn bx_to_json(&self, val: &BxValue) -> serde_json::Value {
        if val.is_number() {
            serde_json::Number::from_f64(val.as_number())
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        } else if val.is_int() {
            serde_json::Value::Number(val.as_int().into())
        } else if val.is_bool() {
            serde_json::Value::Bool(val.as_bool())
        } else if val.is_null() {
            serde_json::Value::Null
        } else if let Some(id) = val.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(s) => serde_json::Value::String(s.to_string()),
                GcObject::Array(arr) => {
                    let json_arr: Vec<serde_json::Value> = arr.iter().map(|v| self.bx_to_json(v)).collect();
                    serde_json::Value::Array(json_arr)
                }
                GcObject::Struct(s) => {
                    let mut map = serde_json::Map::new();
                    let shape = &self.shapes.shapes[s.shape_id as usize];
                    for (k, &idx) in shape.fields.iter() {
                        if let Some(v) = s.properties.get(idx as usize) {
                            map.insert(k.clone(), self.bx_to_json(v));
                        }
                    }
                    serde_json::Value::Object(map)
                }
                _ => serde_json::Value::String(format!("<ptr {}>", id)),
            }
        } else {
            serde_json::Value::Null
        }
    }

    pub fn json_to_bx(&mut self, val: serde_json::Value) -> BxValue {
        match val {
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    BxValue::new_int(i as i32)
                } else {
                    BxValue::new_number(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::Bool(b) => BxValue::new_bool(b),
            serde_json::Value::String(s) => {
                let id = self.heap.alloc(GcObject::String(BoxString::new(&s)));
                BxValue::new_ptr(id)
            }
            serde_json::Value::Array(arr) => {
                let bx_arr: Vec<BxValue> = arr.into_iter().map(|v| self.json_to_bx(v)).collect();
                let id = self.heap.alloc(GcObject::Array(bx_arr));
                BxValue::new_ptr(id)
            }
            serde_json::Value::Object(obj) => {
                let mut bx_struct = BxStruct {
                    shape_id: self.shapes.get_root(),
                    properties: Vec::new(),
                };
                for (name, val) in obj {
                    let bx_val = self.json_to_bx(val);
                    let shape_id = bx_struct.shape_id;
                    bx_struct.shape_id = self.shapes.transition(shape_id, &name);
                    bx_struct.properties.push(bx_val);
                }
                let id = self.heap.alloc(GcObject::Struct(bx_struct));
                BxValue::new_ptr(id)
            }
            serde_json::Value::Null => BxValue::new_null(),
        }
    }
}
