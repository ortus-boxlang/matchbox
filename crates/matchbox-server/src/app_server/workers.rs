//! Supervised, single-flight BoxLang workers. VM values never cross worker boundaries.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

type Job = (String, JsonValue);
type Notify = Arc<dyn Fn(JsonValue) + Send + Sync>;

pub(super) struct Worker {
    sender: Mutex<Option<mpsc::SyncSender<Job>>>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
    state: Arc<Mutex<JsonValue>>,
    accepting: AtomicBool,
}
impl std::fmt::Debug for Worker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BackgroundWorker")
    }
}
impl Worker {
    pub fn start(
        path: PathBuf,
        project: PathBuf,
        home: Option<PathBuf>,
        config: Option<PathBuf>,
        notify: Notify,
    ) -> anyhow::Result<Arc<Self>> {
        let state = Arc::new(Mutex::new(
            serde_json::json!({"id":"", "status":"idle", "result":null, "error":""}),
        ));
        let output = state.clone();
        let (sender, requests) = mpsc::sync_channel::<Job>(1);
        let (ready, initialized) = mpsc::channel();
        let thread = thread::Builder::new().name("matchbox-background-worker".into())
            .stack_size(crate::VM_THREAD_STACK_SIZE).spawn(move || {
            let mut vm = VM::new();
            let mut modules = None;
            let startup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> anyhow::Result<_> {
                modules = Some(runtime::load_modules(&mut vm, &project, home.as_deref(), config.as_deref())?.0);
                let source = stdfs::read_to_string(&path)?;
                let ast = parser::parse(&source, path.to_str())?;
                let mut compiler = Compiler::new(&path.to_string_lossy());
                compiler.runtime_module_classes = true;
                vm.interpret(compiler.compile(&ast, &source)?)?;
                let handler = vm.get_global("onJob").ok_or_else(|| anyhow::anyhow!("Worker script must define onJob(payload)"))?;
                anyhow::ensure!(vm.value_matches_type_name(handler, "function"), "Worker onJob must be a function");
                let root = vm.retain_value(handler).map_err(anyhow::Error::msg)?;
                vm.with_isolated_globals(|_| Ok(()))?;
                Ok((handler, root))
            })).map_err(|_| anyhow::anyhow!("Worker startup panicked")).and_then(|result| result);
            match startup {
                Ok((handler, _root)) => {
                    if ready.send(Ok(())).is_ok() {
                        while let Ok((id, payload)) = requests.recv() {
                            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vm.with_isolated_globals(|vm| {
                                let data = json_to_bx(vm, &payload).map_err(anyhow::Error::msg)?;
                                let result = vm.call_function_by_value(&handler, vec![data], Rc::new(RefCell::new(Chunk::default()))).map_err(anyhow::Error::msg)?;
                                let json = bx_to_json(vm, result).map_err(anyhow::Error::msg)?;
                                anyhow::ensure!(serde_json::to_vec(&json)?.len() <= 1024 * 1024, "Worker result exceeds limit");
                                Ok(json)
                            })));
                            let completed = match outcome {
                                Ok(Ok(result)) => serde_json::json!({"id":id,"status":"succeeded","result":result,"error":""}),
                                _ => serde_json::json!({"id":id,"status":"failed","result":null,"error":"Worker failed or its outcome is uncertain; inspect before retrying."}),
                            };
                            *output.lock().unwrap() = completed.clone();
                            notify(completed);
                        }
                    }
                }
                Err(error) => { let _ = ready.send(Err(error.to_string())); }
            }
            if let Some(modules) = modules { if modules.unload(&mut vm).is_err() { eprintln!("Worker module shutdown failed"); } }
        })?;
        match initialized.recv()? {
            Ok(()) => Ok(Arc::new(Self {
                sender: Mutex::new(Some(sender)),
                thread: Mutex::new(Some(thread)),
                state,
                accepting: AtomicBool::new(false),
            })),
            Err(error) => {
                drop(sender);
                let _ = thread.join();
                Err(anyhow::Error::msg(error))
            }
        }
    }
    pub fn open(&self) {
        self.accepting.store(true, Ordering::Release);
    }
    pub fn pause(&self) {
        let _sender = self.sender.lock().unwrap();
        self.accepting.store(false, Ordering::Release);
    }
    pub fn close(&self) {
        self.pause();
        self.sender.lock().unwrap().take();
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
    fn submit(&self, payload: JsonValue) -> Result<String, String> {
        if serde_json::to_vec(&payload)
            .map_err(|e| e.to_string())?
            .len()
            > 64 * 1024
        {
            return Err("Worker input exceeds limit".into());
        }
        let sender = self.sender.lock().unwrap();
        if !self.accepting.load(Ordering::Acquire) {
            return Err("Service Unavailable: worker is not accepting jobs".into());
        }
        let sender = sender
            .as_ref()
            .ok_or("Service Unavailable: worker stopped")?;
        let mut state = self.state.lock().unwrap();
        if state["status"] == "running" {
            return Ok(String::new());
        }
        let id = Uuid::new_v4().to_string();
        let previous = state.clone();
        *state = serde_json::json!({"id":id,"status":"running","result":null,"error":""});
        if sender.try_send((id.clone(), payload)).is_err() {
            *state = previous;
            return Err("Worker stopped or queue is full".into());
        }
        Ok(id)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug)]
pub(super) struct WorkerObject(pub Arc<Worker>);
impl BxNativeObject for WorkerObject {
    fn get_property(&self, _: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _: &str, _: BxValue) {}
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_ascii_lowercase().as_str() {
            "submit" => {
                if args.len() != 1 {
                    return Err("submit(payload) requires one JSON-compatible value".into());
                }
                let payload = bx_to_json(vm, args[0])?;
                let id = self.0.submit(payload)?;
                Ok(BxValue::new_ptr(vm.string_new(id)))
            }
            "status" => {
                if !args.is_empty() {
                    return Err("status() takes no arguments".into());
                }
                json_to_bx(vm, &self.0.state.lock().unwrap())
            }
            _ => Err(format!("Unknown worker method: {name}")),
        }
    }
}
