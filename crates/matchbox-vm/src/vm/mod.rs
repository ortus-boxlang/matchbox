pub mod chunk;
pub mod opcode;
pub mod gc;
pub mod shape;
pub mod intern;
#[cfg(feature = "jit")]
pub mod jit;

#[cfg(all(test, target_arch = "wasm32", feature = "js"))]
mod interop_tests;

use crate::types::{BxValue, BxCompiledFunction, BxClass, BxInstance, BxFuture, FutureStatus, Constant, BxVM, BxStruct, BxNativeObject, BxNativeFunction, NativeFutureHandle, NativeFutureMessage, NativeFutureValue, Tracer, box_string::BoxString};
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use crate::types::take_wasm_future_thunk;
use self::chunk::{Chunk, IcEntry};
use self::opcode::op;
use self::gc::{Heap, GcObject};
use self::shape::ShapeRegistry;
use self::intern::StringInterner;
use anyhow::{Result, bail};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Instant, Duration};
use std::sync::atomic::{AtomicBool, Ordering};
use std::vec;

pub static INTERRUPT_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct VariablesScopeProxy {
    variables: Rc<RefCell<HashMap<String, BxValue>>>,
}

impl BxNativeObject for VariablesScopeProxy {
    fn get_property(&self, name: &str) -> BxValue {
        self.variables
            .borrow()
            .get(&name.to_lowercase())
            .copied()
            .unwrap_or(BxValue::new_null())
    }

    fn set_property(&mut self, name: &str, value: BxValue) {
        self.variables.borrow_mut().insert(name.to_lowercase(), value);
    }

    fn call_method(&mut self, _vm: &mut dyn BxVM, _id: usize, name: &str, _args: &[BxValue]) -> Result<BxValue, String> {
        Err(format!("Method {} not found on variables scope.", name))
    }

    fn trace(&self, tracer: &mut dyn Tracer) {
        for value in self.variables.borrow().values() {
            tracer.mark(value);
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "js"))]
use wasm_bindgen::prelude::*;
#[cfg(all(target_arch = "wasm32", feature = "js"))]
use js_sys::{Array, Function, Reflect};

#[cfg(all(target_arch = "wasm32", not(feature = "js")))]
#[link(wasm_import_module = "matchbox_js_host")]
unsafe extern "C" {
    fn bx_js_get_prop(
        obj_id: u32, key_ptr: *const u8, key_len: usize,
        str_buf: *mut u8, str_buf_len: usize, out_str_len: *mut usize,
        out_num: *mut f64, out_bool: *mut i32, out_obj: *mut u32,
    ) -> i32;
    fn bx_js_set_prop_null(obj_id: u32, key_ptr: *const u8, key_len: usize);
    fn bx_js_set_prop_bool(obj_id: u32, key_ptr: *const u8, key_len: usize, val: i32);
    fn bx_js_set_prop_num(obj_id: u32, key_ptr: *const u8, key_len: usize, val: f64);
    fn bx_js_set_prop_str(obj_id: u32, key_ptr: *const u8, key_len: usize, val_ptr: *const u8, val_len: usize);
    fn bx_js_set_prop_obj(obj_id: u32, key_ptr: *const u8, key_len: usize, val_id: u32);
    fn bx_js_call_method(
        obj_id: u32, method_ptr: *const u8, method_len: usize,
        args_json_ptr: *const u8, args_json_len: usize,
        str_buf: *mut u8, str_buf_len: usize, out_str_len: *mut usize,
        out_num: *mut f64, out_bool: *mut i32, out_obj: *mut u32,
    ) -> i32;
}

pub struct CallFrame {
    pub function: Rc<BxCompiledFunction>,
    pub chunk: Rc<RefCell<crate::vm::chunk::Chunk>>,
    pub ip: usize,
    pub stack_base: usize,
    pub receiver: Option<BxValue>,
    pub handlers: Vec<usize>,
    pub promoted_constants: Vec<Option<BxValue>>,
}

pub struct BxFiber {
    pub stack: Vec<BxValue>,
    pub frames: Vec<CallFrame>,
    pub future_id: usize,
    pub wait_until: Option<Instant>,
    pub yield_requested: bool,
    pub priority: u8,
    pub root_stack: Vec<BxValue>,
}

enum NativeCompletion {
    Resolve { future: BxValue, value: BxValue },
    Reject { future: BxValue, error: BxValue },
}

#[derive(Clone, Copy, Debug)]
pub enum HostFutureState {
    Pending,
    Completed(BxValue),
    Failed(BxValue),
}

pub struct VM {
    pub fibers: Vec<BxFiber>,
    pub global_names: HashMap<u32, usize>,
    pub global_values: Vec<BxValue>,
    pub current_fiber_idx: Option<usize>,
    pub shapes: ShapeRegistry,
    pub heap: Heap,
    pub native_classes: HashMap<String, BxNativeFunction>,
    pub interner: StringInterner,
    pub cli_args: Vec<String>,
    pub output_buffer: Option<String>,
    pub gc_suspended: bool,
    native_completions: VecDeque<NativeCompletion>,
    native_future_tx: Sender<NativeFutureMessage>,
    native_future_rx: Receiver<NativeFutureMessage>,
    pending_native_futures: HashMap<usize, usize>,
    #[cfg(feature = "jit")]
    pub jit: Option<Box<jit::JitState>>,
}

impl BxVM for VM {
    fn current_chunk(&self) -> Option<Rc<RefCell<crate::vm::chunk::Chunk>>> {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].frames.last().map(|f| Rc::clone(&f.chunk))
        } else {
            None
        }
    }

    fn interpret_chunk(&mut self, chunk: Chunk) -> Result<BxValue, String> {
        // Legacy consuming execution path. Keep this behavior intact so the
        // main VM can migrate to the borrowed path incrementally later.
        self.interpret(chunk).map_err(|e| e.to_string())
    }

    fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>, priority: u8, _chunk: Rc<RefCell<crate::vm::chunk::Chunk>>) -> BxValue {
        let dummy = Rc::new(RefCell::new(Chunk::default()));
        self.spawn(func, args, priority, dummy)
    }

    fn spawn_by_value(&mut self, func: &BxValue, args: Vec<BxValue>, priority: u8, _chunk: Rc<RefCell<crate::vm::chunk::Chunk>>) -> Result<BxValue, String> {
        if let Some(id) = func.as_gc_id() {
            let obj = self.heap.get(id);
            if let GcObject::CompiledFunction(f) = obj {
                let f = Rc::clone(f);
                let dummy = Rc::new(RefCell::new(Chunk::default()));
                Ok(self.spawn(f, args, priority, dummy))
            } else {
                Err("Value is not a callable function".to_string())
            }
        } else {
            Err("Value is not a callable function".to_string())
        }
    }

    fn call_function_by_value(&mut self, func: &BxValue, args: Vec<BxValue>, chunk: Rc<RefCell<crate::vm::chunk::Chunk>>) -> Result<BxValue, String> {
        self.call_function_value(*func, args, Some(chunk)).map_err(|e| e.to_string())
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
        if let Some(id) = self.interner.get_id(field_name) {
            self.shapes.get_index(shape_id, id)
        } else {
            None
        }
    }

    fn get_len(&self, id: usize) -> usize {
        match self.heap.get(id) {
            GcObject::Array(arr) => arr.len(),
            GcObject::Struct(s) => s.properties.len(),
            GcObject::String(s) => s.len(),
            GcObject::Bytes(bytes) => bytes.len(),
            _ => 0,
        }
    }

    fn is_array_value(&self, val: BxValue) -> bool {
        val.as_gc_id()
            .map(|id| matches!(self.heap.get(id), GcObject::Array(_)))
            .unwrap_or(false)
    }

    fn is_struct_value(&self, val: BxValue) -> bool {
        val.as_gc_id()
            .map(|id| matches!(self.heap.get(id), GcObject::Struct(_)))
            .unwrap_or(false)
    }

    fn is_string_value(&self, val: BxValue) -> bool {
        val.as_gc_id()
            .map(|id| matches!(self.heap.get(id), GcObject::String(_)))
            .unwrap_or(false)
    }

    fn is_bytes(&self, val: BxValue) -> bool {
        if let Some(id) = val.as_gc_id() {
            matches!(self.heap.get(id), GcObject::Bytes(_))
        } else {
            false
        }
    }

    fn bytes_new(&mut self, data: Vec<u8>) -> usize {
        self.heap.alloc(GcObject::Bytes(data))
    }

    fn bytes_len(&self, id: usize) -> usize {
        if let GcObject::Bytes(bytes) = self.heap.get(id) {
            bytes.len()
        } else {
            0
        }
    }

    fn bytes_get(&self, id: usize, idx: usize) -> Result<u8, String> {
        if let GcObject::Bytes(bytes) = self.heap.get(id) {
            bytes
                .get(idx)
                .copied()
                .ok_or_else(|| format!("Index {} out of bounds", idx))
        } else {
            Err("Not bytes".to_string())
        }
    }

    fn bytes_set(&mut self, id: usize, idx: usize, value: u8) -> Result<(), String> {
        if let GcObject::Bytes(bytes) = self.heap.get_mut(id) {
            if idx < bytes.len() {
                bytes[idx] = value;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", idx))
            }
        } else {
            Err("Not bytes".to_string())
        }
    }

    fn to_bytes(&self, val: BxValue) -> Result<Vec<u8>, String> {
        if let Some(id) = val.as_gc_id() {
            if let GcObject::Bytes(bytes) = self.heap.get(id) {
                return Ok(bytes.clone());
            }
        }
        Err("Value is not bytes".to_string())
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

    fn array_pop(&mut self, id: usize) -> Result<BxValue, String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            Ok(arr.pop().unwrap_or(BxValue::new_null()))
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_get(&self, id: usize, idx: usize) -> BxValue {
        if let GcObject::Array(arr) = self.heap.get(id) {
            arr.get(idx).copied().unwrap_or(BxValue::new_null())
        } else { BxValue::new_null() }
    }

    fn array_set(&mut self, id: usize, idx: usize, val: BxValue) -> Result<(), String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            if idx < arr.len() {
                arr[idx] = val;
                Ok(())
            } else if idx < 100_000 { // Reasonable limit for sparse expansion
                arr.resize(idx + 1, BxValue::new_null());
                arr[idx] = val;
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", idx))
            }
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_delete_at(&mut self, id: usize, idx: usize) -> Result<BxValue, String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            if idx < arr.len() {
                Ok(arr.remove(idx))
            } else {
                Err(format!("Index {} out of bounds", idx))
            }
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_insert_at(&mut self, id: usize, idx: usize, val: BxValue) -> Result<(), String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            if idx <= arr.len() {
                arr.insert(idx, val);
                Ok(())
            } else if idx < 100_000 {
                arr.resize(idx, BxValue::new_null());
                arr.push(val);
                Ok(())
            } else {
                Err(format!("Index {} out of bounds", idx))
            }
        } else {
            Err("Not an array".to_string())
        }
    }

    fn array_clear(&mut self, id: usize) -> Result<(), String> {
        if let GcObject::Array(arr) = self.heap.get_mut(id) {
            arr.clear();
            Ok(())
        } else {
            Err("Not an array".to_string())
        }
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

    fn struct_set(&mut self, id: usize, key: &str, val: BxValue) {
        let key_id = self.interner.intern(key);
        if let GcObject::Struct(s) = self.heap.get_mut(id) {
            if let Some(idx) = self.shapes.get_index(s.shape_id, key_id) {
                s.properties[idx as usize] = val;
            } else {
                s.shape_id = self.shapes.transition(s.shape_id, key_id);
                s.properties.push(val);
            }
        }
    }

    fn struct_get(&self, id: usize, key: &str) -> BxValue {
        let key_id = self.interner.get_id(key).unwrap_or(u32::MAX);
        if let GcObject::Struct(s) = self.heap.get(id) {
            if let Some(idx) = self.shapes.get_index(s.shape_id, key_id) {
                return s.properties[idx as usize];
            }
        }
        BxValue::new_null()
    }

    fn struct_delete(&mut self, id: usize, key: &str) -> bool {
        let key_id = self.interner.get_id(key).unwrap_or(u32::MAX);
        if let GcObject::Struct(s) = self.heap.get_mut(id) {
            if let Some(idx) = self.shapes.get_index(s.shape_id, key_id) {
                // To delete from a shape-based struct, we must reconstruct the struct's state
                // minus the deleted field and find/create a new shape.
                let mut entries = Vec::new();
                let current_shape = &self.shapes.shapes[s.shape_id as usize];
                for (&fid, &fidx) in &current_shape.fields {
                    if fid != key_id {
                        entries.push((fid, s.properties[fidx as usize]));
                    }
                }
                
                // Sort by index to maintain some consistency if possible, 
                // but really we just want a shape that has these fields.
                // For simplicity, we'll build a new shape chain from root.
                let mut new_shape_id = self.shapes.get_root();
                let mut new_properties = Vec::with_capacity(entries.len());
                for (fid, val) in entries {
                    new_shape_id = self.shapes.transition(new_shape_id, fid);
                    new_properties.push(val);
                }
                
                s.shape_id = new_shape_id;
                s.properties = new_properties;
                return true;
            }
        }
        false
    }

    fn struct_key_exists(&self, id: usize, key: &str) -> bool {
        let key_id = self.interner.get_id(key).unwrap_or(u32::MAX);
        if let GcObject::Struct(s) = self.heap.get(id) {
            return self.shapes.get_index(s.shape_id, key_id).is_some();
        }
        false
    }

    fn struct_key_array(&self, id: usize) -> Vec<String> {
        if let GcObject::Struct(s) = self.heap.get(id) {
            let shape = &self.shapes.shapes[s.shape_id as usize];
            let mut keys = vec![String::new(); shape.fields.len()];
            for (&fid, &fidx) in &shape.fields {
                keys[fidx as usize] = self.interner.resolve(fid).to_string();
            }
            return keys;
        }
        Vec::new()
    }

    fn struct_clear(&mut self, id: usize) {
        if let GcObject::Struct(s) = self.heap.get_mut(id) {
            s.shape_id = self.shapes.get_root();
            s.properties.clear();
        }
    }

    fn struct_get_shape(&self, id: usize) -> u32 {
        if let GcObject::Struct(s) = self.heap.get(id) {
            s.shape_id
        } else { 0 }
    }

    fn future_new(&mut self) -> BxValue {
        BxValue::new_ptr(self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
        })))
    }

    fn future_resolve(&mut self, future: BxValue, value: BxValue) -> Result<(), String> {
        let id = future.as_gc_id().ok_or_else(|| "Value is not a future".to_string())?;
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            if !matches!(f.status, FutureStatus::Pending) {
                return Err("Future is already settled".to_string());
            }
            f.value = value;
            f.status = FutureStatus::Completed;
            Ok(())
        } else {
            Err("Value is not a future".to_string())
        }
    }

    fn future_reject(&mut self, future: BxValue, error: BxValue) -> Result<(), String> {
        let id = future.as_gc_id().ok_or_else(|| "Value is not a future".to_string())?;
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            if !matches!(f.status, FutureStatus::Pending) {
                return Err("Future is already settled".to_string());
            }
            f.status = FutureStatus::Failed(error);
            Ok(())
        } else {
            Err("Value is not a future".to_string())
        }
    }

    fn future_schedule_resolve(&mut self, future: BxValue, value: BxValue) -> Result<(), String> {
        let id = future.as_gc_id().ok_or_else(|| "Value is not a future".to_string())?;
        if matches!(self.heap.get(id), GcObject::Future(_)) {
            self.native_completions.push_back(NativeCompletion::Resolve { future, value });
            Ok(())
        } else {
            Err("Value is not a future".to_string())
        }
    }

    fn future_schedule_reject(&mut self, future: BxValue, error: BxValue) -> Result<(), String> {
        let id = future.as_gc_id().ok_or_else(|| "Value is not a future".to_string())?;
        if matches!(self.heap.get(id), GcObject::Future(_)) {
            self.native_completions.push_back(NativeCompletion::Reject { future, error });
            Ok(())
        } else {
            Err("Value is not a future".to_string())
        }
    }

    fn native_future_new(&mut self) -> NativeFutureHandle {
        let future = self.future_new();
        if let Some(id) = future.as_gc_id() {
            *self.pending_native_futures.entry(id).or_insert(0) += 1;
        }
        NativeFutureHandle::new(future, self.native_future_tx.clone())
    }

    fn future_on_error(&mut self, id: usize, handler: BxValue) {
        if let GcObject::Future(f) = self.heap.get_mut(id) {
            f.error_handler = Some(handler);
        }
    }

    fn native_object_new(&mut self, obj: Rc<RefCell<dyn BxNativeObject>>) -> usize {
        self.heap.alloc(GcObject::NativeObject(obj))
    }

    fn native_object_call_method(&mut self, id: usize, name: &str, args: &[BxValue]) -> Result<BxValue, String> {
        self.gc_suspended = true;
        // Clone the Rc to release the heap borrow immediately
        let obj_rc = if let GcObject::NativeObject(obj) = self.heap.get_mut(id) {
            Rc::clone(obj)
        } else {
            self.gc_suspended = false;
            return Err(format!("Value at id {} is not a native object", id));
        };
        
        let res = obj_rc.borrow_mut().call_method(self, id, name, args);
        self.gc_suspended = false;
        res
    }

    fn construct_native_class(&mut self, class_name: &str, args: &[BxValue]) -> Result<BxValue, String> {
        let class_lower = class_name.to_lowercase();
        // Since we need to borrow `self` mutably in the function call, we must clone the function pointer first
        let func = {
            self.native_classes.get(&class_lower).copied()
        };

        if let Some(constructor) = func {
            constructor(self, args)
        } else {
            Err(format!("Native class '{}' not found. Ensure it is registered.", class_name))
        }
    }

    fn instance_class_name(&self, receiver: BxValue) -> Result<String, String> {
        self.instance_class_name(receiver).map_err(|e| e.to_string())
    }

    fn instance_variables_json(&self, receiver: BxValue) -> Result<serde_json::Value, String> {
        self.instance_variables_json(receiver).map_err(|e| e.to_string())
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

    fn insert_global(&mut self, name: String, val: BxValue) {
        VM::insert_global(self, name, val);
    }

    fn get_cli_args(&self) -> Vec<String> {
        self.cli_args.clone()
    }

    fn write_output(&mut self, s: &str) {
        if let Some(ref mut buffer) = self.output_buffer {
            buffer.push_str(s);
        } else {
            print!("{}", s);
        }
    }

    fn begin_output_capture(&mut self) {
        self.output_buffer = Some(String::new());
    }

    fn end_output_capture(&mut self) -> Option<String> {
        self.output_buffer.take()
    }

    fn suspend_gc(&mut self) {
        self.gc_suspended = true;
    }

    fn resume_gc(&mut self) {
        self.gc_suspended = false;
    }

    fn push_root(&mut self, val: BxValue) {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].root_stack.push(val);
        }
    }

    fn pop_root(&mut self) {
        if let Some(idx) = self.current_fiber_idx {
            self.fibers[idx].root_stack.pop();
        }
    }
}

