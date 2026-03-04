use std::collections::HashMap;
use crate::types::{BxValue, BxVM};
use std::time::{SystemTime, UNIX_EPOCH};
use rand::RngExt;
use std::rc::Rc;
use std::cell::RefCell;

pub fn register_all() -> HashMap<String, BxValue> {
    let mut bifs = HashMap::new();

    // Math BIFs
    bifs.insert("abs".to_string(), BxValue::NativeFunction(abs));
    bifs.insert("min".to_string(), BxValue::NativeFunction(min));
    bifs.insert("max".to_string(), BxValue::NativeFunction(max));
    bifs.insert("round".to_string(), BxValue::NativeFunction(round));
    bifs.insert("randrange".to_string(), BxValue::NativeFunction(rand_range));

    // Array BIFs
    bifs.insert("len".to_string(), BxValue::NativeFunction(len));
    bifs.insert("arrayappend".to_string(), BxValue::NativeFunction(array_append));
    bifs.insert("arraynew".to_string(), BxValue::NativeFunction(array_new));

    // Struct BIFs
    bifs.insert("structkeyexists".to_string(), BxValue::NativeFunction(struct_key_exists));
    bifs.insert("structcount".to_string(), BxValue::NativeFunction(struct_count));
    bifs.insert("structnew".to_string(), BxValue::NativeFunction(struct_new));

    // Date/Time BIFs
    bifs.insert("now".to_string(), BxValue::NativeFunction(now));
    bifs.insert("gettickcount".to_string(), BxValue::NativeFunction(get_tick_count));
    bifs.insert("sleep".to_string(), BxValue::NativeFunction(sleep));
    bifs.insert("yield".to_string(), BxValue::NativeFunction(bx_yield));

    // Async BIFs
    bifs.insert("runasync".to_string(), BxValue::NativeFunction(run_async));

    // Core BIFs
    bifs.insert("createobject".to_string(), BxValue::NativeFunction(create_object));

    bifs
}

// --- Implementation ---

fn abs(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("abs() expects exactly 1 argument".to_string()); }
    match &args[0] {
        BxValue::Number(n) => Ok(BxValue::Number(n.abs())),
        _ => Err("abs() expects a number".to_string()),
    }
}

fn min(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 { return Err("min() expects exactly 2 arguments".to_string()); }
    match (&args[0], &args[1]) {
        (BxValue::Number(a), BxValue::Number(b)) => Ok(BxValue::Number(a.min(*b))),
        _ => Err("min() expects numbers".to_string()),
    }
}

fn max(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 { return Err("max() expects exactly 2 arguments".to_string()); }
    match (&args[0], &args[1]) {
        (BxValue::Number(a), BxValue::Number(b)) => Ok(BxValue::Number(a.max(*b))),
        _ => Err("max() expects numbers".to_string()),
    }
}

fn round(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("round() expects exactly 1 argument".to_string()); }
    match &args[0] {
        BxValue::Number(n) => Ok(BxValue::Number(n.round())),
        _ => Err("round() expects a number".to_string()),
    }
}

fn rand_range(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 { return Err("randRange() expects exactly 2 arguments".to_string()); }
    match (&args[0], &args[1]) {
        (BxValue::Number(min), BxValue::Number(max)) => {
            let mut rng = rand::rng();
            let val = rng.random_range((*min as i64)..=(*max as i64));
            Ok(BxValue::Number(val as f64))
        }
        _ => Err("randRange() expects numbers".to_string()),
    }
}

fn len(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("len() expects exactly 1 argument".to_string()); }
    match &args[0] {
        BxValue::String(s) => Ok(BxValue::Number(s.len() as f64)),
        BxValue::Array(a) => Ok(BxValue::Number(a.borrow().len() as f64)),
        BxValue::Struct(s) => Ok(BxValue::Number(s.borrow().len() as f64)),
        _ => Err("len() expects a string, array, or struct".to_string()),
    }
}

fn array_append(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 { return Err("arrayAppend() expects exactly 2 arguments".to_string()); }
    match &args[0] {
        BxValue::Array(a) => {
            a.borrow_mut().push(args[1].clone());
            Ok(BxValue::Boolean(true))
        }
        _ => Err("arrayAppend() expects an array as the first argument".to_string()),
    }
}

fn array_new(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::Array(Rc::new(RefCell::new(Vec::new()))))
}

fn struct_key_exists(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 { return Err("structKeyExists() expects exactly 2 arguments".to_string()); }
    match (&args[0], &args[1]) {
        (BxValue::Struct(s), BxValue::String(k)) => {
            Ok(BxValue::Boolean(s.borrow().contains_key(&k.to_lowercase())))
        }
        _ => Err("structKeyExists() expects a struct and a string key".to_string()),
    }
}

fn struct_count(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("structCount() expects exactly 1 argument".to_string()); }
    match &args[0] {
        BxValue::Struct(s) => Ok(BxValue::Number(s.borrow().len() as f64)),
        _ => Err("structCount() expects a struct".to_string()),
    }
}

fn struct_new(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::Struct(Rc::new(RefCell::new(HashMap::new()))))
}

fn now(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let start = SystemTime::now();
    let _since_the_epoch = start.duration_since(UNIX_EPOCH).expect("Time went backwards");
    Ok(BxValue::String(format!("{:?}", start)))
}

