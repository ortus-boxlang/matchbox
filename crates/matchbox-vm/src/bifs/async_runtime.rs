use crate::types::{NativeBif, BxNativeObject, BxVM, BxValue, FutureStatus};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

static EXECUTORS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static THREADS: OnceLock<Mutex<HashMap<(usize, String), ThreadInfo>>> = OnceLock::new();

fn executors() -> &'static Mutex<HashSet<String>> {
    EXECUTORS.get_or_init(|| {
        Mutex::new(
            ["io-tasks", "cpu-tasks", "scheduled-tasks"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
    })
}

fn threads() -> &'static Mutex<HashMap<(usize, String), ThreadInfo>> {
    THREADS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn vm_key(vm: &dyn BxVM) -> usize {
    vm as *const dyn BxVM as *const () as usize
}

#[derive(Clone, Copy)]
struct ThreadInfo {
    future: BxValue,
    terminated: bool,
    interrupted: bool,
}

#[derive(Debug)]
struct ExecutorObject {
    name: String,
}

impl BxNativeObject for ExecutorObject {
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
        match name.to_ascii_lowercase().as_str() {
            "name" => Ok(BxValue::new_ptr(vm.string_new(self.name.clone()))),
            "submit" => {
                let callback = args
                    .first()
                    .ok_or_else(|| "submit() expects a callback".to_string())?;
                let chunk = vm
                    .current_chunk()
                    .ok_or_else(|| "No chunk context available".to_string())?;
                vm.spawn_by_value(callback, Vec::new(), 0, chunk)
            }
            _ => Err(format!("Executor method '{}' not found", name)),
        }
    }
}

pub fn register_async_bifs(bifs: &mut HashMap<String, NativeBif>) {
    bifs.insert("futurenew".to_string(), NativeBif { func: future_new, params: params!["value", "executor"] });
    bifs.insert("futurecompleteexceptionally".to_string(), NativeBif::positional(future_complete_exceptionally));
    bifs.insert("futurecompleteontimeout".to_string(), NativeBif::positional(future_complete_on_timeout));
    bifs.insert("futurejoinordefault".to_string(), NativeBif::positional(future_join_or_default));
    bifs.insert("futuregetordefault".to_string(), NativeBif::positional(future_get_or_default));
    bifs.insert("futureortimeout".to_string(), NativeBif::positional(future_or_timeout));
    bifs.insert("futureiscompletedexceptionally".to_string(), NativeBif::positional(future_is_completed_exceptionally));
    bifs.insert("futureisdone".to_string(), NativeBif::positional(future_is_done));
    bifs.insert("futureexceptionally".to_string(), NativeBif::positional(future_exceptionally));
    bifs.insert("futurethen".to_string(), NativeBif::positional(future_then));
    bifs.insert("futuregetasattempt".to_string(), NativeBif::positional(future_get_as_attempt));
    bifs.insert("asyncallapply".to_string(), NativeBif { func: async_all_apply, params: params!["items", "mapper", "errorhandler", "executor", "timeout" = Int(0), "timeunit" = Str("SECONDS")] });
    bifs.insert("executornew".to_string(), NativeBif { func: executor_new, params: params!["name", "type", "maxthreads" = Int(10)] });
    bifs.insert("executorget".to_string(), NativeBif { func: executor_get, params: params!["name" = Str("io-tasks")] });
    bifs.insert("executordelete".to_string(), NativeBif { func: executor_delete, params: params!["name"] });
    bifs.insert("executorhas".to_string(), NativeBif { func: executor_has, params: params!["name"] });
    bifs.insert("threadnew".to_string(), NativeBif { func: thread_new, params: params!["runnable", "attributes", "name" = Str(""), "priority" = Str("normal"), "virtual" = Bool(false)] });
    bifs.insert("threadjoin".to_string(), NativeBif { func: thread_join, params: params!["threadname" = Str(""), "timeout" = Int(0)] });
    bifs.insert("threadterminate".to_string(), NativeBif { func: thread_terminate, params: params!["threadname"] });
    bifs.insert("threadinterrupt".to_string(), NativeBif { func: thread_interrupt, params: params!["name" = Str("")] });
    bifs.insert("isthreadalive".to_string(), NativeBif { func: is_thread_alive, params: params!["name"] });
    bifs.insert("isthreadinterrupted".to_string(), NativeBif { func: is_thread_interrupted, params: params!["name"] });
    bifs.insert("isinthread".to_string(), NativeBif { func: is_in_thread, params: params![] });
}

