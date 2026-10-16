//! Thread-owned VM state; no BoxLang heap values cross an HTTP worker boundary.
use crate::{
    application::ApplicationRuntime,
    webroot_core::{AssetStore, WebrootContext, WebrootRequest, WebrootResponse},
};
#[cfg(not(target_family = "wasm"))]
use anyhow::Context;
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};

pub(crate) struct RenderJob {
    pub store: Arc<dyn AssetStore + Send + Sync>,
    pub asset_path: String,
    pub scope_id: String,
    pub request: WebrootRequest,
    pub context: WebrootContext,
    pub session_id: String,
    pub session: HashMap<String, String>,
}
pub(crate) struct RenderOutput {
    pub response: WebrootResponse,
    pub session: Option<HashMap<String, String>>,
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
impl WebrootRuntime {
    pub(crate) fn render(&self, job: RenderJob) -> Result<RenderOutput> {
        let sender = {
            let mut worker = self
                .inner
                .worker
                .lock()
                .map_err(|_| anyhow::anyhow!("Web runtime lock poisoned"))?;
            if worker.is_none() {
                let (sender, requests) = std::sync::mpsc::sync_channel::<RenderMessage>(32);
                let thread = std::thread::Builder::new()
                    .name("matchbox-web-application".into())
                    .spawn(move || {
                        let mut runtime = ApplicationRuntime::new();
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
                *worker = Some(Worker {
                    sender: Some(sender),
                    thread: Some(thread),
                });
            }
            worker.as_ref().unwrap().sender.as_ref().unwrap().clone()
        };
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
