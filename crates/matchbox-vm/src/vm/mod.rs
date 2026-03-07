pub mod chunk;
pub mod opcode;
pub mod shape;
pub mod gc;

use std::collections::HashMap;
use std::rc::Rc;
use std::cell::RefCell;
use anyhow::{Result, bail};
use crate::types::{BxValue, BxCompiledFunction, BxInstance, BxFuture, FutureStatus, BxVM, BxStruct};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsValue, JsCast};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::Closure;
#[cfg(target_arch = "wasm32")]
use js_sys::{Reflect, Function, Array};
use self::chunk::{Chunk, IcEntry};
use self::opcode::OpCode;
use self::shape::ShapeRegistry;
use self::gc::{Heap, GcObject, GcId};
use web_time::{Instant, Duration};

#[derive(Debug, Clone)]
struct CallFrame {
    function: Rc<BxCompiledFunction>,
    ip: usize,
    stack_base: usize,
    receiver: Option<BxValue>, 
    handlers: Vec<usize>, // IP targets for catch blocks
}

pub struct BxFiber {
    stack: Vec<BxValue>,
    frames: Vec<CallFrame>,
    pub future_id: GcId,
    pub wait_until: Option<Instant>,
    pub yield_requested: bool,
}

pub struct VM {
    fibers: Vec<BxFiber>,
    pub globals: HashMap<String, BxValue>,
    current_fiber_idx: Option<usize>,
    pub shapes: ShapeRegistry,
    pub heap: Heap,
}

impl BxVM for VM {
    fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>) -> BxValue {
        self.spawn(func, args)
    }

    fn call_function_by_value(&mut self, func: &BxValue, args: Vec<BxValue>) -> Result<BxValue, String> {
        self.call_function_value(func.clone(), args).map_err(|e| e.to_string())
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

    fn get_root_shape(&self) -> usize {
        self.shapes.get_root()
    }

    fn get_shape_index(&self, shape_id: usize, field_name: &str) -> Option<usize> {
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
            arr.get(idx).cloned().unwrap_or(BxValue::Null)
        } else { BxValue::Null }
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

    fn struct_get_shape(&self, id: usize) -> usize {
        if let GcObject::Struct(s) = self.heap.get(id) {
            s.shape_id
        } else { 0 }
    }

    fn future_on_error(&mut self, id: usize, handler: BxValue) {
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            f.error_handler = Some(handler);
        }
    }
}

impl VM {
    pub fn new() -> Self {
        Self::new_with_bifs(HashMap::new())
    }

    pub fn new_with_bifs(external_bifs: HashMap<String, BxValue>) -> Self {
        #[allow(unused_mut)]
        let mut globals = HashMap::new();
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(window) = web_sys::window() {
                globals.insert("js".to_string(), BxValue::JsValue(window.into()));
            }
        }

        // Register standard BIFs
        for (name, val) in crate::bifs::register_all() {
            globals.insert(name, val);
        }

        // Register external/plugin BIFs
        for (name, val) in external_bifs {
            globals.insert(name, val);
        }

