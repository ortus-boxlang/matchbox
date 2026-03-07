#[cfg(not(target_arch = "wasm32"))]
use jni::{
    objects::{GlobalRef, JObject, JValue, JObjectArray},
    InitArgsBuilder, JNIVersion, JavaVM, JNIEnv,
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use std::rc::Rc;
#[cfg(not(target_arch = "wasm32"))]
use std::cell::RefCell;

use crate::types::BxValue;
use crate::types::BxVM;

#[cfg(not(target_arch = "wasm32"))]
use crate::types::BxNativeObject;

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

pub fn create_java_object(vm: &mut dyn BxVM, class_name: &str, args: &[BxValue]) -> Result<BxValue, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (vm, class_name, args);
        return Err("Java interoperability is not supported in WASM environments.".to_string());
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let jvm = get_jvm()?;
        let mut env = jvm.attach_current_thread().map_err(|e| format!("Failed to attach thread: {}", e))?;
        
        // Convert class name from "java.util.ArrayList" to "java/util/ArrayList"
        let jni_class_name = class_name.replace(".", "/");
        let class = env.find_class(&jni_class_name)
            .map_err(|e| format!("Failed to find class {}: {}", class_name, e))?;

        // Support for non-default constructors via reflection
        let constructors_array: JObjectArray = env.call_method(&class, "getConstructors", "()[Ljava/lang/reflect/Constructor;", &[])
            .map_err(|e| format!("Failed to get constructors for {}: {}", class_name, e))?.l().map_err(|e| e.to_string())?.into();
        
        let constructors_count = env.get_array_length(&constructors_array).map_err(|e| e.to_string())?;
        let mut target_constructor = None;

        for i in 0..constructors_count {
            let constructor = env.get_object_array_element(&constructors_array, i).map_err(|e| e.to_string())?;
            let params_array: JObjectArray = env.call_method(&constructor, "getParameterTypes", "()[Ljava/lang/Class;", &[])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?.into();
            let params_count = env.get_array_length(&params_array).map_err(|e| e.to_string())?;
            
            if params_count as usize == args.len() {
                target_constructor = Some(constructor);
                break;
            }
        }

        let constructor = target_constructor.ok_or_else(|| format!("Constructor with {} arguments not found on class {}", args.len(), class_name))?;

        // 3. Prepare Arguments
        let object_class = env.find_class("java/lang/Object").map_err(|e| e.to_string())?;
        let j_args_array = env.new_object_array(args.len() as i32, object_class, JObject::null())
            .map_err(|e| e.to_string())?;
        
        let dummy_jni_obj = JniObject {
            _jvm: Arc::clone(&jvm),
            _global_ref: env.new_global_ref(JObject::null()).map_err(|e| e.to_string())?,
        };

        for (i, arg) in args.iter().enumerate() {
            let j_arg = dummy_jni_obj.bx_to_java(&mut env, vm, arg)?;
            env.set_object_array_element(&j_args_array, i as i32, j_arg).map_err(|e| e.to_string())?;
        }

        // 4. Invoke Constructor
        let obj = env.call_method(&constructor, "newInstance", "([Ljava/lang/Object;)Ljava/lang/Object;", &[
            (&j_args_array).into()
        ]).map_err(|e| format!("Failed to instantiate {}: {}", class_name, e))?.l().map_err(|e| e.to_string())?;
        
        let global_ref = env.new_global_ref(obj)
            .map_err(|e| format!("Failed to create global ref: {}", e))?;
            
        let jni_obj = JniObject {
            _jvm: Arc::clone(&jvm),
            _global_ref: global_ref,
        };
        
        let id = vm.native_object_new(Rc::new(RefCell::new(jni_obj)));
        Ok(BxValue::new_ptr(id))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct JniObject {
    _jvm: Arc<JavaVM>,
    _global_ref: GlobalRef,
}

#[cfg(not(target_arch = "wasm32"))]
impl JniObject {
    fn bx_to_java<'a>(&self, env: &mut JNIEnv<'a>, vm: &dyn BxVM, val: &BxValue) -> Result<JObject<'a>, String> {
        if val.is_null() {
            Ok(JObject::null())
        } else if val.is_bool() {
            let class = env.find_class("java/lang/Boolean").map_err(|e| e.to_string())?;
            let obj = env.new_object(class, "(Z)V", &[JValue::from(val.as_bool())])
                .map_err(|e| e.to_string())?;
            Ok(obj)
        } else if val.is_int() {
            let class = env.find_class("java/lang/Integer").map_err(|e| e.to_string())?;
            let obj = env.new_object(class, "(I)V", &[JValue::from(val.as_int())])
                .map_err(|e| e.to_string())?;
            Ok(obj)
        } else if val.is_number() {
            let class = env.find_class("java/lang/Double").map_err(|e| e.to_string())?;
            let obj = env.new_object(class, "(D)V", &[JValue::from(val.as_number())])
                .map_err(|e| e.to_string())?;
            Ok(obj)
        } else {
            let s = vm.to_string(*val);
            let j_str = env.new_string(s).map_err(|e| e.to_string())?;
            Ok(j_str.into())
        }
    }

    fn java_to_bx(&self, env: &mut JNIEnv, obj: &JObject) -> Result<BxValue, String> {
        if obj.is_null() {
            return Ok(BxValue::new_null());
        }

        let class = env.get_object_class(obj).map_err(|e| e.to_string())?;
        let class_name_obj = env.call_method(class, "getName", "()Ljava/lang/String;", &[])
            .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;
        let class_name: String = env.get_string(&class_name_obj.into()).map_err(|e| e.to_string())?.into();

        match class_name.as_str() {
            "java.lang.Boolean" => {
                let val = env.call_method(obj, "booleanValue", "()Z", &[]).map_err(|e| e.to_string())?.z().map_err(|e| e.to_string())?;
                Ok(BxValue::new_bool(val))
            }
            "java.lang.Integer" | "java.lang.Short" | "java.lang.Byte" => {
                let val = env.call_method(obj, "intValue", "()I", &[]).map_err(|e| e.to_string())?.i().map_err(|e| e.to_string())?;
                Ok(BxValue::new_int(val))
            }
            "java.lang.Long" => {
                let val = env.call_method(obj, "longValue", "()J", &[]).map_err(|e| e.to_string())?.j().map_err(|e| e.to_string())?;
                Ok(BxValue::new_number(val as f64))
            }
            "java.lang.Double" | "java.lang.Float" => {
                let val = env.call_method(obj, "doubleValue", "()D", &[]).map_err(|e| e.to_string())?.d().map_err(|e| e.to_string())?;
                Ok(BxValue::new_number(val))
            }
            _ => {
                Err("Conversion requires VM access".to_string())
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl BxNativeObject for JniObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {
    }

    fn call_method(&mut self, vm: &mut dyn BxVM, name: &str, args: &[BxValue]) -> Result<BxValue, String> {
        let mut env = self._jvm.attach_current_thread().map_err(|e| format!("Failed to attach thread: {}", e))?;
        let obj = self._global_ref.as_obj();

        // 1. Get Class
        let class = env.get_object_class(obj).map_err(|e| e.to_string())?;
        
        // 2. Find Method via Reflection
        let methods_array: JObjectArray = env.call_method(&class, "getMethods", "()[Ljava/lang/reflect/Method;", &[])
            .map_err(|e| format!("Failed to get methods: {}", e))?.l().map_err(|e| e.to_string())?.into();
        
        let methods_count = env.get_array_length(&methods_array).map_err(|e| e.to_string())?;
        let mut target_method = None;

        let name_lower = name.to_lowercase();

        for i in 0..methods_count {
            let method = env.get_object_array_element(&methods_array, i).map_err(|e| e.to_string())?;
            let m_name_obj = env.call_method(&method, "getName", "()Ljava/lang/String;", &[])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;
            let m_name: String = env.get_string(&m_name_obj.into()).map_err(|e| e.to_string())?.into();
            
            if m_name.to_lowercase() == name_lower {
                let params_array: JObjectArray = env.call_method(&method, "getParameterTypes", "()[Ljava/lang/Class;", &[])
                    .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?.into();
                let params_count = env.get_array_length(&params_array).map_err(|e| e.to_string())?;
                
                if params_count as usize == args.len() {
                    target_method = Some(method);
                    break;
                }
            }
        }

        let method = target_method.ok_or_else(|| format!("Method {} with {} arguments not found on class", name, args.len()))?;

        // 3. Prepare Arguments
        let object_class = env.find_class("java/lang/Object").map_err(|e| e.to_string())?;
        let j_args_array = env.new_object_array(args.len() as i32, object_class, JObject::null())
            .map_err(|e| e.to_string())?;
        
        for (i, arg) in args.iter().enumerate() {
            let j_arg = self.bx_to_java(&mut env, vm, arg)?;
            env.set_object_array_element(&j_args_array, i as i32, j_arg).map_err(|e| e.to_string())?;
        }

        // 4. Invoke
        let result_obj = env.call_method(&method, "invoke", "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;", &[
            obj.into(),
            (&j_args_array).into()
        ]).map_err(|e| format!("Invocation of {} failed: {}", name, e))?.l().map_err(|e| e.to_string())?;

        // 5. Convert Result
        if result_obj.is_null() {
            Ok(BxValue::new_null())
        } else {
            let res_class = env.get_object_class(&result_obj).map_err(|e| e.to_string())?;
            let res_class_name_obj = env.call_method(res_class, "getName", "()Ljava/lang/String;", &[])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;
            let res_class_name: String = env.get_string(&res_class_name_obj.into()).map_err(|e| e.to_string())?.into();

            if res_class_name == "java.lang.String" {
                let s: String = env.get_string(&result_obj.into()).map_err(|e| e.to_string())?.into();
                let id = vm.string_new(s);
                Ok(BxValue::new_ptr(id))
            } else {
                match self.java_to_bx(&mut env, &result_obj) {
                    Ok(val) => Ok(val),
                    Err(_) => {
                        let global_ref = env.new_global_ref(&result_obj).map_err(|e| e.to_string())?;
                        let new_jni_obj = JniObject {
                            _jvm: Arc::clone(&self._jvm),
                            _global_ref: global_ref,
                        };
                        let id = vm.native_object_new(Rc::new(RefCell::new(new_jni_obj)));
                        Ok(BxValue::new_ptr(id))
                    }
                }
            }
        }
    }
}