fn future_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() > 1 {
        return Err("futureNew() expects zero or one argument".to_string());
    }

    let future = vm.future_new();
    let Some(value) = args.first().copied() else {
        return Ok(future);
    };

    if value.as_gc_id().is_some() {
        let chunk = vm
            .current_chunk()
            .ok_or_else(|| "No chunk context available".to_string())?;
        return vm.spawn_by_value(&value, Vec::new(), 0, chunk);
    }

    vm.future_resolve(future, value)?;
    Ok(future)
}

fn future_complete_exceptionally(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let [future, error] = args else {
        return Err("completeExceptionally() expects a future and an error".to_string());
    };
    vm.future_reject(*future, *error)?;
    Ok(*future)
}

fn future_complete_on_timeout(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("completeOnTimeout() expects a value and timeout".to_string());
    }
    vm.future_resolve(args[0], args[1])?;
    Ok(args[0])
}

fn future_join_or_default(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    future_value_or_default(vm, args)
}

fn future_get_or_default(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    future_value_or_default(vm, args)
}

fn future_value_or_default(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("Future default methods expect a default value".to_string());
    }
    match vm.future_status(args[0]) {
        Some(FutureStatus::Completed) => Ok(vm.future_value(args[0]).unwrap_or(args[1])),
        Some(FutureStatus::Failed(_)) | Some(FutureStatus::Pending) | None => Ok(args[1]),
    }
}

fn future_or_timeout(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("orTimeout() expects a timeout".to_string());
    }
    let message = vm.string_new("Future timed out".to_string());
    vm.future_reject(args[0], BxValue::new_ptr(message))?;
    Ok(args[0])
}

fn future_is_completed_exceptionally(
    vm: &mut dyn BxVM,
    args: &[BxValue],
) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("isCompletedExceptionally() expects a future".to_string());
    }
    Ok(BxValue::new_bool(matches!(
        vm.future_status(args[0]),
        Some(FutureStatus::Failed(_))
    )))
}

fn future_is_done(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("isDone() expects a future".to_string());
    }
    Ok(BxValue::new_bool(!matches!(
        vm.future_status(args[0]),
        Some(FutureStatus::Pending) | None
    )))
}

fn future_exceptionally(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("exceptionally() expects an error handler".to_string());
    }
    vm.future_on_error(args[0].as_gc_id().ok_or("Value is not a future")?, args[1]);
    Ok(args[0])
}

fn future_then(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 2 {
        return Err("then() expects a continuation".to_string());
    }
    vm.future_on_success(args[0].as_gc_id().ok_or("Value is not a future")?, args[1]);
    Ok(args[0])
}

fn future_get_as_attempt(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() != 1 {
        return Err("getAsAttempt() expects a future".to_string());
    }
    Ok(args[0])
}

