use dzip_workflow::{ArchiveTask, ArchiveTaskResponse, WorkflowErrorCode, WorkflowFailure};
use js_sys::{Array, ArrayBuffer, Function, Object, Promise, Reflect, Set};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{ErrorEvent, MessageEvent, Worker, WorkerOptions, WorkerType};

const WORKER_URL: &str = "assets/worker/dzip-worker.js";

struct PendingRequest {
    resolve: Function,
    reject: Function,
}

struct WorkerClient {
    worker: Worker,
    next_id: u64,
    pending: Rc<RefCell<HashMap<u64, PendingRequest>>>,
    failed: Rc<Cell<bool>>,
    _onmessage: Closure<dyn FnMut(MessageEvent)>,
    _onerror: Closure<dyn FnMut(ErrorEvent)>,
}

thread_local! {
    static WORKER_CLIENT: RefCell<Option<WorkerClient>> = const { RefCell::new(None) };
}

impl WorkerClient {
    fn new() -> Result<Self, String> {
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let worker = Worker::new_with_options(WORKER_URL, &options).map_err(js_error_string)?;
        let pending = Rc::new(RefCell::new(HashMap::<u64, PendingRequest>::new()));
        let failed = Rc::new(Cell::new(false));

        let message_pending = Rc::clone(&pending);
        let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            let message = event.data();
            record_worker_runtime(&message);
            let Some(id) = message_id(&message) else {
                return;
            };
            let Some(request) = message_pending.borrow_mut().remove(&id) else {
                return;
            };
            let ok = Reflect::get(&message, &JsValue::from_str("ok"))
                .ok()
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if ok {
                let response = Reflect::get(&message, &JsValue::from_str("response"))
                    .unwrap_or(JsValue::UNDEFINED);
                let _ = request.resolve.call1(&JsValue::UNDEFINED, &response);
            } else {
                let error = Reflect::get(&message, &JsValue::from_str("error"))
                    .ok()
                    .and_then(|value| value.as_string())
                    .unwrap_or_else(|| "Web Worker 任务失败".to_string());
                let _ = request
                    .reject
                    .call1(&JsValue::UNDEFINED, &JsValue::from_str(&error));
            }
        });
        worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));

        let error_pending = Rc::clone(&pending);
        let error_failed = Rc::clone(&failed);
        let onerror = Closure::<dyn FnMut(ErrorEvent)>::new(move |event: ErrorEvent| {
            error_failed.set(true);
            let message = if event.message().is_empty() {
                "Web Worker 无法启动".to_string()
            } else {
                event.message()
            };
            log::error!(target: "dzip_gui::worker", "{message}");
            for (_, request) in error_pending.borrow_mut().drain() {
                let _ = request
                    .reject
                    .call1(&JsValue::UNDEFINED, &JsValue::from_str(&message));
            }
        });
        worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        log::info!(target: "dzip_gui::worker", "Dzip stateful Web Worker initialized");

        Ok(Self {
            worker,
            next_id: 1,
            pending,
            failed,
            _onmessage: onmessage,
            _onerror: onerror,
        })
    }

    fn request(&mut self, request: JsValue) -> Result<Promise, String> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let message = Object::new();
        Reflect::set(
            &message,
            &JsValue::from_str("id"),
            &JsValue::from_f64(id as f64),
        )
        .map_err(js_error_string)?;
        Reflect::set(&message, &JsValue::from_str("request"), &request).map_err(js_error_string)?;

        let worker = self.worker.clone();
        let pending = Rc::clone(&self.pending);
        let failed = Rc::clone(&self.failed);
        let transfer = transfer_list(message.as_ref());
        Ok(Promise::new(
            &mut move |resolve: Function, reject: Function| {
                pending.borrow_mut().insert(
                    id,
                    PendingRequest {
                        resolve,
                        reject: reject.clone(),
                    },
                );
                if let Err(error) = worker.post_message_with_transfer(message.as_ref(), &transfer) {
                    failed.set(true);
                    pending.borrow_mut().remove(&id);
                    let _ = reject.call1(&JsValue::UNDEFINED, &error);
                }
            },
        ))
    }
}

pub async fn run_archive_task(task: ArchiveTask) -> Result<ArchiveTaskResponse, WorkflowFailure> {
    let request =
        serde_wasm_bindgen::to_value(&task).map_err(|error| protocol_failure(error.to_string()))?;
    let promise = WORKER_CLIENT
        .with(|slot| {
            let mut slot = slot.borrow_mut();
            let recreate = slot.as_ref().is_none_or(|client| client.failed.get());
            if recreate {
                if let Some(client) = slot.take() {
                    client.worker.terminate();
                }
                *slot = Some(WorkerClient::new()?);
            }
            slot.as_mut()
                .expect("worker client was initialized")
                .request(request)
        })
        .map_err(protocol_failure)?;
    let response = JsFuture::from(promise).await.map_err(js_error_failure)?;
    serde_wasm_bindgen::from_value(response).map_err(|error| protocol_failure(error.to_string()))
}

fn message_id(value: &JsValue) -> Option<u64> {
    Reflect::get(value, &JsValue::from_str("id"))
        .ok()?
        .as_f64()
        .map(|value| value as u64)
}

fn transfer_list(value: &JsValue) -> Array {
    let transfer = Array::new();
    let seen = Set::new(&JsValue::UNDEFINED);
    collect_transferables(value, &transfer, &seen);
    transfer
}

fn collect_transferables(value: &JsValue, transfer: &Array, seen: &Set) {
    if value.is_null() || value.is_undefined() || !value.is_object() || seen.has(value) {
        return;
    }
    seen.add(value);
    if ArrayBuffer::is_view(value) {
        if let Ok(buffer) = Reflect::get(value, &JsValue::from_str("buffer"))
            && buffer.is_instance_of::<ArrayBuffer>()
            && !seen.has(&buffer)
        {
            seen.add(&buffer);
            transfer.push(&buffer);
        }
        return;
    }
    if value.is_instance_of::<ArrayBuffer>() {
        transfer.push(value);
        return;
    }
    let object: Object = value.clone().unchecked_into();
    for key in Object::keys(&object) {
        if let Ok(child) = Reflect::get(value, &key) {
            collect_transferables(&child, transfer, seen);
        }
    }
}

fn record_worker_runtime(message: &JsValue) {
    let backend = Reflect::get(message, &JsValue::from_str("backend"))
        .ok()
        .and_then(|value| value.as_string());
    let thread_count = Reflect::get(message, &JsValue::from_str("threadCount"))
        .ok()
        .and_then(|value| value.as_f64());
    let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    else {
        return;
    };
    if let Some(backend) = backend {
        let _ = root.set_attribute("data-dzip-worker-backend", &backend);
    }
    if let Some(thread_count) = thread_count {
        let _ = root.set_attribute(
            "data-dzip-worker-threads",
            &(thread_count as usize).to_string(),
        );
    }
}

fn js_error_string(value: JsValue) -> String {
    value
        .as_string()
        .or_else(|| {
            Reflect::get(&value, &JsValue::from_str("message"))
                .ok()
                .and_then(|message| message.as_string())
        })
        .unwrap_or_else(|| format!("{value:?}"))
}

fn js_error_failure(value: JsValue) -> WorkflowFailure {
    serde_wasm_bindgen::from_value(value.clone())
        .unwrap_or_else(|_| protocol_failure(js_error_string(value)))
}

fn protocol_failure(message: impl Into<String>) -> WorkflowFailure {
    WorkflowFailure {
        code: WorkflowErrorCode::Io,
        message: message.into(),
    }
}
