use dioxus::html::HasFileData;
use dioxus::prelude::DragEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

pub fn drag_has_files(event: &DragEvent) -> bool {
    #[cfg(feature = "web")]
    {
        if let Some(web_event) = event.data().downcast::<web_sys::DragEvent>()
            && let Some(data_transfer) = web_event.data_transfer()
        {
            if data_transfer
                .files()
                .is_some_and(|files| files.length() > 0)
            {
                return true;
            }
            let items = data_transfer.items();
            for index in 0..items.length() {
                if items.get(index).is_some_and(|item| item.kind() == "file") {
                    return true;
                }
            }
        }
    }

    !event.files().is_empty()
}

pub async fn read_dropped_files(event: &DragEvent) -> Result<Vec<DroppedFile>, String> {
    #[cfg(feature = "desktop")]
    {
        let paths = event
            .files()
            .into_iter()
            .map(|file| (file.path(), file.name()))
            .collect::<Vec<_>>();
        crate::task::run_cpu_task(move || read_native_dropped_paths(paths)).await?
    }

    #[cfg(feature = "web")]
    {
        if let Some(files) = collect_web_dropped_directory_files(event).await? {
            return Ok(files);
        }

        let mut dropped = Vec::new();
        for file in event.files() {
            let path = file_data_display_path(&file);
            let bytes = file
                .read_bytes()
                .await
                .map_err(|error| format!("无法读取 {path}：{error}"))?;
            dropped.push(DroppedFile {
                path,
                bytes: bytes.to_vec(),
            });
        }
        dropped.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(dropped)
    }
}

#[cfg(feature = "desktop")]
fn read_native_dropped_paths(
    paths: Vec<(std::path::PathBuf, String)>,
) -> Result<Vec<DroppedFile>, String> {
    let mut dropped = Vec::new();
    for (path, name) in paths {
        if path.is_dir() {
            collect_native_directory(&path, &mut dropped)?;
        } else if path.is_file() {
            dropped.push(DroppedFile {
                path: name,
                bytes: std::fs::read(&path)
                    .map_err(|error| format!("无法读取 {}：{error}", path.display()))?,
            });
        } else {
            return Err(format!("无法读取不存在的拖放路径：{}", path.display()));
        }
    }
    dropped.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(dropped)
}

#[cfg(feature = "web")]
fn file_data_display_path(file: &dioxus::html::FileData) -> String {
    let path = normalise_drop_path(&file.path().to_string_lossy());
    if path.is_empty() { file.name() } else { path }
}

fn normalise_drop_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_string()
}

#[cfg(feature = "desktop")]
fn collect_native_directory(
    root: &std::path::Path,
    output: &mut Vec<DroppedFile>,
) -> Result<(), String> {
    let root_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| "folder".to_string());
    collect_native_directory_entries(root, root, &root_name, output)?;
    output.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(())
}

#[cfg(feature = "desktop")]
fn collect_native_directory_entries(
    root: &std::path::Path,
    directory: &std::path::Path,
    root_name: &str,
    output: &mut Vec<DroppedFile>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("无法读取文件夹 {}：{error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("无法读取 {} 的类型：{error}", path.display()))?;
        if file_type.is_dir() {
            collect_native_directory_entries(root, &path, root_name, output)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .ok()
                .map(|path| normalise_drop_path(&path.to_string_lossy()))
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| path.display().to_string())
                });
            output.push(DroppedFile {
                path: format!("{root_name}/{relative}"),
                bytes: std::fs::read(&path)
                    .map_err(|error| format!("无法读取 {}：{error}", path.display()))?,
            });
        }
    }
    Ok(())
}

#[cfg(feature = "web")]
async fn collect_web_dropped_directory_files(
    event: &DragEvent,
) -> Result<Option<Vec<DroppedFile>>, String> {
    let Some(entries) = web_dropped_entries(event)? else {
        return Ok(None);
    };

    let mut files = Vec::new();
    for entry in entries {
        collect_web_entry_files(entry, String::new(), &mut files).await?;
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Some(files))
}

#[cfg(feature = "web")]
fn web_dropped_entries(event: &DragEvent) -> Result<Option<Vec<wasm_bindgen::JsValue>>, String> {
    let data = event.data();
    let Some(web_event) = data.downcast::<web_sys::DragEvent>() else {
        return Ok(None);
    };
    let Some(data_transfer) = web_event.data_transfer() else {
        return Ok(None);
    };

    let items = data_transfer.items();
    if items.length() == 0 {
        return Ok(None);
    }

    let mut entries = Vec::new();
    let mut has_directory = false;
    for index in 0..items.length() {
        let Some(item) = items.get(index) else {
            continue;
        };
        if item.kind() != "file" {
            continue;
        }
        let Some(entry) = item.webkit_get_as_entry().map_err(js_value_to_string)? else {
            continue;
        };
        let entry = wasm_bindgen::JsValue::from(entry);
        has_directory |= web_entry_is_directory(&entry);
        entries.push(entry);
    }

    if entries.is_empty() || !has_directory {
        Ok(None)
    } else {
        Ok(Some(entries))
    }
}

