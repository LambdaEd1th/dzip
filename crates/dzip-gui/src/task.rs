use dzip_workflow::{ArchiveTask, ArchiveTaskResponse, WorkflowFailure};

#[cfg(feature = "desktop")]
use futures_timer::Delay;
#[cfg(feature = "desktop")]
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(feature = "desktop")]
use std::sync::OnceLock;
#[cfg(feature = "desktop")]
use std::sync::mpsc;
#[cfg(feature = "desktop")]
use std::time::Duration;

#[cfg(feature = "desktop")]
pub async fn run_cpu_task<T, F>(task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result =
            catch_unwind(AssertUnwindSafe(task)).map_err(|_| "后台任务意外终止".to_string());
        let _ = sender.send(result);
    });

    loop {
        match receiver.try_recv() {
            Ok(result) => return result,
            Err(mpsc::TryRecvError::Empty) => Delay::new(Duration::from_millis(8)).await,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("后台任务连接已断开".to_string());
            }
        }
    }
}

#[cfg(feature = "desktop")]
pub async fn run_archive_task(task: ArchiveTask) -> Result<ArchiveTaskResponse, WorkflowFailure> {
    let (sender, receiver) = mpsc::channel();
    archive_worker()
        .send(ArchiveWorkerMessage { task, sender })
        .map_err(|_| backend_failure("archive backend is unavailable"))?;
    loop {
        match receiver.try_recv() {
            Ok(result) => return result,
            Err(mpsc::TryRecvError::Empty) => Delay::new(Duration::from_millis(8)).await,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(backend_failure("archive backend disconnected"));
            }
        }
    }
}

#[cfg(feature = "desktop")]
struct ArchiveWorkerMessage {
    task: ArchiveTask,
    sender: mpsc::Sender<Result<ArchiveTaskResponse, WorkflowFailure>>,
}

#[cfg(feature = "desktop")]
fn archive_worker() -> &'static mpsc::Sender<ArchiveWorkerMessage> {
    static WORKER: OnceLock<mpsc::Sender<ArchiveWorkerMessage>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<ArchiveWorkerMessage>();
        std::thread::Builder::new()
            .name("dzip-archive-backend".to_string())
            .spawn(move || {
                let mut service = dzip_workflow::ArchiveService::default();
                for message in receiver {
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        dzip_workflow::execute_archive_task(&mut service, message.task)
                    }))
                    .unwrap_or_else(|_| Err(backend_failure("archive backend panicked")));
                    let _ = message.sender.send(result);
                }
            })
            .expect("failed to start archive backend thread");
        sender
    })
}

#[cfg(feature = "web")]
pub async fn run_archive_task(task: ArchiveTask) -> Result<ArchiveTaskResponse, WorkflowFailure> {
    yield_to_browser().await;
    crate::worker_client::run_archive_task(task).await
}

#[cfg(feature = "desktop")]
fn backend_failure(message: &str) -> WorkflowFailure {
    WorkflowFailure {
        code: dzip_workflow::WorkflowErrorCode::Io,
        message: message.to_string(),
    }
}

#[cfg(feature = "web")]
async fn yield_to_browser() {
    use js_sys::{Function, Promise};
    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    use wasm_bindgen_futures::JsFuture;

    let promise = Promise::new(&mut |resolve: Function, _reject: Function| {
        let timeout_resolve = resolve.clone();
        let callback = Closure::once(move || {
            let _ = timeout_resolve.call0(&JsValue::UNDEFINED);
        });
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                0,
            );
            callback.forget();
        } else {
            let _ = resolve.call0(&JsValue::UNDEFINED);
        }
    });
    let _ = JsFuture::from(promise).await;
}