        VM {
            fibers: Vec::new(),
            globals,
            current_fiber_idx: None,
            shapes: ShapeRegistry::new(),
            heap: Heap::new(),
        }
    }

    fn resolve_member_method(&self, receiver: &BxValue, method_name: &str) -> Option<String> {
        match receiver {
            BxValue::String(_) => match method_name {
                "len" | "length" => Some("len".to_string()),
                "ucase" | "touppercase" => Some("ucase".to_string()),
                "lcase" | "tolowercase" => Some("lcase".to_string()),
                _ => None,
            },
            BxValue::Array(_) => match method_name {
                "len" | "length" | "count" => Some("len".to_string()),
                "append" | "add" => Some("arrayappend".to_string()),
                "each" => Some("arrayeach".to_string()),
                "map" => Some("arraymap".to_string()),
                "reduce" => Some("arrayreduce".to_string()),
                "filter" => Some("arrayfilter".to_string()),
                "tolist" => Some("arraytolist".to_string()),
                _ => None,
            },
            BxValue::Struct(_) => match method_name {
                "len" | "count" => Some("len".to_string()),
                "exists" | "keyexists" => Some("structkeyexists".to_string()),
                "each" => Some("structeach".to_string()),
                _ => None,
            },
            BxValue::Number(_) => match method_name {
                "abs" => Some("abs".to_string()),
                "round" => Some("round".to_string()),
                _ => None,
            },
            BxValue::Future(_) => match method_name {
                "onerror" => Some("futureonerror".to_string()),
                _ => None,
            },
            _ => None,
        }
    }

    fn resolve_method(&self, class: Rc<RefCell<crate::types::BxClass>>, method_name: &str) -> Option<Rc<BxCompiledFunction>> {
        let mut current_class = class;
        loop {
            let class_ref = current_class.borrow();
            if let Some(method) = class_ref.methods.get(method_name) {
                return Some(Rc::clone(method));
            }
            
            if let Some(parent_name) = &class_ref.extends {
                if let Some(BxValue::Class(parent_class)) = self.globals.get(&parent_name.to_lowercase()) {
                    let next_class = Rc::clone(parent_class);
                    drop(class_ref); // release borrow
                    current_class = next_class;
                    continue;
                }
            }
            return None;
        }
    }

    pub fn interpret(&mut self, chunk: Chunk) -> Result<BxValue> {
        let function = Rc::new(BxCompiledFunction {
            name: "script".to_string(),
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            chunk: Rc::new(RefCell::new(chunk)),
        });
        
        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::Null,
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
            value: BxValue::Null,
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
        BxValue::Future(future_id)
    }

    fn run_all(&mut self) -> Result<BxValue> {
        let mut last_result = BxValue::Null;
        
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
                            f.value = result.clone();
                            f.status = FutureStatus::Completed;
                        }
                        if self.fibers.is_empty() {
                            last_result = result;
                        }
                    }
                    Ok(None) => {
                        i += 1;
                    }
                    Err(e) => {
                        let fiber = self.fibers.remove(i);
                        let mut handler = None;
                        if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                            f.status = FutureStatus::Failed(e.to_string());
                            handler = f.error_handler.clone();
                        }
                        
                        if let Some(h) = handler {
                            let err_val = BxValue::String(e.to_string());
                            let _ = self.call_function_value(h, vec![err_val]);
                            // Error was handled by user, do NOT return Err(e) yet.
                            // If this was the last fiber, run_all will naturally exit the loop.
                            continue;
                        }

                        if self.fibers.is_empty() {
                            return Err(e);
                        } else {
                            // Detached/Async task failed without a handler
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

    fn collect_garbage(&mut self) {
        let mut roots = Vec::new();
        // 1. Fiber stacks and futures
        for fiber in &self.fibers {
            roots.extend(fiber.stack.iter().cloned());
            for frame in &fiber.frames {
                if let Some(recv) = &frame.receiver {
                    roots.push(recv.clone());
                }
            }
            roots.push(BxValue::Future(fiber.future_id));
            if let GcObject::Future(f) = self.heap.get(fiber.future_id) {
                if let Some(handler) = &f.error_handler {
                    roots.push(handler.clone());
                }
            }
        }
        // 2. Globals
        roots.extend(self.globals.values().cloned());

        self.heap.collect(&roots);
    }

    fn run_fiber(&mut self, fiber_idx: usize, quantum: usize) -> Result<Option<BxValue>> {
        for _ in 0..quantum {
            if fiber_idx >= self.fibers.len() {
                return Ok(None);
            }
            if self.fibers[fiber_idx].frames.is_empty() {
                return Ok(Some(BxValue::Null));
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
                    return Ok(Some(BxValue::Null));
                }
                (chunk.code[frame.ip].clone(), frame.ip)
            };
            
            self.fibers[fiber_idx].frames.last_mut().unwrap().ip += 1;

            match instruction {
                OpCode::OpReturn => {
                    let fiber = &mut self.fibers[fiber_idx];
                    let frame = fiber.frames.pop().unwrap();
                    let result = if fiber.stack.len() > frame.stack_base {
                        fiber.stack.pop().unwrap()
                    } else {
                        BxValue::Null
                    };
                    
                    if fiber.frames.is_empty() {
                        return Ok(Some(result));
                    }
                    
                    fiber.stack.truncate(frame.stack_base);
                    
                    if frame.function.name.ends_with(".constructor") {
                        let instance = fiber.stack.pop().unwrap();
                        fiber.stack.push(instance);
                    } else {
                        fiber.stack.pop();
                        fiber.stack.push(result);
                    }
                }
                OpCode::OpConstant(idx) => {
                    let constant = self.read_constant(fiber_idx, idx);
                    self.fibers[fiber_idx].stack.push(constant);
                }
                OpCode::OpAdd => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    match (a, b) {
                        (BxValue::Number(a), BxValue::Number(b)) => self.fibers[fiber_idx].stack.push(BxValue::Number(a + b)),
                        (BxValue::String(a), BxValue::String(b)) => self.fibers[fiber_idx].stack.push(BxValue::String(format!("{}{}", a, b))),
                        _ => { self.throw_error(fiber_idx, "Operands must be two numbers or two strings.")?; continue; },
                    }
                }
                OpCode::OpSubtract => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (BxValue::Number(a), BxValue::Number(b)) = (a, b) {
                        self.fibers[fiber_idx].stack.push(BxValue::Number(a - b));
                    } else {
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        continue;
                    }
                }
                OpCode::OpMultiply => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (BxValue::Number(a), BxValue::Number(b)) = (a, b) {
                        self.fibers[fiber_idx].stack.push(BxValue::Number(a * b));
                    } else {
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        continue;
                    }
                }
                OpCode::OpDivide => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let (BxValue::Number(a), BxValue::Number(b)) = (a, b) {
                        if b == 0.0 { self.throw_error(fiber_idx, "Division by zero")?; continue; }
                        else { self.fibers[fiber_idx].stack.push(BxValue::Number(a / b)); }
                    } else {
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        continue;
                    }
                }
                OpCode::OpStringConcat => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::String(format!("{}{}", a, b)));
                }
                OpCode::OpPrint(count) => {
                    let mut args = Vec::with_capacity(count);
                    for _ in 0..count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    let out = args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ");
                    print!("{}", out);
                }
                OpCode::OpPrintln(count) => {
                    let mut args = Vec::with_capacity(count);
                    for _ in 0..count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    let out = args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ");
                    println!("{}", out);
                }
                OpCode::OpPop => {
                    self.fibers[fiber_idx].stack.pop();
                }
                OpCode::OpDup => {
                    let val = self.fibers[fiber_idx].stack.last().unwrap().clone();
                    self.fibers[fiber_idx].stack.push(val);
                }
                OpCode::OpSwap => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(b);
                    self.fibers[fiber_idx].stack.push(a);
                }
                OpCode::OpOver => {
                    let val = self.fibers[fiber_idx].stack[self.fibers[fiber_idx].stack.len() - 2].clone();
                    self.fibers[fiber_idx].stack.push(val);
                }
                OpCode::OpInc => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let BxValue::Number(n) = val {
                        self.fibers[fiber_idx].stack.push(BxValue::Number(n + 1.0));
                    } else {
                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                        continue;
                    }
                }
                OpCode::OpDec => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let BxValue::Number(n) = val {
                        self.fibers[fiber_idx].stack.push(BxValue::Number(n - 1.0));
                    } else {
                        self.throw_error(fiber_idx, "Decrement operand must be a number")?;
                        continue;
                    }
                }
                OpCode::OpDefineGlobal(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx);
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.globals.insert(name.to_lowercase(), val);
                }
                OpCode::OpGetGlobal(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx);
                    if let Some(val) = self.globals.get(&name.to_lowercase()) {
                        self.fibers[fiber_idx].stack.push(val.clone());
                    } else {
                        self.fibers[fiber_idx].stack.push(BxValue::Null); 
                    }
                }
                OpCode::OpSetGlobal(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx);
                    let val = self.fibers[fiber_idx].stack.last().unwrap().clone();
                    self.globals.insert(name.to_lowercase(), val);
                }
                OpCode::OpGetLocal(slot) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let val = self.fibers[fiber_idx].stack[base + slot].clone();
                    self.fibers[fiber_idx].stack.push(val);
                }
                OpCode::OpSetLocal(slot) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let val = self.fibers[fiber_idx].stack.last().unwrap().clone();
                    self.fibers[fiber_idx].stack[base + slot] = val;
                }
                OpCode::OpArray(count) => {
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        items.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    items.reverse();
                    let id = self.heap.alloc(GcObject::Array(items));
                    self.fibers[fiber_idx].stack.push(BxValue::Array(id));
                }
                OpCode::OpStruct(count) => {
                    let mut shape_id = self.shapes.get_root();
                    let mut props = Vec::with_capacity(count);
                    
                    let mut kv_pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let value = self.fibers[fiber_idx].stack.pop().unwrap();
                        let key = self.fibers[fiber_idx].stack.pop().unwrap().to_string().to_lowercase();
                        kv_pairs.push((key, value));
                    }
                    kv_pairs.reverse(); 

                    for (key, value) in kv_pairs {
                        shape_id = self.shapes.transition(shape_id, &key);
                        props.push(value);
                    }

                    let id = self.heap.alloc(GcObject::Struct(BxStruct { shape_id, properties: props }));
                    self.fibers[fiber_idx].stack.push(BxValue::Struct(id));
                }
                OpCode::OpIndex => {
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    match base_val {
                        BxValue::Array(id) => {
                            if let BxValue::Number(n) = index_val {
                                let idx = n as usize;
                                if let GcObject::Array(arr) = self.heap.get(id) {
                                    if idx < 1 || idx > arr.len() {
                                        self.throw_error(fiber_idx, &format!("Array index out of bounds: {}", idx))?;
                                        continue;
                                    } else {
                                        self.fibers[fiber_idx].stack.push(arr[idx - 1].clone());
                                    }
                                }
                            } else {
                                self.throw_error(fiber_idx, "Array index must be a number")?;
                                continue;
                            }
                        }
                        BxValue::Struct(id) => {
                            let key = index_val.to_string().to_lowercase();
                            if let GcObject::Struct(s) = self.heap.get(id) {
                                if let Some(idx) = self.shapes.get_index(s.shape_id, &key) {
                                    self.fibers[fiber_idx].stack.push(s.properties[idx].clone());
                                } else {
                                    self.fibers[fiber_idx].stack.push(BxValue::Null);
                                }
                            }
                        }
                        _ => { self.throw_error(fiber_idx, "Invalid access: base must be array or struct")?; continue; }
                    }
                }
                OpCode::OpSetIndex => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    
                    match base_val {
                        BxValue::Array(id) => {
                            if let BxValue::Number(n) = index_val {
                                let idx = n as usize;
                                if let GcObject::Array(arr) = self.heap.get_mut(id) {
                                    if idx < 1 || idx > arr.len() {
                                        self.throw_error(fiber_idx, &format!("Array index out of bounds: {}", idx))?;
                                        continue;
                                    } else {
                                        arr[idx - 1] = val.clone();
                                        self.fibers[fiber_idx].stack.push(val);
                                    }
                                }
                            } else {
                                self.throw_error(fiber_idx, "Array index must be a number")?;
                                continue;
                            }
                        }
                        BxValue::Struct(id) => {
                            let key = index_val.to_string().to_lowercase();
                            if let GcObject::Struct(s) = self.heap.get_mut(id) {
                                if let Some(idx) = self.shapes.get_index(s.shape_id, &key) {
                                    s.properties[idx] = val.clone();
                                } else {
                                    s.shape_id = self.shapes.transition(s.shape_id, &key);
                                    s.properties.push(val.clone());
                                }
                            }
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        BxValue::Instance(id) => {
                            let key = index_val.to_string().to_lowercase();
                            if let GcObject::Instance(inst) = self.heap.get_mut(id) {
                                if let Some(idx) = self.shapes.get_index(inst.shape_id, &key) {
                                    inst.properties[idx] = val.clone();
                                } else {
                                    inst.shape_id = self.shapes.transition(inst.shape_id, &key);
                                    inst.properties.push(val.clone());
                                }
                            }
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        _ => { self.throw_error(fiber_idx, "Invalid indexed assignment")?; continue; }
                    }
                }
                OpCode::OpMember(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx).to_lowercase();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    
                    match base_val {
                        BxValue::Struct(id) => {
                            let (shape_id, properties_ptr) = if let GcObject::Struct(s) = self.heap.get(id) {
                                (s.shape_id, &s.properties as *const Vec<BxValue>)
                            } else { unreachable!() };

                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    let val = unsafe { &*properties_ptr }[index].clone();
                                    self.fibers[fiber_idx].stack.push(val);
                                    continue;
                                }
                            }

                            if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let mut chunk = frame.function.chunk.borrow_mut();
                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                }
                                let val = unsafe { &*properties_ptr }[idx].clone();
                                self.fibers[fiber_idx].stack.push(val);
                            } else {
                                self.fibers[fiber_idx].stack.push(BxValue::Null);
                            }
                        }
                        BxValue::Instance(id) => {
                            let (shape_id, properties_ptr, class) = if let GcObject::Instance(inst) = self.heap.get(id) {
                                (inst.shape_id, &inst.properties as *const Vec<BxValue>, Rc::clone(&inst.class))
                            } else { unreachable!() };

                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    let val = unsafe { &*properties_ptr }[index].clone();
                                    self.fibers[fiber_idx].stack.push(val);
                                    continue;
                                }
                            }

                            if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let mut chunk = frame.function.chunk.borrow_mut();
                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                }
                                let val = unsafe { &*properties_ptr }[idx].clone();
                                self.fibers[fiber_idx].stack.push(val);
                            } else if let Some(method) = self.resolve_method(Rc::clone(&class), &name) {
                                self.fibers[fiber_idx].stack.push(BxValue::CompiledFunction(method));
                            } else {
                                self.fibers[fiber_idx].stack.push(BxValue::Null);
                            }
                        }
                        #[cfg(target_arch = "wasm32")]
                        BxValue::JsValue(js) => {
                            let prop = JsValue::from_str(&name);
                            match Reflect::get(&js, &prop) {
                                Ok(val) => {
                                    let bx_val = self.js_to_bx(val);
                                    self.fibers[fiber_idx].stack.push(bx_val);
                                }
                                Err(_) => self.fibers[fiber_idx].stack.push(BxValue::Null),
                            }
                        }
                        BxValue::NativeObject(obj) => {
                            let val = obj.borrow().get_property(&name);
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        _ => { self.throw_error(fiber_idx, "Member access only supported on structs, instances, JS objects, and native objects")?; continue; }
                    }
                }
                OpCode::OpSetMember(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx).to_lowercase();
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    
                    match base_val {
                        BxValue::Struct(id) => {
                            let shape_id = if let GcObject::Struct(s) = self.heap.get(id) {
                                s.shape_id
                            } else { unreachable!() };

                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    if let GcObject::Struct(s) = self.heap.get_mut(id) {
                                        s.properties[index] = val.clone();
                                    }
                                    self.fibers[fiber_idx].stack.push(val);
                                    continue;
                                }
                            }

                            if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let mut chunk = frame.function.chunk.borrow_mut();
                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                }
                                if let GcObject::Struct(s) = self.heap.get_mut(id) {
                                    s.properties[idx] = val.clone();
                                }
                            } else {
                                if let GcObject::Struct(s) = self.heap.get_mut(id) {
                                    s.shape_id = self.shapes.transition(shape_id, &name);
                                    s.properties.push(val.clone());
                                }
                            }
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        BxValue::Instance(id) => {
                            let shape_id = if let GcObject::Instance(inst) = self.heap.get(id) {
                                inst.shape_id
                            } else { unreachable!() };

                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    if let GcObject::Instance(inst) = self.heap.get_mut(id) {
                                        inst.properties[index] = val.clone();
                                    }
                                    self.fibers[fiber_idx].stack.push(val);
                                    continue;
                                }
                            }

                            if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let mut chunk = frame.function.chunk.borrow_mut();
                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                }
                                if let GcObject::Instance(inst) = self.heap.get_mut(id) {
                                    inst.properties[idx] = val.clone();
                                }
                            } else {
                                if let GcObject::Instance(inst) = self.heap.get_mut(id) {
                                    inst.shape_id = self.shapes.transition(shape_id, &name);
                                    inst.properties.push(val.clone());
                                }
                            }
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        #[cfg(target_arch = "wasm32")]
                        BxValue::JsValue(js) => {
                            let prop = JsValue::from_str(&name);
                            let js_val = self.bx_to_js(&val);
                            Reflect::set(&js, &prop, &js_val).ok();
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        BxValue::NativeObject(obj) => {
                            obj.borrow_mut().set_property(&name, val.clone());
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        _ => { self.throw_error(fiber_idx, "Member assignment only supported on structs, instances, JS objects, and native objects")?; continue; }
                    }
                }
                OpCode::OpCall(arg_count) => {
                    self.execute_call(fiber_idx, arg_count, None)?;
                }
                OpCode::OpCallNamed(total_count, names_idx) => {
                    let names = match self.read_constant(fiber_idx, names_idx) {
                        BxValue::StringArray(arr) => arr.clone(),
                        _ => bail!("Internal VM error: names constant is not a StringArray"),
                    };
                    self.execute_call(fiber_idx, total_count, Some(names))?;
                }
                OpCode::OpInvoke(idx, arg_count) => {
                    let name = self.read_string_constant(fiber_idx, idx).to_lowercase();
                    self.execute_invoke(fiber_idx, name, arg_count, None, ip_at_start)?;
                }
                OpCode::OpInvokeNamed(name_idx, total_count, names_idx) => {
                    let name = self.read_string_constant(fiber_idx, name_idx).to_lowercase();
                    let names = match self.read_constant(fiber_idx, names_idx) {
                        BxValue::StringArray(arr) => arr.clone(),
                        _ => bail!("Internal VM error: names constant is not a StringArray"),
                    };
                    self.execute_invoke(fiber_idx, name, total_count, Some(names), ip_at_start)?;
                }
                OpCode::OpEqual => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::Boolean(a == b));
                }
                OpCode::OpNotEqual => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::Boolean(a != b));
                }
                OpCode::OpLess => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    match (a, b) {
                        (BxValue::Number(a), BxValue::Number(b)) => self.fibers[fiber_idx].stack.push(BxValue::Boolean(a < b)),
                        _ => { self.throw_error(fiber_idx, "Comparison only supported for numbers currently")?; continue; }
                    }
                }
                OpCode::OpLessEqual => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    match (a, b) {
                        (BxValue::Number(a), BxValue::Number(b)) => self.fibers[fiber_idx].stack.push(BxValue::Boolean(a <= b)),
                        _ => { self.throw_error(fiber_idx, "Comparison only supported for numbers currently")?; continue; }
                    }
                }
                OpCode::OpGreater => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    match (a, b) {
                        (BxValue::Number(a), BxValue::Number(b)) => self.fibers[fiber_idx].stack.push(BxValue::Boolean(a > b)),
                        _ => { self.throw_error(fiber_idx, "Comparison only supported for numbers currently")?; continue; }
                    }
                }
                OpCode::OpGreaterEqual => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    match (a, b) {
                        (BxValue::Number(a), BxValue::Number(b)) => self.fibers[fiber_idx].stack.push(BxValue::Boolean(a >= b)),
                        _ => { self.throw_error(fiber_idx, "Comparison only supported for numbers currently")?; continue; }
                    }
                }
                OpCode::OpJump(offset) => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().ip += offset;
                }
                OpCode::OpJumpIfFalse(offset) => {
                    if !self.is_truthy(self.fibers[fiber_idx].stack.last().unwrap()) {
                        self.fibers[fiber_idx].frames.last_mut().unwrap().ip += offset;
                    }
                }
                OpCode::OpLoop(offset) => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= offset;
                }
                OpCode::OpIterNext(collection_slot, cursor_slot, offset, push_index) => {
                    let base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                    let collection_idx = base + collection_slot;
                    let cursor_idx = base + cursor_slot;
                    
                    let (is_done, next_val, next_idx) = {
                        let cursor_val = match &self.fibers[fiber_idx].stack[cursor_idx] {
                            BxValue::Number(n) => *n as usize,
                            _ => bail!("Internal VM error: iterator cursor is not a number"),
                        };
                        
                        match &self.fibers[fiber_idx].stack[collection_idx] {
                            BxValue::Array(id) => {
                                if let GcObject::Array(arr) = self.heap.get(*id) {
                                    if cursor_val < arr.len() {
                                        (false, Some(arr[cursor_val].clone()), Some(BxValue::Number(cursor_val as f64 + 1.0)))
                                    } else {
                                        (true, None, None)
                                    }
                                } else { unreachable!() }
                            }
                            BxValue::Struct(id) => {
                                if let GcObject::Struct(s) = self.heap.get(*id) {
                                    let keys = {
                                        let mut k = Vec::new();
                                        let shape = &self.shapes.shapes[s.shape_id];
                                        for key in shape.fields.keys() {
                                            k.push(key.clone());
                                        }
                                        k.sort();
                                        k
                                    };
                                    if cursor_val < keys.len() {
                                        let key = &keys[cursor_val];
                                        let idx = self.shapes.get_index(s.shape_id, key).unwrap();
                                        let val = &s.properties[idx];
                                        (false, Some(BxValue::String(key.clone())), Some(val.clone()))
                                    } else {
                                        (true, None, None)
                                    }
                                } else { unreachable!() }
                            }
                            _ => { 
                                self.throw_error(fiber_idx, "Iteration only supported for arrays and structs")?;
                                (true, None, None)
                            }
                        }
                    };

                    if is_done {
                        self.fibers[fiber_idx].frames.last_mut().unwrap().ip += offset;
                    } else {
                        if let BxValue::Number(ref mut n) = self.fibers[fiber_idx].stack[cursor_idx] {
                            *n += 1.0;
                        }
                        self.fibers[fiber_idx].stack.push(next_val.unwrap());
                        if push_index {
                            self.fibers[fiber_idx].stack.push(next_idx.unwrap());
                        }
                    }
                }
                OpCode::OpNew(arg_count) => {
                    let class_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count;
                    let class_val = self.fibers[fiber_idx].stack[class_idx].clone();
                    if let BxValue::Class(class) = class_val {
                        let variables_scope = Rc::new(RefCell::new(HashMap::new()));
                        
                        let inst_id = self.heap.alloc(GcObject::Instance(BxInstance {
                            class: Rc::clone(&class),
                            shape_id: self.shapes.get_root(),
                            properties: Vec::new(),
                            variables: variables_scope.clone(),
                        }));
                        
                        let instance_val = BxValue::Instance(inst_id);
                        self.fibers[fiber_idx].stack[class_idx] = instance_val.clone();

                        let frame = CallFrame {
                            function: Rc::new(BxCompiledFunction {
                                name: format!("{}.constructor", class.borrow().name),
                                arity: 0,
                                min_arity: 0,
                                params: Vec::new(),
                                chunk: Rc::new(RefCell::new(class.borrow().constructor.borrow().clone())),
                            }),
                            ip: 0,
                            stack_base: class_idx + 1 + arg_count,
                            receiver: Some(instance_val),
                            handlers: Vec::new(),
                        };
                        self.fibers[fiber_idx].frames.push(frame);
                    } else {
                        self.throw_error(fiber_idx, "Can only instantiate classes.")?;
                        continue;
                    }
                }
                OpCode::OpGetPrivate(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx).to_lowercase();
                    let val = if let Some(BxValue::Instance(id)) = self.fibers[fiber_idx].frames.last().unwrap().receiver {
                        if name == "this" {
                            Some(BxValue::Instance(id))
                        } else if name == "variables" {
                            if let GcObject::Instance(inst) = self.heap.get(id) {
                                let _vars = Rc::clone(&inst.variables);
                                Some(BxValue::Struct(self.heap.alloc(GcObject::Struct(BxStruct {
                                    shape_id: self.shapes.get_root(),
                                    properties: Vec::new(),
                                }))))
                            } else { None }
                        } else {
                            if let GcObject::Instance(inst) = self.heap.get(id) {
                                let val = inst.variables.borrow().get(&name).cloned().unwrap_or(BxValue::Null);
                                Some(val)
                            } else { None }
                        }
                    } else {
                        None
                    };

                    if let Some(v) = val {
                        self.fibers[fiber_idx].stack.push(v);
                    } else {
                        self.throw_error(fiber_idx, "'variables' scope only available in classes.")?;
                        continue;
                    }
                }
                OpCode::OpSetPrivate(idx) => {
                    let name = self.read_string_constant(fiber_idx, idx).to_lowercase();
                    let val = self.fibers[fiber_idx].stack.last().unwrap().clone();
                    if let Some(BxValue::Instance(id)) = self.fibers[fiber_idx].frames.last().unwrap().receiver {
                        if let GcObject::Instance(inst) = self.heap.get_mut(id) {
                            inst.variables.borrow_mut().insert(name, val);
                        }
                    } else {
                        self.throw_error(fiber_idx, "'variables' scope only available in classes.")?;
                        continue;
                    }
                }
                OpCode::OpPushHandler(offset) => {
                    let target_ip = self.fibers[fiber_idx].frames.last().unwrap().ip + offset;
                    self.fibers[fiber_idx].frames.last_mut().unwrap().handlers.push(target_ip);
                }
                OpCode::OpPopHandler => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().handlers.pop();
                }
                OpCode::OpThrow => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.throw_value(fiber_idx, val)?;
                }
            }
        }
        Ok(None)
    }

    fn throw_error(&mut self, fiber_idx: usize, msg: &str) -> Result<()> {
        let val = BxValue::String(msg.to_string());
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
                    if line <= lines.len() {
                        let code_line = lines[line - 1].trim();
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
        bail!("VM Runtime Error: {}{}\n(at {} line {})", val, source_snippet, filename, line);
    }

    pub fn call_function(&mut self, name: &str, args: Vec<BxValue>) -> Result<BxValue> {
        let name_lower = name.to_lowercase();
        let func = self.globals.get(&name_lower).cloned();
        if let Some(f) = func {
            return self.call_function_value(f, args);
        }
        anyhow::bail!("Function {} not found", name)
    }

    pub fn call_function_value(&mut self, func: BxValue, args: Vec<BxValue>) -> Result<BxValue> {
        match func {
            BxValue::CompiledFunction(f) => {
                if args.len() != f.arity {
                    anyhow::bail!("Expected {} arguments but got {}", f.arity, args.len());
                }
                
                let future_id = self.heap.alloc(GcObject::Future(crate::types::BxFuture {
                    value: BxValue::Null,
                    status: crate::types::FutureStatus::Pending,
                    error_handler: None,
                }));

                let fiber = BxFiber {
                    stack: args,
                    frames: vec![CallFrame {
                        function: f,
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
                let fiber_idx = self.fibers.len() - 1;
                self.current_fiber_idx = Some(fiber_idx);
                let res = self.run_fiber(fiber_idx, 1000000);
                self.current_fiber_idx = None;
                
                match res? {
                    Some(val) => Ok(val),
                    None => Ok(BxValue::Null),
                }
            }
            _ => anyhow::bail!("Value is not a callable function"),
        }
    }

    fn is_truthy(&self, val: &BxValue) -> bool {
        match val {
            BxValue::Boolean(b) => *b,
            BxValue::Null => false,
            BxValue::Number(n) => *n != 0.0,
            BxValue::String(s) => !s.is_empty() && s.to_lowercase() != "false",
            _ => true,
        }
    }

    fn reorder_arguments(&self, args: Vec<BxValue>, names: Vec<String>, params: &[String]) -> Vec<BxValue> {
        let mut final_args = vec![BxValue::Null; params.len()];
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

    fn execute_call(&mut self, fiber_idx: usize, arg_count: usize, names: Option<Vec<String>>) -> Result<()> {
        let func_val = self.fibers[fiber_idx].stack[self.fibers[fiber_idx].stack.len() - 1 - arg_count].clone();
        match func_val {
            BxValue::CompiledFunction(ref func) => {
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                }
                args.reverse();
                self.fibers[fiber_idx].stack.pop(); // Pop the function object

                let final_args = if let Some(names_list) = names {
                    self.reorder_arguments(args, names_list, &func.params)
                } else {
                    let mut a = args;
                    for _ in 0..(func.arity - arg_count) {
                        a.push(BxValue::Null);
                    }
                    a
                };

                // Push function back
                let func_rc = Rc::clone(func);
                self.fibers[fiber_idx].stack.push(BxValue::CompiledFunction(func_rc.clone()));
                
                // Push args back in order
                for arg in final_args {
                    self.fibers[fiber_idx].stack.push(arg);
                }

                let frame = CallFrame {
                    function: func_rc,
                    ip: 0,
                    stack_base: self.fibers[fiber_idx].stack.len() - func.arity,
                    receiver: self.fibers[fiber_idx].frames.last().unwrap().receiver.clone(),
                    handlers: Vec::new(),
                };
                self.fibers[fiber_idx].frames.push(frame);
                Ok(())
            }
            BxValue::NativeFunction(func) => {
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
            #[cfg(target_arch = "wasm32")]
            BxValue::JsValue(js) => {
                if let Ok(func) = js.clone().dyn_into::<Function>() {
                    let js_args = Array::new();
                    for _ in 0..arg_count {
                        let arg_val = self.fibers[fiber_idx].stack.pop().unwrap();
                        js_args.unshift(&self.bx_to_js(&arg_val));
                    }
                    self.fibers[fiber_idx].stack.pop(); // Pop the function
                    match Reflect::apply(&func, &JsValue::UNDEFINED, &js_args) {
                        Ok(val) => {
                            let bx_val = self.js_to_bx(val);
                            self.fibers[fiber_idx].stack.push(bx_val);
                            Ok(())
                        }
                        Err(e) => self.throw_error(fiber_idx, &format!("JS Error: {:?}", e)),
                    }
                } else {
                    self.throw_error(fiber_idx, "Can only call JS functions.")
                }
            }
            _ => self.throw_error(fiber_idx, "Can only call functions."),
        }
    }

    fn execute_invoke(&mut self, fiber_idx: usize, name: String, arg_count: usize, names: Option<Vec<String>>, ip_at_start: usize) -> Result<()> {
        if self.fibers[fiber_idx].stack.len() < arg_count + 1 {
            bail!("Stack underflow: missing receiver or arguments for method call");
        }
        let receiver_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count;
        let receiver_val = self.fibers[fiber_idx].stack.get(receiver_idx).cloned().unwrap();
        
        match receiver_val {
            BxValue::Future(id) => {
                let (status, value) = if let GcObject::Future(f) = self.heap.get(id) {
                    (f.status.clone(), f.value.clone())
                } else { unreachable!() };

                if name == "get" {
                    match status {
                        FutureStatus::Pending => {
                            self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= 1;
                            Ok(())
                        }
                        FutureStatus::Completed => {
                            for _ in 0..arg_count { self.fibers[fiber_idx].stack.pop(); }
                            self.fibers[fiber_idx].stack.pop();
                            self.fibers[fiber_idx].stack.push(value);
                            Ok(())
                        }
                        FutureStatus::Failed(e) => {
                            self.throw_error(fiber_idx, &format!("Async operation failed: {}", e))
                        }
                    }
                } else if let Some(bif_name) = self.resolve_member_method(&receiver_val, &name) {
                    if let Some(BxValue::NativeFunction(bif)) = self.globals.get(&bif_name).cloned() {
                        let mut args = Vec::with_capacity(arg_count + 1);
                        for _ in 0..arg_count {
                            args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        args.reverse();
                        self.fibers[fiber_idx].stack.pop(); // receiver
                        
                        let mut final_args = vec![receiver_val];
                        final_args.extend(args);
                        
                        match bif(self, &final_args) {
                            Ok(res) => {
                                self.fibers[fiber_idx].stack.push(res);
                                Ok(())
                            }
                            Err(e) => self.throw_error(fiber_idx, &e),
                        }
                    } else {
                        self.throw_error(fiber_idx, &format!("Method {} not found on future.", name))
                    }
                } else {
                    self.throw_error(fiber_idx, &format!("Method {} not found on future.", name))
                }
            }
            BxValue::Instance(id) => {
                let (shape_id, properties_ptr, class) = if let GcObject::Instance(inst) = self.heap.get(id) {
                    (inst.shape_id, &inst.properties as *const Vec<BxValue>, Rc::clone(&inst.class))
                } else { unreachable!() };

                let ic = {
                    let fiber = &self.fibers[fiber_idx];
                    let frame = fiber.frames.last().unwrap();
                    let chunk = frame.function.chunk.borrow();
                    chunk.caches[ip_at_start].clone()
                };

                let method = if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                    if cached_shape == shape_id {
                        if let BxValue::CompiledFunction(f) = unsafe { &*properties_ptr }[index].clone() {
                            Some(f)
                        } else { None }
                    } else { None }
                } else { None };

                let method = if method.is_none() {
                    if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                        if let BxValue::CompiledFunction(f) = unsafe { &*properties_ptr }[idx].clone() {
                            {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let mut chunk = frame.function.chunk.borrow_mut();
                                chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                            }
                            Some(f)
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
                    self.fibers[fiber_idx].stack.pop(); // receiver

                    let final_args = if let Some(names_list) = names {
                        self.reorder_arguments(args, names_list, &func.params)
                    } else {
                        let mut a = args;
                        for _ in 0..(func.arity - arg_count) {
                            a.push(BxValue::Null);
                        }
                        a
                    };
                    
                    // Push receiver back
                    self.fibers[fiber_idx].stack.push(receiver_val.clone());

                    for arg in final_args { self.fibers[fiber_idx].stack.push(arg); }

                    let frame = CallFrame {
                        function: func.clone(),
                        ip: 0,
                        stack_base: self.fibers[fiber_idx].stack.len() - func.arity,
                        receiver: Some(receiver_val),
                        handlers: Vec::new(),
                    };
                    self.fibers[fiber_idx].frames.push(frame);
                    Ok(())
                } else if let Some(on_missing) = self.resolve_method(Rc::clone(&class), "onmissingmethod") {
                    let mut original_args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        original_args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    original_args.reverse();
                    self.fibers[fiber_idx].stack.pop(); // receiver

                    let args_array_id = self.heap.alloc(GcObject::Array(original_args));
                    
                    self.fibers[fiber_idx].stack.push(BxValue::String(name.clone()));
                    self.fibers[fiber_idx].stack.push(BxValue::Array(args_array_id));

                    let mut frame = CallFrame {
                        function: on_missing.clone(),
                        ip: 0,
                        stack_base: self.fibers[fiber_idx].stack.len() - 2,
                        receiver: Some(receiver_val),
                        handlers: Vec::new(),
                    };
                    
                    for _ in 0..(on_missing.arity - 2) {
                        self.fibers[fiber_idx].stack.push(BxValue::Null);
                    }
                    frame.stack_base = self.fibers[fiber_idx].stack.len() - on_missing.arity;

                    self.fibers[fiber_idx].frames.push(frame);
                    Ok(())
                } else {
                    self.throw_error(fiber_idx, &format!("Method {} not found.", name))
                }
            }
            BxValue::Struct(id) => {
                let (shape_id, properties_ptr) = if let GcObject::Struct(s) = self.heap.get(id) {
                    (s.shape_id, &s.properties as *const Vec<BxValue>)
                } else { unreachable!() };

                let ic = {
                    let fiber = &self.fibers[fiber_idx];
                    let frame = fiber.frames.last().unwrap();
                    let chunk = frame.function.chunk.borrow();
                    chunk.caches[ip_at_start].clone()
                };

                let method = if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                    if cached_shape == shape_id {
                        if let BxValue::CompiledFunction(f) = unsafe { &*properties_ptr }[index].clone() {
                            Some(f)
                        } else { None }
                    } else { None }
                } else { None };

                let method = if method.is_none() {
                    if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                        if let BxValue::CompiledFunction(func) = unsafe { &*properties_ptr }[idx].clone() {
                            {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let mut chunk = frame.function.chunk.borrow_mut();
                                chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                            }
                            Some(func)
                        } else { None }
                    } else { None }
                } else { method };

                if let Some(func) = method {
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    self.fibers[fiber_idx].stack.pop(); // receiver

                    let final_args = if let Some(names_list) = names {
                        self.reorder_arguments(args, names_list, &func.params)
                    } else {
                        let mut a = args;
                        for _ in 0..(func.arity - arg_count) {
                            a.push(BxValue::Null);
                        }
                        a
                    };
                    
                    // Push receiver back
                    self.fibers[fiber_idx].stack.push(receiver_val.clone());

                    for arg in final_args { self.fibers[fiber_idx].stack.push(arg); }

                    let frame = CallFrame {
                        function: func.clone(),
                        ip: 0,
                        stack_base: self.fibers[fiber_idx].stack.len() - func.arity,
                        receiver: Some(receiver_val),
                        handlers: Vec::new(),
                    };
                    self.fibers[fiber_idx].frames.push(frame);
                    Ok(())
                } else if let Some(bif_name) = self.resolve_member_method(&receiver_val, &name) {
                    if let Some(BxValue::NativeFunction(bif)) = self.globals.get(&bif_name).cloned() {
                        let mut args = Vec::with_capacity(arg_count + 1);
                        for _ in 0..arg_count {
                            args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        args.reverse();
                        self.fibers[fiber_idx].stack.pop(); // Pop the receiver
                        
                        let mut final_args = vec![receiver_val];
                        final_args.extend(args);
                        
                        match bif(self, &final_args) {
                            Ok(res) => {
                                self.fibers[fiber_idx].stack.push(res);
                                Ok(())
                            }
                            Err(e) => self.throw_error(fiber_idx, &e),
                        }
                    } else {
                        self.throw_error(fiber_idx, &format!("Member {} not found or not callable.", name))
                    }
                } else {
                    self.throw_error(fiber_idx, &format!("Member {} not found or not callable.", name))
                }
            }
            #[cfg(target_arch = "wasm32")]
            BxValue::JsValue(js) => {
                let prop = js_sys::JsString::from(name);
                match Reflect::get(&js, &prop) {
                    Ok(val) => {
                        if let Ok(func) = val.clone().dyn_into::<Function>() {
                            let js_args = Array::new();
                            for _ in 0..arg_count {
                                let arg_val = self.fibers[fiber_idx].stack.pop().unwrap();
                                js_args.unshift(&self.bx_to_js(&arg_val));
                            }
                            self.fibers[fiber_idx].stack.pop(); // Pop the receiver
                            match Reflect::apply(&func, &js, &js_args) {
                                Ok(res) => {
                                    let bx_res = self.js_to_bx(res);
                                    self.fibers[fiber_idx].stack.push(bx_res);
                                    Ok(())
                                }
                                Err(e) => self.throw_error(fiber_idx, &format!("JS Error: {:?}", e)),
                            }
                        } else {
                            self.throw_error(fiber_idx, &format!("Member {} is not a function", name))
                        }
                    }
                    Err(e) => self.throw_error(fiber_idx, &format!("JS Error: {:?}", e)),
                }
            }
            BxValue::NativeObject(obj) => {
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                }
                args.reverse();
                self.fibers[fiber_idx].stack.pop(); // receiver
                match obj.borrow_mut().call_method(self, &name, &args) {
                    Ok(res) => {
                        self.fibers[fiber_idx].stack.push(res);
                        Ok(())
                    }
                    Err(e) => self.throw_error(fiber_idx, &e),
                }
            }
            _ => {
                if let Some(bif_name) = self.resolve_member_method(&receiver_val, &name) {
                    if let Some(BxValue::NativeFunction(bif)) = self.globals.get(&bif_name).cloned() {
                        let mut args = Vec::with_capacity(arg_count + 1);
                        for _ in 0..arg_count {
                            args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                        }
                        args.reverse();
                        self.fibers[fiber_idx].stack.pop(); // Pop the receiver
                        
                        let mut final_args = vec![receiver_val];
                        final_args.extend(args);
                        
                        match bif(self, &final_args) {
                            Ok(res) => {
                                self.fibers[fiber_idx].stack.push(res);
                                Ok(())
                            }
                            Err(e) => self.throw_error(fiber_idx, &e),
                        }
                    } else {
                        self.throw_error(fiber_idx, &format!("Method {} not found or cannot be invoked on this type.", name))
                    }
                } else {
                    self.throw_error(fiber_idx, &format!("Method {} not found or cannot be invoked on this type.", name))
                }
            }
        }
    }

    fn read_constant(&self, fiber_idx: usize, idx: usize) -> BxValue {
        let frame = self.fibers[fiber_idx].frames.last().unwrap();
        frame.function.chunk.borrow().constants[idx].clone()
    }

    fn read_string_constant(&self, fiber_idx: usize, idx: usize) -> String {
        let frame = self.fibers[fiber_idx].frames.last().unwrap();
        let chunk = frame.function.chunk.borrow();
        match &chunk.constants[idx] {
            BxValue::String(s) => s.clone(),
            _ => panic!("Constant at index {} is not a string", idx),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn bx_to_js(&self, val: &BxValue) -> JsValue {
        match val {
            BxValue::String(s) => JsValue::from_str(s),
            BxValue::Number(n) => JsValue::from_f64(*n),
            BxValue::Boolean(b) => JsValue::from_bool(*b),
            BxValue::Null => JsValue::NULL,
            BxValue::Array(id) => {
                let js_arr = Array::new();
                if let GcObject::Array(arr) = self.heap.get(*id) {
                    for item in arr {
                        js_arr.push(&self.bx_to_js(item));
                    }
                }
                js_arr.into()
            }
            BxValue::Struct(id) => {
                let js_obj = js_sys::Object::new();
                if let GcObject::Struct(s) = self.heap.get(*id) {
                    let shape = &self.shapes.shapes[s.shape_id];
                    for (k, &idx) in shape.fields.iter() {
                        Reflect::set(&js_obj, &JsValue::from_str(k), &self.bx_to_js(&s.properties[idx])).ok();
                    }
                }
                js_obj.into()
            }
            BxValue::JsValue(js) => js.clone(),
            _ => JsValue::UNDEFINED,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn js_to_bx(&mut self, val: JsValue) -> BxValue {
        if val.is_string() {
            BxValue::String(val.as_string().unwrap())
        } else if let Some(n) = val.as_f64() {
            BxValue::Number(n)
        } else if let Some(b) = val.as_bool() {
            BxValue::Boolean(b)
        } else if val.is_null() {
            BxValue::Null
        } else if Array::is_array(&val) {
            let js_arr: Array = val.into();
            let mut bx_arr = Vec::new();
            for i in 0..js_arr.length() {
                bx_arr.push(self.js_to_bx(js_arr.get(i)));
            }
            BxValue::Array(self.heap.alloc(GcObject::Array(bx_arr)))
        } else if val.is_instance_of::<js_sys::Promise>() {
            let promise: js_sys::Promise = val.into();
            BxValue::JsValue(promise.into())
        } else {
            BxValue::JsValue(val)
        }
    }
}
