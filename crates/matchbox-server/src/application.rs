//! Application.bx listeners and live application scopes in one VM-owned heap.
use crate::{
    application_runtime::{RenderJob, RenderOutput},
    web_context::install_web_context,
    webroot_core::{
        AssetStore, cgi_scope_from_request, form_scope_from_request, insert_scope,
        parse_cookie_header,
    },
};
use anyhow::{Context, Result, bail, ensure};
use matchbox_compiler::{ast::StatementKind, compiler::Compiler, parser};
use matchbox_vm::{
    types::{BxVM, BxValue, NativeCallback},
    vm::{AbortSignal, VM, chunk::Chunk, gc::RootedValue},
};
use std::{
    cell::Cell,
    collections::HashMap,
    path::{Component, Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

struct IncludeDepth<'a>(&'a Cell<usize>);
impl Drop for IncludeDepth<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

struct ApplicationState {
    scope: Rc<RootedValue>,
    listener: Option<Rc<RootedValue>>,
    touched: Instant,
    timeout: Option<Duration>,
}
pub(crate) struct ApplicationRuntime {
    vm: VM,
    applications: HashMap<String, ApplicationState>,
}
impl ApplicationRuntime {
    pub(crate) fn new() -> Self {
        Self {
            vm: VM::new(),
            applications: HashMap::new(),
        }
    }
    pub(crate) fn render(&mut self, job: RenderJob) -> Result<RenderOutput> {
        self.expire();
        let Self { vm, applications } = self;
        vm.with_isolated_globals(|vm| render(vm, applications, job))
    }
    pub(crate) fn expire(&mut self) {
        let expired: Vec<_> = self
            .applications
            .iter()
            .filter(|(_, app)| {
                app.timeout
                    .is_some_and(|timeout| app.touched.elapsed() >= timeout)
            })
            .map(|(name, _)| name.clone())
            .collect();
        let changed = !expired.is_empty();
        for name in expired {
            if let Some(app) = self.applications.remove(&name) {
                end_application(&mut self.vm, app);
            }
        }
        if changed {
            self.vm.collect_garbage_now();
        }
    }
    pub(crate) fn shutdown(&mut self) {
        for (_, app) in self.applications.drain() {
            end_application(&mut self.vm, app);
        }
    }
}
fn end_application(vm: &mut VM, app: ApplicationState) {
    let Some(listener) = app.listener else {
        return;
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vm.with_isolated_globals(|vm| {
            vm.begin_output_capture();
            let scope = vm
                .read_retained_value(&app.scope)
                .map_err(anyhow::Error::msg)?;
            let listener = vm
                .read_retained_value(&listener)
                .map_err(anyhow::Error::msg)?;
            vm.insert_global("application".into(), scope);
            hook(vm, listener, "onApplicationEnd", vec![scope])?;
            vm.end_output_capture();
            Ok(())
        })
    }))
    .map_err(|_| anyhow::anyhow!("onApplicationEnd panicked"))
    .and_then(|result| result);
    if let Err(error) = result {
        eprintln!("onApplicationEnd failed: {error}");
    }
}

