//! Native module application envelope and verified, content-addressed extraction.
use crate::{
    Chunk,
    types::{BxVM, BxValue, NativeCallback},
    vm::VM,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
};

pub const TARGET: &str = env!("MATCHBOX_TARGET");
pub const MAGIC: &[u8; 8] = b"MBMODS\0\x0B";

#[derive(Clone, Serialize, Deserialize)]
pub struct SharedLibrary {
    pub filename: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub target: String,
    pub abi: u32,
    pub filename: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
    pub libraries: Vec<SharedLibrary>,
}
impl Artifact {
    pub fn validate(&self) -> Result<()> {
        if self.target != TARGET {
            bail!(
                "Native artifact target {} does not match host {TARGET}",
                self.target
            );
        }
        if self.abi != matchbox_plugin_sdk::ABI_MAJOR {
            bail!("Unsupported native plugin ABI {}", self.abi);
        }
        self.validate_files()
    }
    fn files(&self) -> impl Iterator<Item = (&str, &str, &[u8])> {
        std::iter::once((
            self.filename.as_str(),
            self.sha256.as_str(),
            self.bytes.as_slice(),
        ))
        .chain(self.libraries.iter().map(|library| {
            (
                library.filename.as_str(),
                library.sha256.as_str(),
                library.bytes.as_slice(),
            )
        }))
    }
    /// Packaging validation does not require the output target to match this host.
    pub fn validate_files(&self) -> Result<()> {
        let mut names = HashSet::new();
        for (filename, sha256, bytes) in self.files() {
            validate_filename(filename)?;
            let name = if self.target.contains("windows") || self.target.contains("apple") {
                filename.to_ascii_lowercase()
            } else {
                filename.to_owned()
            };
            if !names.insert(name) {
                bail!("Duplicate native artifact filename: {filename}");
            }
            if sha256 != hash(bytes) {
                bail!("Native artifact hash mismatch: {filename}");
            }
        }
        Ok(())
    }
    pub fn extract(&self, root: &Path) -> Result<PathBuf> {
        self.validate()?;
        // A changed dependency or filename must never reuse a previously loaded library path.
        let mut files: Vec<_> = self.files().collect();
        files.sort_by_key(|file| file.0);
        let mut digest = Sha256::new();
        for (filename, sha256, _) in &files {
            digest.update(filename.as_bytes());
            digest.update([0]);
            digest.update(sha256.as_bytes());
        }
        private_directory(root)?;
        let target = root.join(&self.target);
        private_directory(&target)?;
        let dir = target.join(format!("{:x}", digest.finalize()));
        private_directory(&dir)?;
        for (filename, sha256, bytes) in files {
            extract_file(&dir, filename, sha256, bytes)?;
        }
        Ok(dir.join(&self.filename).canonicalize()?)
    }
}

