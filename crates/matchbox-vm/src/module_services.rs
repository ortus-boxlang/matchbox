//! Live module records and adapters to the services implemented by this VM.
use crate::{
    bundle::Module,
    types::{BxNativeObject, BxVM, BxValue, NativeCallback, Tracer},
    vm::{VM, gc::GcObject},
};
use anyhow::{Context, Result, bail};
use std::{cell::RefCell, collections::HashMap, path::{Component, Path, PathBuf}, rc::Rc};

type Fields = Rc<RefCell<HashMap<String, BxValue>>>;

#[derive(Debug)]
struct ModuleRecord {
    fields: HashMap<String, BxValue>,
}
impl BxNativeObject for ModuleRecord {
    fn get_property(&self, name: &str) -> BxValue {
        self.fields
            .get(&name.to_lowercase())
            .copied()
            .unwrap_or(BxValue::new_null())
    }
    fn set_property(&mut self, name: &str, value: BxValue) {
        self.fields.insert(name.to_lowercase(), value);
    }
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        if !args.is_empty() {
            return Err(format!("ModuleRecord.{name}() takes no arguments"));
        }
        match name.to_ascii_lowercase().as_str() {
            "asstruct" => {
                let id = vm.struct_new();
                for (key, value) in &self.fields {
                    vm.struct_set(id, key, *value);
                }
                Ok(BxValue::new_ptr(id))
            }
            "isenabled" => Ok(self.get_property("enabled")),
            "isactivated" => Ok(self.get_property("activated")),
            _ => Err(format!("Unsupported ModuleRecord method: {name}")),
        }
    }
    fn trace(&self, tracer: &mut dyn Tracer) {
        for value in self.fields.values() {
            tracer.mark(value);
        }
    }
}

pub(crate) fn record(
    vm: &mut VM,
    module: &Module,
    instance: BxValue,
    settings: BxValue,
) -> Result<BxValue> {
    let (mapping, name) = if let Some(value) = vm.instance_property(instance, "mapping") {
        if vm.is_struct_value(value) {
            mapping_from_struct(vm, module, value, false)?
        } else if vm.is_string_value(value) && !vm.to_string(value).trim().is_empty() {
            let name = vm.to_string(value);
            (mapping(&format!("/bxModules/{name}"), &module.path, false), name)
        } else {
            bail!("Module {} mapping must be a nonblank string or struct", module.name);
        }
    } else {
        (mapping(&format!("/bxModules/{}", module.name), &module.path, false), module.name.clone())
    };
    let public_mapping = match vm.instance_property(instance, "publicMapping") {
        Some(value) if vm.is_struct_value(value) => mapping_from_struct(vm, module, value, true)?.0,
        Some(value) if vm.is_string_value(value) && !vm.to_string(value).trim().is_empty() => {
            let public = vm.to_string(value);
            mapping_value(&format!("{}{public}", mapping["name"].as_str().unwrap()), &module.path, &public, true)
        }
        _ => mapping_value(&format!("/bxModules/{}/public", module.name), &module.path, "public", true),
    };
    let json = serde_json::json!({
        "name": module.name, "path": module.path, "physicalPath": module.path,
        "invocationPath": format!("bxModules.{name}"),
        "mapping": mapping, "publicMapping": public_mapping,
        "dependencies": module.dependencies, "enabled": true, "activated": false,
        "bifs": [], "memberMethods": []
    });
    let mut fields = HashMap::new();
    for (key, value) in json.as_object().unwrap() {
        fields.insert(key.to_lowercase(), vm.json_to_bx(value.clone()));
    }
    for (name, default) in [
        ("version", "1.0.0"),
        ("author", ""),
        ("description", ""),
        ("webURL", ""),
    ] {
        let value = vm
            .instance_property(instance, name)
            .unwrap_or_else(|| BxValue::new_ptr(vm.string_new(default.into())));
        fields.insert(name.to_lowercase(), value);
    }
    fields.insert("settings".into(), settings);
    fields.insert("moduleconfig".into(), instance);
    Ok(BxValue::new_ptr(vm.native_object_new(Rc::new(RefCell::new(ModuleRecord { fields })))))
}

