#[cfg(not(target_arch = "wasm32"))]
use jni::{
    objects::{GlobalRef, JObject},
    InitArgsBuilder, JNIVersion, JavaVM,
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, OnceLock};

use crate::types::BxValue;

#[cfg(not(target_arch = "wasm32"))]
use crate::types::{BxNativeObject, BxVM};

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

pub fn create_java_object(vm: &mut dyn BxVM, class_name: &str) -> Result<BxValue, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (vm, class_name);
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
        
        let _global_ref = env.new_global_ref(obj)
            .map_err(|e| format!("Failed to create global ref: {}", e))?;
            
        let id = vm.string_new("java_object".to_string()); 
        // NativeObject needs to be on GC heap. BxVM should have a way to alloc it.
        // For now I'll just return null or error until I add native object support to BxVM trait
        let _ = id;
        Err("Java JNI support needs BxVM to support NativeObject allocation".to_string())
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub struct JniObject {
    _jvm: Arc<JavaVM>,
    _global_ref: GlobalRef,
}

#[cfg(not(target_arch = "wasm32"))]
impl BxNativeObject for JniObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {
    }

    fn call_method(&mut self, _vm: &mut dyn BxVM, name: &str, args: &[BxValue]) -> Result<BxValue, String> {
        Err(format!("Method {} with {} arguments not yet implemented for JNI objects", name, args.len()))
    }
}