/// Implements `asyncAllApply( items, mapper, errorHandler, executor, timeout, timeUnit )`.
///
/// Applies `mapper` to each item of an array or struct and returns the results.
/// `errorHandler`, `executor` and `timeUnit` are accepted and not used yet.
///
/// # Errors
/// Returns an error when items or the mapper are missing, when a positive
/// `timeout` of 500 or less is given (the mapping is treated as timed out), or
/// when the mapper fails.
fn async_all_apply(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("asyncAllApply() expects items and a mapper".to_string());
    }
    let timeout = args.get(4).filter(|value| !value.is_null() && value.as_number() > 0.0);
    if timeout.is_some_and(|value| value.as_number() <= 500.0) {
        return Err("asyncAllApply() timed out".to_string());
    }

    let callback = args[1];
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "No chunk context available".to_string())?;
    if vm.is_array_value(args[0]) {
        let result = vm.array_new();
        for index in 0..vm.array_len(args[0].as_gc_id().ok_or("Value is not an array")?) {
            let value = vm.array_get(args[0].as_gc_id().unwrap(), index);
            let mapped = vm.call_function_by_value(&callback, vec![value], chunk.clone())?;
            vm.array_push(result, mapped);
        }
        return Ok(BxValue::new_ptr(result));
    }

    if vm.is_struct_value(args[0]) {
        let source_id = args[0].as_gc_id().ok_or("Value is not a struct")?;
        let result = vm.struct_new();
        for key in vm.struct_key_array(source_id) {
            let item = vm.struct_new();
            let key_value = BxValue::new_ptr(vm.string_new(key.clone()));
            vm.struct_set(item, "key", key_value);
            vm.struct_set(item, "value", vm.struct_get(source_id, &key));
            let mapped =
                vm.call_function_by_value(&callback, vec![BxValue::new_ptr(item)], chunk.clone())?;
            let mapped_value = mapped
                .as_gc_id()
                .filter(|id| vm.is_struct_value(BxValue::new_ptr(*id)))
                .map(|id| vm.struct_get(id, "value"))
                .unwrap_or(mapped);
            vm.struct_set(result, &key, mapped_value);
        }
        return Ok(BxValue::new_ptr(result));
    }

    Err("asyncAllApply() expects an array or struct".to_string())
}

fn new_executor(vm: &mut dyn BxVM, name: String) -> Result<BxValue, String> {
    let id = vm.native_object_new(Rc::new(RefCell::new(ExecutorObject { name })));
    Ok(BxValue::new_ptr(id))
}

fn executor_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = args
        .first()
        .map(|value| vm.to_string(*value))
        .ok_or_else(|| "executorNew() expects a name".to_string())?;
    executors()
        .lock()
        .map_err(|_| "Executor registry lock poisoned".to_string())?
        .insert(name.clone());
    new_executor(vm, name)
}

fn executor_get(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = args
        .first()
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| "io-tasks".to_string());
    if !executors()
        .lock()
        .map_err(|_| "Executor registry lock poisoned".to_string())?
        .contains(&name)
    {
        return Err(format!("Executor '{}' does not exist", name));
    }
    new_executor(vm, name)
}

fn executor_delete(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = args
        .first()
        .map(|value| vm.to_string(*value))
        .ok_or_else(|| "executorDelete() expects a name".to_string())?;
    executors()
        .lock()
        .map_err(|_| "Executor registry lock poisoned".to_string())?
        .remove(&name);
    Ok(BxValue::new_bool(true))
}

fn executor_has(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = args
        .first()
        .map(|value| vm.to_string(*value))
        .ok_or_else(|| "executorHas() expects a name".to_string())?;
    Ok(BxValue::new_bool(
        executors()
            .lock()
            .map_err(|_| "Executor registry lock poisoned".to_string())?
            .contains(&name),
    ))
}

fn thread_scope(vm: &mut dyn BxVM) -> Result<usize, String> {
    if let Some(value) = vm.get_global_value("bxthread") {
        if let Some(id) = value.as_gc_id() {
            if vm.is_struct_value(value) {
                return Ok(id);
            }
        }
    }
    let id = vm.struct_new();
    vm.insert_global("bxthread".to_string(), BxValue::new_ptr(id));
    Ok(id)
}

fn set_thread_status(vm: &mut dyn BxVM, name: &str, status: &str) -> Result<(), String> {
    let scope = thread_scope(vm)?;
    let record = match vm.struct_get(scope, name).as_gc_id() {
        Some(id) if vm.is_struct_value(BxValue::new_ptr(id)) => id,
        _ => vm.struct_new(),
    };
    let name_value = BxValue::new_ptr(vm.string_new(name.to_string()));
    let status_value = BxValue::new_ptr(vm.string_new(status.to_string()));
    vm.struct_set(record, "name", name_value);
    vm.struct_set(record, "status", status_value);
    vm.struct_set(scope, name, BxValue::new_ptr(record));
    Ok(())
}