#[cfg(feature = "web")]
async fn collect_web_entry_files(
    root: wasm_bindgen::JsValue,
    root_parent: String,
    output: &mut Vec<DroppedFile>,
) -> Result<(), String> {
    let mut stack = vec![(root, root_parent)];
    while let Some((entry, parent_path)) = stack.pop() {
        let name = normalise_drop_path(&web_entry_name(&entry));
        if name.is_empty() {
            continue;
        }
        let display_path = if parent_path.is_empty() {
            name
        } else {
            format!("{parent_path}/{name}")
        };

        if web_entry_is_file(&entry) {
            let file = read_web_file_entry(&entry).await?;
            output.push(DroppedFile {
                path: display_path,
                bytes: read_web_file_bytes(&file).await?,
            });
        } else if web_entry_is_directory(&entry) {
            let mut children = read_web_directory_entries(&entry).await?;
            children.reverse();
            for child in children {
                stack.push((child, display_path.clone()));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "web")]
fn web_entry_name(entry: &wasm_bindgen::JsValue) -> String {
    js_sys::Reflect::get(entry, &wasm_bindgen::JsValue::from_str("name"))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_default()
}

#[cfg(feature = "web")]
fn web_entry_is_file(entry: &wasm_bindgen::JsValue) -> bool {
    web_entry_bool(entry, "isFile")
}

#[cfg(feature = "web")]
fn web_entry_is_directory(entry: &wasm_bindgen::JsValue) -> bool {
    web_entry_bool(entry, "isDirectory")
}

#[cfg(feature = "web")]
fn web_entry_bool(entry: &wasm_bindgen::JsValue, property: &str) -> bool {
    js_sys::Reflect::get(entry, &wasm_bindgen::JsValue::from_str(property))
        .ok()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

#[cfg(feature = "web")]
fn web_object_method(
    object: &wasm_bindgen::JsValue,
    name: &str,
) -> Result<js_sys::Function, String> {
    use wasm_bindgen::JsCast;

    js_sys::Reflect::get(object, &wasm_bindgen::JsValue::from_str(name))
        .map_err(js_value_to_string)?
        .dyn_into::<js_sys::Function>()
        .map_err(|_| format!("浏览器不支持拖放条目方法 {name}"))
}

#[cfg(feature = "web")]
async fn read_web_file_entry(entry: &wasm_bindgen::JsValue) -> Result<web_sys::File, String> {
    use wasm_bindgen::JsCast;

    let file_method = web_object_method(entry, "file")?;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        if let Err(error) = file_method.call2(entry, &resolve, &reject) {
            let _ = reject.call1(&wasm_bindgen::JsValue::UNDEFINED, &error);
        }
    });
    let file_value = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(js_value_to_string)?;
    file_value
        .dyn_into::<web_sys::File>()
        .map_err(|_| "拖放条目没有返回文件".to_string())
}

#[cfg(feature = "web")]
async fn read_web_file_bytes(file: &web_sys::File) -> Result<Vec<u8>, String> {
    use wasm_bindgen::JsCast;

    let blob = wasm_bindgen::JsValue::from(file.clone())
        .dyn_into::<web_sys::Blob>()
        .map_err(|_| "拖放文件没有提供 Blob 数据".to_string())?;
    let buffer = wasm_bindgen_futures::JsFuture::from(blob.array_buffer())
        .await
        .map_err(js_value_to_string)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

#[cfg(feature = "web")]
async fn read_web_directory_entries(
    entry: &wasm_bindgen::JsValue,
) -> Result<Vec<wasm_bindgen::JsValue>, String> {
    let reader = web_object_method(entry, "createReader")?
        .call0(entry)
        .map_err(js_value_to_string)?;
    let mut entries = Vec::new();
    loop {
        let batch = read_web_directory_batch(&reader).await?;
        if batch.length() == 0 {
            break;
        }
        entries.extend(batch.iter());
    }
    Ok(entries)
}

#[cfg(feature = "web")]
async fn read_web_directory_batch(reader: &wasm_bindgen::JsValue) -> Result<js_sys::Array, String> {
    let read_entries = web_object_method(reader, "readEntries")?;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        if let Err(error) = read_entries.call2(reader, &resolve, &reject) {
            let _ = reject.call1(&wasm_bindgen::JsValue::UNDEFINED, &error);
        }
    });
    let value = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(js_value_to_string)?;
    Ok(js_sys::Array::from(&value))
}

#[cfg(feature = "web")]
fn js_value_to_string(value: wasm_bindgen::JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "desktop")]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalises_dropped_paths() {
        assert_eq!(normalise_drop_path(r".\Data\file.bin"), "Data/file.bin");
        assert_eq!(normalise_drop_path("/Data/file.bin/"), "Data/file.bin");
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn native_directory_drop_preserves_the_root_and_relative_paths() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let test_root =
            std::env::temp_dir().join(format!("dzip-file-drop-{}-{unique}", std::process::id()));
        let dropped_root = test_root.join("Assets");
        let nested = dropped_root.join("Data");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dropped_root.join("root.txt"), b"root").unwrap();
        std::fs::write(nested.join("nested.bin"), b"nested").unwrap();

        let mut files = Vec::new();
        collect_native_directory(&dropped_root, &mut files).unwrap();

        assert_eq!(
            files,
            vec![
                DroppedFile {
                    path: "Assets/Data/nested.bin".to_string(),
                    bytes: b"nested".to_vec(),
                },
                DroppedFile {
                    path: "Assets/root.txt".to_string(),
                    bytes: b"root".to_vec(),
                },
            ]
        );
        std::fs::remove_dir_all(test_root).unwrap();
    }
}
