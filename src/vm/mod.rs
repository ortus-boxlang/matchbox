pub mod chunk;
pub mod opcode;
pub mod shape;

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

#[derive(Debug, Clone)]
struct CallFrame {
    function: Rc<BxCompiledFunction>,
    ip: usize,
    stack_base: usize,
    receiver: Option<Rc<RefCell<BxInstance>>>,
    handlers: Vec<usize>, // IP targets for catch blocks
}

pub struct BxFiber {
    stack: Vec<BxValue>,
    frames: Vec<CallFrame>,
    pub future: Rc<RefCell<BxFuture>>,
    pub wait_until: Option<std::time::Instant>,
    pub yield_requested: bool,
}

pub struct VM {
    fibers: Vec<BxFiber>,
    pub globals: HashMap<String, BxValue>,
    current_fiber_idx: Option<usize>,
    pub shapes: ShapeRegistry,
}

impl BxVM for VM {
    fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>) -> BxValue {
        self.spawn(func, args)
    }

    fn yield_fiber(&mut self) {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].yield_requested = true;
        }
    }

    fn sleep(&mut self, ms: u64) {
        if let Some(idx) = self.current_fiber_idx {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
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
}

impl VM {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut globals = HashMap::new();
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(window) = web_sys::window() {
                globals.insert("js".to_string(), BxValue::JsValue(window.into()));
            }
        }

        VM {
            fibers: Vec::new(),
            globals,
            current_fiber_idx: None,
            shapes: ShapeRegistry::new(),
        }
    }

    pub fn interpret(&mut self, chunk: Chunk) -> Result<BxValue> {
        let function = Rc::new(BxCompiledFunction {
            name: "script".to_string(),
            arity: 0,
            chunk: Rc::new(RefCell::new(chunk)),
        });
        
        let future = Rc::new(RefCell::new(BxFuture {
            value: BxValue::Null,
            status: FutureStatus::Pending,
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
            future: Rc::clone(&future),
            wait_until: None,
            yield_requested: false,
        };
        
        self.fibers.push(fiber);
        
        self.run_all()
    }

    pub fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>) -> BxValue {
        let future = Rc::new(RefCell::new(BxFuture {
            value: BxValue::Null,
            status: FutureStatus::Pending,
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
            future: Rc::clone(&future),
            wait_until: None,
            yield_requested: false,
        };

        self.fibers.push(fiber);
        BxValue::Future(future)
    }

    fn run_all(&mut self) -> Result<BxValue> {
        let mut last_result = BxValue::Null;
        
        while !self.fibers.is_empty() {
            let mut i = 0;
            let mut all_waiting = true;
            while i < self.fibers.len() {
                let now = std::time::Instant::now();
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
                        fiber.future.borrow_mut().value = result.clone();
                        fiber.future.borrow_mut().status = FutureStatus::Completed;
                        if self.fibers.is_empty() {
                            last_result = result;
                        }
                    }
                    Ok(None) => {
                        i += 1;
                    }
                    Err(e) => {
                        let fiber = self.fibers.remove(i);
                        fiber.future.borrow_mut().status = FutureStatus::Failed(e.to_string());
                        if self.fibers.is_empty() {
                            return Err(e);
                        }
                    }
                }
                self.current_fiber_idx = None;
            }
            
            if all_waiting && !self.fibers.is_empty() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        
        Ok(last_result)
    }

    fn run_fiber(&mut self, fiber_idx: usize, quantum: usize) -> Result<Option<BxValue>> {
        for _ in 0..quantum {
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
                    self.fibers[fiber_idx].stack.push(BxValue::Array(Rc::new(RefCell::new(items))));
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

                    let bx_struct = BxStruct {
                        shape_id,
                        properties: props,
                    };
                    self.fibers[fiber_idx].stack.push(BxValue::Struct(Rc::new(RefCell::new(bx_struct))));
                }
                OpCode::OpIndex => {
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    match base_val {
                        BxValue::Array(arr) => {
                            if let BxValue::Number(n) = index_val {
                                let idx = n as usize;
                                let arr = arr.borrow();
                                if idx < 1 || idx > arr.len() {
                                    self.throw_error(fiber_idx, &format!("Array index out of bounds: {}", idx))?;
                                    continue;
                                } else {
                                    self.fibers[fiber_idx].stack.push(arr[idx - 1].clone());
                                }
                            } else {
                                self.throw_error(fiber_idx, "Array index must be a number")?;
                                continue;
                            }
                        }
                        BxValue::Struct(s) => {
                            let key = index_val.to_string().to_lowercase();
                            let s_borrow = s.borrow();
                            if let Some(idx) = self.shapes.get_index(s_borrow.shape_id, &key) {
                                self.fibers[fiber_idx].stack.push(s_borrow.properties[idx].clone());
                            } else {
                                self.fibers[fiber_idx].stack.push(BxValue::Null);
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
                        BxValue::Array(arr) => {
                            if let BxValue::Number(n) = index_val {
                                let idx = n as usize;
                                let mut arr = arr.borrow_mut();
                                if idx < 1 || idx > arr.len() {
                                    self.throw_error(fiber_idx, &format!("Array index out of bounds: {}", idx))?;
                                    continue;
                                } else {
                                    arr[idx - 1] = val.clone();
                                    self.fibers[fiber_idx].stack.push(val);
                                }
                            } else {
                                self.throw_error(fiber_idx, "Array index must be a number")?;
                                continue;
                            }
                        }
                        BxValue::Struct(s) => {
                            let key = index_val.to_string().to_lowercase();
                            let mut s_borrow = s.borrow_mut();
                            if let Some(idx) = self.shapes.get_index(s_borrow.shape_id, &key) {
                                s_borrow.properties[idx] = val.clone();
                            } else {
                                s_borrow.shape_id = self.shapes.transition(s_borrow.shape_id, &key);
                                s_borrow.properties.push(val.clone());
                            }
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        BxValue::Instance(inst) => {
                            let key = index_val.to_string().to_lowercase();
                            let mut inst_borrow = inst.borrow_mut();
                            if let Some(idx) = self.shapes.get_index(inst_borrow.shape_id, &key) {
                                inst_borrow.properties[idx] = val.clone();
                            } else {
                                inst_borrow.shape_id = self.shapes.transition(inst_borrow.shape_id, &key);
                                inst_borrow.properties.push(val.clone());
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
                        BxValue::Struct(s) => {
                            let shape_id = s.borrow().shape_id;

                            // IC Fast Path
                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    let val = s.borrow().properties[index].clone();
                                    self.fibers[fiber_idx].stack.push(val);
                                    continue;
                                }
                            }

                            // Slow Path
                            if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                // Update Cache
                                {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let mut chunk = frame.function.chunk.borrow_mut();
                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                }
                                let val = s.borrow().properties[idx].clone();
                                self.fibers[fiber_idx].stack.push(val);
                            } else {
                                self.fibers[fiber_idx].stack.push(BxValue::Null);
                            }
                        }
                        BxValue::Instance(inst) => {
                            let shape_id = inst.borrow().shape_id;

                            // IC Fast Path
                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    let val = inst.borrow().properties[index].clone();
                                    self.fibers[fiber_idx].stack.push(val);
                                    continue;
                                }
                            }

                            // Slow Path
                            if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                // Update Cache
                                {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let mut chunk = frame.function.chunk.borrow_mut();
                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                }
                                let val = inst.borrow().properties[idx].clone();
                                self.fibers[fiber_idx].stack.push(val);
                            } else if let Some(method) = inst.borrow().class.borrow().methods.get(&name) {
                                self.fibers[fiber_idx].stack.push(BxValue::CompiledFunction(Rc::clone(method)));
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
                        BxValue::Struct(s) => {
                            let shape_id = s.borrow().shape_id;

                            // Monomorphic IC for SetMember (Same Shape -> Same Index)
                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    s.borrow_mut().properties[index] = val.clone();
                                    self.fibers[fiber_idx].stack.push(val);
                                    continue;
                                }
                            }

                            if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                // Update Cache
                                {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let mut chunk = frame.function.chunk.borrow_mut();
                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                }
                                s.borrow_mut().properties[idx] = val.clone();
                            } else {
                                // Shape transition
                                s.borrow_mut().shape_id = self.shapes.transition(shape_id, &name);
                                s.borrow_mut().properties.push(val.clone());
                            }
                            self.fibers[fiber_idx].stack.push(val);
                        }
                        BxValue::Instance(inst) => {
                            let shape_id = inst.borrow().shape_id;

                            // IC Fast Path
                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    inst.borrow_mut().properties[index] = val.clone();
                                    self.fibers[fiber_idx].stack.push(val);
                                    continue;
                                }
                            }

                            if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                // Update Cache
                                {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let mut chunk = frame.function.chunk.borrow_mut();
                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                }
                                inst.borrow_mut().properties[idx] = val.clone();
                            } else {
                                inst.borrow_mut().shape_id = self.shapes.transition(shape_id, &name);
                                inst.borrow_mut().properties.push(val.clone());
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
                OpCode::OpInvoke(idx, arg_count) => {
                    let name = self.read_string_constant(fiber_idx, idx).to_lowercase();
                    
                    if self.fibers[fiber_idx].stack.len() < arg_count + 1 {
                        bail!("Stack underflow: missing receiver or arguments for method call");
                    }
                    let receiver_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count;
                    let receiver_val = self.fibers[fiber_idx].stack.get(receiver_idx).cloned().unwrap();
                    
                    match receiver_val {
                        BxValue::Future(f) => {
                            if name == "get" {
                                let status = f.borrow().status.clone();
                                match status {
                                    FutureStatus::Pending => {
                                        // Yield and retry
                                        self.fibers[fiber_idx].frames.last_mut().unwrap().ip -= 1;
                                        return Ok(None);
                                    }
                                    FutureStatus::Completed => {
                                        // Pop args and receiver
                                        for _ in 0..arg_count { self.fibers[fiber_idx].stack.pop(); }
                                        self.fibers[fiber_idx].stack.pop();
                                        let val = f.borrow().value.clone();
                                        self.fibers[fiber_idx].stack.push(val);
                                        continue;
                                    }
                                    FutureStatus::Failed(e) => {
                                        self.throw_error(fiber_idx, &format!("Async operation failed: {}", e))?;
                                        continue;
                                    }
                                }
                            } else {
                                self.throw_error(fiber_idx, &format!("Method {} not found on future.", name))?;
                                continue;
                            }
                        }
                        BxValue::Instance(inst) => {
                            let shape_id = inst.borrow().shape_id;

                            // Monomorphic IC for Instance Method Lookups
                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            let method = if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    if let BxValue::CompiledFunction(f) = &inst.borrow().properties[index] {
                                        Some(Rc::clone(f))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            let method = if method.is_none() {
                                if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                    if let BxValue::CompiledFunction(f) = &inst.borrow().properties[idx] {
                                        // Update cache if found in properties
                                        {
                                            let fiber = &self.fibers[fiber_idx];
                                            let frame = fiber.frames.last().unwrap();
                                            let mut chunk = frame.function.chunk.borrow_mut();
                                            chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                        }
                                        Some(Rc::clone(f))
                                    } else {
                                        None
                                    }
                                } else if let Some(f) = inst.borrow().class.borrow().methods.get(&name) {
                                    Some(Rc::clone(f))
                                } else {
                                    None
                                }
                            } else {
                                method
                            };
                            
                            if let Some(func) = method {
                                if arg_count != func.arity {
                                    self.throw_error(fiber_idx, &format!("Expected {} arguments but got {}.", func.arity, arg_count))?;
                                    continue;
                                } else {
                                    // Pop args and receiver, then push args back for the new frame
                                    let mut args = Vec::with_capacity(arg_count);
                                    for _ in 0..arg_count {
                                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                                    }
                                    args.reverse();
                                    self.fibers[fiber_idx].stack.pop(); // Pop the receiver
                                    
                                    for arg in args { self.fibers[fiber_idx].stack.push(arg); }
                                    let frame = CallFrame {
                                        function: func,
                                        ip: 0,
                                        stack_base: self.fibers[fiber_idx].stack.len() - arg_count,
                                        receiver: Some(inst),
                                        handlers: Vec::new(),
                                    };
                                    self.fibers[fiber_idx].frames.push(frame);
                                }
                            } else {
                                self.throw_error(fiber_idx, &format!("Method {} not found.", name))?;
                                continue;
                            }
                        }
                        BxValue::Struct(s) => {
                            let shape_id = s.borrow().shape_id;

                            // IC Fast Path for Struct Method Lookups
                            let ic = {
                                let fiber = &self.fibers[fiber_idx];
                                let frame = fiber.frames.last().unwrap();
                                let chunk = frame.function.chunk.borrow();
                                chunk.caches[ip_at_start].clone()
                            };

                            let method = if let Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) = ic {
                                if cached_shape == shape_id {
                                    if let BxValue::CompiledFunction(f) = &s.borrow().properties[index] {
                                        Some(Rc::clone(f))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            let method = if method.is_none() {
                                if let Some(idx) = self.shapes.get_index(shape_id, &name) {
                                    if let BxValue::CompiledFunction(func) = &s.borrow().properties[idx] {
                                        {
                                            let fiber = &self.fibers[fiber_idx];
                                            let frame = fiber.frames.last().unwrap();
                                            let mut chunk = frame.function.chunk.borrow_mut();
                                            chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id, index: idx });
                                        }
                                        Some(Rc::clone(func))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                method
                            };

                            if let Some(func) = method {
                                if arg_count != func.arity {
                                    self.throw_error(fiber_idx, &format!("Expected {} arguments but got {}.", func.arity, arg_count))?;
                                    continue;
                                } else {
                                    let mut args = Vec::with_capacity(arg_count);
                                    for _ in 0..arg_count {
                                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                                    }
                                    args.reverse();
                                    self.fibers[fiber_idx].stack.pop(); // Pop the receiver
                                    
                                    for arg in args { self.fibers[fiber_idx].stack.push(arg); }
                                    let frame = CallFrame {
                                        function: Rc::clone(&func),
                                        ip: 0,
                                        stack_base: self.fibers[fiber_idx].stack.len() - arg_count,
                                        receiver: None,
                                        handlers: Vec::new(),
                                    };
                                    self.fibers[fiber_idx].frames.push(frame);
                                }
                            } else {
                                self.throw_error(fiber_idx, &format!("Member {} not found or not callable.", name))?;
                                continue;
                            }
                        }
                        #[cfg(target_arch = "wasm32")]
                        BxValue::JsValue(js) => {
                            let prop = JsValue::from_str(&name);
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
                                                continue;
                                            }
                                            Err(e) => {
                                                self.throw_error(fiber_idx, &format!("JS Error: {:?}", e))?;
                                                continue;
                                            }
                                        }
                                    } else {
                                        self.throw_error(fiber_idx, &format!("Member {} is not a function", name))?;
                                        continue;
                                    }
                                }
                                Err(e) => {
                                    self.throw_error(fiber_idx, &format!("JS Error: {:?}", e))?;
                                    continue;
                                }
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
                                    continue;
                                }
                                Err(e) => {
                                    self.throw_error(fiber_idx, &e)?;
                                    continue;
                                }
                            }
                        }
                        #[cfg(feature = "jvm")]
                        BxValue::JavaObject(_) => {
                            self.throw_error(fiber_idx, "Java method invocation not yet implemented in this POC")?;
                            continue;
                        }
                        _ => { self.throw_error(fiber_idx, "Can only invoke methods on instances, structs, JS objects, and native objects.")?; continue; }
                    }
                }
                OpCode::OpCall(arg_count) => {
                    let func_val = self.fibers[fiber_idx].stack[self.fibers[fiber_idx].stack.len() - 1 - arg_count].clone();
                    match func_val {
                        BxValue::CompiledFunction(func) => {
                            if arg_count != func.arity {
                                self.throw_error(fiber_idx, &format!("Expected {} arguments but got {}.", func.arity, arg_count))?;
                                continue;
                            } else {
                                let frame = CallFrame {
                                    function: Rc::clone(&func),
                                    ip: 0,
                                    stack_base: self.fibers[fiber_idx].stack.len() - arg_count,
                                    receiver: self.fibers[fiber_idx].frames.last().unwrap().receiver.clone(),
                                    handlers: Vec::new(),
                                };
                                self.fibers[fiber_idx].frames.push(frame);
                            }
                        }
                        BxValue::NativeFunction(func) => {
                            let mut args = Vec::with_capacity(arg_count);
                            for _ in 0..arg_count {
                                args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                            }
                            args.reverse();
                            self.fibers[fiber_idx].stack.pop(); // Pop the function object
                            
                            match func(self, &args) {
                                Ok(val) => self.fibers[fiber_idx].stack.push(val),
                                Err(e) => {
                                    self.throw_error(fiber_idx, &e)?;
                                    continue;
                                }
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
                                    }
                                    Err(e) => {
                                        self.throw_error(fiber_idx, &format!("JS Error: {:?}", e))?;
                                        continue;
                                    }
                                }
                            } else {
                                self.throw_error(fiber_idx, "Can only call JS functions.")?;
                                continue;
                            }
                        }
                        _ => { self.throw_error(fiber_idx, "Can only call functions.")?; continue; }
                    }
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
                            BxValue::Array(arr) => {
                                let arr = arr.borrow();
                                if cursor_val < arr.len() {
                                    (false, Some(arr[cursor_val].clone()), Some(BxValue::Number(cursor_val as f64 + 1.0)))
                                } else {
                                    (true, None, None)
                                }
                            }
                            BxValue::Struct(s) => {
                                let s = s.borrow();
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
                        
                        let instance = Rc::new(RefCell::new(BxInstance {
                            class: Rc::clone(&class),
                            shape_id: self.shapes.get_root(),
                            properties: Vec::new(),
                            variables: variables_scope.clone(),
                        }));
                        
                        self.fibers[fiber_idx].stack[class_idx] = BxValue::Instance(Rc::clone(&instance));

                        let frame = CallFrame {
                            function: Rc::new(BxCompiledFunction {
                                name: format!("{}.constructor", class.borrow().name),
                                arity: 0,
                                chunk: Rc::new(RefCell::new(class.borrow().constructor.borrow().clone())),
                            }),
                            ip: 0,
                            stack_base: class_idx + 1 + arg_count,
                            receiver: Some(Rc::clone(&instance)),
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
                    let val = if let Some(ref receiver) = self.fibers[fiber_idx].frames.last().unwrap().receiver {
                        if name == "this" {
                            Some(BxValue::Instance(Rc::clone(receiver)))
                        } else if name == "variables" {
                            let _vars = Rc::clone(&receiver.borrow().variables);
                            Some(BxValue::Struct(Rc::new(RefCell::new(BxStruct {
                                shape_id: self.shapes.get_root(), 
                                properties: Vec::new(),
                            }))))
                        } else {
                            let val = receiver.borrow().variables.borrow().get(&name).cloned().unwrap_or(BxValue::Null);
                            Some(val)
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
                    if let Some(ref receiver) = self.fibers[fiber_idx].frames.last().unwrap().receiver {
                        receiver.borrow().variables.borrow_mut().insert(name, val);
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
        if !self.fibers[fiber_idx].frames.is_empty() {
            let frame = self.fibers[fiber_idx].frames.last().unwrap();
            let chunk = frame.function.chunk.borrow();
            filename = chunk.filename.clone();
            if frame.ip > 0 && frame.ip <= chunk.lines.len() {
                line = chunk.lines[frame.ip - 1];
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
        bail!("VM Runtime Error: {} (at {} line {})", val, filename, line);
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
    fn bx_to_js(&self, val: &BxValue) -> JsValue {
        match val {
            BxValue::String(s) => JsValue::from_str(s),
            BxValue::Number(n) => JsValue::from_f64(*n),
            BxValue::Boolean(b) => JsValue::from_bool(*b),
            BxValue::Null => JsValue::NULL,
            BxValue::Array(arr) => {
                let js_arr = Array::new();
                for item in arr.borrow().iter() {
                    js_arr.push(&self.bx_to_js(item));
                }
                js_arr.into()
            }
            BxValue::Struct(s) => {
                let js_obj = js_sys::Object::new();
                let s_borrow = s.borrow();
                let shape = &self.shapes.shapes[s_borrow.shape_id];
                for (k, &idx) in shape.fields.iter() {
                    Reflect::set(&js_obj, &JsValue::from_str(k), &self.bx_to_js(&s_borrow.properties[idx])).ok();
                }
                js_obj.into()
            }
            BxValue::JsValue(js) => js.clone(),
            _ => JsValue::UNDEFINED,
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn js_to_bx(&self, val: JsValue) -> BxValue {
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
            BxValue::Array(Rc::new(RefCell::new(bx_arr)))
        } else if val.is_instance_of::<js_sys::Promise>() {
            let promise: js_sys::Promise = val.into();
            let future = Rc::new(RefCell::new(BxFuture {
                value: BxValue::Null,
                status: FutureStatus::Pending,
            }));
            
            let f_clone = Rc::clone(&future);
            let on_success = Closure::wrap(Box::new(move |val: JsValue| {
                let mut f = f_clone.borrow_mut();
                f.value = BxValue::JsValue(val);
                f.status = FutureStatus::Completed;
            }) as Box<dyn FnMut(JsValue)>);

            let f_clone_err = Rc::clone(&future);
            let on_error = Closure::wrap(Box::new(move |err: JsValue| {
                let mut f = f_clone_err.borrow_mut();
                f.status = FutureStatus::Failed(format!("{:?}", err));
            }) as Box<dyn FnMut(JsValue)>);

            let _ = promise.then2(&on_success, &on_error);
            on_success.forget();
            on_error.forget();

            BxValue::Future(future)
        } else {
            BxValue::JsValue(val)
        }
    }
}
