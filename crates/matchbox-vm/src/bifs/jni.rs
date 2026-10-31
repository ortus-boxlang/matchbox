use crate::types::{BxVM, BxValue};

#[cfg(target_arch = "wasm32")]
pub fn create_java_class(vm: &mut dyn BxVM, class_name: &str) -> Result<BxValue, String> {
    let _ = (vm, class_name);
    Err("JNI class loading is not supported in WASM".into())
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::create_java_class;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::{BxVM, BxValue};
    use crate::types::BxNativeObject;
    use jni::{
        objects::{GlobalRef, JObject, JObjectArray, JValue},
        InitArgsBuilder, JNIEnv, JNIVersion, JavaVM,
    };
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, OnceLock};

    static JVM: OnceLock<Result<Arc<JavaVM>, String>> = OnceLock::new();

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

    /// Optional JNI backend: resolve without invoking a constructor.
    pub fn create_java_class(vm: &mut dyn BxVM, class_name: &str) -> Result<BxValue, String> {
        let jvm = get_jvm()?;
        let mut env = jvm.attach_current_thread().map_err(|error| error.to_string())?;
        env.find_class(class_name.replace('.', "/")).map_err(|error| format!("Java class not provided and JNI lookup failed for {class_name}: {error}"))?;
        Ok(BxValue::new_ptr(vm.native_object_new(Rc::new(RefCell::new(JniClassReference(class_name.into()))))))
    }

    #[derive(Debug)]
    struct JniClassReference(String);

    impl BxNativeObject for JniClassReference {
        fn get_property(&self, _name: &str) -> BxValue { BxValue::new_null() }
        fn set_property(&mut self, _name: &str, _value: BxValue) {}
        fn call_method(&mut self, vm: &mut dyn BxVM, _id: usize, name: &str, args: &[BxValue]) -> Result<BxValue, String> {
            let jvm = get_jvm()?;
            let mut env = jvm.attach_current_thread().map_err(|e| format!("Failed to attach thread: {}", e))?;
            let class = env
                .find_class(self.0.replace('.', "/"))
                .map_err(|e| format!("Failed to find class {}: {}", self.0, e))?;
            if name.eq_ignore_ascii_case("init") {
                let object = new_instance(&mut env, vm, &class, &self.0, args)?;
                return wrap_object(&mut env, vm, &jvm, &object);
            }
            let result = invoke_method(&mut env, vm, &class, &JObject::null(), name, args, true)?;
            java_result_to_bx(&mut env, vm, &jvm, &result)
        }
    }

    #[derive(Debug)]
    pub struct JniObject {
        jvm: Arc<JavaVM>,
        global_ref: GlobalRef,
    }

    impl BxNativeObject for JniObject {
        fn has_method(&self, _name: &str) -> bool { true }
        fn get_property(&self, _name: &str) -> BxValue {
            BxValue::new_null()
        }

        fn set_property(&mut self, _name: &str, _value: BxValue) {}

        fn call_method(
            &mut self,
            vm: &mut dyn BxVM,
            _id: usize,
            name: &str,
            args: &[BxValue],
        ) -> Result<BxValue, String> {
            let mut env = self
                .jvm
                .attach_current_thread()
                .map_err(|e| format!("Failed to attach thread: {}", e))?;
            let obj = self.global_ref.as_obj();
            let class = env.get_object_class(obj).map_err(|e| e.to_string())?;
            let result = invoke_method(&mut env, vm, &class, obj, name, args, false)?;
            java_result_to_bx(&mut env, vm, &self.jvm, &result)
        }
    }

    fn wrap_object(
        env: &mut JNIEnv,
        vm: &mut dyn BxVM,
        jvm: &Arc<JavaVM>,
        obj: &JObject,
    ) -> Result<BxValue, String> {
        let global_ref = env
            .new_global_ref(obj)
            .map_err(|e| format!("Failed to create global ref: {}", e))?;
        let id = vm.native_object_new(Rc::new(RefCell::new(JniObject {
            jvm: Arc::clone(jvm),
            global_ref,
        })));
        Ok(BxValue::new_ptr(id))
    }

    /// The message of the pending Java exception (unwrapping reflection's wrapper), clearing it.
    fn pending_exception_message(env: &mut JNIEnv) -> Option<String> {
        if !env.exception_check().unwrap_or(false) {
            return None;
        }
        let exception = env.exception_occurred().ok()?;
        env.exception_clear().ok()?;
        let mut target: JObject = exception.into();
        if class_name_of(env, &target).ok()? == "java.lang.reflect.InvocationTargetException" {
            let cause = env
                .call_method(&target, "getCause", "()Ljava/lang/Throwable;", &[])
                .ok()?
                .l()
                .ok()?;
            if !cause.is_null() {
                target = cause;
            }
        }
        let text = env
            .call_method(&target, "toString", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        Some(env.get_string(&text.into()).ok()?.into())
    }

    fn jni_error(env: &mut JNIEnv, context: &str, error: jni::errors::Error) -> String {
        match pending_exception_message(env) {
            Some(message) => format!("{context}: {message}"),
            None => format!("{context}: {error}"),
        }
    }

    fn class_name_of(env: &mut JNIEnv, obj: &JObject) -> Result<String, String> {
        let class = env.get_object_class(obj).map_err(|e| e.to_string())?;
        class_name(env, &class)
    }

    fn class_name(env: &mut JNIEnv, class: &JObject) -> Result<String, String> {
        let name = env
            .call_method(class, "getName", "()Ljava/lang/String;", &[])
            .map_err(|e| e.to_string())?
            .l()
            .map_err(|e| e.to_string())?;
        let name: String = env.get_string(&name.into()).map_err(|e| e.to_string())?.into();
        Ok(name)
    }

    /// Java class names of a constructor's or method's parameters (`int`, `java.lang.String`, ...).
    fn parameter_types(env: &mut JNIEnv, executable: &JObject) -> Result<Vec<String>, String> {
        let params: JObjectArray = env
            .call_method(executable, "getParameterTypes", "()[Ljava/lang/Class;", &[])
            .map_err(|e| e.to_string())?
            .l()
            .map_err(|e| e.to_string())?
            .into();
        let count = env.get_array_length(&params).map_err(|e| e.to_string())?;
        let mut names = Vec::with_capacity(count as usize);
        for i in 0..count {
            let class = env.get_object_array_element(&params, i).map_err(|e| e.to_string())?;
            names.push(class_name(env, &class)?);
        }
        Ok(names)
    }

    fn integral(vm: &dyn BxVM, value: BxValue) -> Option<i64> {
        let _ = vm;
        if value.is_int() {
            Some(value.as_int() as i64)
        } else if value.is_number() {
            let n = value.as_number();
            (n.fract() == 0.0 && n.abs() < 9.0e15).then_some(n as i64)
        } else {
            None
        }
    }

    fn is_numeric(value: BxValue) -> bool {
        value.is_int() || value.is_number()
    }

    /// How well `value` fits a Java parameter type (higher is better, `None` means it does not fit).
    fn score_argument(vm: &dyn BxVM, value: BxValue, param: &str) -> Option<u32> {
        if value.is_null() {
            return match param {
                "boolean" | "byte" | "short" | "char" | "int" | "long" | "float" | "double" => None,
                _ => Some(1),
            };
        }
        let fits_range = |min: i64, max: i64| integral(vm, value).filter(|n| *n >= min && *n <= max);
        match param {
            "boolean" | "java.lang.Boolean" => value.is_bool().then_some(5),
            // Whole numbers prefer int, then long, then double, like BoxLang's integer literals.
            "int" | "java.lang.Integer" => fits_range(i32::MIN as i64, i32::MAX as i64).map(|_| 5),
            "long" | "java.lang.Long" => fits_range(i64::MIN, i64::MAX).map(|_| 4),
            "short" | "java.lang.Short" => fits_range(i16::MIN as i64, i16::MAX as i64).map(|_| 1),
            "byte" | "java.lang.Byte" => fits_range(i8::MIN as i64, i8::MAX as i64).map(|_| 1),
            "double" | "java.lang.Double" => {
                if integral(vm, value).is_some() { Some(2) } else { is_numeric(value).then_some(5) }
            }
            "float" | "java.lang.Float" => is_numeric(value).then_some(1),
            "char" | "java.lang.Character" => {
                (vm.is_string_value(value) && vm.to_string(value).chars().count() == 1).then_some(3)
            }
            "java.lang.String" => Some(if vm.is_string_value(value) { 4 } else { 1 }),
            "java.lang.CharSequence" => vm.is_string_value(value).then_some(3),
            "java.lang.Number" => is_numeric(value).then_some(3),
            "java.lang.Object" => Some(1),
            _ => None,
        }
    }

    fn new_wrapper<'a>(
        env: &mut JNIEnv<'a>,
        class: &str,
        signature: &str,
        value: JValue,
    ) -> Result<JObject<'a>, String> {
        let class = env.find_class(class).map_err(|e| e.to_string())?;
        env.new_object(class, signature, &[value]).map_err(|e| e.to_string())
    }

    /// Box `value` as the Java type the selected overload expects.
    fn to_java<'a>(
        env: &mut JNIEnv<'a>,
        vm: &dyn BxVM,
        value: BxValue,
        param: &str,
    ) -> Result<JObject<'a>, String> {
        if value.is_null() {
            return Ok(JObject::null());
        }
        let whole = integral(vm, value);
        let number = if value.is_int() { value.as_int() as f64 } else { value.as_number() };
        match param {
            "int" | "java.lang.Integer" => new_wrapper(env, "java/lang/Integer", "(I)V", JValue::Int(whole.unwrap_or(0) as i32)),
            "long" | "java.lang.Long" => new_wrapper(env, "java/lang/Long", "(J)V", JValue::Long(whole.unwrap_or(0))),
            "short" | "java.lang.Short" => new_wrapper(env, "java/lang/Short", "(S)V", JValue::Short(whole.unwrap_or(0) as i16)),
            "byte" | "java.lang.Byte" => new_wrapper(env, "java/lang/Byte", "(B)V", JValue::Byte(whole.unwrap_or(0) as i8)),
            "double" | "java.lang.Double" => new_wrapper(env, "java/lang/Double", "(D)V", JValue::Double(number)),
            "float" | "java.lang.Float" => new_wrapper(env, "java/lang/Float", "(F)V", JValue::Float(number as f32)),
            "char" | "java.lang.Character" => {
                let code = vm.to_string(value).encode_utf16().next().unwrap_or(0);
                new_wrapper(env, "java/lang/Character", "(C)V", JValue::Char(code))
            }
            _ => {
                if value.is_bool() {
                    new_wrapper(env, "java/lang/Boolean", "(Z)V", JValue::Bool(value.as_bool() as u8))
                } else if value.is_int() {
                    new_wrapper(env, "java/lang/Integer", "(I)V", JValue::Int(value.as_int()))
                } else if value.is_number() {
                    new_wrapper(env, "java/lang/Double", "(D)V", JValue::Double(value.as_number()))
                } else {
                    let text = env.new_string(vm.to_string(value)).map_err(|e| e.to_string())?;
                    Ok(text.into())
                }
            }
        }
    }

    /// The candidate whose parameter types fit `args` best, with those parameter types.
    fn select_overload<'a>(
        env: &mut JNIEnv<'a>,
        vm: &dyn BxVM,
        candidates: Vec<JObject<'a>>,
        args: &[BxValue],
    ) -> Result<Option<(JObject<'a>, Vec<String>)>, String> {
        let mut best: Option<(u32, JObject<'a>, Vec<String>)> = None;
        for candidate in candidates {
            let params = parameter_types(env, &candidate)?;
            if params.len() != args.len() {
                continue;
            }
            let mut total = 0;
            let mut fits = true;
            for (value, param) in args.iter().zip(&params) {
                match score_argument(vm, *value, param) {
                    Some(score) => total += score,
                    None => {
                        fits = false;
                        break;
                    }
                }
            }
            if fits && best.as_ref().is_none_or(|(score, _, _)| total > *score) {
                best = Some((total, candidate, params));
            }
        }
        Ok(best.map(|(_, candidate, params)| (candidate, params)))
    }

    fn argument_array<'a>(
        env: &mut JNIEnv<'a>,
        vm: &dyn BxVM,
        args: &[BxValue],
        params: &[String],
    ) -> Result<JObjectArray<'a>, String> {
        let object_class = env.find_class("java/lang/Object").map_err(|e| e.to_string())?;
        let array = env
            .new_object_array(args.len() as i32, object_class, JObject::null())
            .map_err(|e| e.to_string())?;
        for (i, (value, param)) in args.iter().zip(params).enumerate() {
            let converted = to_java(env, vm, *value, param)?;
            env.set_object_array_element(&array, i as i32, converted)
                .map_err(|e| e.to_string())?;
        }
        Ok(array)
    }

    fn objects_of<'a>(env: &mut JNIEnv<'a>, array: JObjectArray<'a>) -> Result<Vec<JObject<'a>>, String> {
        let count = env.get_array_length(&array).map_err(|e| e.to_string())?;
        (0..count)
            .map(|i| env.get_object_array_element(&array, i).map_err(|e| e.to_string()))
            .collect()
    }

    fn new_instance<'a>(
        env: &mut JNIEnv<'a>,
        vm: &dyn BxVM,
        class: &JObject,
        class_name: &str,
        args: &[BxValue],
    ) -> Result<JObject<'a>, String> {
        let constructors: JObjectArray = env
            .call_method(class, "getConstructors", "()[Ljava/lang/reflect/Constructor;", &[])
            .map_err(|e| format!("Failed to get constructors for {}: {}", class_name, e))?
            .l()
            .map_err(|e| e.to_string())?
            .into();
        let candidates = objects_of(env, constructors)?;
        let (constructor, params) = select_overload(env, vm, candidates, args)?.ok_or_else(|| {
            format!("No constructor of {} accepts {} argument(s) of these types", class_name, args.len())
        })?;
        let array = argument_array(env, vm, args, &params)?;
        let created = env.call_method(
            &constructor,
            "newInstance",
            "([Ljava/lang/Object;)Ljava/lang/Object;",
            &[(&array).into()],
        );
        match created {
            Ok(value) => value.l().map_err(|e| e.to_string()),
            Err(error) => Err(jni_error(env, &format!("Failed to instantiate {class_name}"), error)),
        }
    }

    /// Call `name` on `receiver` (null for a static call) by reflection.
    fn invoke_method<'a>(
        env: &mut JNIEnv<'a>,
        vm: &dyn BxVM,
        class: &JObject,
        receiver: &JObject,
        name: &str,
        args: &[BxValue],
        static_only: bool,
    ) -> Result<JObject<'a>, String> {
        let methods: JObjectArray = env
            .call_method(class, "getMethods", "()[Ljava/lang/reflect/Method;", &[])
            .map_err(|e| format!("Failed to get methods: {}", e))?
            .l()
            .map_err(|e| e.to_string())?
            .into();
        let mut candidates = Vec::new();
        for method in objects_of(env, methods)? {
            let method_name = env
                .call_method(&method, "getName", "()Ljava/lang/String;", &[])
                .map_err(|e| e.to_string())?
                .l()
                .map_err(|e| e.to_string())?;
            let method_name: String = env.get_string(&method_name.into()).map_err(|e| e.to_string())?.into();
            if !method_name.eq_ignore_ascii_case(name) {
                continue;
            }
            if static_only {
                let modifiers = env
                    .call_method(&method, "getModifiers", "()I", &[])
                    .map_err(|e| e.to_string())?
                    .i()
                    .map_err(|e| e.to_string())?;
                let is_static = env
                    .call_static_method("java/lang/reflect/Modifier", "isStatic", "(I)Z", &[JValue::Int(modifiers)])
                    .map_err(|e| e.to_string())?
                    .z()
                    .map_err(|e| e.to_string())?;
                if !is_static {
                    continue;
                }
            }
            candidates.push(method);
        }
        if candidates.is_empty() {
            return Err(format!("Method {name} not found on class"));
        }
        let (method, params) = select_overload(env, vm, candidates, args)?
            .ok_or_else(|| format!("No overload of {name} accepts {} argument(s) of these types", args.len()))?;
        let array = argument_array(env, vm, args, &params)?;
        let result = env.call_method(
            &method,
            "invoke",
            "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
            &[receiver.into(), (&array).into()],
        );
        match result {
            Ok(value) => value.l().map_err(|e| e.to_string()),
            Err(error) => Err(jni_error(env, &format!("Invocation of {name} failed"), error)),
        }
    }

    fn java_result_to_bx(
        env: &mut JNIEnv,
        vm: &mut dyn BxVM,
        jvm: &Arc<JavaVM>,
        result: &JObject,
    ) -> Result<BxValue, String> {
        if result.is_null() {
            return Ok(BxValue::new_null());
        }
        match class_name_of(env, result)?.as_str() {
            "java.lang.String" => {
                let text: String = env
                    .get_string(&JObject::from(env.new_local_ref(result).map_err(|e| e.to_string())?).into())
                    .map_err(|e| e.to_string())?
                    .into();
                Ok(BxValue::new_ptr(vm.string_new(text)))
            }
            "java.lang.Boolean" => {
                let value = env.call_method(result, "booleanValue", "()Z", &[]).map_err(|e| e.to_string())?.z().map_err(|e| e.to_string())?;
                Ok(BxValue::new_bool(value))
            }
            "java.lang.Integer" | "java.lang.Short" | "java.lang.Byte" => {
                let value = env.call_method(result, "intValue", "()I", &[]).map_err(|e| e.to_string())?.i().map_err(|e| e.to_string())?;
                Ok(BxValue::new_int(value))
            }
            "java.lang.Long" => {
                let value = env.call_method(result, "longValue", "()J", &[]).map_err(|e| e.to_string())?.j().map_err(|e| e.to_string())?;
                Ok(BxValue::new_number(value as f64))
            }
            "java.lang.Double" | "java.lang.Float" => {
                let value = env.call_method(result, "doubleValue", "()D", &[]).map_err(|e| e.to_string())?.d().map_err(|e| e.to_string())?;
                Ok(BxValue::new_number(value))
            }
            "java.lang.Character" => {
                let text = env
                    .call_method(result, "toString", "()Ljava/lang/String;", &[])
                    .map_err(|e| e.to_string())?
                    .l()
                    .map_err(|e| e.to_string())?;
                let text: String = env.get_string(&text.into()).map_err(|e| e.to_string())?.into();
                Ok(BxValue::new_ptr(vm.string_new(text)))
            }
            _ => wrap_object(env, vm, jvm, result),
        }
    }
}
