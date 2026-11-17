//! One module-aware VM per routed application. Only owned request/response data crosses threads.
#[cfg(not(target_family = "wasm"))]
mod native {
    use super::super::*;
    use matchbox_vm::bundle::LoadedModules;
    use std::sync::mpsc::{SyncSender, TrySendError};

    pub(crate) enum Message {
        Request(RequestData, mpsc::Sender<Result<ResponseData, String>>),
        Socket(crate::websocket::WebSocketRuntimeCommand),
        Completed(usize, JsonValue),
    }

    pub(crate) struct Runtime {
        sender: Option<Arc<SyncSender<Message>>>,
        thread: Mutex<Option<thread::JoinHandle<()>>>,
        workers: Vec<Arc<workers::Worker>>,
        pub listen: ListenConfig,
        pub websocket: Option<WebSocketConfig>,
    }

    impl Runtime {
        pub fn start(
            program: CompiledScriptApp,
            home: Option<PathBuf>,
            config: Option<PathBuf>,
        ) -> anyhow::Result<Arc<Self>> {
            let (sender, requests) = mpsc::sync_channel::<Message>(32);
            let sender = Arc::new(sender);
            let events = Arc::downgrade(&sender);
            let (ready, initialized) = mpsc::channel();
            let thread = thread::Builder::new()
                .name("matchbox-routed-application".into())
                .stack_size(crate::VM_THREAD_STACK_SIZE)
                .spawn(move || {
                    let mut vm = VM::new();
                    let mut modules = None;
                    let build = Arc::new(Mutex::new(BuildState {
                        events,
                        home: home.clone(),
                        config: config.clone(),
                        ..BuildState::default()
                    }));
                    let startup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || -> anyhow::Result<_> {
                            let (loaded, private_paths) = load_modules(
                                &mut vm,
                                &compiled_app_root(&program),
                                home.as_deref(),
                                config.as_deref(),
                            )?;
                            modules = Some(loaded);
                            build.lock().unwrap().private_paths = private_paths;
                            if program.web_imported {
                                install_web_namespace(
                                    &mut vm,
                                    build.clone(),
                                    compiled_app_root(&program),
                                );
                            }
                            vm.interpret(program.chunk.clone())?;
                            let app = pick_primary_app(&build)?;
                            for definition in &build.lock().unwrap().apps {
                                definition.lock().unwrap().configuring = false;
                            }
                            // Native route tables contain VM values; retain callbacks across request GC.
                            let mut roots = Vec::new();
                            for value in app
                                .middleware
                                .iter()
                                .copied()
                                .chain(app.routes.iter().flat_map(|route| {
                                    route
                                        .middleware
                                        .iter()
                                        .copied()
                                        .chain(std::iter::once(route.handler))
                                }))
                                .chain(app.websocket_listener)
                                .chain(app.job_callbacks.iter().copied())
                                .chain(app.timers.iter().map(|(_, handler)| *handler))
                            {
                                roots.push(vm.retain_value(value).map_err(anyhow::Error::msg)?);
                            }
                            let sockets = app.websocket_listener.map(|listener| {
                                crate::websocket::SocketState::new(&mut vm, listener)
                            });
                            vm.with_isolated_globals(|_| Ok(()))?;
                            for worker in &build.lock().unwrap().workers {
                                worker.open();
                            }
                            Ok((app, roots, sockets))
                        },
                    ))
                    .map_err(|_| "Routed application startup panicked".to_string())
                    .and_then(|result| result.map_err(|e| e.to_string()));
                    match startup {
                        Ok((app, _roots, sockets)) => {
                            if ready
                                .send(Ok((
                                    app.listen.clone().unwrap_or_default(),
                                    app.websocket.clone(),
                                    build.lock().unwrap().workers.clone(),
                                )))
                                .is_ok()
                            {
                                let mut deadlines =
                                    vec![std::time::Instant::now(); app.timers.len()];
                                loop {
                                    for (index, (interval, handler)) in
                                        app.timers.iter().enumerate()
                                    {
                                        let now = std::time::Instant::now();
                                        if now >= deadlines[index] {
                                            deadlines[index] = now + *interval;
                                            let result = std::panic::catch_unwind(
                                                std::panic::AssertUnwindSafe(|| {
                                                    vm.with_isolated_globals(|vm| {
                                                        vm.call_function_by_value(
                                                            handler,
                                                            vec![],
                                                            Rc::new(RefCell::new(Chunk::default())),
                                                        )
                                                        .map_err(anyhow::Error::msg)?;
                                                        Ok(())
                                                    })
                                                }),
                                            );
                                            if !matches!(result, Ok(Ok(()))) {
                                                eprintln!("Timer callback failed");
                                            }
                                        }
                                    }
                                    let wait =
                                        deadlines
                                            .iter()
                                            .map(|deadline| {
                                                deadline.saturating_duration_since(
                                                    std::time::Instant::now(),
                                                )
                                            })
                                            .min()
                                            .unwrap_or(std::time::Duration::from_secs(3600));
                                    let message = match requests.recv_timeout(wait) {
                                        Ok(message) => message,
                                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                                    };
                                    match message {
                                        Message::Request(request, reply) => {
                                            let result = std::panic::catch_unwind(
                                                std::panic::AssertUnwindSafe(|| {
                                                    vm.with_isolated_globals(|vm| {
                                                        dispatch_in_vm(&program, vm, &app, request)
                                                    })
                                                }),
                                            )
                                            .map_err(|_| "Routed request panicked".to_string())
                                            .and_then(|result| result.map_err(|e| e.to_string()));
                                            let _ = reply.send(result);
                                        }
                                        Message::Completed(index, result) => {
                                            if let Some(handler) = app.job_callbacks.get(index) {
                                                let completed = std::panic::catch_unwind(
                                                    std::panic::AssertUnwindSafe(|| {
                                                        vm.with_isolated_globals(|vm| {
                                                            let data = json_to_bx(vm, &result)
                                                                .map_err(anyhow::Error::msg)?;
                                                            vm.call_function_by_value(
                                                                handler,
                                                                vec![data],
                                                                Rc::new(RefCell::new(
                                                                    Chunk::default(),
                                                                )),
                                                            )
                                                            .map_err(anyhow::Error::msg)?;
                                                            Ok(())
                                                        })
                                                    }),
                                                );
                                                if !matches!(completed, Ok(Ok(()))) {
                                                    eprintln!("Worker completion callback failed");
                                                }
                                            }
                                        }
                                        Message::Socket(command) => {
                                            if let Some(sockets) = &sockets {
                                                let result = std::panic::catch_unwind(
                                                    std::panic::AssertUnwindSafe(|| {
                                                        vm.with_isolated_globals(|vm| {
                                                            sockets.command(vm, command);
                                                            Ok(())
                                                        })
                                                    }),
                                                );
                                                if !matches!(result, Ok(Ok(()))) {
                                                    eprintln!("WebSocket callback failed");
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(error) => {
                            let _ = ready.send(Err(error));
                        }
                    }
                    for worker in &build.lock().unwrap().workers {
                        worker.close();
                    }
                    if let Some(modules) = modules {
                        if let Err(error) = modules.unload(&mut vm) {
                            eprintln!("Routed module shutdown failed: {error}");
                        }
                    }
                })?;
            match initialized.recv()? {
                Ok((listen, websocket, workers)) => Ok(Arc::new(Self {
                    sender: Some(sender),
                    thread: Mutex::new(Some(thread)),
                    workers,
                    listen,
                    websocket,
                })),
                Err(error) => {
                    drop(sender);
                    let _ = thread.join();
                    Err(anyhow::Error::msg(error))
                }
            }
        }
        pub fn begin_shutdown(&self) {
            for worker in &self.workers {
                worker.pause();
            }
        }
        pub fn socket(
            &self,
            command: crate::websocket::WebSocketRuntimeCommand,
        ) -> Result<(), String> {
            self.sender
                .as_ref()
                .unwrap()
                .send(Message::Socket(command))
                .map_err(|_| "Routed runtime stopped".to_string())
        }
        pub fn request(&self, request: RequestData) -> anyhow::Result<ResponseData> {
            let (reply, result) = mpsc::channel();
            self.sender
                .as_ref()
                .unwrap()
                .try_send(Message::Request(request, reply))
                .map_err(|error| match error {
                    TrySendError::Full(_) => {
                        anyhow::anyhow!("Service Unavailable: routed request queue is full")
                    }
                    TrySendError::Disconnected(_) => {
                        anyhow::anyhow!("Service Unavailable: routed runtime stopped")
                    }
                })?;
            result.recv()?.map_err(anyhow::Error::msg)
        }
    }
    impl Drop for Runtime {
        fn drop(&mut self) {
            self.begin_shutdown();
            self.sender.take();
            if let Some(thread) = self.thread.lock().unwrap().take() {
                let _ = thread.join();
            }
        }
    }

    pub(crate) fn load_modules(
        vm: &mut VM,
        project: &Path,
        home: Option<&Path>,
        config: Option<&Path>,
    ) -> anyhow::Result<(LoadedModules, Vec<PathBuf>)> {
        // Module/class callbacks compile separately, so startup must load the full standard library.
        let source = matchbox_compiler::PRELUDE_SOURCE;
        let ast = matchbox_compiler::parser::parse(source, Some("prelude.bxs"))?;
        let chunk =
            matchbox_compiler::compiler::Compiler::new("prelude.bxs").compile(&ast, source)?;
        vm.interpret(chunk)?;
        let default_home = std::env::var_os("BOXLANG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".boxlang")));
        let discovered = matchbox_modules::discovery::discover_module_installations(
            project,
            &[],
            home.or(default_home.as_deref()),
            config,
        )?;
        let programs = matchbox_modules::programs::prepare_installed(
            &discovered.modules,
            matchbox_vm::bundle::TARGET,
        )?;
        let modules = LoadedModules::load(vm, programs, &matchbox_vm::bundle::cache_root())?;
        Ok((modules, discovered.private_paths))
    }
}
#[cfg(not(target_family = "wasm"))]
pub(super) use native::{load_modules, Message, Runtime};

#[cfg(target_family = "wasm")]
mod unsupported {
    use super::super::*;
    pub(crate) enum Message {}
    pub(crate) struct Runtime {
        pub listen: ListenConfig,
        pub websocket: Option<WebSocketConfig>,
    }
    impl Runtime {
        pub fn start(
            _: CompiledScriptApp,
            _: Option<PathBuf>,
            _: Option<PathBuf>,
        ) -> anyhow::Result<Arc<Self>> {
            anyhow::bail!(
                "Routed application workers require a native server; use the WASI webroot frontend"
            )
        }
        pub fn begin_shutdown(&self) {}
        pub fn request(&self, _: RequestData) -> anyhow::Result<ResponseData> {
            anyhow::bail!("Native routed runtime unavailable")
        }
        pub fn socket(&self, _: crate::websocket::WebSocketRuntimeCommand) -> Result<(), String> {
            Err("Native routed runtime unavailable".into())
        }
    }
}
#[cfg(target_family = "wasm")]
pub(super) use unsupported::{Message, Runtime};