fn mapping(name: &str, path: &str, external: bool) -> serde_json::Value {
    let name = format!("{}{}{}", if name.starts_with('/') { "" } else { "/" }, name, if name.ends_with('/') { "" } else { "/" });
    serde_json::json!({"name": name, "path": path, "external": external})
}

fn mapping_value(name: &str, root: &str, relative: &str, external: bool) -> serde_json::Value {
    if relative.trim().is_empty() { return mapping(name, root, external); }
    mapping(name, &std::path::Path::new(root).join(relative).to_string_lossy(), external)
}

fn mapping_from_struct(vm: &mut VM, module: &Module, value: BxValue, public: bool) -> Result<(serde_json::Value, String)> {
    let id = value.as_gc_id().unwrap();
    let mut name = mapping_string(vm, id, "name")?;
    if name.trim().is_empty() {
        name = module.name.clone();
        let value = BxValue::new_ptr(vm.string_new(name.clone()));
        vm.struct_set(id, "name", value);
    }
    let use_prefix = mapping_bool(vm, id, "usePrefix", true)?;
    let external = mapping_bool(vm, id, "external", public)?;
    let path = mapping_string(vm, id, "path")?;
    let mapping_name = if use_prefix { format!("/bxModules/{name}") } else { name.clone() };
    Ok((mapping_value(&mapping_name, &module.path, &path, external), name))
}

fn mapping_string(vm: &VM, id: usize, key: &str) -> Result<String> {
    if !vm.struct_key_exists(id, key) { return Ok(String::new()); }
    let value = vm.struct_get(id, key);
    if !vm.is_string_value(value) { bail!("Module mapping {key} must be a string"); }
    Ok(vm.to_string(value))
}

fn mapping_bool(vm: &mut VM, id: usize, key: &str, default: bool) -> Result<bool> {
    if !vm.struct_key_exists(id, key) { return Ok(default); }
    let value = vm.struct_get(id, key);
    if vm.is_string_value(value) {
        let text = vm.to_string(value);
        let text = text.trim();
        if !matches!(text.to_ascii_lowercase().as_str(), "true" | "false" | "yes" | "no") && text.parse::<f64>().is_err() {
            bail!("Module mapping {key} must be a boolean");
        }
    } else if !value.is_bool() && !value.is_number() && !value.is_null() {
        bail!("Module mapping {key} must be a boolean");
    }
    Ok(vm.cast_value_to_type(value, "boolean").map_err(anyhow::Error::msg)?.as_bool())
}

pub(crate) fn register_classes(vm: &mut VM, module: &Module, record: BxValue) -> Result<()> {
    for field in ["mapping", "publicMapping"] {
        let mapping = record_field(vm, record, field).as_gc_id().context("Module mapping missing")?;
        let prefix = vm.to_string(vm.struct_get(mapping, "name")).trim_matches('/').replace('/', ".");
        let root = normalize_path(Path::new(&vm.to_string(vm.struct_get(mapping, "path"))));
        for class in &module.classes {
            let path = normalize_path(&Path::new(&module.path).join(&class.path));
            let Ok(relative) = path.strip_prefix(&root) else { continue; };
            let name = relative.with_extension("").to_string_lossy().replace(['/', '\\'], ".");
            let name = if prefix.is_empty() { name } else { format!("{prefix}.{name}") };
            let value = vm.get_global(&class.global).context("Embedded module class missing")?;
            let GcObject::Class(target) = vm.heap.get(value.as_gc_id().context("Embedded module class invalid")?) else {
                bail!("Embedded module global is not a class");
            };
            if target.borrow().name == class.global { target.borrow_mut().name = name.clone(); }
            vm.insert_global(name, value);
        }
    }
    Ok(())
}

// Lexical resolution must also work after the source package has been deleted.
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => (),
            Component::ParentDir => { result.pop(); },
            _ => result.push(component.as_os_str()),
        }
    }
    result
}

pub(crate) fn record_field(vm: &VM, record: BxValue, name: &str) -> BxValue {
    let GcObject::NativeObject(object) = vm.heap.get(record.as_gc_id().unwrap()) else { unreachable!() };
    object.borrow().get_property(name)
}

