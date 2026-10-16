//! Thread-owned VM state; no BoxLang heap values cross an HTTP worker boundary.
use crate::{
    application::ApplicationRuntime,
    webroot_core::{AssetStore, WebrootContext, WebrootRequest, WebrootResponse},
};
#[cfg(not(target_family = "wasm"))]
use anyhow::Context;
use anyhow::Result;
#[cfg(target_family = "wasm")]
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) struct RenderJob {
    pub store: Arc<dyn AssetStore + Send + Sync>,
    pub asset_path: String,
    pub scope_id: String,
    pub request: WebrootRequest,
    pub context: WebrootContext,
}
pub(crate) struct RenderOutput {
    pub response: WebrootResponse,
}
#[derive(Clone, Default)]
pub struct WebrootRuntime {
    inner: Arc<RuntimeInner>,
}
#[cfg(not(target_family = "wasm"))]
type RenderMessage = (
    RenderJob,
    std::sync::mpsc::Sender<Result<RenderOutput, String>>,
);
#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
struct RuntimeInner {
    worker: std::sync::Mutex<Option<Worker>>,
}
#[cfg(not(target_family = "wasm"))]
struct Worker {
    sender: Option<std::sync::mpsc::SyncSender<RenderMessage>>,
    thread: Option<std::thread::JoinHandle<()>>,
    module_paths: Vec<std::path::PathBuf>,
}
#[cfg(not(target_family = "wasm"))]
impl Drop for Worker {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(not(target_family = "wasm"))]
struct ModuleBootstrap {
    project: std::path::PathBuf,
    home: Option<std::path::PathBuf>,
    config: Option<std::path::PathBuf>,
}
#[cfg(not(target_family = "wasm"))]
impl WebrootRuntime {
    /// Eager, offline module startup before an HTTP listener is published. Compilation/VM values stay on the worker.
    pub fn from_project(
        project: std::path::PathBuf,
        home: Option<std::path::PathBuf>,
        config: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        let home = home
            .filter(|path| !path.as_os_str().is_empty())
            .or_else(|| {
                std::env::var_os("BOXLANG_HOME")
                    .filter(|value| !value.is_empty())
                    .map(Into::into)
            })
            .or_else(|| {
                let user = if cfg!(windows) {
                    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
                } else {
                    std::env::var_os("HOME")
                };
                user.map(|user| std::path::PathBuf::from(user).join(".boxlang"))
            });
        if let Some(home) = &home
            && home.join("config/boxlang.json").exists()
        {
            eprintln!(
                "Warning: serverHome selects installed modules only; its runtime configuration is not applied. Use project boxlang.json for module settings; full runtime configuration remains unsupported."
            );
        }
        let runtime = Self::default();
        runtime.sender(Some(ModuleBootstrap {
            project,
            home,
            config,
        }))?;
        Ok(runtime)
    }
    /// Source-package and installation directories hosts must keep private, including through aliases.
    pub fn module_paths(&self) -> Result<Vec<std::path::PathBuf>> {
        let worker = self
            .inner
            .worker
            .lock()
            .map_err(|_| anyhow::anyhow!("Web runtime lock poisoned"))?;
        Ok(worker
            .as_ref()
            .map(|worker| worker.module_paths.clone())
            .unwrap_or_default())
    }
    fn sender(
        &self,
        bootstrap: Option<ModuleBootstrap>,
    ) -> Result<std::sync::mpsc::SyncSender<RenderMessage>> {
        let mut worker = self
            .inner
            .worker
            .lock()
            .map_err(|_| anyhow::anyhow!("Web runtime lock poisoned"))?;
        if worker.is_none() {
            let (sender, requests) = std::sync::mpsc::sync_channel::<RenderMessage>(32);
            let (ready, initialized) = std::sync::mpsc::channel();
            let thread =
                std::thread::Builder::new()
                    .name("matchbox-web-application".into())
                    .spawn(move || {
                        let mut runtime = ApplicationRuntime::new();
                        let startup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                            || match bootstrap {
                                Some(options) => runtime.load_modules(
                                    &options.project,
                                    options.home.as_deref(),
                                    options.config.as_deref(),
                                ),
                                None => Ok(Vec::new()),
                            },
                        ))
                        .map_err(|_| "Module startup panicked".to_string())
                        .and_then(|result| result.map_err(|error| error.to_string()));
                        match startup {
                            Ok(paths) => {
                                if ready.send(Ok(paths)).is_err() {
                                    runtime.shutdown();
                                    return;
                                }
                            }
                            Err(error) => {
                                runtime.shutdown();
                                let _ = ready.send(Err(error));
                                return;
                            }
                        }
                        loop {
                            match requests.recv_timeout(std::time::Duration::from_secs(1)) {
                                Ok((job, reply)) => {
                                    let result = std::panic::catch_unwind(
                                        std::panic::AssertUnwindSafe(|| runtime.render(job)),
                                    )
                                    .map_err(|_| "Web runtime panicked".to_string())
                                    .and_then(|result| result.map_err(|error| error.to_string()));
                                    let _ = reply.send(result);
                                }
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => runtime.expire(),
                                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                        runtime.shutdown();
                    })
                    .context("Starting application runtime")?;
            let paths = initialized
                .recv()
                .context("Web runtime stopped during module startup")
                .and_then(|result| result.map_err(anyhow::Error::msg));
            match paths {
                Ok(module_paths) => {
                    *worker = Some(Worker {
                        sender: Some(sender),
                        thread: Some(thread),
                        module_paths,
                    })
                }
                Err(error) => {
                    drop(sender);
                    let _ = thread.join();
                    return Err(error);
                }
            }
        }
        Ok(worker.as_ref().unwrap().sender.as_ref().unwrap().clone())
    }
    pub(crate) fn render(&self, job: RenderJob) -> Result<RenderOutput> {
        let sender = self.sender(None)?;
        let (reply, result) = std::sync::mpsc::channel();
        sender
            .send((job, reply))
            .map_err(|_| anyhow::anyhow!("Web runtime stopped"))?;
        result
            .recv()
            .context("Web runtime stopped before replying")?
            .map_err(anyhow::Error::msg)
    }
}
#[cfg(target_family = "wasm")]
struct RuntimeInner {
    id: usize,
}
#[cfg(target_family = "wasm")]
impl Default for RuntimeInner {
    fn default() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }
}
#[cfg(target_family = "wasm")]
thread_local! { static RUNTIMES: std::cell::RefCell<HashMap<usize, ApplicationRuntime>> = std::cell::RefCell::new(HashMap::new()); }
#[cfg(target_family = "wasm")]
impl WebrootRuntime {
    pub(crate) fn render(&self, job: RenderJob) -> Result<RenderOutput> {
        RUNTIMES.with(|runtimes| {
            runtimes
                .borrow_mut()
                .entry(self.inner.id)
                .or_insert_with(ApplicationRuntime::new)
                .render(job)
        })
    }
}
#[cfg(target_family = "wasm")]
impl Drop for RuntimeInner {
    fn drop(&mut self) {
        RUNTIMES.with(|runtimes| {
            if let Some(mut runtime) = runtimes.borrow_mut().remove(&self.id) {
                runtime.shutdown();
            }
        });
    }
}
