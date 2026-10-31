//! Application.bx listeners and live application scopes in one VM-owned heap.
use crate::{
    application_runtime::{RenderJob, RenderOutput},
    web_context::install_web_context,
    webroot_core::{
        AssetStore, cgi_scope_from_request, form_scope_from_request, insert_parameter_scope,
        insert_scope, parameter_values, parse_cookie_header,
    },
};
use anyhow::{Context, Result, bail, ensure};
use matchbox_compiler::{ast::StatementKind, compiler::Compiler, parser};
use matchbox_vm::{
    types::{BxVM, BxValue, NativeCallback},
    vm::{AbortSignal, VM, chunk::Chunk, gc::RootedValue},
};
use std::{
    cell::{Cell, RefCell},
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
    sessions: crate::sessions::Store,
}
pub(crate) struct ApplicationRuntime {
    vm: VM,
    applications: HashMap<String, ApplicationState>,
    templates: Rc<RefCell<crate::template_cache::TemplateCache>>,
    #[cfg(not(target_family = "wasm"))]
    modules: Option<matchbox_vm::bundle::LoadedModules>,
}
impl ApplicationRuntime {
    pub(crate) fn new() -> Self {
        Self {
            vm: VM::new(),
            applications: HashMap::new(),
            templates: Rc::new(RefCell::new(Default::default())),
            #[cfg(not(target_family = "wasm"))]
            modules: None,
        }
    }
    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn load_modules(
        &mut self,
        project: &Path,
        home: Option<&Path>,
        config: Option<&Path>,
    ) -> Result<(Vec<PathBuf>, Vec<matchbox_vm::bundle::ModuleMapping>)> {
        let discovered =
            matchbox_modules::discovery::discover_module_installations(project, &[], home, config)?;
        let modules = matchbox_modules::programs::prepare_installed(
            &discovered.modules,
            matchbox_vm::bundle::TARGET,
        )?;
        // Install even an empty registry so module inventory/settings APIs remain real, not missing BIFs.
        self.modules = Some(matchbox_vm::bundle::LoadedModules::load(
            &mut self.vm,
            modules,
            &matchbox_vm::bundle::cache_root(),
        )?);
        // Validate idle startup using the same boundary that will guard actual requests.
        self.vm.with_isolated_globals(|_| Ok(()))?;
        let mappings = self.modules.as_ref().unwrap().mappings(&self.vm)?;
        Ok((discovered.private_paths, mappings))
    }
    pub(crate) fn render(&mut self, job: RenderJob) -> Result<RenderOutput> {
        self.expire();
        let Self {
            vm, applications, templates, ..
        } = self;
        vm.with_isolated_globals(|vm| render(vm, applications, templates.clone(), job))
    }
    pub(crate) fn expire(&mut self) {
        for app in self.applications.values() {
            crate::sessions::expire(&mut self.vm, &app.scope, &app.sessions, false);
        }
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
        #[cfg(not(target_family = "wasm"))]
        if let Some(modules) = self.modules.take()
            && let Err(error) = modules.unload(&mut self.vm)
        {
            eprintln!("Module shutdown failed: {error}");
        }
    }
}
fn end_application(vm: &mut VM, app: ApplicationState) {
    crate::sessions::expire(vm, &app.scope, &app.sessions, true);
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
    templates: Rc<RefCell<crate::template_cache::TemplateCache>>,
    job: RenderJob,
) -> Result<RenderOutput> {
    let debug = job.context.debug;
    match render_request(vm, applications, templates, job) {
        Ok(output) => Ok(output),
        Err(error) => {
            eprintln!("Request failed: {error}");
            match crate::web_context::error_response(vm, &error.to_string(), debug) {
                Ok(response) => Ok(RenderOutput { response }),
                Err(_) => Err(error),
            }
        }
    }
}
fn render_request(
    vm: &mut VM,
    applications: &mut HashMap<String, ApplicationState>,
    templates: Rc<RefCell<crate::template_cache::TemplateCache>>,
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
    let query = if job.context.query_string.is_empty() {
        (request.query.clone(), Default::default())
    } else {
        parameter_values(job.context.query_string.as_bytes(), true)
    };
    insert_parameter_scope(vm, "url", query);
    insert_parameter_scope(vm, "form", form_scope_from_request(&request));
    let cookies = request
        .headers
        .get("cookie")
        .map(|value| parse_cookie_header(value))
        .unwrap_or_default();
    let cookie_jar = crate::web_context::cookie_jar(vm).map_err(anyhow::Error::msg)?;
    crate::cookies::install(vm, &cookies, cookie_jar.clone()).map_err(anyhow::Error::msg)?;
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
    let include_templates = templates.clone();
    let include_scope = job.scope_id.clone();
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
                let chunk = load_template(
                    &include_templates,
                    store.as_ref(),
                    &include_scope,
                    &path,
                    receiver.is_some(),
                    vm.has_global_function("getModuleInfo"),
                )
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
        let mut compiler = Compiler::new(&filename);
        compiler.runtime_module_classes = vm.has_global_function("getModuleInfo");
        let chunk = compiler.compile(&ast, source)?;
        vm.interpret_chunk_borrowed_current_task(&chunk)?;
        let listener = match vm.construct_global_class(&classes[0], vec![]) {
            Ok(listener) => listener,
            Err(error) => {
                if let Some(abort) = error.downcast_ref::<AbortSignal>() {
                    if let Some(cause) = &abort.cause {
                        bail!("{cause}");
                    }
                    let mut response = response.borrow_mut();
                    response.headers.extend(
                        cookie_jar
                            .borrow()
                            .headers()
                            .map(|value| ("set-cookie".into(), value)),
                    );
                    response
                        .body
                        .extend(vm.end_output_capture().unwrap_or_default().into_bytes());
                    return Ok(RenderOutput {
                        response: response.clone(),
                    });
                }
                return Err(error);
            }
        };
        vm.insert_global("__mb_web_listener".into(), listener);
        for (property, global) in [
            ("allowedFileOperationExtensions", "__mb_upload_allowed"),
            ("disallowedFileOperationExtensions", "__mb_upload_blocked"),
        ] {
            if let Some(value) = vm
                .instance_property(listener, property)
                .filter(|value| !value.is_null())
            {
                vm.insert_global(global.into(), value);
            }
        }
        Some(listener)
    } else {
        None
    };
    let (key, timeout) = if let Some(listener) = listener {
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
                sessions: Default::default(),
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
    let session_settings = crate::sessions::Settings::from_listener(vm, listener)?;
    let session_id = session_settings.id(&cookies);
    if session_settings.enabled {
        session_settings
            .emit(&cookie_jar, &session_id)
            .map_err(anyhow::Error::msg)?;
    }
    let session_store = app.sessions.clone();
    let app_scope = app.scope.clone();
    let end_listener = app.listener.clone();
    let execution = (|| -> Result<()> {
        if first
            && let Some(listener) = listener
            && let Err(error) = hook(vm, listener, "onApplicationStart", vec![])
        {
            applications.remove(&key);
            return Err(error);
        }
        if let Some(listener) = listener {
            if session_settings.enabled {
                let name = vm
                    .instance_property(listener, "name")
                    .map(|value| vm.to_string(value))
                    .unwrap_or_else(|| key.clone());
                crate::sessions::attach(
                    vm,
                    session_store,
                    app_scope,
                    listener,
                    end_listener.unwrap(),
                    name,
                    session_id,
                    session_settings,
                    cookie_jar.clone(),
                )?;
            }
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
                } else if let Some(chunk) = load_template(
                    &templates,
                    job.store.as_ref(),
                    &job.scope_id,
                    &job.asset_path,
                    true,
                    vm.has_global_function("getModuleInfo"),
                )? {
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
        } else if let Some(chunk) = load_template(
            &templates,
            job.store.as_ref(),
            &job.scope_id,
            &job.asset_path,
            false,
            vm.has_global_function("getModuleInfo"),
        )? {
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
    let mut response = response.borrow_mut();
    response.headers.extend(
        cookie_jar
            .borrow()
            .headers()
            .map(|value| ("set-cookie".into(), value)),
    );
    response
        .body
        .extend(vm.end_output_capture().unwrap_or_default().into_bytes());
    Ok(RenderOutput {
        response: response.clone(),
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
// End the cache borrow before execution, which can recursively include templates.
fn load_template(
    templates: &RefCell<crate::template_cache::TemplateCache>,
    store: &dyn AssetStore,
    scope: &str,
    path: &str,
    class_context: bool,
    module_classes: bool,
) -> Result<Option<Chunk>> {
    templates.borrow_mut().load(store, scope, path, class_context, module_classes)
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