impl VM {
    pub fn interpret_sync(&mut self, mut chunk: Chunk) -> Result<BxValue> {
        chunk.ensure_caches();
        let chunk_for_func = chunk.clone();
        let function = Rc::new(BxCompiledFunction {
            name: "script".to_string(),
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            chunk: chunk_for_func,
        });

        let future = self.spawn(function, Vec::new(), 0, Rc::new(RefCell::new(Chunk::default())));
        self.run_future_to_completion(future)
    }

    fn enqueue_function_call(
        &mut self,
        func: BxValue,
        function: Rc<BxCompiledFunction>,
        args: Vec<BxValue>,
        priority: u8,
    ) -> BxValue {
        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
        }));

        let mut stack = Vec::with_capacity(function.arity as usize + 1);
        stack.push(func);
        for arg in args {
            stack.push(arg);
        }
        while stack.len() < (function.arity + 1) as usize {
            stack.push(BxValue::new_null());
        }

        let chunk = Rc::new(RefCell::new(function.chunk.clone()));

        let fiber = BxFiber {
            stack,
            frames: vec![CallFrame {
                function,
                chunk,
                ip: 0,
                stack_base: 1,
                receiver: None,
                handlers: Vec::new(),
                promoted_constants: Vec::new(),
            }],
            future_id,
            wait_until: None,
            yield_requested: false,
            priority,
            root_stack: Vec::new(),
        };

        self.fibers.push(fiber);
        BxValue::new_ptr(future_id)
    }

    fn native_future_value_to_bx(&mut self, value: NativeFutureValue) -> BxValue {
        match value {
            NativeFutureValue::Null => BxValue::new_null(),
            NativeFutureValue::Bool(v) => BxValue::new_bool(v),
            NativeFutureValue::Int(v) => BxValue::new_int(v),
            NativeFutureValue::Number(v) => BxValue::new_number(v),
            NativeFutureValue::String(v) => BxValue::new_ptr(self.string_new(v)),
            NativeFutureValue::Bytes(v) => BxValue::new_ptr(self.bytes_new(v)),
            NativeFutureValue::Error { message } => {
                let struct_id = self.struct_new();
                let message_id = self.string_new(message);
                self.struct_set(struct_id, "message", BxValue::new_ptr(message_id));
                BxValue::new_ptr(struct_id)
            }
        }
    }

    fn release_pending_native_future(&mut self, future: BxValue) {
        if let Some(id) = future.as_gc_id() {
            if let Some(count) = self.pending_native_futures.get_mut(&id) {
                if *count <= 1 {
                    self.pending_native_futures.remove(&id);
                } else {
                    *count -= 1;
                }
            }
        }
    }

    fn drain_native_completions(&mut self) {
        while let Ok(message) = self.native_future_rx.try_recv() {
            match message {
                NativeFutureMessage::Resolve { future, value } => {
                    let value = self.native_future_value_to_bx(value);
                    let _ = self.future_resolve(future, value);
                    self.release_pending_native_future(future);
                }
                NativeFutureMessage::Reject { future, error } => {
                    let error = self.native_future_value_to_bx(error);
                    let _ = self.future_reject(future, error);
                    self.release_pending_native_future(future);
                }
                #[cfg(all(target_arch = "wasm32", feature = "js"))]
                NativeFutureMessage::ResolveWasmThunk { future, thunk_id } => {
                    let result = take_wasm_future_thunk(thunk_id)
                        .ok_or_else(|| "WASM future thunk not found".to_string())
                        .and_then(|thunk| thunk(self));

                    match result {
                        Ok(value) => {
                            let _ = self.future_resolve(future, value);
                        }
                        Err(message) => {
                            let error = self.native_future_value_to_bx(NativeFutureValue::Error { message });
                            let _ = self.future_reject(future, error);
                        }
                    }
                    self.release_pending_native_future(future);
                }
                NativeFutureMessage::Abandon { future } => {
                    self.release_pending_native_future(future);
                }
            }
        }
        while let Some(completion) = self.native_completions.pop_front() {
            match completion {
                NativeCompletion::Resolve { future, value } => {
                    let _ = self.future_resolve(future, value);
                }
                NativeCompletion::Reject { future, error } => {
                    let _ = self.future_reject(future, error);
                }
            }
        }
    }

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
                GcObject::Bytes(bytes) => format!("<bytes len:{}>", bytes.len()),
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
                #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
                GcObject::JsHandle(h) => format!("<js object #{}>", h),
            }
        } else {
            "<invalid>".to_string()
        }
    }

    fn is_equal(&self, a: BxValue, b: BxValue) -> bool {
        if a == b { return true; }
        if let (Some(id_a), Some(id_b)) = (a.as_gc_id(), b.as_gc_id()) {
            match (self.heap.get(id_a), self.heap.get(id_b)) {
                (GcObject::String(s1), GcObject::String(s2)) => {
                    s1.to_string().to_lowercase() == s2.to_string().to_lowercase()
                }
                (GcObject::Bytes(a), GcObject::Bytes(b)) => a == b,
                _ => false,
            }
        } else {
            false
        }
    }

    pub fn new() -> Self {
        Self::new_with_bifs(HashMap::new(), HashMap::new())
    }

    pub fn new_with_args(args: Vec<String>) -> Self {
        let mut vm = Self::new();
        vm.cli_args = args;
        vm
    }

    pub fn new_with_bifs(external_bifs: HashMap<String, BxNativeFunction>, native_classes: HashMap<String, BxNativeFunction>) -> Self {
        let (native_future_tx, native_future_rx) = mpsc::channel();
        let mut vm = VM {
            fibers: Vec::new(),
            global_names: HashMap::new(),
            global_values: Vec::new(),
            current_fiber_idx: None,
            shapes: ShapeRegistry::new(),
            heap: Heap::new(),
            native_classes: native_classes.into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect(),
            interner: StringInterner::new(),
            cli_args: Vec::new(),
            output_buffer: None,
            gc_suspended: false,
            native_completions: VecDeque::new(),
            native_future_tx,
            native_future_rx,
            pending_native_futures: HashMap::new(),
            #[cfg(feature = "jit")]
            jit: None,
        };

        #[cfg(all(target_arch = "wasm32", feature = "js"))]
        {
            if let Some(window) = web_sys::window() {
                let id = vm.heap.alloc(GcObject::JsValue(window.into()));
                vm.insert_global("js".to_string(), BxValue::new_ptr(id));
            }
        }
        #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
        {
            // WASI build: register `js` as handle 1 (window) for browser JS interop
            let id = vm.heap.alloc(GcObject::JsHandle(1));
            vm.insert_global("js".to_string(), BxValue::new_ptr(id));
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

        // Initialize 'server' scope
        vm.init_server_scope();

        vm
    }

    /// Activate the Cranelift JIT. Call this before `interpret` to enable
    /// hot-loop compilation. No-op (compile error) without the `jit` feature.
    #[cfg(feature = "jit")]
    pub fn enable_jit(&mut self) {
        match jit::JitState::new() {
            Ok(state) => self.jit = Some(Box::new(state)),
            Err(e) => eprintln!("[JIT] init failed: {}", e),
        }
    }

    fn init_server_scope(&mut self) {
        use crate::types::BxStruct;
        
        let mut os_struct = BxStruct {
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
        };

        let os_name = if cfg!(target_os = "espidf") {
            "FreeRTOS"
        } else if cfg!(target_os = "windows") {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "macOS"
        } else if cfg!(target_os = "linux") {
            "Linux"
        } else if cfg!(target_arch = "wasm32") {
            "WebAssembly"
        } else {
            "Unknown"
        };

        let os_arch = if cfg!(target_arch = "xtensa") {
            "xtensa"
        } else if cfg!(target_arch = "riscv32") {
            "riscv32"
        } else if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else if cfg!(target_arch = "wasm32") {
            "wasm32"
        } else {
            "unknown"
        };

        let os_name_id = self.heap.alloc(GcObject::String(BoxString::new(os_name)));
        let os_arch_id = self.heap.alloc(GcObject::String(BoxString::new(os_arch)));

        // Manual struct property insertion (since we don't have BxStruct::set yet)
        let name_idx = self.interner.intern("name");
        let arch_idx = self.interner.intern("arch");
        
        os_struct.shape_id = self.shapes.transition(os_struct.shape_id, name_idx);
        os_struct.properties.push(BxValue::new_ptr(os_name_id));
        
        os_struct.shape_id = self.shapes.transition(os_struct.shape_id, arch_idx);
        os_struct.properties.push(BxValue::new_ptr(os_arch_id));

        let os_ptr = self.heap.alloc(GcObject::Struct(os_struct));

        let mut server_struct = BxStruct {
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
        };
        let os_key_idx = self.interner.intern("os");
        server_struct.shape_id = self.shapes.transition(server_struct.shape_id, os_key_idx);
        server_struct.properties.push(BxValue::new_ptr(os_ptr));

        let server_ptr = self.heap.alloc(GcObject::Struct(server_struct));
        self.insert_global("server".to_string(), BxValue::new_ptr(server_ptr));
    }

    pub fn insert_global(&mut self, name: String, val: BxValue) {
        let name_lower = name.to_lowercase();
        let name_id = self.interner.intern(&name_lower);
        self.insert_global_interned(name_id, val);
    }

    fn insert_global_interned(&mut self, name_id: u32, val: BxValue) {
        if let Some(&idx) = self.global_names.get(&name_id) {
            self.global_values[idx] = val;
        } else {
            let idx = self.global_values.len();
            self.global_names.insert(name_id, idx);
            self.global_values.push(val);
        }
    }

    pub fn get_global(&self, name: &str) -> Option<BxValue> {
        let name_lower = name.to_lowercase();
        if let Some(name_id) = self.interner.get_id(&name_lower) {
            self.global_names.get(&name_id).map(|&idx| self.global_values[idx])
        } else {
            None
        }
    }


    fn resolve_member_method(&self, receiver: &BxValue, method_name: &str) -> Option<String> {
        let name = method_name;
        if receiver.is_number() {
            return match name {
                "abs" => Some("abs".to_string()),
                "round" => Some("round".to_string()),
                _ => None,
            };
        }

        if let Some(id) = receiver.as_gc_id() {
            match self.heap.get(id) {
                GcObject::String(_) => match name {
                    "len" | "length" => Some("len".to_string()),
                    "ucase" | "touppercase" => Some("ucase".to_string()),
                    "lcase" | "tolowercase" => Some("lcase".to_string()),
                    "split" => Some("listtoarray".to_string()),
                    "indexof" => Some("indexof".to_string()),
                    _ => None,
                },
                GcObject::Array(_) => match name {
                    "len" | "length" | "count" => Some("len".to_string()),
                    "append" | "add" => Some("arrayappend".to_string()),
                    "each" => Some("arrayeach".to_string()),
                    "map" => Some("arraymap".to_string()),
                    "reduce" => Some("arrayreduce".to_string()),
                    "filter" => Some("arrayfilter".to_string()),
                    "tolist" => Some("arraytolist".to_string()),
                    _ => None,
                },
                GcObject::Struct(_) => match name {
                    "len" | "count" => Some("len".to_string()),
                    "exists" | "keyexists" => Some("structkeyexists".to_string()),
                    "each" => Some("structeach".to_string()),
                    _ => None,
                },
                GcObject::Future(_) => match name {
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
            if let Some((_, method)) = class_ref.methods.iter().find(|(name, _)| name == method_name) {
                return Some(Rc::new(method.clone()));
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
        // Legacy consuming execution path. This still clones the chunk into a
        // per-run Rc/RefCell wrapper and is kept as-is for existing callers.
        chunk.ensure_caches();
        let chunk_for_func = chunk.clone();
        let chunk_rc = Rc::new(RefCell::new(chunk));
        self.interpret_chunk_shared(chunk_for_func, chunk_rc)
    }

    pub fn interpret_chunk_borrowed(&mut self, chunk: &Chunk) -> Result<BxValue> {
        // New borrowed execution path used by the ESP32 runner to avoid
        // cloning the entire route chunk on every request.
        let mut chunk_for_func = chunk.clone();
        chunk_for_func.ensure_caches();
        let mut owned_chunk = chunk_for_func.clone();
        owned_chunk.ensure_caches();
        let chunk_rc = Rc::new(RefCell::new(owned_chunk));
        self.interpret_chunk_shared(chunk_for_func, chunk_rc)
    }

    fn interpret_chunk_shared(
        &mut self,
        chunk_for_func: Chunk,
        chunk_rc: Rc<RefCell<Chunk>>,
    ) -> Result<BxValue> {
        let function = Rc::new(BxCompiledFunction {
            name: "script".to_string(),
            arity: 0,
            min_arity: 0,
            params: Vec::new(),
            chunk: chunk_for_func,
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
                chunk: chunk_rc,
                ip: 0,
                stack_base: 0,
                receiver: None,
                handlers: Vec::new(),
                promoted_constants: Vec::new(),
            }],

            future_id,
            wait_until: None,
            yield_requested: false,
            priority: 0,
            root_stack: Vec::new(),
        };

        self.fibers.push(fiber);
        let res = self.run_all();
        self.current_fiber_idx = None;
        res
    }

    pub fn start_call_function_value(&mut self, func: BxValue, args: Vec<BxValue>) -> Result<BxValue> {
        if let Some(id) = func.as_gc_id() {
            match self.heap.get(id) {
                GcObject::CompiledFunction(f) => {
                    let f = Rc::clone(f);
                    if args.len() < f.min_arity as usize || args.len() > f.arity as usize {
                        anyhow::bail!("Expected {}-{} arguments but got {}", f.min_arity, f.arity, args.len());
                    }
                    Ok(self.enqueue_function_call(func, f, args, 0))
                }
                GcObject::NativeFunction(f) => {
                    let f = *f;
                    self.gc_suspended = true;
                    let res = f(self, &args).map_err(|e| anyhow::anyhow!(e));
                    self.gc_suspended = false;
                    let future = self.future_new();
                    match res {
                        Ok(value) => {
                            let _ = self.future_resolve(future, value);
                            Ok(future)
                        }
                        Err(err) => {
                            let error_id = self.string_new(err.to_string());
                            let _ = self.future_reject(future, BxValue::new_ptr(error_id));
                            Ok(future)
                        }
                    }
                }
                _ => anyhow::bail!("Value is not a callable function"),
            }
        } else {
            anyhow::bail!("Value is not a callable function")
        }
    }

    pub fn pump_until_blocked(&mut self) -> Result<()> {
        self.drain_native_completions();
        let mut i = 0;

        while i < self.fibers.len() {
            self.current_fiber_idx = Some(i);
            match self.run_fiber(i, None) {
                Ok(Some(result)) => {
                    let fiber = self.fibers.swap_remove(i);
                    if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                        f.value = result;
                        f.status = FutureStatus::Completed;
                    }
                }
                Ok(None) => {
                    i += 1;
                }
                Err(err) => {
                    let err_str = err.to_string();
                    let err_id = self.string_new(err_str);
                    let err_val = BxValue::new_ptr(err_id);
                    let fiber = self.fibers.swap_remove(i);
                    if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                        f.status = FutureStatus::Failed(err_val);
                    }
                }
            }
        }

        self.current_fiber_idx = None;
        Ok(())
    }

    pub fn future_state(&self, future: BxValue) -> Result<HostFutureState> {
        let id = future.as_gc_id().ok_or_else(|| anyhow::anyhow!("Value is not a future"))?;
        match self.heap.get(id) {
            GcObject::Future(f) => match &f.status {
                FutureStatus::Pending => Ok(HostFutureState::Pending),
                FutureStatus::Completed => Ok(HostFutureState::Completed(f.value)),
                FutureStatus::Failed(error) => Ok(HostFutureState::Failed(*error)),
            },
            _ => anyhow::bail!("Value is not a future"),
        }
    }

    pub fn run_future_to_completion(&mut self, future: BxValue) -> Result<BxValue> {
        loop {
            self.pump_until_blocked()?;
            match self.future_state(future)? {
                HostFutureState::Pending => continue,
                HostFutureState::Completed(value) => return Ok(value),
                HostFutureState::Failed(err) => {
                    let message = self.to_string(err);
                    anyhow::bail!("{}", message);
                }
            }
        }
    }

    pub fn spawn(&mut self, func: Rc<BxCompiledFunction>, args: Vec<BxValue>, priority: u8, _chunk: Rc<RefCell<crate::vm::chunk::Chunk>>) -> BxValue {
        let future_id = self.heap.alloc(GcObject::Future(BxFuture {
            value: BxValue::new_null(),
            status: FutureStatus::Pending,
            error_handler: None,
        }));

        let mut stack = Vec::with_capacity(func.arity as usize + 1);
        let func_val = BxValue::new_ptr(self.heap.alloc(GcObject::CompiledFunction(Rc::clone(&func))));
        stack.push(func_val); // function itself at base
        for arg in args {
            stack.push(arg);
        }
        while stack.len() < (func.arity + 1) as usize {
            stack.push(BxValue::new_null());
        }

        let chunk = Rc::new(RefCell::new(func.chunk.clone()));
        let fiber = BxFiber {
            stack,
            frames: vec![CallFrame {
                function: func,
                chunk,
                ip: 0,
                stack_base: 1,
                receiver: None,
                handlers: Vec::new(),
                promoted_constants: Vec::new(),
            }],
            future_id,
            wait_until: None,
            yield_requested: false,
            priority,
            root_stack: Vec::new(),
        };

        self.fibers.push(fiber);
        BxValue::new_ptr(future_id)
    }

    fn run_all(&mut self) -> Result<BxValue> {
        let mut last_result = BxValue::new_null();
        
        while !self.fibers.is_empty() {
            self.drain_native_completions();
            let mut i = 0;
            let mut all_waiting = true;
            let mut earliest_wait: Option<Instant> = None;
            
            // 1. Find the highest priority among non-waiting fibers
            let mut max_priority = 0;
            let now = Instant::now();
            for f in &self.fibers {
                if let Some(until) = f.wait_until {
                    if now < until {
                        if earliest_wait.is_none() || until < earliest_wait.unwrap() {
                            earliest_wait = Some(until);
                        }
                        continue;
                    }
                }
                if f.priority > max_priority {
                    max_priority = f.priority;
                }
            }

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
                
                // Only run fibers with the current maximum priority to avoid starvation of I/O/callbacks
                if self.fibers[i].priority < max_priority {
                    i += 1;
                    all_waiting = false;
                    continue;
                }

                all_waiting = false;
                self.current_fiber_idx = Some(i);
                // Only pay for timeslice tracking when there are multiple fibers
                // to cooperatively schedule. Single-fiber scripts skip Instant::now()
                // entirely inside run_fiber, eliminating a syscall from every loop.
                let deadline = if self.fibers.len() > 1 {
                    Some(Instant::now() + Duration::from_millis(2))
                } else {
                    None
                };
                match self.run_fiber(i, deadline) {
                    Ok(Some(result)) => {
                        let fiber = self.fibers.swap_remove(i);
                        if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                            f.value = result;
                            f.status = FutureStatus::Completed;
                        }
                        last_result = result;
                        // No i += 1 here because swap_remove moved another fiber into index i
                    }
                    Ok(None) => {
                        i += 1;
                    }
                    Err(e) => {
                        let fiber = self.fibers.swap_remove(i);
                        let mut handler = None;
                        let err_val = BxValue::new_ptr(self.heap.alloc(GcObject::String(BoxString::new(&e.to_string()))));
                        if let GcObject::Future(f) = self.heap.get_mut(fiber.future_id) {
                            f.status = FutureStatus::Failed(err_val);
                            handler = f.error_handler;
                        }
                        
                        if let Some(h) = handler {
                            self.spawn_error_handler(h, err_val);
                            // Since we spawned a new fiber, it will be at the end of the list.
                            // The swap_removed fiber is gone, index i now has a different fiber.
                        } else {
                            if self.fibers.is_empty() {
                                return Err(e);
                            } else {
                                eprintln!("\n[Async Task Error] {}", e);
                            }
                        }
                    }
                }
                self.current_fiber_idx = None;
            }
            
            if all_waiting && !self.fibers.is_empty() {
                if let Some(until) = earliest_wait {
                    let now = Instant::now();
                    if until > now {
                        std::thread::sleep(until - now);
                    }
                } else {
                    // Fallback if somehow all_waiting but no earliest_wait
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }

            // Periodically collect garbage
            if self.heap.should_collect() {
                self.collect_garbage();
            }
        }
        
        Ok(last_result)
    }

    fn run_fiber(&mut self, fiber_idx: usize, timeslice_end: Option<Instant>) -> Result<Option<BxValue>> {
        // Persistent state across dispatch iterations. Refreshed only when
        // `frame_changed` is true (after CALL/RETURN/THROW/NEW/etc.).
        // In tight loops there are no frame changes, so these are loaded just once.
        let mut frame_changed  = true;
        let mut ip:           usize                     = 0;
        let mut stack_base:   usize                     = 0;
        let mut code_ptr:     *const u32                = std::ptr::null();
        let mut code_len:     usize                     = 0;
        let mut promoted_ptr: *mut Vec<Option<BxValue>> = std::ptr::null_mut();
        // Pointer to the base of the current frame's locals on the value stack.
        // Refreshed whenever frame_changed is true. Avoids the double pointer
        // chase (fibers[idx] → stack Vec → slot) in hot opcode arms.
        let mut locals_ptr:   *mut BxValue              = std::ptr::null_mut();
        // Counter used to throttle Instant::now() at safe points.
        // We only call the (expensive) system clock every 1024 backward branches
        // to avoid the ~20–50ns syscall cost on every loop iteration.
        // Skipped entirely (Option::None fast path) when only one fiber is running.
        let mut safe_point_count: u32 = 0;
        let trace = std::env::var("BX_TRACE").is_ok();

        // JIT profiling state — tracked per run_fiber call to avoid per-iteration
        // HashMap overhead.  Only a single local counter is hot; the JitState's
        // HashMap is consulted at most twice (once to compile, once to cache here).
        #[cfg(feature = "jit")]
        let mut jit_hot_ip: usize = usize::MAX;   // ip_at_start of the loop being counted
        #[cfg(feature = "jit")]
        let mut jit_hot_count: u64 = 0;           // consecutive iterations of that loop
        // Once compiled, we cache the fn pointer locally so subsequent invocations
        // don't need a HashMap lookup either.
        #[cfg(feature = "jit")]
        let mut jit_active: Option<(usize, jit::JitLoopFn)> = None; // (ip_at_start, fn)
        // Generic body-loop JIT state.
        // jit_body_active is a quantum-local cache of the compiled fn pointer.
        // Profiling counters now live in JitState (persistent across quanta).
        #[cfg(feature = "jit")]
        let mut jit_body_active: Option<(usize, jit::GenericJitLoopFn)> = None; // (ip_at_start, fn)
        // Tier-3: array iterator JIT state.
        // jit_iter_active is a quantum-local cache of the compiled fn pointer.
        #[cfg(feature = "jit")]
        let mut jit_iter_active: Option<(usize, jit::ArrayIterJitFn)> = None;

        'quantum: loop {
            self.drain_native_completions();
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

            // Reload all frame-derived state when the frame changes.
            // In tight loops `frame_changed` stays false — this block never runs.
            if frame_changed {
                frame_changed = false;
                ip = self.fibers[fiber_idx].frames.last().unwrap().ip;
                stack_base = self.fibers[fiber_idx].frames.last().unwrap().stack_base;
                if ip == 0 {
                    let chunk_rc = Rc::clone(
                        &self.fibers[fiber_idx].frames.last().unwrap().chunk,
                    );
                    chunk_rc.borrow_mut().ensure_caches();
                }
                // SAFETY: code/promoted_constants never mutated; Rc keeps them alive.
                unsafe {
                    let frame = self.fibers[fiber_idx].frames.last_mut().unwrap();
                    let chunk_ptr = frame.chunk.as_ptr();
                    code_ptr     = (*chunk_ptr).code.as_ptr();
                    code_len     = (*chunk_ptr).code.len();
                    promoted_ptr = &mut frame.promoted_constants as *mut _;
                }
                // Reserve headroom so that push() within this frame's execution
                // won't reallocate the stack Vec and invalidate locals_ptr.
                // 256 slots is generous; expression temporaries rarely exceed ~20.
                self.fibers[fiber_idx].stack.reserve(256);
                // SAFETY: stack is not reallocated within a single frame's dispatch
                // (reserve above guarantees capacity). Any op that changes frames
                // (CALL / RETURN / THROW / NEW) sets frame_changed = true, which
                // refreshes locals_ptr on the next iteration before any access.
                unsafe {
                    locals_ptr = self.fibers[fiber_idx].stack
                        .as_mut_ptr()
                        .add(stack_base);
                }
            }

            if ip >= code_len {
                return Ok(Some(BxValue::new_null()));
            }
            // SAFETY: ip < code_len; pointer is valid for the Rc<Chunk> lifetime.
            let word0 = unsafe { *code_ptr.add(ip) };
            let ip_at_start = ip;
            ip += 1;

            let opcode = (word0 & 0xFF) as u8;
            let op0 = word0 >> 8;

            if trace {
                let stack = &self.fibers[fiber_idx].stack;
                let stack_display: Vec<String> = stack.iter().map(|v| {
                    if v.is_null() { "null".to_string() }
                    else if v.is_bool() { format!("bool({})", v.as_bool()) }
                    else if v.is_int() { format!("int({})", v.as_int()) }
                    else if v.is_number() { format!("num({})", v.as_number()) }
                    else if v.is_ptr() { format!("ptr({:?})", v.as_gc_id()) }
                    else { "?".to_string() }
                }).collect();
                eprintln!("[TRACE] ip={:04} sb={} op={} op0={} stack=[{}]",
                    ip_at_start,
                    stack_base,
                    crate::vm::opcode::opcode_name(opcode),
                    op0,
                    stack_display.join(", "));
            }

            // Flush ip to frame before frame changes or throws.
            macro_rules! flush_ip {
                () => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().ip = ip;
                };
            }

            // Read next word via raw code pointer — zero RefCell overhead.
            macro_rules! next_word {
                () => {{
                    let w = unsafe { *code_ptr.add(ip) };
                    ip += 1;
                    w
                }};
            }

            // vm_throw! flushes ip, throws, then marks frame_changed so the next
            // iteration reloads ip = handler_ip set by throw_value.
            macro_rules! vm_throw {
                ($msg:expr) => {{
                    flush_ip!();
                    self.throw_error(fiber_idx, $msg)?;
                    frame_changed = true;
                    continue 'quantum;
                }};
                ($fmt:literal, $($args:expr),+) => {{
                    flush_ip!();
                    self.throw_error(fiber_idx, &format!($fmt, $($args),+))?;
                    frame_changed = true;
                    continue 'quantum;
                }};
            }

            if INTERRUPT_REQUESTED.load(Ordering::Relaxed) {
                vm_throw!("Force Quit (Ctrl+C)");
            }

            match opcode {
                // --- Hot Loop / Specialized Opcodes ---
                op::INC_LOCAL => {
                    let slot = op0;
                    let val = unsafe { *locals_ptr.add(slot as usize) };
                    if val.is_number() {
                        unsafe { *locals_ptr.add(slot as usize) = BxValue::new_number(val.as_number() + 1.0) };
                    } else if val.is_int() {
                        unsafe { *locals_ptr.add(slot as usize) = BxValue::new_int(val.as_int() + 1) };
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::LOCAL_COMPARE_JUMP => {
                    let slot = op0;
                    let const_idx = next_word!();
                    let offset = next_word!();
                    let val = unsafe { *locals_ptr.add(slot as usize) };
                    let limit: BxValue = {
                        let already = unsafe {
                            (&*promoted_ptr).get(const_idx as usize).copied().flatten()
                        };
                        if let Some(v) = already { v } else { self.read_constant(fiber_idx, const_idx as usize) }
                    };
                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            ip -= offset as usize;
                            if let Some(end) = timeslice_end {
                                safe_point_count = safe_point_count.wrapping_add(1);
                                if safe_point_count & 1023 == 0 && Instant::now() >= end { break 'quantum; }
                            }
                        }
                    } else if val.is_int() && limit.is_int() {
                        if val.as_int() < limit.as_int() {
                            ip -= offset as usize;
                            if let Some(end) = timeslice_end {
                                safe_point_count = safe_point_count.wrapping_add(1);
                                if safe_point_count & 1023 == 0 && Instant::now() >= end { break 'quantum; }
                            }
                        }
                    }
                }
                op::FOR_LOOP_STEP => {
                    // Fused: increment local, compare to const, jump back if still less.
                    // Replaces INC_LOCAL + LOCAL_COMPARE_JUMP — halves dispatch overhead for tight for-loops.
                    let slot = op0;
                    let const_idx = next_word!();
                    let offset = next_word!();
                    let val = unsafe { *locals_ptr.add(slot as usize) };
                    let next_val = if val.is_int() {
                        BxValue::new_int(val.as_int() + 1)
                    } else if val.is_number() {
                        BxValue::new_number(val.as_number() + 1.0)
                    } else {
                        vm_throw!("For loop variable must be a number");
                    };
                    unsafe { *locals_ptr.add(slot as usize) = next_val };
                    // Hot-path: read the loop-limit constant without a RefCell borrow.
                    // SAFETY: single-threaded VM; the code_ptr borrow has already dropped;
                    // no concurrent mutable access to promoted_constants is possible here.
                    let limit: BxValue = {
                        let already = unsafe { (&*promoted_ptr).get(const_idx as usize).copied().flatten() };
                        if let Some(v) = already { v } else { self.read_constant(fiber_idx, const_idx as usize) }
                    };
                    let should_loop = if next_val.is_int() && limit.is_int() {
                        next_val.as_int() < limit.as_int()
                    } else if next_val.is_number() && limit.is_number() {
                        next_val.as_number() < limit.as_number()
                    } else {
                        false
                    };
                    if should_loop {
                        // JIT fast path: eliminate remaining loop iterations with one native call.
                        // Uses local counters (no per-iteration HashMap) for near-zero overhead.
                        #[cfg(feature = "jit")]
                        {
                            if next_val.is_float() && limit.is_float() && offset == 3 {
                                // ── Tier-1: empty self-loop (floats only for now) ────────────────
                                if let Some((active_ip, compiled)) = jit_active {
                                    if active_ip == ip_at_start {
                                        let final_val = unsafe {
                                            compiled(next_val.as_number(), limit.as_number())
                                        };
                                        unsafe {
                                            *locals_ptr.add(slot as usize) =
                                                BxValue::new_number(final_val);
                                        }
                                        // Loop complete — do NOT jump back.
                                        jit_active = None;
                                        jit_body_active = None;
                                    } else {
                                        ip -= offset as usize;
                                    }
                                } else {
                                    if jit_hot_ip == ip_at_start {
                                        jit_hot_count += 1;
                                        const JIT_PROFILE_THRESHOLD: u64 = 5_000;
                                        if jit_hot_count >= JIT_PROFILE_THRESHOLD {
                                            let fn_id = code_ptr as usize;
                                            if let Some(ref mut jit) = self.jit {
                                                jit.profile_loop(fn_id, ip_at_start, jit_hot_count);
                                                if let Some(f) = jit.get_compiled_loop(fn_id, ip_at_start) {
                                                    jit_active = Some((ip_at_start, f));
                                                }
                                            }
                                            jit_hot_count = 0;
                                        }
                                    } else {
                                        jit_hot_ip    = ip_at_start;
                                        jit_hot_count = 1;
                                    }
                                    ip -= offset as usize;
                                    if let Some(end) = timeslice_end {
                                        safe_point_count = safe_point_count.wrapping_add(1);
                                        if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                            break 'quantum;
                                        }
                                    }
                                }
                            } else if next_val.is_number() && limit.is_number() && offset > 3 {
                                // ── Tier-2: generic numeric body ─────────────────────────────
                                // The JIT translates each body bytecode 1:1 into Cranelift IR
                                // and emits a real native loop — no mathematical shortcuts.
                                // OSR: 't2 loop lets us activate a freshly compiled fn (or one
                                // compiled in a prior quantum) and call it in the same dispatch.
                                't2: loop {
                                    let fn_id = code_ptr as usize;

                                    // ── OSR check: already compiled (possibly prior quantum) ──
                                    if jit_body_active.is_none() {
                                        if let Some(ref mut jit) = self.jit {
                                            if let Some(f) = jit.get_compiled_generic(fn_id, ip_at_start) {
                                                jit_body_active = Some((ip_at_start, f));
                                                continue 't2; // re-enter to call via active path
                                            }
                                        }
                                    }

                                    // ── Active path: call the compiled native loop ────────────
                                    if let Some((active_ip, compiled)) = jit_body_active {
                                        if active_ip == ip_at_start {
                                            eprintln!("[JIT] calling compiled loop at ip={}!", ip_at_start);
                                            let deopt = unsafe { compiled(locals_ptr as *mut u64, &self.heap as *const _ as *const std::ffi::c_void) };
                                            if deopt == 1 {
                                                eprintln!("[JIT] deoptimizing loop at ip={} (type mismatch)!", ip_at_start);
                                                // JIT bailed out — resume at start of this iteration.
                                                ip -= offset as usize;
                                                jit_body_active = None;
                                                jit_active = None;
                                            } else {
                                                // Loop ran to completion — do NOT jump back.
                                                jit_body_active = None;
                                                jit_active = None;
                                                if let Some(end) = timeslice_end {
                                                    safe_point_count = safe_point_count.wrapping_add(1);
                                                    if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                                        break 'quantum;
                                                    }
                                                }
                                            }
                                            break 't2;
                                        } else {
                                            // Different loop site — fall through to back-edge.
                                        }
                                    }

                                    // ── Profiling: accumulate count in JitState (survives quanta) ──
                                    // Fire every JIT_BODY_THRESHOLD iterations so that profile_generic's
                                    // internal counter (which requires 2×5000 = 10000 total) can be
                                    // reached across multiple quanta.
                                    const JIT_BODY_THRESHOLD: u64 = 5_000;
                                    let reached_threshold = if let Some(ref mut jit) = self.jit {
                                        let count = jit.inc_loop_profile(fn_id, ip_at_start);
                                        count % JIT_BODY_THRESHOLD == 0
                                    } else {
                                        false
                                    };

                                    if reached_threshold {
                                        let fn_id = code_ptr as usize;
                                        // Copy body bytes (offset - 3 words before FOR_LOOP_STEP).
                                        let body_start = ip - offset as usize;
                                        let body_len   = offset as usize - 3;
                                        let body_code: Vec<u32> = unsafe {
                                            std::slice::from_raw_parts(
                                                code_ptr.add(body_start), body_len,
                                            ).to_vec()
                                        };
                                        // Extract numeric constants referenced in the body.
                                        let mut const_map: HashMap<u32, f64> = HashMap::new();
                                        let ic_entries = {
                                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                            let c = frame.chunk.borrow();
                                            c.caches[body_start .. body_start + body_len].to_vec()
                                        };
                                        for &word in &body_code {
                                            if (word & 0xFF) as u8 == op::CONSTANT {
                                                let cidx = word >> 8;
                                                let cv = self.read_constant(fiber_idx, cidx as usize);
                                                if cv.is_number() {
                                                    const_map.insert(cidx, cv.as_number());
                                                }
                                            }
                                        }
                                        if let Some(ref mut jit) = self.jit {
                                            jit.profile_generic(
                                                fn_id, ip_at_start,
                                                JIT_BODY_THRESHOLD,
                                                &body_code,
                                                &ic_entries,
                                                slot,
                                                limit.as_number(),
                                                &const_map,
                                            );
                                            if let Some(f) = jit.get_compiled_generic(fn_id, ip_at_start) {
                                                jit_body_active = Some((ip_at_start, f));
                                                continue 't2; // immediately run the freshly compiled fn
                                            }
                                        }
                                    }

                                    // Not compiled yet — back-edge to loop header.
                                    ip -= offset as usize;
                                    if let Some(end) = timeslice_end {
                                        safe_point_count = safe_point_count.wrapping_add(1);
                                        if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                            break 'quantum;
                                        }
                                    }
                                    break 't2;
                                }
                            } else {
                                // Non-float or unhandled: plain interpreter.
                                ip -= offset as usize;
                                if let Some(end) = timeslice_end {
                                    safe_point_count = safe_point_count.wrapping_add(1);
                                    if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                        break 'quantum;
                                    }
                                }
                            }
                        }
                        #[cfg(not(feature = "jit"))]
                        {
                            ip -= offset as usize;
                            // Safe point: yield to scheduler if timeslice expired.
                            // Skipped entirely when timeslice_end is None (single-fiber case).
                            if let Some(end) = timeslice_end {
                                safe_point_count = safe_point_count.wrapping_add(1);
                                if safe_point_count & 1023 == 0 && Instant::now() >= end {
                                    break 'quantum; // ip flushed after the loop
                                }
                            }
                        }
                    }
                }
                op::COMPARE_JUMP => {
                    let const_idx = op0;
                    let offset = next_word!();
                    let limit = self.read_constant(fiber_idx, const_idx as usize);
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            ip -= offset as usize;
                        }
                    } else {
                        vm_throw!("OpCompareJump expects numeric operands");
                    }
                }
                op::INC_GLOBAL => {
                    let idx = op0;
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    if let Some(IcEntry::Global { index }) = ic {
                        let val = self.global_values[index];
                        if val.is_number() {
                            self.global_values[index] = BxValue::new_number(val.as_number() + 1.0);
                        } else {
                            flush_ip!();
                            self.throw_error(fiber_idx, "Operand of increment must be a number")?;
                            frame_changed = true; continue 'quantum;
                        }
                    } else {
                        // Slow path: resolve global and update IC
                        let name_id = self.read_intern_id(fiber_idx, idx as usize);
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            let val = self.global_values[global_idx];
                            if val.is_number() {
                                self.global_values[global_idx] = BxValue::new_number(val.as_number() + 1.0);
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.chunk.borrow_mut();
                                chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                            } else {
                                flush_ip!();
                                self.throw_error(fiber_idx, "Operand of increment must be a number")?;
                                frame_changed = true; continue 'quantum;
                            }
                        } else {
                            let name = self.interner.resolve(name_id).to_string();
                            flush_ip!();
                            self.throw_error(fiber_idx, &format!("Global {} not found", name))?;
                            frame_changed = true; continue 'quantum;
                        }
                    }
                }
                op::GLOBAL_COMPARE_JUMP => {
                    let name_idx = op0;
                    let const_idx = next_word!();
                    let offset = next_word!();
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    let val = if let Some(IcEntry::Global { index }) = ic {
                        self.global_values[index]
                    } else {
                        let name_id = self.read_intern_id(fiber_idx, name_idx as usize);
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            let v = self.global_values[global_idx];
                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.chunk.borrow_mut();
                            chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                            v
                        } else {
                            BxValue::new_null()
                        }
                    };

                    let limit = self.read_constant(fiber_idx, const_idx as usize);
                    if val.is_number() && limit.is_number() {
                        if val.as_number() < limit.as_number() {
                            ip -= offset as usize;
                            if let Some(end) = timeslice_end {
                                safe_point_count = safe_point_count.wrapping_add(1);
                                if safe_point_count & 1023 == 0 && Instant::now() >= end { break 'quantum; }
                            }
                        }
                    }
                }

                // --- Basic Hot Opcodes ---
                op::GET_LOCAL => {
                    let slot = op0;
                    let val = unsafe { *locals_ptr.add(slot as usize) };
                    self.fibers[fiber_idx].stack.push(val);
                }
                op::SET_LOCAL => {
                    let slot = op0;
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    unsafe { *locals_ptr.add(slot as usize) = val };
                }
                op::SET_LOCAL_POP => {
                    let slot = op0;
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    unsafe { *locals_ptr.add(slot as usize) = val };
                }
                op::CONSTANT => {
                    let idx = op0;
                    let constant = self.read_constant(fiber_idx, idx as usize);
                    self.fibers[fiber_idx].stack.push(constant);
                }
                op::ADD_INT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_int(a.as_number() as i32 + b.as_number() as i32));
                }
                op::ADD_FLOAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() + b.as_number()));
                }
                op::ADD => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    
                    let a_num = if a.is_number() { Some(a.as_number()) } else { self.to_string(a).parse::<f64>().ok() };
                    let b_num = if b.is_number() { Some(b.as_number()) } else { self.to_string(b).parse::<f64>().ok() };

                    if let (Some(na), Some(nb)) = (a_num, b_num) {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(na + nb));
                    } else {
                        let a_s = self.to_box_string(a);
                        let b_s = self.to_box_string(b);
                        let res_id = self.heap.alloc(GcObject::String(a_s.concat(&b_s)));
                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(res_id));
                    }
                }
                op::SUBTRACT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() - b.as_number()));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::SUB_INT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_int(a.as_number() as i32 - b.as_number() as i32));
                }
                op::SUB_FLOAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() - b.as_number()));
                }
                op::MULTIPLY => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() * b.as_number()));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::MUL_INT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_int(a.as_number() as i32 * b.as_number() as i32));
                }
                op::MUL_FLOAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() * b.as_number()));
                }
                op::DIVIDE => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        let b_n = b.as_number();
                        if b_n == 0.0 { flush_ip!(); self.throw_error(fiber_idx, "Division by zero")?; frame_changed = true; continue 'quantum; }
                        else { self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() / b_n)); }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers.")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::DIV_FLOAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() / b.as_number()));
                }
                op::MODULO => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        let b_n = b.as_number();
                        if b_n == 0.0 { flush_ip!(); self.throw_error(fiber_idx, "Division by zero (modulo)")?; frame_changed = true; continue 'quantum; }
                        else { self.fibers[fiber_idx].stack.push(BxValue::new_number(a.as_number() % b_n)); }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Operands must be two numbers for modulo.")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::POP => {
                    self.fibers[fiber_idx].stack.pop();
                }
                op::JUMP_IF_FALSE => {
                    let offset = op0;
                    if !self.is_truthy(*self.fibers[fiber_idx].stack.last().unwrap()) {
                        ip += offset as usize;
                    }
                }
                op::JUMP_IF_NULL => {
                    let offset = op0;
                    if self.fibers[fiber_idx].stack.last().unwrap().is_null() {
                        ip += offset as usize;
                    }
                }
                op::JUMP => {
                    let offset = op0;
                    ip += offset as usize;
                }
                op::LOOP => {
                    let offset = op0;
                    ip -= offset as usize;
                    // Safe point: yield to scheduler if timeslice expired.
                    // Skipped entirely (None fast-path) when only one fiber is running.
                    // Throttled to every 1024 backward branches when active.
                    if let Some(end) = timeslice_end {
                        safe_point_count = safe_point_count.wrapping_add(1);
                        if safe_point_count & 1023 == 0 && Instant::now() >= end {
                            break 'quantum; // ip flushed after the loop
                        }
                    }
                }
                op::RETURN => {
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
                    // Reload frame state for the caller on the next iteration.
                    frame_changed = true;
                    continue 'quantum;
                }

                // --- Global / Scope Opcodes ---
                op::GET_GLOBAL => {
                    let idx = op0;
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    if let Some(IcEntry::Global { index }) = ic {
                        let val = self.global_values[index];
                        self.fibers[fiber_idx].stack.push(val);
                    } else {
                        let name_id = self.read_intern_id(fiber_idx, idx as usize);
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            let val = self.global_values[global_idx];
                            self.fibers[fiber_idx].stack.push(val);

                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.chunk.borrow_mut();
                            chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                        } else {
                            self.fibers[fiber_idx].stack.push(BxValue::new_null());
                        }
                    }
                }
                op::SET_GLOBAL => {
                    let idx = op0;
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    let val = *self.fibers[fiber_idx].stack.last().unwrap();

                    if let Some(IcEntry::Global { index }) = ic {
                        self.global_values[index] = val;
                    } else {
                        let name_id = self.read_intern_id(fiber_idx, idx as usize);
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            self.global_values[global_idx] = val;

                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.chunk.borrow_mut();
                            chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                        } else {
                            self.insert_global_interned(name_id, val);
                            if let Some(&global_idx) = self.global_names.get(&name_id) {
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.chunk.borrow_mut();
                                chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                            }
                        }
                    }
                }
                op::SET_GLOBAL_POP => {
                    let idx = op0;
                    let ic = {
                        let frame = self.fibers[fiber_idx].frames.last().unwrap();
                        let chunk = frame.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    let val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(IcEntry::Global { index }) = ic {
                        self.global_values[index] = val;
                    } else {
                        let name_id = self.read_intern_id(fiber_idx, idx as usize);
                        if let Some(&global_idx) = self.global_names.get(&name_id) {
                            self.global_values[global_idx] = val;

                            let frame = self.fibers[fiber_idx].frames.last().unwrap();
                            let mut chunk = frame.chunk.borrow_mut();
                            chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                        } else {
                            self.insert_global_interned(name_id, val);
                            if let Some(&global_idx) = self.global_names.get(&name_id) {
                                let frame = self.fibers[fiber_idx].frames.last().unwrap();
                                let mut chunk = frame.chunk.borrow_mut();
                                chunk.caches[ip_at_start] = Some(IcEntry::Global { index: global_idx });
                            }
                        }
                    }
                }
                op::DEFINE_GLOBAL => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize);
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.insert_global_interned(name_id, val);
                }
                op::GET_PRIVATE => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize);
                    let name = self.interner.resolve(name_id).to_string().to_lowercase();
                    let val = {
                        let mut found = None;
                        if let Some(receiver) = self.fibers[fiber_idx].frames.last().unwrap().receiver {
                            if let Some(id) = receiver.as_gc_id() {
                                if name == "this" {
                                    found = Some(receiver);
                                } else if name == "variables" {
                                    if let GcObject::Instance(inst) = self.heap.get(id) {
                                        let proxy = VariablesScopeProxy {
                                            variables: Rc::clone(&inst.variables),
                                        };
                                        found = Some(BxValue::new_ptr(
                                            self.heap.alloc(GcObject::NativeObject(Rc::new(RefCell::new(proxy))))
                                        ));
                                    }
                                } else if let GcObject::Instance(inst) = self.heap.get(id) {
                                    found = inst.variables.borrow().get(&name).copied();
                                }
                            }
                        }
                        
                        if found.is_none() {
                            found = self.get_global(&name);
                        }
                        found
                    };

                    if let Some(v) = val {
                        self.fibers[fiber_idx].stack.push(v);
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, &format!("Variable '{}' not found in class or global scope.", name))?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::SET_PRIVATE => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize);
                    let name = self.interner.resolve(name_id).to_string().to_lowercase();
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    if let Some(receiver) = self.fibers[fiber_idx].frames.last().unwrap().receiver {
                        if let Some(id) = receiver.as_gc_id() {
                            if let GcObject::Instance(inst) = self.heap.get_mut(id) {
                                inst.variables.borrow_mut().insert(name, val);
                            }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "'variables' scope only available in classes.")?;
                        frame_changed = true; continue 'quantum;
                    }
                }

                // --- Stack Manipulation ---
                op::DUP => {
                    let val = *self.fibers[fiber_idx].stack.last().unwrap();
                    self.fibers[fiber_idx].stack.push(val);
                }
                op::SWAP => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    self.fibers[fiber_idx].stack.push(b);
                    self.fibers[fiber_idx].stack.push(a);
                }
                op::OVER => {
                    let val = self.fibers[fiber_idx].stack[self.fibers[fiber_idx].stack.len() - 2];
                    self.fibers[fiber_idx].stack.push(val);
                }
                op::INC => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if val.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(val.as_number() + 1.0));
                    } else if val.is_int() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_int(val.as_int() + 1));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::DEC => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if val.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_number(val.as_number() - 1.0));
                    } else if val.is_int() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_int(val.as_int() - 1));
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Decrement operand must be a number")?;
                        frame_changed = true; continue 'quantum;
                    }
                }

                // --- Data Structures ---
                op::ARRAY => {
                    let count = op0;
                    let mut items = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        items.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    items.reverse();
                    let id = self.heap.alloc(GcObject::Array(items));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                }
                op::STRUCT => {
                    let count = op0;
                    let mut shape_id = self.shapes.get_root();
                    let mut props = Vec::with_capacity(count as usize);

                    let mut kv_pairs = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        let value = self.fibers[fiber_idx].stack.pop().unwrap();
                        let key_val = self.fibers[fiber_idx].stack.pop().unwrap();
                        let key_str = self.to_string(key_val);
                        let key_id = self.interner.intern(&key_str);
                        kv_pairs.push((key_id, value));
                    }
                    kv_pairs.reverse();

                    for (key_id, value) in kv_pairs {
                        shape_id = self.shapes.transition(shape_id, key_id);
                        props.push(value);
                    }

                    let id = self.heap.alloc(GcObject::Struct(BxStruct { shape_id, properties: props }));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(id));
                }
                op::INDEX => {
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    if let Some(id) = base_val.as_gc_id() {
                        match self.heap.get(id) {
                            GcObject::Array(arr) => {
                                if index_val.is_number() || index_val.is_int() {
                                    let idx = if index_val.is_int() { index_val.as_int() as usize } else { index_val.as_number() as usize };
                                    if idx < 1 || idx > arr.len() {
                                        // Out-of-bounds reads return null (sparse array semantics)
                                        self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                    } else {
                                        self.fibers[fiber_idx].stack.push(arr[idx - 1]);
                                    }
                                } else {
                                    flush_ip!();
                                    self.throw_error(fiber_idx, "Array index must be a number")?;
                                    frame_changed = true; continue 'quantum;
                                }
                            }
                            GcObject::Struct(s) => {
                                let key_str = self.to_string(index_val);
                                let key_id = self.interner.intern(&key_str);
                                if let Some(idx) = self.shapes.get_index(s.shape_id, key_id) {
                                    self.fibers[fiber_idx].stack.push(s.properties[idx as usize]);
                                } else {
                                    self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                }
                            }
                            _ => { flush_ip!(); self.throw_error(fiber_idx, "Invalid access: base must be array or struct")?; frame_changed = true; continue 'quantum; }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Invalid access: base must be array or struct")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::SET_INDEX => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let index_val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(id) = base_val.as_gc_id() {
                        let key_id = if !index_val.is_number() && !index_val.is_int() {
                            let key_str = self.to_string(index_val);
                            Some(self.interner.intern(&key_str))
                        } else {
                            None
                        };

                        match self.heap.get_mut(id) {
                            GcObject::Array(arr) => {
                                if index_val.is_number() || index_val.is_int() {
                                    let idx = if index_val.is_int() { index_val.as_int() as usize } else { index_val.as_number() as usize };
                                    if idx < 1 {
                                        flush_ip!();
                                        self.throw_error(fiber_idx, &format!("Array index out of bounds: {}", idx))?;
                                        frame_changed = true; continue 'quantum;
                                    } else if idx > arr.len() {
                                        // Auto-grow: fill gaps with null
                                        arr.resize(idx, BxValue::new_null());
                                        arr[idx - 1] = val;
                                        self.fibers[fiber_idx].stack.push(val);
                                    } else {
                                        arr[idx - 1] = val;
                                        self.fibers[fiber_idx].stack.push(val);
                                    }
                                } else {
                                    flush_ip!();
                                    self.throw_error(fiber_idx, "Array index must be a number")?;
                                    frame_changed = true; continue 'quantum;
                                }
                            }
                            GcObject::Struct(s) => {
                                let key_id = key_id.unwrap();
                                if let Some(idx) = self.shapes.get_index(s.shape_id, key_id) {
                                    s.properties[idx as usize] = val;
                                } else {
                                    s.shape_id = self.shapes.transition(s.shape_id, key_id);
                                    s.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::Instance(inst) => {
                                let key_id = key_id.unwrap();
                                if let Some(idx) = self.shapes.get_index(inst.shape_id, key_id) {
                                    inst.properties[idx as usize] = val;
                                } else {
                                    inst.shape_id = self.shapes.transition(inst.shape_id, key_id);
                                    inst.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => { flush_ip!(); self.throw_error(fiber_idx, "Invalid indexed assignment")?; frame_changed = true; continue 'quantum; }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Invalid indexed assignment")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::MEMBER => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize);
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(id) = base_val.as_gc_id() {
                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        if let GcObject::JsValue(js) = self.heap.get(id) {
                            let js = js.clone();
                            let name = self.interner.resolve(name_id);
                            let prop = JsValue::from_str(name);
                            match Reflect::get(&js, &prop) {
                                Ok(val) => {
                                    let bx_val = self.js_to_bx(val);
                                    self.fibers[fiber_idx].stack.push(bx_val);
                                }
                                Err(_) => self.fibers[fiber_idx].stack.push(BxValue::new_null()),
                            }
                            flush_ip!();
                            continue 'quantum;
                        }

                        #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
                        {
                            let maybe_handle = if let GcObject::JsHandle(h) = self.heap.get(id) { Some(*h) } else { None };
                            if let Some(handle) = maybe_handle {
                                let name = self.interner.resolve(name_id);
                                let key_bytes = name.as_bytes();
                                let mut str_buf = [0u8; 4096];
                                let mut out_str_len: usize = 0;
                                let mut out_num: f64 = 0.0;
                                let mut out_bool: i32 = 0;
                                let mut out_obj: u32 = 0;
                                let rtype = unsafe {
                                    bx_js_get_prop(
                                        handle, key_bytes.as_ptr(), key_bytes.len(),
                                        str_buf.as_mut_ptr(), 4096, &mut out_str_len,
                                        &mut out_num, &mut out_bool, &mut out_obj,
                                    )
                                };
                                let bx_val = self.js_result_to_bx(rtype, &str_buf, out_str_len, out_num, out_bool, out_obj);
                                self.fibers[fiber_idx].stack.push(bx_val);
                                flush_ip!();
                                continue 'quantum;
                            }
                        }

                        match self.heap.get(id) {
                            GcObject::Struct(s) => {
                                let shape_id = s.shape_id;
                                let properties_ptr = &s.properties as *const Vec<BxValue>;

                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                match ic {
                                    Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) => {
                                        if cached_shape == shape_id as usize {
                                            let val = unsafe { &*properties_ptr }[index as usize];
                                            self.fibers[fiber_idx].stack.push(val);
                                            flush_ip!();
                                            continue 'quantum;
                                        }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                let val = unsafe { &*properties_ptr }[entries[i].1];
                                                self.fibers[fiber_idx].stack.push(val);
                                                flush_ip!();
                                                continue 'quantum;
                                            }
                                        }
                                    }
                                    _ => {}
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, name_id) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        match chunk.caches[ip_at_start] {
                                            None => {
                                                chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                            }
                                            Some(IcEntry::Monomorphic { shape_id: s, index: i }) => {
                                                let mut entries = [(0, 0); 4];
                                                entries[0] = (s, i);
                                                entries[1] = (shape_id as usize, idx as usize);
                                                chunk.caches[ip_at_start] = Some(IcEntry::Polymorphic { entries, count: 2 });
                                            }
                                            Some(IcEntry::Polymorphic { ref mut entries, ref mut count }) => {
                                                if *count < 4 {
                                                    entries[*count] = (shape_id as usize, idx as usize);
                                                    *count += 1;
                                                } else {
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Megamorphic);
                                                }
                                            }
                                            _ => {}
                                        }
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
                                    let chunk = frame.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                match ic {
                                    Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) => {
                                        if cached_shape == shape_id as usize {
                                            let val = unsafe { &*properties_ptr }[index as usize];
                                            self.fibers[fiber_idx].stack.push(val);
                                            flush_ip!();
                                            continue 'quantum;
                                        }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                let val = unsafe { &*properties_ptr }[entries[i].1];
                                                self.fibers[fiber_idx].stack.push(val);
                                                flush_ip!();
                                                continue 'quantum;
                                            }
                                        }
                                    }
                                    _ => {}
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, name_id) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        match chunk.caches[ip_at_start] {
                                            None => {
                                                chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                            }
                                            Some(IcEntry::Monomorphic { shape_id: s, index: i }) => {
                                                let mut entries = [(0, 0); 4];
                                                entries[0] = (s, i);
                                                entries[1] = (shape_id as usize, idx as usize);
                                                chunk.caches[ip_at_start] = Some(IcEntry::Polymorphic { entries, count: 2 });
                                            }
                                            Some(IcEntry::Polymorphic { ref mut entries, ref mut count }) => {
                                                if *count < 4 {
                                                    entries[*count] = (shape_id as usize, idx as usize);
                                                    *count += 1;
                                                } else {
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Megamorphic);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    let val = unsafe { &*properties_ptr }[idx as usize];
                                    self.fibers[fiber_idx].stack.push(val);
                                } else {
                                    let name = self.interner.resolve(name_id).to_string();
                                    if let Some(method) = self.resolve_method(Rc::clone(&class), &name) {
                                        let m_id = self.heap.alloc(GcObject::CompiledFunction(method));
                                        self.fibers[fiber_idx].stack.push(BxValue::new_ptr(m_id));
                                    } else {
                                        self.fibers[fiber_idx].stack.push(BxValue::new_null());
                                    }
                                }
                            }
                            GcObject::NativeObject(obj) => {
                                let name = self.interner.resolve(name_id).to_string().to_lowercase();
                                let val = obj.borrow().get_property(&name);
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => { flush_ip!(); self.throw_error(fiber_idx, "Member access only supported on structs, instances, JS objects, and native objects")?; frame_changed = true; continue 'quantum; }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Member access only supported on structs, instances, JS objects, and native objects")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::SET_MEMBER => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize);
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(id) = base_val.as_gc_id() {
                        #[cfg(all(target_arch = "wasm32", feature = "js"))]
                        if let GcObject::JsValue(js) = self.heap.get(id) {
                            let js = js.clone();
                            let name = self.interner.resolve(name_id);
                            let prop = JsValue::from_str(name);
                            let js_val = self.bx_to_js(&val);
                            Reflect::set(&js, &prop, &js_val).ok();
                            self.fibers[fiber_idx].stack.push(val);
                            flush_ip!();
                            continue 'quantum;
                        }

                        #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
                        {
                            let maybe_handle = if let GcObject::JsHandle(h) = self.heap.get(id) { Some(*h) } else { None };
                            if let Some(handle) = maybe_handle {
                                let name = self.interner.resolve(name_id);
                                let key_bytes = name.as_bytes();
                                if val.is_null() {
                                    unsafe { bx_js_set_prop_null(handle, key_bytes.as_ptr(), key_bytes.len()); }
                                } else if val.is_bool() {
                                    unsafe { bx_js_set_prop_bool(handle, key_bytes.as_ptr(), key_bytes.len(), if val.as_bool() { 1 } else { 0 }); }
                                } else if val.is_number() {
                                    unsafe { bx_js_set_prop_num(handle, key_bytes.as_ptr(), key_bytes.len(), val.as_number()); }
                                } else if val.is_int() {
                                    unsafe { bx_js_set_prop_num(handle, key_bytes.as_ptr(), key_bytes.len(), val.as_int() as f64); }
                                } else if let Some(val_gc_id) = val.as_gc_id() {
                                    let maybe_str_bytes: Option<Vec<u8>> = if let GcObject::String(s) = self.heap.get(val_gc_id) {
                                        Some(s.to_string().into_bytes())
                                    } else { None };
                                    let maybe_val_handle: Option<u32> = if let GcObject::JsHandle(h) = self.heap.get(val_gc_id) {
                                        Some(*h)
                                    } else { None };
                                    if let Some(str_bytes) = maybe_str_bytes {
                                        unsafe { bx_js_set_prop_str(handle, key_bytes.as_ptr(), key_bytes.len(), str_bytes.as_ptr(), str_bytes.len()); }
                                    } else if let Some(val_handle) = maybe_val_handle {
                                        unsafe { bx_js_set_prop_obj(handle, key_bytes.as_ptr(), key_bytes.len(), val_handle); }
                                    } else {
                                        unsafe { bx_js_set_prop_null(handle, key_bytes.as_ptr(), key_bytes.len()); }
                                    }
                                } else {
                                    unsafe { bx_js_set_prop_null(handle, key_bytes.as_ptr(), key_bytes.len()); }
                                }
                                self.fibers[fiber_idx].stack.push(val);
                                flush_ip!();
                                continue 'quantum;
                            }
                        }

                        match self.heap.get_mut(id) {
                            GcObject::Struct(s) => {
                                let shape_id = s.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                match ic {
                                    Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) => {
                                        if cached_shape == shape_id as usize {
                                            s.properties[index as usize] = val;
                                            self.fibers[fiber_idx].stack.push(val);
                                            flush_ip!();
                                            continue 'quantum;
                                        }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                s.properties[entries[i].1] = val;
                                                self.fibers[fiber_idx].stack.push(val);
                                                flush_ip!();
                                                continue 'quantum;
                                            }
                                        }
                                    }
                                    _ => {}
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, name_id) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        match chunk.caches[ip_at_start] {
                                            None => {
                                                chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                            }
                                            Some(IcEntry::Monomorphic { shape_id: s, index: i }) => {
                                                let mut entries = [(0, 0); 4];
                                                entries[0] = (s, i);
                                                entries[1] = (shape_id as usize, idx as usize);
                                                chunk.caches[ip_at_start] = Some(IcEntry::Polymorphic { entries, count: 2 });
                                            }
                                            Some(IcEntry::Polymorphic { ref mut entries, ref mut count }) => {
                                                if *count < 4 {
                                                    entries[*count] = (shape_id as usize, idx as usize);
                                                    *count += 1;
                                                } else {
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Megamorphic);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    s.properties[idx as usize] = val;
                                } else {
                                    s.shape_id = self.shapes.transition(shape_id, name_id);
                                    s.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::Instance(inst) => {
                                let shape_id = inst.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                match ic {
                                    Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) => {
                                        if cached_shape == shape_id as usize {
                                            inst.properties[index as usize] = val;
                                            self.fibers[fiber_idx].stack.push(val);
                                            flush_ip!();
                                            continue 'quantum;
                                        }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                inst.properties[entries[i].1] = val;
                                                self.fibers[fiber_idx].stack.push(val);
                                                flush_ip!();
                                                continue 'quantum;
                                            }
                                        }
                                    }
                                    _ => {}
                                }

                                if let Some(idx) = self.shapes.get_index(shape_id, name_id) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        match chunk.caches[ip_at_start] {
                                            None => {
                                                chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                            }
                                            Some(IcEntry::Monomorphic { shape_id: s, index: i }) => {
                                                let mut entries = [(0, 0); 4];
                                                entries[0] = (s, i);
                                                entries[1] = (shape_id as usize, idx as usize);
                                                chunk.caches[ip_at_start] = Some(IcEntry::Polymorphic { entries, count: 2 });
                                            }
                                            Some(IcEntry::Polymorphic { ref mut entries, ref mut count }) => {
                                                if *count < 4 {
                                                    entries[*count] = (shape_id as usize, idx as usize);
                                                    *count += 1;
                                                } else {
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Megamorphic);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    inst.properties[idx as usize] = val;
                                } else {
                                    inst.shape_id = self.shapes.transition(shape_id, name_id);
                                    inst.properties.push(val);
                                }
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            GcObject::NativeObject(obj) => {
                                let name = self.interner.resolve(name_id).to_string().to_lowercase();
                                obj.borrow_mut().set_property(&name, val);
                                self.fibers[fiber_idx].stack.push(val);
                            }
                            _ => { flush_ip!(); self.throw_error(fiber_idx, "Member assignment only supported on structs, instances, JS objects, and native objects")?; frame_changed = true; continue 'quantum; }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Member assignment only supported on structs, instances, JS objects, and native objects")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::INC_MEMBER => {
                    let idx = op0;
                    let name_id = self.read_intern_id(fiber_idx, idx as usize);
                    let base_val = self.fibers[fiber_idx].stack.pop().unwrap();

                    if let Some(id) = base_val.as_gc_id() {
                        match self.heap.get_mut(id) {
                            GcObject::Struct(s) => {
                                let shape_id = s.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                let index = match ic {
                                    Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) => {
                                        if cached_shape == shape_id as usize { Some(index as usize) } else { None }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        let mut found = None;
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                found = Some(entries[i].1);
                                                break;
                                            }
                                        }
                                        found
                                    }
                                    _ => None,
                                };

                                if let Some(idx) = index.or_else(|| self.shapes.get_index(shape_id, name_id).map(|i| i as usize)) {
                                    let old_val = s.properties[idx];
                                    if old_val.is_number() {
                                        let new_val = BxValue::new_number(old_val.as_number() + 1.0);
                                        s.properties[idx] = new_val;
                                        self.fibers[fiber_idx].stack.push(new_val);
                                        
                                        if index.is_none() {
                                            let fiber = &self.fibers[fiber_idx];
                                            let frame = fiber.frames.last().unwrap();
                                            let mut chunk = frame.chunk.borrow_mut();
                                            match chunk.caches[ip_at_start] {
                                                None => {
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                                }
                                                Some(IcEntry::Monomorphic { shape_id: s, index: i }) => {
                                                    let mut entries = [(0, 0); 4];
                                                    entries[0] = (s, i);
                                                    entries[1] = (shape_id as usize, idx as usize);
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Polymorphic { entries, count: 2 });
                                                }
                                                Some(IcEntry::Polymorphic { ref mut entries, ref mut count }) => {
                                                    if *count < 4 {
                                                        entries[*count] = (shape_id as usize, idx as usize);
                                                        *count += 1;
                                                    } else {
                                                        chunk.caches[ip_at_start] = Some(IcEntry::Megamorphic);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    } else {
                                        flush_ip!();
                                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                                        frame_changed = true; continue 'quantum;
                                    }
                                } else {
                                    let name = self.interner.resolve(name_id).to_string();
                                    flush_ip!();
                                    self.throw_error(fiber_idx, &format!("Member {} not found", name))?;
                                    frame_changed = true; continue 'quantum;
                                }
                            }
                            GcObject::Instance(inst) => {
                                let shape_id = inst.shape_id;
                                let ic = {
                                    let fiber = &self.fibers[fiber_idx];
                                    let frame = fiber.frames.last().unwrap();
                                    let chunk = frame.chunk.borrow();
                                    chunk.caches[ip_at_start].clone()
                                };

                                let index = match ic {
                                    Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) => {
                                        if cached_shape == shape_id as usize { Some(index as usize) } else { None }
                                    }
                                    Some(IcEntry::Polymorphic { entries, count }) => {
                                        let mut found = None;
                                        for i in 0..count {
                                            if entries[i].0 == shape_id as usize {
                                                found = Some(entries[i].1);
                                                break;
                                            }
                                        }
                                        found
                                    }
                                    _ => None,
                                };

                                if let Some(idx) = index.or_else(|| self.shapes.get_index(shape_id, name_id).map(|i| i as usize)) {
                                    let old_val = inst.properties[idx];
                                    if old_val.is_number() {
                                        let new_val = BxValue::new_number(old_val.as_number() + 1.0);
                                        inst.properties[idx] = new_val;
                                        self.fibers[fiber_idx].stack.push(new_val);

                                        if index.is_none() {
                                            let fiber = &self.fibers[fiber_idx];
                                            let frame = fiber.frames.last().unwrap();
                                            let mut chunk = frame.chunk.borrow_mut();
                                            match chunk.caches[ip_at_start] {
                                                None => {
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                                }
                                                Some(IcEntry::Monomorphic { shape_id: s, index: i }) => {
                                                    let mut entries = [(0, 0); 4];
                                                    entries[0] = (s, i);
                                                    entries[1] = (shape_id as usize, idx as usize);
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Polymorphic { entries, count: 2 });
                                                }
                                                Some(IcEntry::Polymorphic { ref mut entries, ref mut count }) => {
                                                    if *count < 4 {
                                                        entries[*count] = (shape_id as usize, idx as usize);
                                                        *count += 1;
                                                    } else {
                                                        chunk.caches[ip_at_start] = Some(IcEntry::Megamorphic);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    } else {
                                        flush_ip!();
                                        self.throw_error(fiber_idx, "Increment operand must be a number")?;
                                        frame_changed = true; continue 'quantum;
                                    }
                                } else {
                                    let name = self.interner.resolve(name_id).to_string();
                                    flush_ip!();
                                    self.throw_error(fiber_idx, &format!("Member {} not found", name))?;
                                    frame_changed = true; continue 'quantum;
                                }
                            }
                            _ => { 
                                flush_ip!();
                                self.throw_error(fiber_idx, "Fused increment only supported on structs and instances for now")?;
                                frame_changed = true; continue 'quantum; 
                            }
                        }
                    } else {
                        flush_ip!();
                        self.throw_error(fiber_idx, "Member access only supported on objects")?;
                        frame_changed = true; continue 'quantum;
                    }
                }
                op::STRING_CONCAT => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a_s = self.to_box_string(a);
                    let b_s = self.to_box_string(b);
                    let res_id = self.heap.alloc(GcObject::String(a_s.concat(&b_s)));
                    self.fibers[fiber_idx].stack.push(BxValue::new_ptr(res_id));
                }

                // --- Calls / Invocations ---
                op::CALL => {
                    let arg_count = op0;
                    flush_ip!();
                    self.execute_call(fiber_idx, arg_count as usize, None)?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) { return Ok(None); } // ip already flushed
                    frame_changed = true;
                    continue 'quantum;
                }
                op::CALL_NAMED => {
                    let total_count = op0;
                    let names_idx = next_word!();
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
                    flush_ip!();
                    self.execute_call(fiber_idx, total_count as usize, Some(names))?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) { return Ok(None); } // ip already flushed
                    frame_changed = true;
                    continue 'quantum;
                }
                op::INVOKE => {
                    let name_idx = op0;
                    let arg_count = next_word!();
                    let name_id = self.read_intern_id(fiber_idx, name_idx as usize);
                    let name = self.interner.resolve(name_id).to_string();
                    #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
                    {
                        let receiver_idx = self.fibers[fiber_idx].stack.len() - 1 - arg_count as usize;
                        let receiver_val = self.fibers[fiber_idx].stack[receiver_idx];
                        let maybe_handle = if let Some(id) = receiver_val.as_gc_id() {
                            if let GcObject::JsHandle(h) = self.heap.get(id) { Some(*h) } else { None }
                        } else { None };
                        if let Some(handle) = maybe_handle {
                            let method_bytes = name.as_bytes();
                            let mut args = Vec::with_capacity(arg_count as usize);
                            for i in 0..(arg_count as usize) {
                                args.push(self.fibers[fiber_idx].stack[receiver_idx + 1 + i]);
                            }
                            let args_json = self.bx_args_to_json(&args);
                            let mut str_buf = [0u8; 4096];
                            let mut out_str_len: usize = 0;
                            let mut out_num: f64 = 0.0;
                            let mut out_bool: i32 = 0;
                            let mut out_obj: u32 = 0;
                            let rtype = unsafe {
                                bx_js_call_method(
                                    handle, method_bytes.as_ptr(), method_bytes.len(),
                                    args_json.as_ptr(), args_json.len(),
                                    str_buf.as_mut_ptr(), 4096, &mut out_str_len,
                                    &mut out_num, &mut out_bool, &mut out_obj,
                                )
                            };
                            for _ in 0..(arg_count as usize + 1) { self.fibers[fiber_idx].stack.pop(); }
                            let bx_val = self.js_result_to_bx(rtype, &str_buf, out_str_len, out_num, out_bool, out_obj);
                            self.fibers[fiber_idx].stack.push(bx_val);
                            flush_ip!();
                            frame_changed = true;
                            continue 'quantum;
                        }
                    }
                    flush_ip!();
                    self.execute_invoke(fiber_idx, name, arg_count as usize, None, ip_at_start)?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) { return Ok(None); } // ip already flushed
                    frame_changed = true;
                    continue 'quantum;
                }
                op::INVOKE_NAMED => {
                    let name_idx = op0;
                    let total_count = next_word!();
                    let names_idx = next_word!();
                    let invoke_name_id = self.read_intern_id(fiber_idx, name_idx as usize);
                    let name = self.interner.resolve(invoke_name_id).to_string();
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
                    flush_ip!();
                    self.execute_invoke(fiber_idx, name, total_count as usize, Some(names), ip_at_start)?;
                    if timeslice_end.map_or(false, |end| Instant::now() >= end) { return Ok(None); } // ip already flushed
                    frame_changed = true;
                    continue 'quantum;
                }
                op::NEW => {
                    let arg_count = op0;
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

                            let constructor = class.borrow().constructor.clone();
                            let sub_chunk = constructor.chunk.clone();
                            let constant_count = sub_chunk.constants.len();

                            let frame = CallFrame {
                                function: Rc::new(constructor),
                                chunk: Rc::new(RefCell::new(sub_chunk)),
                                ip: 0,
                                stack_base: class_idx + 1 + arg_count as usize,
                                receiver: Some(instance_val),
                                handlers: Vec::new(),
                                promoted_constants: vec![None; constant_count],
                            };
                            flush_ip!();
                            self.fibers[fiber_idx].frames.push(frame);
                            frame_changed = true;
                            continue 'quantum;
                        } else {
                            vm_throw!("Can only instantiate classes.");
                        }
                    } else {
                        vm_throw!("Can only instantiate classes.");
                    }
                }

                // --- Comparison ---
                op::EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let res = self.is_equal(a, b);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(res));
                }
                op::NOT_EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let res = self.is_equal(a, b);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(!res));
                }
                op::LESS => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(a.as_number() < b.as_number()));
                    } else {
                        let sa = self.to_string_internal(a);
                        let sb = self.to_string_internal(b);
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(sa < sb));
                    }
                }
                op::LESS_EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(a.as_number() <= b.as_number()));
                    } else {
                        let sa = self.to_string_internal(a);
                        let sb = self.to_string_internal(b);
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(sa <= sb));
                    }
                }
                op::GREATER => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(a.as_number() > b.as_number()));
                    } else {
                        let sa = self.to_string_internal(a);
                        let sb = self.to_string_internal(b);
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(sa > sb));
                    }
                }
                op::GREATER_EQUAL => {
                    let b = self.fibers[fiber_idx].stack.pop().unwrap();
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    if a.is_number() && b.is_number() {
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(a.as_number() >= b.as_number()));
                    } else {
                        let sa = self.to_string_internal(a);
                        let sb = self.to_string_internal(b);
                        self.fibers[fiber_idx].stack.push(BxValue::new_bool(sa >= sb));
                    }
                }
                op::NOT => {
                    let a = self.fibers[fiber_idx].stack.pop().unwrap();
                    let res = self.is_truthy(a);
                    self.fibers[fiber_idx].stack.push(BxValue::new_bool(!res));
                }

                // --- Control Flow / Misc ---
                op::ITER_NEXT => {
                    let collection_slot = op0;
                    let word1 = next_word!();
                    let cursor_slot = word1 & 0x7FFF_FFFF;
                    let push_index = (word1 >> 31) != 0;
                    let offset = next_word!();
                    let collection_idx = stack_base + collection_slot as usize;
                    let cursor_idx = stack_base + cursor_slot as usize;
                    
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
                                        for &intern_id in shape.fields.keys() {
                                            let resolved = self.interner.resolve(intern_id).to_string();
                                            k.push((intern_id, resolved));
                                        }
                                        k.sort_by(|a, b| a.1.cmp(&b.1));
                                        k
                                    };
                                    if cursor_val < keys.len() {
                                        let (field_id, key_str) = &keys[cursor_val];
                                        let idx = self.shapes.get_index(s.shape_id, *field_id).unwrap();
                                        let val = s.properties[idx as usize];
                                        let key_gc_id = self.heap.alloc(GcObject::String(BoxString::new(key_str)));
                                        (false, Some(BxValue::new_ptr(key_gc_id)), Some(val))
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
                        ip += offset as usize;
                    } else {
                        // ── Tier-3 JIT fast-path (numeric arrays, no index push) ──────────
                        // OSR: 't3 loop lets us activate an already-compiled iter fn (from
                        // this or a prior quantum) and call it in the same dispatch.
                        #[cfg(feature = "jit")]
                        let handled_by_jit = {
                            let mut handled = false;
                            if !push_index {
                                let collection = self.fibers[fiber_idx].stack[collection_idx];
                                if let Some(gc_id) = collection.as_gc_id() {
                                    let is_array = matches!(self.heap.get_opt(gc_id), Some(GcObject::Array(_)));
                                    if is_array {
                                        let fn_id = code_ptr as usize;
                                        't3: loop {
                                            // ── OSR check: compiled fn from prior quantum ──────
                                            if jit_iter_active.is_none() {
                                                if let Some(ref mut jit) = self.jit {
                                                    if let Some(f) = jit.get_compiled_iter(fn_id, ip_at_start) {
                                                        jit_iter_active = Some((ip_at_start, f));
                                                        continue 't3;
                                                    }
                                                }
                                            }

                                            // ── Active path: call the compiled native iter loop ─
                                            if let Some((active_ip, compiled)) = jit_iter_active {
                                                if active_ip == ip_at_start {
                                                    let (arr_ptr, arr_len) = match self.heap.get(gc_id) {
                                                        GcObject::Array(arr) => (arr.as_ptr() as *const u64, arr.len() as u64),
                                                        _ => unreachable!(),
                                                    };
                                                    let deopt = unsafe {
                                                        compiled(locals_ptr as *mut u64, arr_ptr, arr_len)
                                                    };
                                                    if deopt == 0 {
                                                        ip += offset as usize; // jump past loop
                                                    } else {
                                                        eprintln!("[JIT] deopt iter loop ip={}", ip_at_start);
                                                        jit_iter_active = None;
                                                    }
                                                    handled = deopt == 0;
                                                    break 't3;
                                                }
                                            }

                                            // ── Profiling: accumulate count in JitState ────────
                                            // Fire every JIT_ITER_THRESHOLD iterations so that
                                            // profile_iter's internal counter can reach 10000 across quanta.
                                            const JIT_ITER_THRESHOLD: u64 = 5_000;
                                            let reached_threshold = if let Some(ref mut jit) = self.jit {
                                                let count = jit.inc_iter_profile(fn_id, ip_at_start);
                                                count % JIT_ITER_THRESHOLD == 0
                                            } else {
                                                false
                                            };

                                            if reached_threshold {
                                                let body_start = ip_at_start + 3;
                                                let body_len   = offset as usize - 1;
                                                let body_code: Vec<u32> = unsafe {
                                                    std::slice::from_raw_parts(code_ptr.add(body_start), body_len).to_vec()
                                                };
                                                let mut const_map: HashMap<u32, f64> = HashMap::new();
                                                for &word in &body_code {
                                                    if (word & 0xFF) as u8 == op::CONSTANT {
                                                        let cidx = word >> 8;
                                                        let cv = self.read_constant(fiber_idx, cidx as usize);
                                                        if cv.is_number() {
                                                            const_map.insert(cidx, cv.as_number());
                                                        }
                                                    }
                                                }
                                                if let Some(ref mut jit) = self.jit {
                                                    jit.profile_iter(
                                                        fn_id, ip_at_start, cursor_slot,
                                                        JIT_ITER_THRESHOLD,
                                                        &body_code, &const_map,
                                                    );
                                                    if let Some(f) = jit.get_compiled_iter(fn_id, ip_at_start) {
                                                        jit_iter_active = Some((ip_at_start, f));
                                                        continue 't3; // immediately run freshly compiled fn
                                                    }
                                                }
                                            }
                                            break 't3;
                                        }
                                    }
                                }
                            }
                            handled
                        };

                        // Normal single-iteration path
                        #[cfg(feature = "jit")]
                        let do_normal = !handled_by_jit;
                        #[cfg(not(feature = "jit"))]
                        let do_normal = true;

                        if do_normal {
                            let current_cursor = self.fibers[fiber_idx].stack[cursor_idx];
                            let next_cursor_val = if current_cursor.is_int() { BxValue::new_int(current_cursor.as_int() + 1) } else { BxValue::new_number(current_cursor.as_number() + 1.0) };
                            self.fibers[fiber_idx].stack[cursor_idx] = next_cursor_val;
                            self.fibers[fiber_idx].stack.push(next_val.unwrap());
                            if push_index {
                                self.fibers[fiber_idx].stack.push(next_idx.unwrap());
                            }
                        }
                    }
                }
                op::LOCAL_JUMP_IF_NE_CONST => {
                    let slot = op0;
                    let const_idx = next_word!();
                    let offset = next_word!();
                    let val = unsafe { *locals_ptr.add(slot as usize) };
                    let constant = self.read_constant(fiber_idx, const_idx as usize);
                    if val != constant {
                        ip += offset as usize;
                    }
                }
                op::PUSH_HANDLER => {
                    let offset = op0;
                    let target_ip = ip + offset as usize;
                    self.fibers[fiber_idx].frames.last_mut().unwrap().handlers.push(target_ip);
                }
                op::POP_HANDLER => {
                    self.fibers[fiber_idx].frames.last_mut().unwrap().handlers.pop();
                }
                op::THROW => {
                    let val = self.fibers[fiber_idx].stack.pop().unwrap();
                    flush_ip!();
                    self.throw_value(fiber_idx, val)?;
                    frame_changed = true;
                    continue 'quantum;
                }
                op::PRINT => {
                    let count = op0;
                    let mut args = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    let out = args.iter().map(|a| self.to_string(*a)).collect::<Vec<_>>().join(" ");
                    if let Some(ref mut buffer) = self.output_buffer {
                        buffer.push_str(&out);
                    } else {
                        print!("{}", out);
                    }
                }
                op::PRINTLN => {
                    let count = op0;
                    let mut args = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        args.push(self.fibers[fiber_idx].stack.pop().unwrap());
                    }
                    args.reverse();
                    let out = args.iter().map(|a| self.to_string(*a)).collect::<Vec<_>>().join(" ");
                    if let Some(ref mut buffer) = self.output_buffer {
                        buffer.push_str(&out);
                        buffer.push('\n');
                    } else {
                        println!("{}", out);
                    }
                }
                _ => {
                    bail!("Unknown opcode: {}", opcode);
                }
            }
            // ip persists across iterations within a single timeslice.
        }
        // Timeslice expired (safe-point break) — flush ip so the next run_fiber resumes correctly.
        if !self.fibers[fiber_idx].frames.is_empty() {
            self.fibers[fiber_idx].frames.last_mut().unwrap().ip = ip;
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
            let chunk = frame.chunk.borrow();
            filename = chunk.filename.clone();
            if frame.ip > 0 && frame.ip <= chunk.lines.len() {
                line = chunk.lines[frame.ip - 1];
                
                // Extract source snippet — prefer embedded source, fall back to disk
                let source_text: Option<String> = if !chunk.source.is_empty() {
                    Some(chunk.source.clone())
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    { std::fs::read_to_string(&filename).ok() }
                    #[cfg(target_arch = "wasm32")]
                    { None }
                };
                if let Some(src) = source_text {
                    if line > 0 {
                        let src_lines: Vec<&str> = src.lines().collect();
                        if line as usize <= src_lines.len() {
                            let code_line = src_lines[line as usize - 1].trim();
                            source_snippet = format!("\n\n  |  {}\n  |  {}", line, code_line);
                        }
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

    pub fn call_function(&mut self, name: &str, args: Vec<BxValue>, chunk: Option<Rc<RefCell<Chunk>>>) -> Result<BxValue> {
        if let Some(f) = self.get_global(name) {
            return self.call_function_value(f, args, chunk);
        }
        anyhow::bail!("Function {} not found", name)
    }

    pub fn call_function_value(&mut self, func: BxValue, args: Vec<BxValue>, _chunk: Option<Rc<RefCell<Chunk>>>) -> Result<BxValue> {
        if let Some(id) = func.as_gc_id() {
            match self.heap.get(id) {
                GcObject::CompiledFunction(f) => {
                    let f = Rc::clone(f);
                    if args.len() < f.min_arity as usize || args.len() > f.arity as usize {
                        anyhow::bail!("Expected {}-{} arguments but got {}", f.min_arity, f.arity, args.len());
                    }
                    let _future = self.enqueue_function_call(func, f, args, 0);
                    let fiber_idx = self.fibers.len() - 1;
                    self.current_fiber_idx = Some(fiber_idx);
                    // Loop until the fiber completes — this is a synchronous blocking call.
                    let result = loop {
                        match self.run_fiber(fiber_idx, Some(Instant::now() + Duration::from_millis(2))) {
                            Ok(Some(val)) => break Ok(val),
                            Ok(None) => continue, // timeslice expired, keep running
                            Err(e) => break Err(e),
                        }
                    };
                    self.current_fiber_idx = None;
                    result
                }
                GcObject::NativeFunction(f) => {
                    let f = *f;
                    self.gc_suspended = true;
                    let res = f(self, &args).map_err(|e| anyhow::anyhow!(e));
                    self.gc_suspended = false;
                    res
                }

                _ => anyhow::bail!("Value is not a callable function"),
            }
        } else {
            anyhow::bail!("Value is not a callable function")
        }
    }

    pub fn call_method_value(
        &mut self,
        receiver: BxValue,
        name: &str,
        args: Vec<BxValue>,
    ) -> Result<BxValue> {
        let id = receiver
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;
        let method_name = name.to_lowercase();

        match self.heap.get(id) {
            GcObject::NativeObject(_) => {
                return self
                    .native_object_call_method(id, &method_name, &args)
                    .map_err(|e| anyhow::anyhow!(e));
            }
            GcObject::Instance(inst) => {
                let class = Rc::clone(&inst.class);
                if let Some(func) = self.resolve_method(class, &method_name) {
                    let mut final_args = args;
                    for _ in 0..(func.arity as usize).saturating_sub(final_args.len()) {
                        final_args.push(BxValue::new_null());
                    }
                    if final_args.len() < func.min_arity as usize || final_args.len() > func.arity as usize {
                        anyhow::bail!(
                            "Expected {}-{} arguments but got {}",
                            func.min_arity,
                            func.arity,
                            final_args.len()
                        );
                    }

                    let sub_chunk = func.chunk.clone();
                    let constant_count = sub_chunk.constants.len();
                    let future_id = self.future_new().as_gc_id().unwrap();
                    let fiber = BxFiber {
                        stack: {
                            let mut stack = Vec::with_capacity(1 + final_args.len());
                            stack.push(receiver);
                            stack.extend(final_args);
                            stack
                        },
                        frames: vec![CallFrame {
                            function: func.clone(),
                            chunk: Rc::new(RefCell::new(sub_chunk)),
                            ip: 0,
                            stack_base: 1,
                            receiver: Some(receiver),
                            handlers: Vec::new(),
                            promoted_constants: vec![None; constant_count],
                        }],
                        future_id,
                        wait_until: None,
                        yield_requested: false,
                        priority: 0,
                        root_stack: Vec::new(),
                    };
                    self.fibers.push(fiber);
                    let fiber_idx = self.fibers.len() - 1;
                    self.current_fiber_idx = Some(fiber_idx);
                    let result = loop {
                        match self.run_fiber(fiber_idx, Some(Instant::now() + Duration::from_millis(2))) {
                            Ok(Some(val)) => break Ok(val),
                            Ok(None) => continue,
                            Err(e) => break Err(e),
                        }
                    };
                    self.current_fiber_idx = None;
                    let _ = self.fibers.pop();
                    result
                } else {
                    anyhow::bail!("Method {} not found on instance", name)
                }
            }
            _ => anyhow::bail!("Value is not an object instance"),
        }
    }

    pub fn instance_class_name(&self, receiver: BxValue) -> Result<String> {
        let id = receiver
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;

        match self.heap.get(id) {
            GcObject::Instance(inst) => Ok(inst.class.borrow().name.clone()),
            _ => anyhow::bail!("Value is not an object instance"),
        }
    }

    pub fn construct_global_class(
        &mut self,
        class_name: &str,
        args: Vec<BxValue>,
    ) -> Result<BxValue> {
        let class_val = self
            .get_global(class_name)
            .ok_or_else(|| anyhow::anyhow!("Class {} not found", class_name))?;
        let id = class_val
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Global {} is not a class", class_name))?;

        let class = match self.heap.get(id) {
            GcObject::Class(class) => Rc::clone(class),
            _ => anyhow::bail!("Global {} is not a class", class_name),
        };

        let variables_scope = Rc::new(RefCell::new(HashMap::new()));
        let inst_id = self.heap.alloc(GcObject::Instance(BxInstance {
            class: Rc::clone(&class),
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
            variables: variables_scope,
        }));
        let instance_val = BxValue::new_ptr(inst_id);

        let constructor = class.borrow().constructor.clone();
        let sub_chunk = constructor.chunk.clone();
        let constant_count = sub_chunk.constants.len();
        let future_id = self.future_new().as_gc_id().unwrap();
        let fiber = BxFiber {
            stack: {
                let mut stack = Vec::with_capacity(1 + args.len());
                stack.push(instance_val);
                stack.extend(args);
                stack
            },
            frames: vec![CallFrame {
                function: Rc::new(constructor),
                chunk: Rc::new(RefCell::new(sub_chunk)),
                ip: 0,
                stack_base: 1,
                receiver: Some(instance_val),
                handlers: Vec::new(),
                promoted_constants: vec![None; constant_count],
            }],
            future_id,
            wait_until: None,
            yield_requested: false,
            priority: 0,
            root_stack: Vec::new(),
        };
        self.fibers.push(fiber);
        let fiber_idx = self.fibers.len() - 1;
        self.current_fiber_idx = Some(fiber_idx);
        let result = loop {
            match self.run_fiber(fiber_idx, Some(Instant::now() + Duration::from_millis(2))) {
                Ok(Some(_)) => break Ok(instance_val),
                Ok(None) => continue,
                Err(e) => break Err(e),
            }
        };
        self.current_fiber_idx = None;
        let _ = self.fibers.pop();
        result
    }

    pub fn instantiate_global_class_without_constructor(
        &mut self,
        class_name: &str,
    ) -> Result<BxValue> {
        let class_val = self
            .get_global(class_name)
            .ok_or_else(|| anyhow::anyhow!("Class {} not found", class_name))?;
        let id = class_val
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Global {} is not a class", class_name))?;

        let class = match self.heap.get(id) {
            GcObject::Class(class) => Rc::clone(class),
            _ => anyhow::bail!("Global {} is not a class", class_name),
        };

        let variables_scope = Rc::new(RefCell::new(HashMap::new()));
        let inst_id = self.heap.alloc(GcObject::Instance(BxInstance {
            class,
            shape_id: self.shapes.get_root(),
            properties: Vec::new(),
            variables: variables_scope,
        }));
        Ok(BxValue::new_ptr(inst_id))
    }

    pub fn instance_variables_json(&self, receiver: BxValue) -> Result<serde_json::Value> {
        let id = receiver
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;

        match self.heap.get(id) {
            GcObject::Instance(inst) => {
                let mut object = serde_json::Map::new();
                for (key, value) in inst.variables.borrow().iter() {
                    object.insert(key.clone(), self.bx_to_json(value));
                }
                Ok(serde_json::Value::Object(object))
            }
            _ => anyhow::bail!("Value is not an object instance"),
        }
    }

    pub fn set_instance_variables_json(
        &mut self,
        receiver: BxValue,
        json: serde_json::Value,
    ) -> Result<()> {
        let id = receiver
            .as_gc_id()
            .ok_or_else(|| anyhow::anyhow!("Value is not an object instance"))?;

        let serde_json::Value::Object(values) = json else {
            anyhow::bail!("Listener state must be a JSON object");
        };

        let mut converted = Vec::with_capacity(values.len());
        for (key, value) in values {
            converted.push((key.to_lowercase(), self.json_to_bx(value)));
        }

        match self.heap.get_mut(id) {
            GcObject::Instance(inst) => {
                let mut variables = inst.variables.borrow_mut();
                variables.clear();
                for (key, value) in converted {
                    variables.insert(key, value);
                }
                Ok(())
            }
            _ => anyhow::bail!("Value is not an object instance"),
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

    fn spawn_error_handler(&mut self, handler: BxValue, err_val: BxValue) {
        if let Some(id) = handler.as_gc_id() {
            match self.heap.get(id) {
                GcObject::CompiledFunction(f) => {
                    let f_rc = Rc::clone(f);
                    let dummy_chunk = Rc::new(RefCell::new(Chunk::default()));
                    self.spawn(f_rc, vec![err_val], 1, dummy_chunk);
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
                        if (func.arity as usize) > arg_count {
                            for _ in 0..(func.arity as usize - arg_count) {
                                a.push(BxValue::new_null());
                            }
                        } else if arg_count > (func.arity as usize) {
                            // Trim extra arguments if function doesn't support varargs (MatchBox doesn't yet)
                            a.truncate(func.arity as usize);
                        }
                        a
                    };

                    // Stack: ... [func] [arg1] [arg2] ...
                    // Function is already at len() - 1 - arg_count.
                    // We popped args, now we push final_args back.
                    for arg in final_args {
                        self.fibers[fiber_idx].stack.push(arg);
                    }

                    // ── Tier-4 hot function fast-path ─────────────────────────
                    #[cfg(feature = "jit")]
                    {
                        let compiled_opt = if let Some(ref jit) = self.jit {
                            let fn_id = Rc::as_ptr(&func) as usize;
                            jit.get_compiled_fn(fn_id)
                        } else {
                            None
                        };

                        let compiled = if compiled_opt.is_none() {
                            if let Some(ref mut jit) = self.jit {
                                let fn_id  = Rc::as_ptr(&func) as usize;
                                let code   = func.chunk.code.as_slice();
                                let consts = func.chunk.constants.as_slice();
                                jit.profile_fn(fn_id, id, code, consts, func.arity)
                            } else {
                                None
                            }
                        } else {
                            compiled_opt
                        };

                        // Set thread-local pointer so compiled callees can resolve other
                        // compiled functions via jit_resolve_fn without passing extra state.
                        if let Some(ref jit) = self.jit {
                            crate::vm::jit::set_compiled_fns_ptr(
                                &jit.compiled_fns_by_gcid as *const _
                            );
                        }

                        if let Some(compiled_fn) = compiled {
                            let stack_base = self.fibers[fiber_idx].stack.len()
                                - func.arity as usize;
                            // Reserve extra space for additional locals the function may use
                            self.fibers[fiber_idx].stack.reserve(64);
                            let locals_raw = unsafe {
                                self.fibers[fiber_idx].stack.as_mut_ptr().add(stack_base)
                                    as *mut u64
                            };
                            let heap_raw = &self.heap as *const _ as *const std::ffi::c_void;
                            let mut ret_bits: u64 = 0;

                            let status = unsafe {
                                compiled_fn(locals_raw, heap_raw, &mut ret_bits)
                            };

                            if status == 0 {
                                // Success: remove the function object + args, push return value
                                let func_slot = stack_base - 1;
                                self.fibers[fiber_idx].stack.truncate(func_slot);
                                self.fibers[fiber_idx].stack.push(unsafe {
                                    std::mem::transmute::<u64, BxValue>(ret_bits)
                                });
                                return Ok(());
                            }
                            // status == 1 → deopt: fall through to normal frame creation
                            eprintln!("[JIT] Tier-4 deopt fn_id=0x{:x}", Rc::as_ptr(&func) as usize);
                            if let Some(ref mut jit) = self.jit {
                                jit.inc_fn_deopt(Rc::as_ptr(&func) as usize);
                            }
                        }
                    }
                    // ── End Tier-4 ────────────────────────────────────────────

                    let sub_chunk = func.chunk.clone();
                    let constant_count = sub_chunk.constants.len();
                    let mut frame = CallFrame {
                        function: Rc::clone(&func),
                        chunk: Rc::new(RefCell::new(sub_chunk)),
                        ip: 0,
                        stack_base: 0,
                        receiver: self.fibers[fiber_idx].frames.last().unwrap().receiver,
                        handlers: Vec::new(),
                        promoted_constants: vec![None; constant_count],
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

    #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
    fn js_result_to_bx(&mut self, rtype: i32, str_buf: &[u8], str_len: usize, num: f64, b: i32, obj_id: u32) -> BxValue {
        match rtype {
            1 => BxValue::new_bool(b != 0),
            2 => BxValue::new_number(num),
            3 => {
                let s = std::str::from_utf8(&str_buf[..str_len.min(str_buf.len())]).unwrap_or("");
                let id = self.heap.alloc(GcObject::String(BoxString::new(s)));
                BxValue::new_ptr(id)
            }
            4 => {
                let id = self.heap.alloc(GcObject::JsHandle(obj_id));
                BxValue::new_ptr(id)
            }
            _ => BxValue::new_null(),
        }
    }

    #[cfg(all(target_arch = "wasm32", not(feature = "js")))]
    fn bx_args_to_json(&self, args: &[BxValue]) -> Vec<u8> {
        let mut out = b"[".to_vec();
        for (i, v) in args.iter().enumerate() {
            if i > 0 { out.push(b','); }
            if v.is_null() {
                out.extend_from_slice(b"null");
            } else if v.is_bool() {
                out.extend_from_slice(if v.as_bool() { b"true" } else { b"false" });
            } else if v.is_number() {
                out.extend_from_slice(format!("{}", v.as_number()).as_bytes());
            } else if v.is_int() {
                out.extend_from_slice(format!("{}", v.as_int()).as_bytes());
            } else if let Some(gc_id) = v.as_gc_id() {
                let maybe_handle = if let GcObject::JsHandle(h) = self.heap.get(gc_id) { Some(*h) } else { None };
                if let Some(h) = maybe_handle {
                    out.extend_from_slice(format!("{{\"h\":{}}}", h).as_bytes());
                } else {
                    let s = self.to_string(*v);
                    out.push(b'"');
                    for ch in s.chars() {
                        match ch {
                            '"' => out.extend_from_slice(b"\\\""),
                            '\\' => out.extend_from_slice(b"\\\\"),
                            '\n' => out.extend_from_slice(b"\\n"),
                            '\r' => out.extend_from_slice(b"\\r"),
                            '\t' => out.extend_from_slice(b"\\t"),
                            c => { let mut buf = [0u8; 4]; out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes()); }
                        }
                    }
                    out.push(b'"');
                }
            } else {
                out.extend_from_slice(b"null");
            }
        }
        out.push(b']');
        out
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
                                let fiber = &mut self.fibers[fiber_idx];
                                fiber.frames.last_mut().unwrap().ip = ip_at_start;
                                fiber.yield_requested = true;
                                return Ok(());
                            }
                            FutureStatus::Completed => {
                                for _ in 0..arg_count { self.fibers[fiber_idx].stack.pop(); }
                                self.fibers[fiber_idx].stack.pop();
                                self.fibers[fiber_idx].stack.push(value);
                                return Ok(());
                            }
                            FutureStatus::Failed(e) => {
                                return self.throw_value(fiber_idx, e);
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
                    match obj_borrow.call_method(self, id, &name, &args) {
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
                        let chunk = frame.chunk.borrow();
                        chunk.caches[ip_at_start].clone()
                    };

                    let method = match ic {
                        Some(IcEntry::Monomorphic { shape_id: cached_shape, index }) => {
                            if cached_shape == shape_id as usize {
                                let method_val = inst.properties[index as usize];
                                if let Some(m_id) = method_val.as_gc_id() {
                                    if let GcObject::CompiledFunction(f) = self.heap.get(m_id) {
                                        Some(Rc::clone(f))
                                    } else { None }
                                } else { None }
                            } else { None }
                        }
                        Some(IcEntry::Polymorphic { entries, count }) => {
                            let mut found_idx = None;
                            for i in 0..count {
                                if entries[i].0 == shape_id as usize {
                                    found_idx = Some(entries[i].1);
                                    break;
                                }
                            }
                            if let Some(idx) = found_idx {
                                let method_val = inst.properties[idx];
                                if let Some(m_id) = method_val.as_gc_id() {
                                    if let GcObject::CompiledFunction(f) = self.heap.get(m_id) {
                                        Some(Rc::clone(f))
                                    } else { None }
                                } else { None }
                            } else { None }
                        }
                        _ => None,
                    };

                    let method = if method.is_none() {
                        let name_intern_id = self.interner.intern(&name);
                        if let Some(idx) = self.shapes.get_index(shape_id, name_intern_id) {
                            let method_val = inst.properties[idx as usize];
                            if let Some(m_id) = method_val.as_gc_id() {
                                if let GcObject::CompiledFunction(f) = self.heap.get(m_id) {
                                    {
                                        let fiber = &self.fibers[fiber_idx];
                                        let frame = fiber.frames.last().unwrap();
                                        let mut chunk = frame.chunk.borrow_mut();
                                        match chunk.caches[ip_at_start] {
                                            None => {
                                                chunk.caches[ip_at_start] = Some(IcEntry::Monomorphic { shape_id: shape_id as usize, index: idx as usize });
                                            }
                                            Some(IcEntry::Monomorphic { shape_id: s, index: i }) => {
                                                let mut entries = [(0, 0); 4];
                                                entries[0] = (s, i);
                                                entries[1] = (shape_id as usize, idx as usize);
                                                chunk.caches[ip_at_start] = Some(IcEntry::Polymorphic { entries, count: 2 });
                                            }
                                            Some(IcEntry::Polymorphic { ref mut entries, ref mut count }) => {
                                                if *count < 4 {
                                                    entries[*count] = (shape_id as usize, idx as usize);
                                                    *count += 1;
                                                } else {
                                                    chunk.caches[ip_at_start] = Some(IcEntry::Megamorphic);
                                                }
                                            }
                                            _ => {}
                                        }
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

                        let sub_chunk = func.chunk.clone();
                        let constant_count = sub_chunk.constants.len();
                        let frame = CallFrame {
                            function: func.clone(),
                            chunk: Rc::new(RefCell::new(sub_chunk)),
                            ip: 0,
                            stack_base: self.fibers[fiber_idx].stack.len() - func.arity as usize,
                            receiver: Some(receiver_val),
                            handlers: Vec::new(),
                            promoted_constants: vec![None; constant_count],
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

                        let sub_chunk = on_missing.chunk.clone();
                        let constant_count = sub_chunk.constants.len();
                        let mut frame = CallFrame {
                            function: on_missing.clone(),
                            chunk: Rc::new(RefCell::new(sub_chunk)),
                            ip: 0,
                            stack_base: self.fibers[fiber_idx].stack.len() - 2,
                            receiver: Some(receiver_val),
                            handlers: Vec::new(),
                            promoted_constants: vec![None; constant_count],
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
            
            let promoted = &frame.promoted_constants;
            if idx < promoted.len() {
                promoted[idx]
            } else {
                None
            }
        };

        if let Some(v) = val {
            return v;
        }

        let constant = {
            let fiber = &self.fibers[fiber_idx];
            let frame = fiber.frames.last().unwrap();
            let chunk = frame.chunk.borrow();
            chunk.constants[idx].clone()
        };

        let promoted = self.promote_constant(constant);
        
        {
            let fiber = &mut self.fibers[fiber_idx];
            let frame = fiber.frames.last_mut().unwrap();
            if idx >= frame.promoted_constants.len() {
                let chunk_len = frame.chunk.borrow().constants.len();
                frame.promoted_constants.resize(chunk_len, None);
            }
            frame.promoted_constants[idx] = Some(promoted);
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

    /// Read a string constant, intern it, and return the InternId.
    /// Since InternId is Copy (u32), the borrow on self is released.
    fn read_intern_id(&mut self, fiber_idx: usize, idx: usize) -> u32 {
        let s = self.read_string_constant(fiber_idx, idx);
        self.interner.intern(&s)
    }

    #[cfg(all(target_arch = "wasm32", feature = "js"))]
    pub fn bx_to_js(&self, val: &BxValue) -> JsValue {
        if val.is_int() {
            JsValue::from(val.as_int())
        } else if val.is_number() {
            JsValue::from_f64(val.as_number())
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
                    for (&k, &idx) in shape.fields.iter() {
                        let key_str = self.interner.resolve(k);
                        Reflect::set(&js_obj, &JsValue::from_str(key_str), &self.bx_to_js(&s.properties[idx as usize])).ok();
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
            if n.fract() == 0.0 && n >= i32::MIN as f64 && n <= i32::MAX as f64 {
                BxValue::new_int(n as i32)
            } else {
                BxValue::new_number(n)
            }
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
        } else if val.is_object() {
            // Check if it's a plain object (not a special type we already handled)
            let keys = js_sys::Object::keys(val.unchecked_ref::<js_sys::Object>());
            let struct_id = self.struct_new();
            for i in 0..keys.length() {
                let key = keys.get(i).as_string().unwrap();
                let prop_val = Reflect::get(&val, &key.clone().into()).unwrap();
                let bx_prop = self.js_to_bx(prop_val);
                self.struct_set(struct_id, &key, bx_prop);
            }
            BxValue::new_ptr(struct_id)
        } else {
            BxValue::new_ptr(self.heap.alloc(GcObject::JsValue(val)))
        }
    }

    fn collect_garbage(&mut self) {
        if self.gc_suspended {
            return;
        }
        let mut roots = Vec::new();
        // 1. Fiber stacks and frames
        for fiber in &self.fibers {
            roots.extend(fiber.stack.iter().cloned());
            roots.extend(fiber.root_stack.iter().cloned());
            for frame in &fiber.frames {
                if let Some(recv) = &frame.receiver {
                    roots.push(*recv);
                }
                roots.extend(frame.promoted_constants.iter().flatten().copied());
            }
            roots.push(BxValue::new_ptr(fiber.future_id));
        }
        // 2. Globals
        roots.extend(self.global_values.iter().cloned());
        for completion in &self.native_completions {
            match completion {
                NativeCompletion::Resolve { future, value } => {
                    roots.push(*future);
                    roots.push(*value);
                }
                NativeCompletion::Reject { future, error } => {
                    roots.push(*future);
                    roots.push(*error);
                }
            }
        }
        for future_id in self.pending_native_futures.keys() {
            roots.push(BxValue::new_ptr(*future_id));
        }

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
                    for (&k, &idx) in shape.fields.iter() {
                        if let Some(v) = s.properties.get(idx as usize) {
                            let key_str = self.interner.resolve(k).to_string();
                            map.insert(key_str, self.bx_to_json(v));
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
                    let name_id = self.interner.intern(&name);
                    bx_struct.shape_id = self.shapes.transition(shape_id, name_id);
                    bx_struct.properties.push(bx_val);
                }
                let id = self.heap.alloc(GcObject::Struct(bx_struct));
                BxValue::new_ptr(id)
            }
            serde_json::Value::Null => BxValue::new_null(),
        }
    }
}