/// Implements `threadNew( runnable, attributes, name, priority, virtual )`.
///
/// Starts `runnable` on a new thread named `name` (a generated name when it is
/// missing) and returns its future. `attributes`, `priority` and `virtual` are
/// accepted for BoxLang compatibility and not used.
///
/// # Errors
/// Returns an error when no runnable is given or the thread cannot be started.
fn thread_new(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let callback = args
        .first()
        .ok_or_else(|| "threadNew() expects a runnable".to_string())?;
    let name = args
        .get(2)
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| format!("thread-{}", vm_key(vm)));
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "No chunk context available".to_string())?;
    let future = vm.spawn_by_value(callback, Vec::new(), 0, chunk)?;
    vm.mark_future_as_thread(future);
    threads()
        .lock()
        .map_err(|_| "Thread registry lock poisoned".to_string())?
        .insert(
            (vm_key(vm), name.clone()),
            ThreadInfo {
                future,
                terminated: false,
                interrupted: false,
            },
        );
    set_thread_status(vm, &name, "RUNNING")?;
    Ok(future)
}

fn thread_join(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let key = vm_key(vm);
    let names = if let Some(name) = args.first() {
        vec![vm.to_string(*name)]
    } else {
        threads()
            .lock()
            .map_err(|_| "Thread registry lock poisoned".to_string())?
            .keys()
            .filter(|(owner, _)| *owner == key)
            .map(|(_, name)| name.clone())
            .collect()
    };
    for name in names {
        let info = threads()
            .lock()
            .map_err(|_| "Thread registry lock poisoned".to_string())?
            .get(&(key, name.clone()))
            .copied();
        let Some(info) = info else { continue };
        if vm.future_status(info.future).is_none() {
            continue;
        }
        if !info.terminated {
            vm.future_wait(info.future)?;
        }
        set_thread_status(
            vm,
            &name,
            if info.terminated {
                "TERMINATED"
            } else {
                "COMPLETED"
            },
        )?;
    }
    Ok(BxValue::new_null())
}

fn thread_terminate(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = args
        .first()
        .map(|value| vm.to_string(*value))
        .ok_or_else(|| "threadTerminate() expects a name".to_string())?;
    if let Some(info) = threads()
        .lock()
        .map_err(|_| "Thread registry lock poisoned".to_string())?
        .get_mut(&(vm_key(vm), name.clone()))
    {
        info.terminated = true;
    }
    set_thread_status(vm, &name, "TERMINATED")?;
    Ok(BxValue::new_bool(true))
}

fn thread_interrupt(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = args
        .first()
        .map(|value| vm.to_string(*value))
        .ok_or_else(|| "threadInterrupt() expects a name".to_string())?;
    if let Some(info) = threads()
        .lock()
        .map_err(|_| "Thread registry lock poisoned".to_string())?
        .get_mut(&(vm_key(vm), name))
    {
        info.interrupted = true;
    }
    Ok(BxValue::new_bool(true))
}

fn is_thread_alive(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let name = args
        .first()
        .map(|value| vm.to_string(*value))
        .ok_or_else(|| "isThreadAlive() expects a name".to_string())?;
    let alive = threads()
        .lock()
        .map_err(|_| "Thread registry lock poisoned".to_string())?
        .get(&(vm_key(vm), name))
        .is_some_and(|info| {
            !info.terminated && matches!(vm.future_status(info.future), Some(FutureStatus::Pending))
        });
    Ok(BxValue::new_bool(alive))
}

fn is_thread_interrupted(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let name = vm.to_string(args[0]);
    let interrupted = threads()
        .lock()
        .map_err(|_| "Thread registry lock poisoned".to_string())?
        .get(&(vm_key(vm), name))
        .is_some_and(|info| info.interrupted);
    Ok(BxValue::new_bool(interrupted))
}

fn is_in_thread(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if !args.is_empty() {
        return Err("isInThread() expects no arguments".to_string());
    }
    Ok(BxValue::new_bool(vm.is_in_thread()))
}
