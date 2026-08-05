use dzip_workflow::{ArchiveService, ArchiveTask, execute_archive_task};
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static ARCHIVES: RefCell<ArchiveService> = RefCell::new(ArchiveService::default());
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn dzip_worker_run(request: JsValue) -> Result<JsValue, JsValue> {
    let request = serde_wasm_bindgen::from_value::<ArchiveTask>(request)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let response = ARCHIVES
        .with(|archives| execute_archive_task(&mut archives.borrow_mut(), request))
        .map_err(|error| {
            serde_wasm_bindgen::to_value(&error)
                .unwrap_or_else(|_| JsValue::from_str(&error.to_string()))
        })?;
    serde_wasm_bindgen::to_value(&response).map_err(|error| JsValue::from_str(&error.to_string()))
}