fn get_tick_count(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let start = SystemTime::now();
    let since_the_epoch = start.duration_since(UNIX_EPOCH).expect("Time went backwards");
    Ok(BxValue::Number(since_the_epoch.as_millis() as f64))
}

fn sleep(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 { return Err("sleep() expects exactly 1 argument".to_string()); }
    match &args[0] {
        BxValue::Number(ms) => {
            vm.sleep(*ms as u64);
            Ok(BxValue::Null)
        }
        _ => Err("sleep() expects a number (milliseconds)".to_string()),
    }
}

fn bx_yield(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    vm.yield_fiber();
    Ok(BxValue::Null)
}

fn run_async(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("runAsync() expects at least 1 argument".to_string()); }
    match &args[0] {
        BxValue::CompiledFunction(func) => {
            let func_args = args[1..].to_vec();
            Ok(vm.spawn(Rc::clone(func), func_args))
        }
        _ => Err("runAsync() expects a function as the first argument".to_string()),
    }
}

fn create_object(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("createObject() expects at least 2 arguments: (type, class)".to_string()); }
    let obj_type = match &args[0] {
        BxValue::String(s) => s.to_lowercase(),
        _ => return Err("First argument to createObject must be a string (type)".to_string()),
    };
    
    let class_name = match &args[1] {
        BxValue::String(s) => s.clone(),
        _ => return Err("Second argument to createObject must be a string (class)".to_string()),
    };

    match obj_type.as_str() {
        "java" => {
            #[cfg(feature = "jvm")]
            {
                // Real JNI initialization and class loading would go here.
                // For this POC, we'll fall through to simulator if not matched.
            }
            
            // SIMULATOR for Demo Purposes
            if class_name == "java.util.ArrayList" {
                return Ok(BxValue::NativeObject(Rc::new(RefCell::new(JavaArrayListSimulator {
                    items: Vec::new(),
                }))));
            } else if class_name == "java.lang.System" {
                return Ok(BxValue::NativeObject(Rc::new(RefCell::new(JavaSystemSimulator))));
            }
            
            Err(format!("Java class {} not found in simulator", class_name))
        }
        "rust" | "native" => {
            // For this POC, we'll return a Mock Native Object if the class matches "Mock"
            if class_name == "Mock" {
                return Ok(BxValue::NativeObject(Rc::new(RefCell::new(MockNativeObject {
                    data: "I am a Rust Mock".to_string(),
                }))));
            }
            Err(format!("Native class {} not found", class_name))
        }
        _ => Err(format!("Unknown object type: {}", obj_type)),
    }
}

#[derive(Debug)]
struct MockNativeObject {
    data: String,
}

impl crate::types::BxNativeObject for MockNativeObject {
    fn get_property(&self, name: &str) -> BxValue {
        if name == "data" {
            return BxValue::String(self.data.clone());
        }
        BxValue::Null
    }

    fn set_property(&mut self, name: &str, value: BxValue) {
        if name == "data" {
            self.data = value.to_string();
        }
    }

    fn call_method(&mut self, _vm: &mut dyn BxVM, name: &str, _args: &[BxValue]) -> Result<BxValue, String> {
        if name == "greet" {
            return Ok(BxValue::String(format!("Rust says: {}", self.data)));
        }
        Err(format!("Method {} not found on native object", name))
    }
}

// --- Java Simulators for Demo ---

#[derive(Debug)]
struct JavaArrayListSimulator {
    items: Vec<BxValue>,
}

impl crate::types::BxNativeObject for JavaArrayListSimulator {
    fn get_property(&self, _name: &str) -> BxValue { BxValue::Null }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn call_method(&mut self, _vm: &mut dyn BxVM, name: &str, args: &[BxValue]) -> Result<BxValue, String> {
        match name {
            "add" => {
                if args.len() != 1 { return Err("ArrayList.add() expects 1 argument".to_string()); }
                self.items.push(args[0].clone());
                Ok(BxValue::Boolean(true))
            }
            "size" => {
                Ok(BxValue::Number(self.items.len() as f64))
            }
            "get" => {
                if args.len() != 1 { return Err("ArrayList.get() expects 1 argument".to_string()); }
                if let BxValue::Number(idx) = &args[0] {
                    let i = *idx as usize;
                    if i < self.items.len() {
                        Ok(self.items[i].clone())
                    } else {
                        Err("Index out of bounds".to_string())
                    }
                } else {
                    Err("Index must be a number".to_string())
                }
            }
            _ => Err(format!("Method {} not found on ArrayList", name)),
        }
    }
}

#[derive(Debug)]
struct JavaSystemSimulator;

impl crate::types::BxNativeObject for JavaSystemSimulator {
    fn get_property(&self, _name: &str) -> BxValue { BxValue::Null }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn call_method(&mut self, _vm: &mut dyn BxVM, name: &str, _args: &[BxValue]) -> Result<BxValue, String> {
        if name == "currenttimemillis" {
            let start = SystemTime::now();
            let since_the_epoch = start.duration_since(UNIX_EPOCH).expect("Time went backwards");
            return Ok(BxValue::Number(since_the_epoch.as_millis() as f64));
        }
        Err(format!("Method {} not found on java.lang.System", name))
    }
}
