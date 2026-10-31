use crate::types::{BxNativeFunction, BxNativeObject, BxVM, BxValue};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const WATCHER_REGISTRY: &str = "__matchboxWatcherRegistry";
static NEXT_WATCHER_NAME: AtomicUsize = AtomicUsize::new(1);

fn watcher_registry(vm: &dyn BxVM) -> Option<usize> {
    vm.get_global_value(WATCHER_REGISTRY)
        .and_then(|value| value.as_gc_id())
}

fn ensure_watcher_registry(vm: &mut dyn BxVM) -> usize {
    watcher_registry(vm).unwrap_or_else(|| {
        let id = vm.struct_new();
        vm.insert_global(WATCHER_REGISTRY.to_string(), BxValue::new_ptr(id));
        id
    })
}

#[derive(Debug)]
struct WatcherObject {
    name: String,
    state: WatcherState,
    roots: Vec<PathBuf>,
    listener: BxValue,
    known_paths: HashSet<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatcherState {
    Created,
    Running,
    Stopped,
}

#[derive(Debug)]
struct WatcherNameObject {
    name: String,
}

#[derive(Debug)]
struct WatcherEventObject {
    kind: String,
    path: String,
    watch_root: String,
    relative_path: String,
}

impl BxNativeObject for WatcherObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_ascii_lowercase().as_str() {
            "getname" => {
                let name_id = vm.native_object_new(std::rc::Rc::new(std::cell::RefCell::new(
                    WatcherNameObject {
                        name: self.name.clone(),
                    },
                )));
                Ok(BxValue::new_ptr(name_id))
            }
            "getstateasstring" => Ok(BxValue::new_ptr(
                vm.string_new(
                    match self.state {
                        WatcherState::Created => "CREATED",
                        WatcherState::Running => "RUNNING",
                        WatcherState::Stopped => "STOPPED",
                    }
                    .to_string(),
                ),
            )),
            "isrunning" => Ok(BxValue::new_bool(self.state == WatcherState::Running)),
            "isstopped" => Ok(BxValue::new_bool(self.state == WatcherState::Stopped)),
            "start" => {
                self.state = WatcherState::Running;
                self.known_paths = snapshot_paths(&self.roots);
                Ok(BxValue::new_ptr(id))
            }
            "stop" | "shutdown" => {
                self.state = WatcherState::Stopped;
                Ok(BxValue::new_ptr(id))
            }
            "restart" => {
                self.state = WatcherState::Running;
                self.known_paths = snapshot_paths(&self.roots);
                Ok(BxValue::new_ptr(id))
            }
            "poll" => {
                if self.state != WatcherState::Running {
                    return Ok(BxValue::new_null());
                }
                let new_path = self.roots.iter().find_map(|root| {
                    snapshot_paths(std::slice::from_ref(root))
                        .difference(&self.known_paths)
                        .next()
                        .cloned()
                        .map(|path| (path, root.clone()))
                });
                let Some((path, root)) = new_path else {
                    return Ok(BxValue::new_null());
                };
                self.known_paths.insert(path.clone());
                let relative_path = path
                    .strip_prefix(&root)
                    .unwrap_or(path.as_path())
                    .to_string_lossy()
                    .trim_start_matches(std::path::MAIN_SEPARATOR)
                    .to_string();
                let event_id = vm.native_object_new(std::rc::Rc::new(std::cell::RefCell::new(
                    WatcherEventObject {
                        kind: "CREATED".to_string(),
                        path: path.to_string_lossy().to_string(),
                        watch_root: root.to_string_lossy().to_string(),
                        relative_path,
                    },
                )));
                let chunk = vm
                    .current_chunk()
                    .ok_or_else(|| "No chunk context available".to_string())?;
                vm.call_function_by_value(
                    &self.listener,
                    vec![BxValue::new_ptr(event_id), BxValue::new_ptr(id)],
                    chunk,
                )?;
                Ok(BxValue::new_null())
            }
            _ => Err(format!("Watcher method '{}' not found", name)),
        }
    }
}

impl BxNativeObject for WatcherNameObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_ascii_lowercase().as_str() {
            "getname" => Ok(BxValue::new_ptr(vm.string_new(self.name.clone()))),
            _ => Err(format!("Watcher name method '{}' not found", name)),
        }
    }
}

impl BxNativeObject for WatcherEventObject {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        let value = match name.to_ascii_lowercase().as_str() {
            "getkind" => &self.kind,
            "getpath" => &self.path,
            "getwatchroot" => &self.watch_root,
            "getrelativepath" => &self.relative_path,
            _ => return Err(format!("Watcher event method '{}' not found", name)),
        };
        Ok(BxValue::new_ptr(vm.string_new(value.clone())))
    }
}

fn snapshot_paths(roots: &[PathBuf]) -> HashSet<PathBuf> {
    roots
        .iter()
        .flat_map(|root| {
            std::fs::read_dir(root)
                .ok()
                .into_iter()
                .flat_map(|entries| entries.filter_map(Result::ok))
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        })
        .collect()
}

