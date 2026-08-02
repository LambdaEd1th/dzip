#[cfg(feature = "desktop")]
use std::path::{Path, PathBuf};

#[cfg(feature = "desktop")]
pub async fn save_bytes(file_name: &str, bytes: Vec<u8>) -> Result<String, String> {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .set_file_name(file_name)
        .save_file()
        .await
    else {
        return Err("已取消保存".to_string());
    };
    std::fs::write(handle.path(), bytes).map_err(|error| format!("保存失败：{error}"))?;
    Ok(handle.path().display().to_string())
}

#[cfg(feature = "web")]
pub async fn save_bytes(file_name: &str, bytes: Vec<u8>) -> Result<String, String> {
    use js_sys::{Array, Uint8Array};
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or_else(|| "浏览器窗口不可用".to_string())?;
    let document = window
        .document()
        .ok_or_else(|| "浏览器文档不可用".to_string())?;
    let array = Uint8Array::from(bytes.as_slice());
    let parts = Array::new();
    parts.push(&array);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)
        .map_err(|error| format!("创建下载内容失败：{error:?}"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|error| format!("创建下载链接失败：{error:?}"))?;
    let anchor = document
        .create_element("a")
        .map_err(|error| format!("创建下载按钮失败：{error:?}"))?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "浏览器不支持下载链接".to_string())?;
    anchor.set_href(&url);
    anchor.set_download(file_name);
    let _ = anchor.style().set_property("display", "none");
    let body = document.body().ok_or_else(|| "页面尚未就绪".to_string())?;
    body.append_child(&anchor)
        .map_err(|error| format!("准备下载失败：{error:?}"))?;
    anchor.click();
    anchor.remove();
    web_sys::Url::revoke_object_url(&url)
        .map_err(|error| format!("释放下载链接失败：{error:?}"))?;
    Ok(format!("已下载 {file_name}"))
}

#[cfg(feature = "desktop")]
pub async fn export_files(
    archive_name: &str,
    files: Vec<(String, Vec<u8>)>,
) -> Result<String, String> {
    let Some(folder) = rfd::AsyncFileDialog::new()
        .set_title("选择解压目录")
        .pick_folder()
        .await
    else {
        return Err("已取消解压".to_string());
    };
    let root = folder.path();
    for (relative, bytes) in &files {
        let target = safe_join(root, relative)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("无法创建目录 {}：{error}", parent.display()))?;
        }
        std::fs::write(&target, bytes)
            .map_err(|error| format!("无法写入 {}：{error}", target.display()))?;
    }
    Ok(format!(
        "已从 {archive_name} 解压 {} 个文件到 {}",
        files.len(),
        root.display()
    ))
}

#[cfg(feature = "web")]
pub async fn export_files(
    archive_name: &str,
    files: Vec<(String, Vec<u8>)>,
) -> Result<String, String> {
    let count = files.len();
    let zip = crate::archive_ops::make_store_zip(&files)?;
    let stem = archive_name
        .strip_suffix(".dz")
        .or_else(|| archive_name.strip_suffix(".DZ"))
        .unwrap_or(archive_name);
    let file_name = format!("{stem}-extracted.zip");
    save_bytes(&file_name, zip).await?;
    Ok(format!("已将 {count} 个文件打包为 {file_name}"))
}

#[cfg(feature = "desktop")]
fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut output = root.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            std::path::Component::Normal(part) => output.push(part),
            std::path::Component::CurDir => {}
            _ => return Err(format!("归档路径不安全：{relative}")),
        }
    }
    Ok(output)
}
