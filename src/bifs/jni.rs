#[cfg(not(target_arch = "wasm32"))]
use jni::{
    objects::{GlobalRef, JObject, JValue},
    InitArgsBuilder, JNIVersion, JavaVM,
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, OnceLock};

use crate::types::BxValue;

#[cfg(not(target_arch = "wasm32"))]
use crate::types::{BxNativeObject, BxVM};
#[cfg(not(target_arch = "wasm32"))]
use std::cell::RefCell;
#[cfg(not(target_arch = "wasm32"))]
use std::rc::Rc;

#[cfg(not(target_arch = "wasm32"))]
static JVM: OnceLock<Result<Arc<JavaVM>, String>> = OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
fn get_jvm() -> Result<Arc<JavaVM>, String> {
    let res = JVM.get_or_init(|| {
        let jvm_args = InitArgsBuilder::new()
            .version(JNIVersion::V8)
            .build()
            .map_err(|e| format!("Failed to build JVM args: {}", e))?;
        let jvm = JavaVM::new(jvm_args).map_err(|e| format!("Failed to create JVM: {}", e))?;
        Ok(Arc::new(jvm))
    });
    res.clone()
}

pub fn create_java_object(#[allow(unused_variables)] class_name: &str) -> Result<BxValue, String> {
    #[cfg(target_arch = "wasm32")]
    {
        return Err("Java interoperability is not supported in WASM environments.".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let jvm = get_jvm()?;
        let mut env = jvm.attach_current_thread().map_err(|e| format!("Failed to attach thread: {}", e))?;
        
        // Convert class name from "java.util.ArrayList" to "java/util/ArrayList"
        let jni_class_name = class_name.replace(".", "/");
        
        let obj = env.new_object(&jni_class_name, "()V", &[])
            .map_err(|e| format!("Failed to instantiate {}: {}", class_name, e))?;
        
        let global_ref = env.new_global_ref(obj)
            .map_err(|e| format!("Failed to create global ref: {}", e))?;
            
        Ok(BxValue::NativeObject(Rc::new(RefCell::new(JniObject {
            jvm: Arc::clone(&jvm),
            global_ref,
        }))))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct JniObject {
    jvm: Arc<JavaVM>,
    global_ref: GlobalRef,
}

#[cfg(not(target_arch = "wasm32"))]
impl BxNativeObject for JniObject {
    fn get_property(&self, _name: &str) -> BxValue {
        // Full reflection for field access is complex, returning Null for now
        BxValue::Null
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {
        // Field mutation not implemented yet
    }

    fn call_method(&mut self, _vm: &mut dyn BxVM, name: &str, args: &[BxValue]) -> Result<BxValue, String> {
        let mut env = self.jvm.attach_current_thread()
            .map_err(|e| format!("Failed to attach thread: {}", e))?;
            
        let obj = self.global_ref.as_obj();
        let class = env.get_object_class(obj).map_err(|e| format!("Failed to get class: {}", e))?;

        // Special handling for constructors via .init()
        if name.to_lowercase() == "init" {
            let get_constructors = env.call_method(&class, "getConstructors", "()[Ljava/lang/reflect/Constructor;", &[])
                .map_err(|e| format!("Failed to get constructors: {}", e))?
                .l().map_err(|e| format!("Invalid return type: {}", e))?;
            
            let constructors_array: &jni::objects::JObjectArray = (&get_constructors).into();
            let constructor_count = env.get_array_length(constructors_array).map_err(|e| format!("Failed to get array length: {}", e))?;

            for i in 0..constructor_count {
                let constructor_obj = env.get_object_array_element(constructors_array, i).map_err(|e| format!("Failed to get array element: {}", e))?;
                
                let parameter_types_val = env.call_method(&constructor_obj, "getParameterTypes", "()[Ljava/lang/Class;", &[])
                    .map_err(|e| format!("Failed to get parameter types: {}", e))?
                    .l().map_err(|e| format!("Invalid return type: {}", e))?;
                let parameter_types_array: &jni::objects::JObjectArray = (&parameter_types_val).into();
                let param_count = env.get_array_length(parameter_types_array).map_err(|e| format!("Failed to get array length: {}", e))?;

                if param_count as usize == args.len() {
                    // Found matching constructor count. Now check types.
                    let mut compatible = true;
                    for (idx, arg) in args.iter().enumerate() {
                        let param_type_obj = env.get_object_array_element(parameter_types_array, idx as i32).map_err(|e| format!("Failed to get parameter type: {}", e))?;
                        let param_class: &jni::objects::JClass = (&param_type_obj).into();
                        
                        let arg_class_name = match arg {
                            BxValue::String(_) => "java/lang/String",
                            BxValue::Number(_) => "java/lang/Double",
                            BxValue::Boolean(_) => "java/lang/Boolean",
                            _ => "java/lang/Object",
                        };
                        let arg_class = env.find_class(arg_class_name).map_err(|e| format!("Class not found: {}", e))?;
                        
                        let is_assignable = env.call_method(param_class, "isAssignableFrom", "(Ljava/lang/Class;)Z", &[JValue::from(&arg_class)])
                            .map_err(|e| format!("Failed to call isAssignableFrom: {}", e))?
                            .z().map_err(|e| format!("Invalid return type: {}", e))?;
                        
                        if !is_assignable {
                            compatible = false;
                            break;
                        }
                    }

                    if !compatible {
                        continue;
                    }

                    // Found compatible constructor.
                    let object_class = env.find_class("java/lang/Object").map_err(|e| format!("Class not found: {}", e))?;
                    let args_array = env.new_object_array(args.len() as i32, object_class, JObject::null())
                        .map_err(|e| format!("Failed to create args array: {}", e))?;
                    
                    let mut jobjs = Vec::new();
                    for arg in args {
                        let jobj = match arg {
                            BxValue::String(s) => {
                                JObject::from(env.new_string(s).map_err(|e| format!("Failed to create string: {}", e))?)
                            }
                            BxValue::Number(n) => {
                                let double_class = env.find_class("java/lang/Double").map_err(|e| format!("Class not found: {}", e))?;
                                env.new_object(double_class, "(D)V", &[JValue::from(*n)]).map_err(|e| format!("Failed to wrap double: {}", e))?
                            }
                            BxValue::Boolean(b) => {
                                let boolean_class = env.find_class("java/lang/Boolean").map_err(|e| format!("Class not found: {}", e))?;
                                env.new_object(boolean_class, "(Z)V", &[JValue::from(*b)]).map_err(|e| format!("Failed to wrap boolean: {}", e))?
                            }
                            _ => return Err(format!("Unsupported argument type for JNI constructor: {:?}", arg)),
                        };
                        jobjs.push(jobj);
                    }

                    let jargs: Vec<JValue> = jobjs.iter().map(|obj| JValue::Object(obj)).collect();

                    for (idx, jarg) in jargs.iter().enumerate() {
                        env.set_object_array_element(&args_array, idx as i32, jarg.l().map_err(|e| format!("Arg is not an object: {}", e))?)
                            .map_err(|e| format!("Failed to set array element: {}", e))?;
                    }

                    let new_instance = env.call_method(&constructor_obj, "newInstance", "([Ljava/lang/Object;)Ljava/lang/Object;", 
                        &[JValue::Object(&args_array)])
                        .map_err(|e| format!("Failed to invoke constructor: {}", e))?
                        .l().map_err(|e| format!("Constructor returned invalid type: {}", e))?;

                    let global_ref = env.new_global_ref(new_instance).map_err(|e| format!("Failed to create global ref: {}", e))?;
                    return Ok(BxValue::NativeObject(Rc::new(RefCell::new(JniObject {
                        jvm: Arc::clone(&self.jvm),
                        global_ref,
                    }))));
                }
            }
            return Err(format!("Constructor with {} arguments not found on class", args.len()));
        }

        // 1. Convert BxValue args to JNI objects
        let mut jobjs = Vec::new();
        for arg in args {
            match arg {
                BxValue::String(s) => {
                    jobjs.push(JObject::from(env.new_string(s).map_err(|e| format!("Failed to create string: {}", e))?));
                }
                BxValue::Number(n) => {
                    let double_class = env.find_class("java/lang/Double").map_err(|e| format!("Class not found: {}", e))?;
                    jobjs.push(env.new_object(double_class, "(D)V", &[JValue::from(*n)])
                        .map_err(|e| format!("Failed to wrap double: {}", e))?);
                }
                BxValue::Boolean(b) => {
                    let boolean_class = env.find_class("java/lang/Boolean").map_err(|e| format!("Class not found: {}", e))?;
                    jobjs.push(env.new_object(boolean_class, "(Z)V", &[JValue::from(*b)])
                        .map_err(|e| format!("Failed to wrap boolean: {}", e))?);
                }
                _ => return Err(format!("Unsupported argument type for JNI: {:?}", arg)),
            }
        }
        let jargs: Vec<JValue> = jobjs.iter().map(|obj| JValue::Object(obj)).collect();

        // 2. Use reflection to find the method
        let get_methods = env.call_method(&class, "getMethods", "()[Ljava/lang/reflect/Method;", &[])
            .map_err(|e| format!("Failed to get methods: {}", e))?
            .l().map_err(|e| format!("Invalid return type: {}", e))?;
        
        let methods_array: &jni::objects::JObjectArray = (&get_methods).into();
        let method_count = env.get_array_length(methods_array).map_err(|e| format!("Failed to get array length: {}", e))?;

        for i in 0..method_count {
            let method_obj = env.get_object_array_element(methods_array, i).map_err(|e| format!("Failed to get array element: {}", e))?;
            
            let name_val = env.call_method(&method_obj, "getName", "()Ljava/lang/String;", &[])
                .map_err(|e| format!("Failed to get method name: {}", e))?
                .l().map_err(|e| format!("Invalid return type: {}", e))?;
            let rname: String = env.get_string((&name_val).into()).map_err(|e| format!("Failed to get string: {}", e))?.into();

            if rname.to_lowercase() == name.to_lowercase() {
                let parameter_types_val = env.call_method(&method_obj, "getParameterTypes", "()[Ljava/lang/Class;", &[])
                    .map_err(|e| format!("Failed to get parameter types: {}", e))?
                    .l().map_err(|e| format!("Invalid return type: {}", e))?;
                let parameter_types_array: &jni::objects::JObjectArray = (&parameter_types_val).into();
                let param_count = env.get_array_length(parameter_types_array).map_err(|e| format!("Failed to get array length: {}", e))?;

                if param_count as usize == args.len() {
                    // Found matching method count. Now check types.
                    let mut compatible = true;
                    for (idx, arg) in args.iter().enumerate() {
                        let param_type_obj = env.get_object_array_element(parameter_types_array, idx as i32).map_err(|e| format!("Failed to get parameter type: {}", e))?;
                        let param_class: &jni::objects::JClass = (&param_type_obj).into();
                        
                        let arg_class_name = match arg {
                            BxValue::String(_) => "java/lang/String",
                            BxValue::Number(_) => "java/lang/Double",
                            BxValue::Boolean(_) => "java/lang/Boolean",
                            _ => "java/lang/Object",
                        };
                        let arg_class = env.find_class(arg_class_name).map_err(|e| format!("Class not found: {}", e))?;
                        
                        let is_assignable = env.call_method(param_class, "isAssignableFrom", "(Ljava/lang/Class;)Z", &[JValue::from(&arg_class)])
                            .map_err(|e| format!("Failed to call isAssignableFrom: {}", e))?
                            .z().map_err(|e| format!("Invalid return type: {}", e))?;
                        
                        if !is_assignable {
                            compatible = false;
                            break;
                        }
                    }

                    if !compatible {
                        continue;
                    }

                    // Found a candidate. Let's invoke it.
                    let object_class = env.find_class("java/lang/Object").map_err(|e| format!("Class not found: {}", e))?;
                    let args_array = env.new_object_array(args.len() as i32, object_class, JObject::null())
                        .map_err(|e| format!("Failed to create args array: {}", e))?;
                    
                    for (idx, jarg) in jargs.iter().enumerate() {
                        env.set_object_array_element(&args_array, idx as i32, jarg.l().map_err(|e| format!("Arg is not an object: {}", e))?)
                            .map_err(|e| format!("Failed to set array element: {}", e))?;
                    }

                    let result_obj = env.call_method(&method_obj, "invoke", "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;", 
                        &[JValue::Object(&obj), JValue::Object(&args_array)])
                        .map_err(|e| format!("Failed to invoke method {}: {}", name, e))?
                        .l().map_err(|e| format!("Invoke returned invalid type: {}", e))?;

                    if result_obj.is_null() {
                        return Ok(BxValue::Null);
                    }

                    // Convert result back to BxValue
                    let res_class = env.get_object_class(&result_obj).map_err(|e| format!("Failed to get result class: {}", e))?;
                    let res_class_name_val = env.call_method(&res_class, "getName", "()Ljava/lang/String;", &[])
                        .and_then(|v| v.l())
                        .and_then(|o| Ok(env.get_string((&o).into())?.into()))
                        .unwrap_or_else(|_| "unknown".to_string());

                    return match res_class_name_val.as_str() {
                        "java.lang.String" => {
                            let s: String = env.get_string((&result_obj).into()).map_err(|e| format!("Failed to get string: {}", e))?.into();
                            Ok(BxValue::String(s))
                        }
                        "java.lang.Double" | "java.lang.Float" | "java.lang.Integer" | "java.lang.Long" => {
                            let d = env.call_method(&result_obj, "doubleValue", "()D", &[])
                                .map_err(|e| format!("Failed to get double value: {}", e))?
                                .d().map_err(|e| format!("Invalid return type: {}", e))?;
                            Ok(BxValue::Number(d))
                        }
                        "java.lang.Boolean" => {
                            let b = env.call_method(&result_obj, "booleanValue", "()Z", &[])
                                .map_err(|e| format!("Failed to get boolean value: {}", e))?
                                .z().map_err(|e| format!("Invalid return type: {}", e))?;
                            Ok(BxValue::Boolean(b))
                        }
                        _ => {
                            let global_ref = env.new_global_ref(result_obj).map_err(|e| format!("Failed to create global ref: {}", e))?;
                            Ok(BxValue::NativeObject(Rc::new(RefCell::new(JniObject {
                                jvm: Arc::clone(&self.jvm),
                                global_ref,
                            }))))
                        }
                    };
                }
            }
        }

        Err(format!("Method {} with {} arguments not found on class", name, args.len()))
    }
}