pub fn validate_filename(filename: &str) -> Result<()> {
    if filename.is_empty()
        || filename.contains(['/', '\\', ':', '\0'])
        || Path::new(filename)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(filename)
    {
        bail!("Invalid artifact filename: {filename}");
    }
    Ok(())
}
fn private_directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() {
        bail!("Native cache must be a directory, not a symlink");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o022 != 0 {
            bail!("Native cache must be a private, user-owned directory");
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn extract_file(dir: &Path, filename: &str, sha256: &str, bytes: &[u8]) -> Result<PathBuf> {
    let path = dir.join(filename);
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        if !metadata.file_type().is_file() {
            bail!("Native cache entry is not a regular file");
        }
        if hash(&fs::read(&path)?) == sha256 {
            return Ok(path);
        }
        bail!("Native cache entry failed verification: {}", path.display());
    }
    // create_new + hard_link publishes a complete file without replacing another launch's file.
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let (temp, mut file) = loop {
        let temp = dir.join(format!(
            ".{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        if temp == path {
            continue;
        }
        match options.open(&temp) {
            Ok(file) => break (temp, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        match fs::hard_link(&temp, &path) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.file_type().is_file() || hash(&fs::read(&path)?) != sha256 {
            bail!("Native cache verification failed after publication");
        }
        Ok(path)
    })();
    let _ = fs::remove_file(temp);
    result
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn cache_root() -> PathBuf {
    if let Some(path) = std::env::var_os("MATCHBOX_NATIVE_CACHE") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("MatchBox/native");
    }
    #[cfg(target_os = "macos")]
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join("Library/Caches/MatchBox/native");
    }
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path).join("matchbox/native");
    }
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path).join(".cache/matchbox/native");
    }
    std::env::temp_dir().join(format!("matchbox-native-{}", std::process::id()))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct BifMember {
    pub name: String,
    pub type_name: String,
    pub object_argument: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Bif {
    pub instance: String,
    pub function: String,
    pub aliases: Vec<String>,
    pub members: Vec<BifMember>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Component {
    pub class: String,
    pub aliases: Vec<String>,
    pub allows_body: bool,
    pub requires_body: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ModuleClass {
    pub path: String,
    pub global: String,
    pub chunk: Chunk,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub path: String,
    pub dependencies: Vec<String>,
    pub parent_module: Option<String>,
    pub nested_modules: Vec<String>,
    pub instance: String,
    pub descriptor: Chunk,
    pub bifs: Chunk,
    pub bif_registrations: Vec<Bif>,
    pub classes: Vec<ModuleClass>,
    pub components: Vec<Component>,
    #[serde(with = "json_string")]
    pub settings: serde_json::Value,
    pub artifact: Option<Artifact>,
}
#[derive(Serialize, Deserialize)]
pub struct Application {
    pub chunk: Chunk,
    pub modules: Vec<Module>,
}
impl Application {
    pub fn from_executable(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 16 || !bytes.ends_with(MAGIC) {
            bail!("No native module application footer");
        }
        let length = u64::from_le_bytes(bytes[bytes.len() - 16..bytes.len() - 8].try_into()?);
        let length = usize::try_from(length)?;
        let end = bytes.len() - 16;
        let start = end
            .checked_sub(length)
            .context("Invalid native application length")?;
        Ok(postcard::from_bytes(&bytes[start..end])?)
    }
    pub fn append_to(&self, runner: &[u8]) -> Result<Vec<u8>> {
        let payload = postcard::to_stdvec(self)?;
        let mut bytes = runner.to_vec();
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        bytes.extend_from_slice(MAGIC);
        Ok(bytes)
    }
    /// Trusted native modules execute arbitrary machine code in the host process.
    pub fn run(mut self, vm: &mut VM, cache: &Path) -> Result<BxValue> {
        let loaded = LoadedModules::load(vm, self.modules, cache)?;
        self.chunk.reconstruct_functions();
        let result = vm.interpret(self.chunk);
        let shutdown = loaded.unload(vm);
        match (result, shutdown) {
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(value), Ok(())) => Ok(value),
        }
    }
}

struct ModuleActivation {
    name: String,
    parent: Option<String>,
    instance: Rc<crate::vm::gc::RootedValue>,
    record: Rc<crate::vm::gc::RootedValue>,
    targets: Vec<Rc<crate::vm::gc::RootedValue>>,
}
/// VM/thread-owned module lifetime. Hosts must unload before destroying the owning VM.
/// Registrations/native libraries remain VM/process-owned; this is not hot reload.
pub struct LoadedModules {
    records: Vec<ModuleActivation>,
}
#[derive(Clone, Debug)]
pub struct ModuleMapping {
    pub module: String,
    pub name: String,
    pub path: std::path::PathBuf,
    pub external: bool,
}
impl LoadedModules {
    /// Resolved descriptor mappings, copied out without exposing VM heap values to HTTP threads.
    pub fn mappings(&self, vm: &VM) -> Result<Vec<ModuleMapping>> {
        let mut mappings = Vec::new();
        for activation in &self.records {
            let record = vm
                .heap
                .read_retained_value(&activation.record)
                .map_err(anyhow::Error::msg)?;
            for field in ["mapping", "publicMapping"] {
                let value = crate::module_services::record_field(vm, record, field)
                    .as_gc_id()
                    .context("Module mapping missing")?;
                mappings.push(ModuleMapping {
                    module: activation.name.clone(),
                    name: vm.to_string(vm.struct_get(value, "name")),
                    path: vm.to_string(vm.struct_get(value, "path")).into(),
                    external: vm.struct_get(value, "external").as_bool(),
                });
            }
        }
        Ok(mappings)
    }
    pub fn load(vm: &mut VM, mut modules: Vec<Module>, cache: &Path) -> Result<Self> {
        let mut loaded = Self {
            records: Vec::new(),
        };
        let result = (|| -> Result<()> {
            let services = crate::module_services::Services::new(vm);
            let settings_id = vm.struct_new();
            vm.insert_global(
                "__matchbox_module_settings".into(),
                BxValue::new_ptr(settings_id),
            );
            vm.register_callback(
                "getModuleSettings",
                NativeCallback {
                    parameters: vec!["module".into()],
                    callback: Rc::new(move |vm, args| {
                        if args.len() != 1 {
                            return Err("getModuleSettings requires a module name".into());
                        }
                        let name = vm.to_string(args[0]);
                        if !vm.struct_key_exists(settings_id, &name) {
                            return Err(format!("Module not loaded: {name}"));
                        }
                        Ok(vm.struct_get(settings_id, &name))
                    }),
                },
            )
            .map_err(anyhow::Error::msg)?;
            fn construct(
                index: usize,
                modules: &mut [Module],
                vm: &mut VM,
                cache: &Path,
                ready: &mut [u8],
                prepared: &mut [Option<ModuleActivation>],
                services: &crate::module_services::Services,
            ) -> Result<()> {
                if ready[index] == 2 {
                    return Ok(());
                }
                if ready[index] == 1 {
                    bail!("Circular nested module parent");
                }
                ready[index] = 1;
                if let Some(parent) = modules[index].parent_module.clone() {
                    let parent = modules
                        .iter()
                        .position(|module| module.name.eq_ignore_ascii_case(&parent))
                        .context("Nested module parent missing")?;
                    construct(parent, modules, vm, cache, ready, prepared, services)?;
                }
                let module = &mut modules[index];
                if let Some(artifact) = &module.artifact {
                    let path = artifact.extract(cache)?;
                    unsafe { crate::plugins::load(vm, &path) }
                        .map_err(anyhow::Error::msg)
                        .with_context(|| format!("Module {}", module.name))?;
                }
                for class in &mut module.classes {
                    class.chunk.reconstruct_functions();
                    vm.interpret(class.chunk.clone())?;
                }
                module.descriptor.reconstruct_functions();
                vm.interpret(module.descriptor.clone())?;
                // Parent constructors/classes exist before children, but configuration/activation remains child-first.
                let instance = vm
                    .get_global(&module.instance)
                    .context("Module instance missing")?;
                let defaults = vm
                    .instance_variable(instance, "settings")
                    .unwrap_or_else(|| BxValue::new_ptr(vm.struct_new()));
                vm.set_instance_variable(instance, "settings", defaults)?;
                for name in ["interceptors", "customInterceptionPoints", "classProviders"] {
                    if vm.instance_variable(instance, name).is_none() {
                        let value = BxValue::new_ptr(vm.array_new());
                        vm.set_instance_variable(instance, name, value)?;
                    }
                }
                let record = crate::module_services::record(vm, module, instance, defaults)?;
                services.register(vm, &module.name, record);
                services.inject(vm, instance, record, false)?;
                prepared[index] = Some(ModuleActivation {
                    name: module.name.clone(),
                    parent: module.parent_module.clone(),
                    instance: vm.heap.retain_value(instance).map_err(anyhow::Error::msg)?,
                    record: vm.heap.retain_value(record).map_err(anyhow::Error::msg)?,
                    targets: Vec::new(),
                });
                crate::module_services::register_classes(vm, module, record)?;
                ready[index] = 2;
                Ok(())
            }
            let mut ready = vec![0; modules.len()];
            let mut prepared: Vec<_> = (0..modules.len()).map(|_| None).collect();
            for index in 0..modules.len() {
                construct(
                    index,
                    &mut modules,
                    vm,
                    cache,
                    &mut ready,
                    &mut prepared,
                    &services,
                )?;
                let module = &mut modules[index];
                let activation = prepared[index]
                    .take()
                    .context("Module descriptor did not create an instance")?;
                let instance = vm
                    .heap
                    .read_retained_value(&activation.instance)
                    .map_err(anyhow::Error::msg)?;
                let record = vm
                    .heap
                    .read_retained_value(&activation.record)
                    .map_err(anyhow::Error::msg)?;
                // Include partially configured descriptors in error cleanup, in configuration order.
                loaded.records.push(activation);
                if vm.instance_has_method(instance, "configure") {
                    vm.call_method_value(instance, "configure", vec![])?;
                }
                let settings = vm
                    .instance_variable(instance, "settings")
                    .context("Module settings missing")?;
                if !vm.is_struct_value(settings) {
                    bail!("Module {} settings must be a struct", module.name);
                }
                let id = settings.as_gc_id().unwrap();
                if let Some(overrides) = module.settings.as_object() {
                    merge_settings(vm, id, overrides);
                }
                for name in ["customInterceptionPoints", "interceptors"] {
                    let value = vm
                        .instance_variable(instance, name)
                        .unwrap_or_else(|| BxValue::new_ptr(vm.array_new()));
                    if !vm.value_matches_type_name(value, "array") {
                        bail!("Module {} {name} must be an array", module.name);
                    }
                    crate::module_services::set_record(vm, record, name, value)?;
                    if name == "customInterceptionPoints" {
                        let id = value.as_gc_id().unwrap();
                        let points = (0..vm.array_len(id))
                            .map(|index| vm.to_string(vm.array_get(id, index)))
                            .collect();
                        vm.append_interception_points(points);
                    }
                }
                let providers = vm
                    .instance_variable(instance, "classProviders")
                    .unwrap_or_else(|| BxValue::new_ptr(vm.array_new()));
                vm.register_class_providers(record, providers)?;
                crate::module_services::set_record(vm, record, "classProviders", providers)?;
                vm.struct_set(settings_id, &module.name, settings);
                crate::module_services::set_record(vm, record, "settings", settings)?;
                module.bifs.reconstruct_functions();
                vm.interpret(module.bifs.clone())?;
                for bif in &module.bif_registrations {
                    let bif_instance = vm
                        .get_global(&bif.instance)
                        .context("Module BIF instance missing")?;
                    services.inject(vm, bif_instance, record, true)?;
                    let function = vm
                        .get_global(&bif.function)
                        .context("Module BIF callable missing")?;
                    for alias in &bif.aliases {
                        vm.register_global_bif(alias.clone(), function);
                    }
                    for member in &bif.members {
                        vm.register_member_method(function, member)?;
                    }
                }
                for component in &module.components {
                    let target = vm.construct_global_class(&component.class, vec![])?;
                    let _root = vm.heap.retain_value(target).map_err(anyhow::Error::msg)?;
                    if vm.instance_has_method(target, "init") {
                        vm.call_method_value(target, "init", vec![])?;
                    }
                    services.inject(vm, target, record, true)?;
                    // ponytail: linear class lookup per component; index globals if catalogs become large.
                    let class = module
                        .classes
                        .iter()
                        .find(|class| class.global == component.class)
                        .context("Module component class missing")?;
                    let invocation = vm.to_string(crate::module_services::record_field(
                        vm,
                        record,
                        "invocationPath",
                    ));
                    let name = Path::new(&class.path)
                        .with_extension("")
                        .to_string_lossy()
                        .replace(['/', '\\'], ".");
                    let mut registration = component.clone();
                    registration.class = format!("{invocation}.{name}");
                    crate::vm::components::inject_helpers(vm, target, &registration.class)?;
                    vm.register_component(target, &registration)?;
                }
                let components: Vec<_> = module
                    .components
                    .iter()
                    .flat_map(|component| &component.aliases)
                    .collect();
                let components = vm.json_to_bx(serde_json::json!(components));
                crate::module_services::set_record(vm, record, "components", components)?;
                let aliases: Vec<_> = module
                    .bif_registrations
                    .iter()
                    .flat_map(|bif| &bif.aliases)
                    .collect();
                let aliases = vm.json_to_bx(serde_json::json!(aliases));
                crate::module_services::set_record(vm, record, "bifs", aliases)?;
                let members: Vec<_> = module.bif_registrations.iter().flat_map(|bif| &bif.members).map(|member| serde_json::json!({
                    "name": member.name, "type": member.type_name, "objectArgument": member.object_argument.as_deref().unwrap_or("")
                })).collect();
                let members = vm.json_to_bx(serde_json::json!(members));
                crate::module_services::set_record(vm, record, "memberMethods", members)?;
            }
            for activation in &mut loaded.records {
                let instance = vm
                    .heap
                    .read_retained_value(&activation.instance)
                    .map_err(anyhow::Error::msg)?;
                let record = vm
                    .heap
                    .read_retained_value(&activation.record)
                    .map_err(anyhow::Error::msg)?;
                let targets = &mut activation.targets;
                announce_module_event(vm, "preModuleLoad", record)?;
                vm.register_interceptor(instance)
                    .map_err(anyhow::Error::msg)?;
                let entries = vm
                    .instance_variable(instance, "interceptors")
                    .unwrap_or_else(|| BxValue::new_ptr(vm.array_new()))
                    .as_gc_id()
                    .unwrap();
                for index in 0..vm.array_len(entries) {
                    let entry = vm.array_get(entries, index);
                    if !vm.is_struct_value(entry) {
                        bail!("Interceptor record must be a struct");
                    }
                    let entry = entry.as_gc_id().unwrap();
                    if !vm.struct_key_exists(entry, "class") {
                        bail!("Interceptor record is missing the [class] key which is mandatory");
                    }
                    let class_name = vm.to_string(vm.struct_get(entry, "class"));
                    let invocation = vm.to_string(crate::module_services::record_field(
                        vm,
                        record,
                        "invocationPath",
                    ));
                    let class_name = if class_name.starts_with(&format!("{invocation}.")) {
                        class_name
                    } else {
                        format!("{invocation}.{class_name}")
                    };
                    let properties = if vm.struct_key_exists(entry, "properties") {
                        vm.struct_get(entry, "properties")
                    } else {
                        let value = BxValue::new_ptr(vm.struct_new());
                        vm.struct_set(entry, "properties", value);
                        value
                    };
                    let name = if vm.struct_key_exists(entry, "name") {
                        vm.to_string(vm.struct_get(entry, "name"))
                    } else {
                        let name = format!(
                            "{class_name}@{}",
                            vm.to_string(crate::module_services::record_field(vm, record, "name"))
                        );
                        let value = BxValue::new_ptr(vm.string_new(name.clone()));
                        vm.struct_set(entry, "name", value);
                        name
                    };
                    let target = crate::module_services::new_interceptor(
                        vm,
                        &class_name,
                        properties,
                        &name,
                        Some(record),
                    )?;
                    targets.push(vm.heap.retain_value(target).map_err(anyhow::Error::msg)?);
                    vm.struct_set(entry, "interceptor", target);
                }
                if vm.instance_has_method(instance, "onLoad") {
                    vm.call_method_value(instance, "onLoad", vec![])?;
                }
                crate::module_services::set_record(
                    vm,
                    record,
                    "activated",
                    BxValue::new_bool(true),
                )?;
                announce_module_event(vm, "postModuleLoad", record)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = loaded.unload(vm);
            return Err(error);
        }
        Ok(loaded)
    }

    /// Reverse dependency-order cleanup continues after hook errors.
    pub fn unload(self, vm: &mut VM) -> Result<()> {
        let mut shutdown_error = None;
        // Children must unload before their parent, while ordinary dependencies still unload last.
        fn visit(
            index: usize,
            records: &[ModuleActivation],
            seen: &mut [bool],
            order: &mut Vec<usize>,
        ) {
            if seen[index] {
                return;
            }
            seen[index] = true;
            for child in (0..records.len()).rev() {
                if records[child]
                    .parent
                    .as_ref()
                    .is_some_and(|parent| parent.eq_ignore_ascii_case(&records[index].name))
                {
                    visit(child, records, seen, order);
                }
            }
            order.push(index);
        }
        let mut order = Vec::new();
        let mut seen = vec![false; self.records.len()];
        for index in (0..self.records.len()).rev() {
            visit(index, &self.records, &mut seen, &mut order);
        }
        let mut records: Vec<_> = self.records.into_iter().map(Some).collect();
        for index in order {
            let activation = records[index].take().unwrap();
            let instance = vm
                .heap
                .read_retained_value(&activation.instance)
                .map_err(anyhow::Error::msg)?;
            let record = vm
                .heap
                .read_retained_value(&activation.record)
                .map_err(anyhow::Error::msg)?;
            let targets = activation.targets;
            if let Err(error) = announce_module_event(vm, "preModuleUnload", record) {
                shutdown_error.get_or_insert(error);
            }
            if vm.instance_has_method(instance, "onUnload") {
                if let Err(error) = vm.call_method_value(instance, "onUnload", vec![]) {
                    shutdown_error.get_or_insert(error);
                }
            }
            vm.unregister_class_providers(record);
            if let Err(error) = vm.unregister_interceptor(instance) {
                shutdown_error.get_or_insert(anyhow::Error::msg(error));
            }
            for target in targets {
                if let Err(error) = vm
                    .heap
                    .read_retained_value(&target)
                    .and_then(|target| vm.unregister_interceptor(target))
                {
                    shutdown_error.get_or_insert(anyhow::Error::msg(error));
                }
            }
            if let Err(error) = crate::module_services::set_record(
                vm,
                record,
                "activated",
                BxValue::new_bool(false),
            ) {
                shutdown_error.get_or_insert(error);
            }
            if let Err(error) = announce_module_event(vm, "postModuleUnload", record) {
                shutdown_error.get_or_insert(error);
            }
        }
        match shutdown_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn announce_module_event(vm: &mut VM, name: &str, record: BxValue) -> Result<()> {
    let module_name = crate::module_services::record_field(vm, record, "name");
    vm.announce_event(
        name,
        &[("moduleName", module_name), ("moduleRecord", record)],
    )
    .map_err(anyhow::Error::msg)
}

fn merge_settings(vm: &mut VM, id: usize, overrides: &serde_json::Map<String, serde_json::Value>) {
    for (key, value) in overrides {
        let existing = vm.struct_get(id, key);
        if let Some(nested) = value.as_object().filter(|_| vm.is_struct_value(existing)) {
            merge_settings(vm, existing.as_gc_id().unwrap(), nested);
        } else {
            let value = vm.json_to_bx(value.clone());
            vm.struct_set(id, key, value);
        }
    }
}

mod json_string {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        value: &serde_json::Value,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<serde_json::Value, D::Error> {
        serde_json::from_str(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_envelope_lengths() {
        let mut bytes = vec![0; 16];
        bytes[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        bytes[8..].copy_from_slice(MAGIC);
        assert!(Application::from_executable(&bytes).is_err());
    }
    #[test]
    fn publication_does_not_remove_an_artifact_named_like_a_staging_file() {
        let root = std::env::temp_dir().join(format!("matchbox-cache-{}", uuid::Uuid::new_v4()));
        let artifact = Artifact {
            target: TARGET.into(),
            abi: 1,
            filename: format!(".{}-0", std::process::id()),
            sha256: hash(b"test"),
            bytes: b"test".to_vec(),
            libraries: vec![],
        };
        let path = artifact.extract(&root).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"test");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_wrong_targets_and_hashes() {
        let mut a = Artifact {
            target: TARGET.into(),
            abi: 1,
            filename: "image.so".into(),
            sha256: hash(b"test"),
            bytes: b"test".to_vec(),
            libraries: vec![],
        };
        assert!(a.validate().is_ok());
        a.target = "wrong".into();
        assert!(a.validate().is_err());
        a.target = TARGET.into();
        a.bytes.push(1);
        assert!(a.validate().is_err());
    }
}