pub(crate) fn new_interceptor(vm: &mut VM, class: &str, properties: BxValue, name: &str, record: Option<BxValue>) -> Result<BxValue> {
    if !vm.is_struct_value(properties) { bail!("Interceptor properties must be a struct"); }
    let target = vm.construct_global_class(class, vec![])?;
    let _root = vm.heap.retain_value(target).map_err(anyhow::Error::msg)?;
    if vm.instance_has_method(target, "init") { vm.call_method_value(target, "init", vec![])?; }
    for field in ["boxruntime", "interceptorservice", "log"] {
        let value = vm.get_global(&format!("__mb_{field}")).context("Module services unavailable")?;
        vm.set_instance_variable(target, field, value)?;
    }
    if let Some(record) = record { vm.set_instance_variable(target, "moduleRecord", record)?; }
    let name = BxValue::new_ptr(vm.string_new(name.into()));
    vm.set_instance_variable(target, "name", name)?;
    vm.set_instance_variable(target, "properties", properties)?;
    if vm.instance_has_method(target, "configure") { vm.call_method_value(target, "configure", vec![])?; }
    vm.register_interceptor(target).map_err(anyhow::Error::msg)?;
    Ok(target)
}

pub(crate) fn set_record(vm: &mut VM, record: BxValue, name: &str, value: BxValue) -> Result<()> {
    // get_mut also records old-to-young writes for generational GC.
    let GcObject::NativeObject(object) = vm.heap.get_mut(record.as_gc_id().unwrap()) else {
        bail!("Invalid module record");
    };
    object.borrow_mut().set_property(name, value);
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Runtime,
    Functions,
    Modules,
    Interceptors,
    Components,
    Log,
}
#[derive(Debug)]
struct Service {
    kind: Kind,
    fields: Fields,
}
impl BxNativeObject for Service {
    fn delegated_callback(&self, name: &str) -> Option<NativeCallback> {
        let name = name.to_ascii_lowercase();
        if !matches!(self.kind, Kind::Interceptors) || !matches!(name.as_str(), "announce" | "newandregister") { return None; }
        let fields = self.fields.clone();
        let kind = self.kind;
        let parameters = if name == "announce" { vec!["receiver", "state", "data"] } else { vec!["receiver", "clazz", "properties", "name", "moduleRecord"] };
        Some(NativeCallback {
            parameters: parameters.into_iter().map(str::to_owned).collect(),
            callback: Rc::new(move |vm, args| Service { kind, fields: fields.clone() }.call_method(vm, 0, &name, &args[1..])),
        })
    }
    fn get_property(&self, name: &str) -> BxValue {
        self.fields
            .borrow()
            .get(&name.to_lowercase())
            .copied()
            .unwrap_or(BxValue::new_null())
    }
    fn set_property(&mut self, name: &str, value: BxValue) {
        self.fields.borrow_mut().insert(name.to_lowercase(), value);
    }
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        let name = name.to_ascii_lowercase();
        let field = |name: &str| {
            self.fields
                .borrow()
                .get(name)
                .copied()
                .ok_or_else(|| format!("Service unavailable: {name}"))
        };
        match (self.kind, name.as_str()) {
            (Kind::Runtime, "getfunctionservice") if args.is_empty() => field("functionservice"),
            (Kind::Runtime, "getmoduleservice") if args.is_empty() => field("moduleservice"),
            (Kind::Runtime, "getinterceptorservice") if args.is_empty() => {
                field("interceptorservice")
            }
            (Kind::Runtime, "getcomponentservice") if args.is_empty() => field("componentservice"),
            (Kind::Components, "hascomponent") if args.len() == 1 => {
                let name = vm.to_string(args[0]);
                Ok(BxValue::new_bool(vm.component_names().iter().any(|component| component.eq_ignore_ascii_case(&name))))
            }
            (Kind::Components, "getcomponentnames") if args.is_empty() => {
                let id = vm.array_new();
                for name in vm.component_names() {
                    let value = BxValue::new_ptr(vm.string_new(name)); vm.array_push(id, value);
                }
                Ok(BxValue::new_ptr(id))
            }
            (Kind::Interceptors, "register" | "unregister") if args.len() == 1 => {
                if name == "register" {
                    vm.register_interceptor(args[0])?;
                } else {
                    vm.unregister_interceptor(args[0])?;
                }
                field("interceptorservice")
            }
            (Kind::Interceptors, "newandregister") if args.len() == 4 => {
                let class = vm.to_string(args[0]);
                let name = vm.to_string(args[2]);
                vm.new_interceptor(&class, args[1], &name, (!args[3].is_null()).then_some(args[3]))
            }
            (Kind::Interceptors, "announce") if (1..=2).contains(&args.len()) => {
                let state = vm.to_string(args[0]);
                let data = args
                    .get(1)
                    .copied()
                    .unwrap_or_else(|| BxValue::new_ptr(vm.struct_new()));
                vm.announce(&state, data)?;
                Ok(BxValue::new_null())
            }
            (Kind::Interceptors, "hasinterceptionpoint") if args.len() == 1 => {
                let state = vm.to_string(args[0]);
                Ok(BxValue::new_bool(
                    vm.interception_points()
                        .iter()
                        .any(|point| point.eq_ignore_ascii_case(&state)),
                ))
            }
            (Kind::Interceptors, "getinterceptionpoints" | "getinterceptionpointnames")
                if args.is_empty() =>
            {
                let id = vm.array_new();
                for point in vm.interception_points() {
                    let point = BxValue::new_ptr(vm.string_new(point));
                    vm.array_push(id, point);
                }
                Ok(BxValue::new_ptr(id))
            }
            (Kind::Interceptors, "appendinterceptionpoints") if !args.is_empty() => {
                let mut points = Vec::new();
                for value in args {
                    if vm.value_matches_type_name(*value, "array") {
                        let id = value.as_gc_id().unwrap();
                        points.extend(
                            (0..vm.array_len(id))
                                .map(|index| vm.to_string(vm.array_get(id, index))),
                        );
                    } else {
                        points.push(vm.to_string(*value));
                    }
                }
                vm.append_interception_points(points)?;
                field("interceptorservice")
            }
            (Kind::Functions, "hasglobalfunction") if args.len() == 1 => Ok(BxValue::new_bool(
                vm.has_global_function(&vm.to_string(args[0])),
            )),
            (Kind::Functions, "getglobalfunctionnames") if args.is_empty() => {
                let id = vm.array_new();
                for name in vm.global_function_names() {
                    let name = BxValue::new_ptr(vm.string_new(name));
                    vm.array_push(id, name);
                }
                Ok(BxValue::new_ptr(id))
            }
            (Kind::Modules, "getregistry") if args.is_empty() => field("registry"),
            (Kind::Modules, "getmodulerecord" | "hasmodule") if args.len() == 1 => {
                let registry = field("registry")?
                    .as_gc_id()
                    .ok_or("Invalid module registry")?;
                let name_arg = vm.to_string(args[0]);
                if name == "hasmodule" {
                    return Ok(BxValue::new_bool(vm.struct_key_exists(registry, &name_arg)));
                }
                if !vm.struct_key_exists(registry, &name_arg) {
                    return Err(format!("Module not loaded: {name_arg}"));
                }
                Ok(vm.struct_get(registry, &name_arg))
            }
            (Kind::Log, "trace" | "debug" | "info" | "warn" | "error") if args.len() == 1 => {
                eprintln!("[{name}] {}", vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            (
                Kind::Log,
                "istraceenabled" | "isdebugenabled" | "isinfoenabled" | "iswarnenabled"
                | "iserrorenabled",
            ) if args.is_empty() => Ok(BxValue::new_bool(true)),
            _ => Err(format!(
                "Unsupported MatchBox {:?} service method or arguments: {name}",
                self.kind
            )),
        }
    }
    fn trace(&self, tracer: &mut dyn Tracer) {
        for value in self.fields.borrow().values() {
            tracer.mark(value);
        }
    }
}

pub(crate) struct Services {
    fields: Fields,
}
impl Services {
    pub fn new(vm: &mut VM) -> Self {
        let registry = BxValue::new_ptr(vm.struct_new());
        let fields = Rc::new(RefCell::new(HashMap::from([("registry".into(), registry)])));
        for (name, kind) in [
            ("boxruntime", Kind::Runtime),
            ("functionservice", Kind::Functions),
            ("moduleservice", Kind::Modules),
            ("interceptorservice", Kind::Interceptors),
            ("componentservice", Kind::Components),
            ("log", Kind::Log),
        ] {
            let value = BxValue::new_ptr(vm.native_object_new(Rc::new(RefCell::new(Service {
                kind,
                fields: fields.clone(),
            }))));
            fields.borrow_mut().insert(name.into(), value);
            vm.insert_global(format!("__mb_{name}"), value);
        }
        vm.register_callback("__mb_construct_box_class", NativeCallback {
            parameters: vec!["class".into(), "arguments".into(), "names".into()],
            callback: Rc::new(|vm, values| {
                if values.len() != 3 || !vm.is_string_value(values[0]) || !vm.is_array_value(values[1]) || !vm.is_array_value(values[2]) {
                    return Err("Class construction requires a name, arguments and argument names".into());
                }
                let class = vm.to_string(values[0]);
                let args_id = values[1].as_gc_id().unwrap();
                let names_id = values[2].as_gc_id().unwrap();
                if vm.array_len(args_id) != vm.array_len(names_id) { return Err("Class argument names must match arguments".into()); }
                let args = (0..vm.array_len(args_id)).map(|index| vm.array_get(args_id, index)).collect();
                let names: Vec<_> = (0..vm.array_len(names_id)).map(|index| vm.to_string(vm.array_get(names_id, index))).collect();
                let named = names.iter().any(|name| !name.is_empty());
                vm.construct_box_class(&class, args, named.then_some(names))
            }),
        }).expect("VM callback registration");
        vm.register_component_runtime().expect("VM callback registration");
        Self { fields }
    }
    pub fn register(&self, vm: &mut VM, name: &str, record: BxValue) {
        let id = self.fields.borrow()["registry"].as_gc_id().unwrap();
        vm.struct_set(id, name, record);
    }
    pub fn inject(&self, vm: &mut VM, instance: BxValue, record: BxValue, bif: bool) -> Result<()> {
        vm.set_instance_variable(instance, "moduleRecord", record)?;
        for name in ["boxruntime", "log", "interceptorservice", "componentservice"] {
            vm.set_instance_variable(instance, name, self.fields.borrow()[name])?;
        }
        if bif {
            vm.set_instance_variable(
                instance,
                "functionService",
                self.fields.borrow()["functionservice"],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Chunk;

    #[test]
    fn service_registry_roots_records_and_tracks_new_settings_through_gc() {
        let mut vm = VM::new();
        let services = Services::new(&mut vm);
        let module = Module {
            name: "gc-test".into(),
            path: "/module".into(),
            dependencies: vec![],
            instance: "config".into(),
            descriptor: Chunk::default(),
            bifs: Chunk::default(),
            bif_registrations: vec![],
            classes: vec![],
            components: vec![],
            settings: serde_json::json!({}),
            artifact: None,
        };
        let settings = BxValue::new_ptr(vm.struct_new());
        let record = record(&mut vm, &module, BxValue::new_null(), settings).unwrap();
        services.register(&mut vm, "gc-test", record);
        for _ in 0..16 {
            vm.collect_garbage_now();
        }
        let settings_id = vm.struct_new();
        let text = BxValue::new_ptr(vm.string_new("survived".into()));
        vm.struct_set(settings_id, "text", text);
        set_record(&mut vm, record, "settings", BxValue::new_ptr(settings_id)).unwrap();
        for _ in 0..16 {
            vm.collect_garbage_now();
        }
        let snapshot = vm
            .call_method_value(record, "asStruct", vec![])
            .unwrap()
            .as_gc_id()
            .unwrap();
        let settings = vm.struct_get(snapshot, "settings").as_gc_id().unwrap();
        assert_eq!(vm.to_string(vm.struct_get(settings, "text")), "survived");
        let module_service = services.fields.borrow()["moduleservice"];
        let name = BxValue::new_ptr(vm.string_new("GC-TEST".into()));
        assert_eq!(
            vm.call_method_value(module_service, "getModuleRecord", vec![name])
                .unwrap(),
            record
        );
    }
}