pub fn register_watcher_bifs(bifs: &mut HashMap<String, BxNativeFunction>) {
    bifs.insert(
        "watcherExists".to_string(),
        watcher_exists as BxNativeFunction,
    );
    bifs.insert("watcherGet".to_string(), watcher_get as BxNativeFunction);
    bifs.insert(
        "watcherGetAll".to_string(),
        watcher_get_all as BxNativeFunction,
    );
    bifs.insert("watcherList".to_string(), watcher_list as BxNativeFunction);
    bifs.insert("watcherNew".to_string(), watcher_new as BxNativeFunction);
    bifs.insert(
        "watcherRestart".to_string(),
        watcher_restart as BxNativeFunction,
    );
    bifs.insert(
        "watcherShutdown".to_string(),
        watcher_shutdown as BxNativeFunction,
    );
    bifs.insert(
        "watcherShutdownAll".to_string(),
        watcher_shutdown_all as BxNativeFunction,
    );
    bifs.insert(
        "watcherStart".to_string(),
        watcher_start as BxNativeFunction,
    );
    bifs.insert("watcherStop".to_string(), watcher_stop as BxNativeFunction);
    bifs.insert(
        "watcherStopAll".to_string(),
        watcher_stop_all as BxNativeFunction,
    );
}

fn watcher_name(vm: &dyn BxVM, args: &[BxValue]) -> Result<String, String> {
    args.first()
        .copied()
        .map(|value| vm.to_string(value))
        .ok_or_else(|| "Watcher name is required".to_string())
}

fn watcher_id(vm: &dyn BxVM, args: &[BxValue]) -> Result<usize, String> {
    let name = watcher_name(vm, args)?;
    watcher_registry(vm)
        .map(|registry| vm.struct_get(registry, &name))
        .and_then(|value| value.as_gc_id())
        .ok_or_else(|| format!("Watcher '{}' does not exist", name))
}

fn watcher_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = watcher_name(vm, args)?;
    let exists = watcher_registry(vm).is_some_and(|registry| vm.struct_key_exists(registry, &name));
    Ok(BxValue::new_bool(exists))
}

fn watcher_get(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(watcher_id(vm, args)?))
}

fn watcher_get_all(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let result_id = vm.struct_new();
    if let Some(registry) = watcher_registry(vm) {
        for name in vm.struct_key_array(registry) {
            let watcher = vm.struct_get(registry, &name);
            vm.struct_set(result_id, &name, watcher);
        }
    }
    Ok(BxValue::new_ptr(result_id))
}

fn watcher_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let result_id = vm.array_new();
    if let Some(registry) = watcher_registry(vm) {
        for name in vm.struct_key_array(registry) {
            let name_id = vm.string_new(name);
            vm.array_push(result_id, BxValue::new_ptr(name_id));
        }
    }
    Ok(BxValue::new_ptr(result_id))
}

fn watcher_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("watcherNew() expects name, paths, and listener".to_string());
    }
    if !vm.is_array_value(args[1]) && !vm.is_string_value(args[1]) {
        return Err("watcherNew() paths must be an array or string".to_string());
    }
    if !vm.value_matches_type_name(args[2], "function") && !vm.is_struct_value(args[2]) {
        return Err("watcherNew() listener must be a function or struct".to_string());
    }

    let mut name = vm.to_string(args[0]);
    if name.is_empty() {
        name = format!(
            "watcher-{}",
            NEXT_WATCHER_NAME.fetch_add(1, Ordering::Relaxed)
        );
    }
    let force = args.get(3).is_some_and(|value| value.as_bool());
    let registry = ensure_watcher_registry(vm);
    if vm.struct_key_exists(registry, &name) {
        if !force {
            return Err(format!("Watcher '{}' already exists", name));
        }
        vm.struct_delete(registry, &name);
    }
    let roots = if vm.is_string_value(args[1]) {
        vec![PathBuf::from(vm.to_string(args[1]))]
    } else {
        let array_id = args[1]
            .as_gc_id()
            .ok_or_else(|| "watcherNew() paths must be an array or string".to_string())?;
        (0..vm.array_len(array_id))
            .map(|index| PathBuf::from(vm.to_string(vm.array_get(array_id, index))))
            .collect()
    };
    if roots.is_empty() {
        return Err("watcherNew() requires at least one path".to_string());
    }
    let id = vm.native_object_new(std::rc::Rc::new(std::cell::RefCell::new(WatcherObject {
        name: name.clone(),
        state: WatcherState::Created,
        roots,
        listener: args[2],
        known_paths: HashSet::new(),
    })));
    vm.struct_set(registry, &name, BxValue::new_ptr(id));
    Ok(BxValue::new_ptr(id))
}

fn watcher_restart(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let id = watcher_id(vm, args)?;
    vm.native_object_call_method(id, "restart", &[])
}

fn watcher_shutdown(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = watcher_name(vm, args)?;
    let id = watcher_id(vm, args)?;
    vm.struct_delete(
        watcher_registry(vm).expect("watcher registry exists when watcher ID exists"),
        &name,
    );
    let _ = vm.native_object_call_method(id, "shutdown", &[]);
    Ok(BxValue::new_bool(true))
}

fn watcher_shutdown_all(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    if let Some(registry) = watcher_registry(vm) {
        vm.struct_clear(registry);
    }
    Ok(BxValue::new_null())
}

fn watcher_start(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let id = watcher_id(vm, args)?;
    vm.native_object_call_method(id, "start", &[])
}

fn watcher_stop(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let id = watcher_id(vm, args)?;
    vm.native_object_call_method(id, "stop", &[])
}

fn watcher_stop_all(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    if let Some(registry) = watcher_registry(vm) {
        for name in vm.struct_key_array(registry) {
            if let Some(id) = vm.struct_get(registry, &name).as_gc_id() {
                let _ = vm.native_object_call_method(id, "stop", &[]);
            }
        }
    }
    Ok(BxValue::new_null())
}

pub(crate) fn poll_watchers(vm: &mut dyn BxVM) {
    if let Some(registry) = watcher_registry(vm) {
        for name in vm.struct_key_array(registry) {
            if let Some(id) = vm.struct_get(registry, &name).as_gc_id() {
                let _ = vm.native_object_call_method(id, "poll", &[]);
            }
        }
    }
}