fn render(
    vm: &mut VM,
    applications: &mut HashMap<String, ApplicationState>,
    job: RenderJob,
) -> Result<RenderOutput> {
    vm.begin_output_capture();
    let request = Rc::new(job.request);
    let response = install_web_context(
        vm,
        request.clone(),
        &job.context,
        job.store.source_path("")?,
    )?;
    insert_scope(vm, "url", &request.query);
    insert_scope(vm, "form", &form_scope_from_request(&request));
    let mut cookies = request
        .headers
        .get("cookie")
        .map(|value| parse_cookie_header(value))
        .unwrap_or_default();
    insert_scope(vm, "cookie", &cookies);
    insert_scope(
        vm,
        "cgi",
        &cgi_scope_from_request(&request, &job.asset_path, &job.context),
    );
    vm.insert_empty_struct_global("request");
    let public_path = if job.context.script_name.is_empty() {
        format!("/{}", job.asset_path)
    } else {
        job.context.script_name.clone()
    };
    let prefix = public_path
        .strip_suffix(&job.asset_path)
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    let store = job.store.clone();
    let include_depth = Rc::new(Cell::new(0));
    vm.register_callback(
        "include",
        NativeCallback {
            parameters: vec!["template".into()],
            callback: Rc::new(move |vm, args| {
                if args.len() != 1 || !vm.is_string_value(args[0]) {
                    return Err("include requires a template path".into());
                }
                // shortcut: cap dynamic includes at 16, replace recursion with VM include frames before raising this limit.
                if include_depth.get() >= 16 {
                    return Err("Maximum dynamic include depth exceeded".into());
                }
                include_depth.set(include_depth.get() + 1);
                let _depth = IncludeDepth(include_depth.as_ref());
                let target = vm.to_string(args[0]);
                let current = vm
                    .current_chunk()
                    .ok_or("Include context unavailable")?
                    .borrow()
                    .filename
                    .clone();
                let path = include_path(store.as_ref(), &prefix, &current, &target)
                    .map_err(|error| error.to_string())?;
                let receiver = vm.current_receiver();
                let chunk = load_template(store.as_ref(), &path, receiver.is_some())
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Missing template: {target}"))?;
                vm.interpret_chunk_with_receiver(chunk, receiver)
            }),
        },
    )
    .map_err(anyhow::Error::msg)?;
    let definition = find_application(job.store.as_ref(), &job.asset_path)?;
    let listener = if let Some((path, source)) = &definition {
        let filename = job.store.source_path(path)?;
        let ast = parser::parse(source, Some(&filename))?;
        let classes: Vec<_> = ast
            .iter()
            .filter_map(|statement| {
                if let StatementKind::ClassDecl { name, .. } = &statement.kind {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        ensure!(classes.len() == 1, "Application.bx must define one class");
        let chunk = Compiler::new(&filename).compile(&ast, source)?;
        vm.interpret_chunk_borrowed_current_task(&chunk)?;
        let listener = match vm.construct_global_class(&classes[0], vec![]) {
            Ok(listener) => listener,
            Err(error) => {
                if let Some(abort) = error.downcast_ref::<AbortSignal>() {
                    if let Some(cause) = &abort.cause {
                        bail!("{cause}");
                    }
                    let mut response = response.borrow_mut();
                    response
                        .body
                        .extend(vm.end_output_capture().unwrap_or_default().into_bytes());
                    return Ok(RenderOutput {
                        response: response.clone(),
                        session: None,
                    });
                }
                return Err(error);
            }
        };
        vm.insert_global("__mb_web_listener".into(), listener);
        Some(listener)
    } else {
        None
    };
    let (key, timeout) = if let Some(listener) = listener {
        if let Some(setting) = vm.instance_property(listener, "sessionManagement") {
            let enabled = vm
                .cast_value_to_type(setting, "boolean")
                .map_err(anyhow::Error::msg)?;
            ensure!(
                !enabled.as_bool(),
                "Application sessionManagement=true requires the pending MiniServer session runtime; legacy unnamed sessions are not application-isolated"
            );
        }
        let name = vm
            .instance_property(listener, "name")
            .filter(|value| !value.is_null())
            .map(|value| vm.to_string(value))
            .filter(|name| !name.trim().is_empty());
        let key = if let Some(name) = name {
            format!("named:{}", name.to_lowercase())
        } else {
            let path = job.store.source_path(&definition.as_ref().unwrap().0)?;
            if Path::new(&path).is_absolute() {
                format!("source:{path}")
            } else {
                format!("source:{}:{path}", job.scope_id)
            }
        };
        let timeout = if let Some(timeout) = vm
            .instance_property(listener, "applicationTimeout")
            .filter(|value| !value.is_null())
        {
            ensure!(
                timeout.is_number() || timeout.is_int(),
                "applicationTimeout must be a createTimeSpan() duration"
            );
            let seconds = timeout.as_number();
            ensure!(
                seconds.is_finite() && seconds >= 0.0,
                "applicationTimeout must be a finite nonnegative duration"
            );
            if seconds == 0.0 {
                None
            } else {
                Some(Duration::try_from_secs_f64(seconds).context("Invalid applicationTimeout")?)
            }
        } else {
            None
        };
        (key, timeout)
    } else {
        (format!("default:{}", job.scope_id), None)
    };
    let first = !applications.contains_key(&key);
    if first {
        let scope = vm.insert_empty_struct_global("application");
        applications.insert(
            key.clone(),
            ApplicationState {
                scope: vm.retain_value(scope).map_err(anyhow::Error::msg)?,
                listener: listener
                    .map(|value| vm.retain_value(value).map_err(anyhow::Error::msg))
                    .transpose()?,
                touched: Instant::now(),
                timeout,
            },
        );
    }
    let app = applications.get_mut(&key).unwrap();
    app.touched = Instant::now();
    app.timeout = timeout;
    vm.insert_global(
        "application".into(),
        vm.read_retained_value(&app.scope)
            .map_err(anyhow::Error::msg)?,
    );
    let session = if listener.is_none() {
        cookies.insert("MBX_SESSION_ID".into(), job.session_id.clone());
        insert_scope(vm, "cookie", &cookies);
        Some(insert_scope(vm, "session", &job.session))
    } else {
        None
    };
    let _session_root = session
        .map(|id| {
            vm.retain_value(BxValue::new_ptr(id))
                .map_err(anyhow::Error::msg)
        })
        .transpose()?;
    let execution = (|| -> Result<()> {
        if first
            && let Some(listener) = listener
            && let Err(error) = hook(vm, listener, "onApplicationStart", vec![])
        {
            applications.remove(&key);
            return Err(error);
        }
        if let Some(listener) = listener {
            let target = BxValue::new_ptr(vm.string_new(public_path.clone()));
            vm.insert_global("__mb_web_target".into(), target);
            let start = hook(vm, listener, "onRequestStart", vec![target])?;
            let proceed = match start {
                None => true,
                Some(value) if value.is_null() => true,
                Some(value) => vm
                    .cast_value_to_type(value, "boolean")
                    .map_err(anyhow::Error::msg)?
                    .as_bool(),
            };
            if proceed {
                if vm.instance_has_method(listener, "onRequest") {
                    hook(vm, listener, "onRequest", vec![target])?;
                } else if let Some(chunk) =
                    load_template(job.store.as_ref(), &job.asset_path, true)?
                {
                    vm.interpret_chunk_with_receiver(&chunk, Some(listener))?;
                } else {
                    let handled = hook(vm, listener, "onMissingTemplate", vec![target])?
                        .filter(|value| !value.is_null())
                        .map(|value| {
                            vm.cast_value_to_type(value, "boolean")
                                .map(|value| value.as_bool())
                        })
                        .transpose()
                        .map_err(anyhow::Error::msg)?
                        .unwrap_or(false);
                    if !handled {
                        response.borrow_mut().status = 404;
                        vm.write_output("Not Found");
                    }
                    return Ok(());
                }
            }
            hook(vm, listener, "onRequestEnd", vec![target])?;
        } else if let Some(chunk) = load_template(job.store.as_ref(), &job.asset_path, false)? {
            vm.interpret_chunk_with_receiver(&chunk, None)?;
        } else {
            response.borrow_mut().status = 404;
            vm.write_output("Not Found");
        }
        Ok(())
    })();
    let mut error = execution.err();
    let mut error_type = "BoxLangRuntimeException";
    if let Some(abort) = error
        .as_ref()
        .and_then(|error| error.downcast_ref::<AbortSignal>())
        .cloned()
    {
        error = None;
        if let Some(listener) = listener {
            let target = BxValue::new_ptr(vm.string_new(public_path.clone()));
            if let Err(failure) = hook(vm, listener, "onAbort", vec![target]) {
                error = match failure.downcast_ref::<AbortSignal>() {
                    Some(signal) => {
                        error_type = "Custom";
                        signal
                            .cause
                            .as_ref()
                            .map(|cause| anyhow::anyhow!("{cause}"))
                    }
                    None => Some(failure),
                };
            }
        }
        if let Some(cause) = abort.cause {
            error_type = "Custom";
            error = Some(anyhow::anyhow!("{cause}"));
        }
    }
    if let Some(error) = error {
        if let Some(listener) =
            listener.filter(|listener| vm.instance_has_method(*listener, "onError"))
        {
            let error_scope = insert_scope(
                vm,
                "__mb_web_error",
                &HashMap::from([
                    ("message".into(), error.to_string()),
                    ("type".into(), error_type.into()),
                    ("detail".into(), String::new()),
                ]),
            );
            let event = BxValue::new_ptr(vm.string_new(String::new()));
            if let Err(error) = hook(
                vm,
                listener,
                "onError",
                vec![BxValue::new_ptr(error_scope), event],
            ) && error.downcast_ref::<AbortSignal>().is_none()
            {
                return Err(error);
            }
        } else {
            return Err(error);
        }
    }
    let session = session.map(|id| {
        let mut session = HashMap::new();
        for key in vm.struct_key_array(id) {
            session.insert(key.clone(), vm.to_string(vm.struct_get(id, &key)));
        }
        response.borrow_mut().headers.push((
            "set-cookie".into(),
            format!("MBX_SESSION_ID={}; Path=/; HttpOnly", job.session_id),
        ));
        session
    });
    let mut response = response.borrow_mut();
    response
        .body
        .extend(vm.end_output_capture().unwrap_or_default().into_bytes());
    Ok(RenderOutput {
        response: response.clone(),
        session,
    })
}

fn hook(vm: &mut VM, listener: BxValue, name: &str, args: Vec<BxValue>) -> Result<Option<BxValue>> {
    if !vm.instance_has_method(listener, name) {
        return Ok(None);
    }
    let result = vm.call_method_value(listener, name, args)?;
    ensure!(
        vm.fibers.is_empty(),
        "Detached work in application hooks is not supported"
    );
    Ok(Some(result))
}
fn find_application(store: &dyn AssetStore, asset: &str) -> Result<Option<(String, String)>> {
    let mut directory = Path::new(asset)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    loop {
        let path = directory
            .join("Application.bx")
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(bytes) = store.read(&path)? {
            return Ok(Some((path, String::from_utf8(bytes)?)));
        }
        if !directory.pop() {
            return Ok(None);
        }
    }
}
fn load_template(store: &dyn AssetStore, path: &str, class_context: bool) -> Result<Option<Chunk>> {
    if !class_context && let Some(chunk) = store.compiled_template(path)? {
        return Ok(Some(chunk));
    }
    let Some(bytes) = store.read(path)? else {
        return Ok(None);
    };
    let source = String::from_utf8(bytes)?;
    let filename = store.source_path(path)?;
    let ast = if path.ends_with(".bxm") {
        parser::parse_bxm(&source, Some(&filename))?
    } else {
        parser::parse(&source, Some(&filename))?
    };
    let mut compiler = Compiler::new(&filename);
    Ok(Some(if class_context {
        compiler.compile_class_template(&ast, &source)?
    } else {
        compiler.compile(&ast, &source)?
    }))
}
fn include_path(
    store: &dyn AssetStore,
    prefix: &str,
    current: &str,
    target: &str,
) -> Result<String> {
    let root = store.source_path("")?;
    let path = if Path::new(&root).is_absolute() && Path::new(target).starts_with(&root) {
        Path::new(target).strip_prefix(&root)?.to_path_buf()
    } else if target.starts_with('/') {
        PathBuf::from(
            target
                .strip_prefix(&format!("{prefix}/"))
                .unwrap_or(target)
                .trim_start_matches('/'),
        )
    } else {
        let current = Path::new(current)
            .strip_prefix(&root)
            .unwrap_or_else(|_| Path::new(current));
        current
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(target)
    };
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => (),
            Component::ParentDir if !parts.is_empty() => {
                parts.pop();
            }
            _ => bail!("Include path escapes the webroot"),
        }
    }
    Ok(parts.join("/"))
}
