use dzip_gui::worker_protocol::{ArchiveTask, execute_archive_task};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn dzip_worker_run(request: JsValue) -> Result<JsValue, JsValue> {
    let request = serde_wasm_bindgen::from_value::<ArchiveTask>(request)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let response = execute_archive_task(request).map_err(|error| JsValue::from_str(&error))?;
    serde_wasm_bindgen::to_value(&response).map_err(|error| JsValue::from_str(&error.to_string()))
}
