#![cfg_attr(all(feature = "bundle", windows), windows_subsystem = "windows")]

mod archive_io;

use archive_io::{HintChoice, LoadedArchive, NamedBytes, PackInput, PackRequest};
use dioxus::prelude::*;
use dzip::{Compatibility, Compression, EntryId};
use rfd::AsyncFileDialog;
use std::path::Path;

const MAIN_CSS: Asset = asset!("/assets/main.css");

fn main() {
    dioxus::launch(App);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    Browse,
    Create,
}

#[component]
fn App() -> Element {
    let loaded = use_signal(|| None::<LoadedArchive>);
    let mut inputs = use_signal(Vec::<PackInput>::new);
    let mut active_view = use_signal(|| WorkspaceView::Browse);
    let mut selected_entry = use_signal(|| None::<EntryId>);
    let mut selected_input = use_signal(|| None::<usize>);
    let mut archive_query = use_signal(String::new);

    let mut archive_name = use_signal(|| "archive.dz".to_string());
    let mut volume_count = use_signal(|| "1".to_string());
    let mut alignment = use_signal(|| "0".to_string());
    let mut compression = use_signal(|| "dz".to_string());
    let mut compatibility = use_signal(|| "dzip113".to_string());
    let mut hint = use_signal(|| "auto".to_string());
    let mut random_access = use_signal(|| false);

    let status = use_signal(|| "就绪。所有文件都只在本机处理。".to_string());
    let status_is_error = use_signal(|| false);
    let busy = use_signal(|| false);

    let archive_stats = loaded.read().as_ref().map(|archive| {
        let bytes = archive
            .entries()
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>();
        (
            archive.name().to_string(),
            archive.entries().len(),
            archive.volume_count(),
            bytes,
        )
    });
    let queued_bytes = inputs
        .read()
        .iter()
        .map(|input| input.bytes.len() as u64)
        .sum::<u64>();
    let query = archive_query().trim().to_lowercase();

    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        div { class: "app-window",
            header { class: "titlebar",
                div { class: "brand",
                    div { class: "brand-mark", Icon { name: "archive" } }
                    div {
                        strong { "Dzip" }
                        span { "Archive Manager" }
                    }
                }
                div { class: "titlebar-center",
                    if let Some((name, _, _, _)) = &archive_stats {
                        span { "{name}" }
                    } else {
                        span { "未打开归档" }
                    }
                }
                div { class: "local-badge",
                    span { class: "local-dot" }
                    "本地处理"
                }
            }

            nav { class: "command-bar", aria_label: "归档操作",
                div { class: "command-group",
                    button {
                        class: "command primary-command",
                        disabled: busy(),
                        onclick: move |_| {
                            active_view.set(WorkspaceView::Browse);
                            selected_entry.set(None);
                            spawn(open_archive_dialog(loaded, status, status_is_error, busy));
                        },
                        Icon { name: "open" }
                        span { "打开" }
                    }
                    button {
                        class: "command",
                        disabled: busy(),
                        onclick: move |_| {
                            active_view.set(WorkspaceView::Create);
                            spawn(add_input_files(inputs, status, status_is_error, busy));
                        },
                        Icon { name: "add" }
                        span { "添加文件" }
                    }
                    button {
                        class: "command",
                        disabled: busy() || selected_entry().is_none(),
                        onclick: move |_| {
                            if let Some(id) = selected_entry() {
                                spawn(save_archive_entry(
                                    loaded,
                                    id,
                                    status,
                                    status_is_error,
                                    busy,
                                ));
                            }
                        },
                        Icon { name: "extract" }
                        span { "解压所选" }
                    }
                    button {
                        class: "command",
                        disabled: busy() || loaded.read().is_none(),
                        onclick: move |_| {
                            spawn(verify_archive(loaded, status, status_is_error, busy));
                        },
                        Icon { name: "verify" }
                        span { "校验" }
                    }
                }
                div { class: "command-divider" }
                div { class: "command-group",
                    button {
                        class: "command",
                        disabled: busy(),
                        onclick: move |_| active_view.set(WorkspaceView::Create),
                        Icon { name: "new" }
                        span { "新建归档" }
                    }
                }
                div { class: "command-spacer" }
                div { class: "format-chip",
                    span { "格式" }
                    strong { "DZIP 1.1.3" }
                }
            }

            div { class: "app-body",
                aside { class: "sidebar",
                    div { class: "sidebar-label", "工作区" }
                    button {
                        class: if active_view() == WorkspaceView::Browse { "nav-item active" } else { "nav-item" },
                        onclick: move |_| active_view.set(WorkspaceView::Browse),
                        Icon { name: "archive" }
                        span { "归档浏览器" }
                        if let Some((_, files, _, _)) = archive_stats {
                            em { "{files}" }
                        }
                    }
                    button {
                        class: if active_view() == WorkspaceView::Create { "nav-item active" } else { "nav-item" },
                        onclick: move |_| active_view.set(WorkspaceView::Create),
                        Icon { name: "package" }
                        span { "创建归档" }
                        if !inputs.read().is_empty() {
                            em { "{inputs.read().len()}" }
                        }
                    }

                    div { class: "sidebar-section",
                        div { class: "sidebar-label", "当前位置" }
                        if active_view() == WorkspaceView::Browse {
                            if let Some((name, files, volumes, bytes)) = &archive_stats {
                                div { class: "tree-root",
                                    Icon { name: "archive" }
                                    div {
                                        strong { title: "{name}", "{name}" }
                                        span { "Dzip 归档" }
                                    }
                                }
                                div { class: "tree-item",
                                    Icon { name: "folder" }
                                    span { "文件" }
                                    em { "{files}" }
                                }
                                div { class: "tree-item",
                                    Icon { name: "volume" }
                                    span { "分卷" }
                                    em { "{volumes}" }
                                }
                                div { class: "sidebar-metric",
                                    span { "原始大小" }
                                    strong { "{format_bytes(*bytes)}" }
                                }
                            } else {
                                p { class: "sidebar-hint", "打开一个 .dz 文件后，这里会显示归档结构。" }
                            }
                        } else {
                            div { class: "tree-root",
                                Icon { name: "package" }
                                div {
                                    strong { "{archive_name}" }
                                    span { "待创建归档" }
                                }
                            }
                            div { class: "sidebar-metric",
                                span { "队列" }
                                strong { "{inputs.read().len()} 个文件" }
                            }
                            div { class: "sidebar-metric",
                                span { "输入大小" }
                                strong { "{format_bytes(queued_bytes)}" }
                            }
                        }
                    }

                    div { class: "sidebar-footer",
                        Icon { name: "shield" }
                        div {
                            strong { "隐私模式" }
                            span { "数据不会上传到服务器" }
                        }
                    }
                }

                main { class: "content",
                    if active_view() == WorkspaceView::Browse {
                        section { class: "workspace-page",
                            header { class: "content-header",
                                div {
                                    div { class: "breadcrumb", "归档 / 浏览" }
                                    h1 { "归档内容" }
                                    p { "查看、校验并解压 Dzip 文件" }
                                }
                                if archive_stats.is_some() {
                                    label { class: "search-box",
                                        Icon { name: "search" }
                                        input {
                                            value: "{archive_query}",
                                            placeholder: "搜索文件…",
                                            aria_label: "搜索归档文件",
                                            oninput: move |event| archive_query.set(event.value()),
                                        }
                                        if !archive_query().is_empty() {
                                            button {
                                                aria_label: "清除搜索",
                                                onclick: move |_| archive_query.set(String::new()),
                                                "×"
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(archive) = loaded.read().as_ref() {
                                div { class: "stats-row",
                                    div { class: "stat-card",
                                        span { class: "stat-icon blue", Icon { name: "file" } }
                                        div { strong { "{archive.entries().len()}" } span { "文件" } }
                                    }
                                    div { class: "stat-card",
                                        span { class: "stat-icon green", Icon { name: "database" } }
                                        div {
                                            strong { "{format_bytes(archive.entries().iter().map(|entry| entry.size).sum())}" }
                                            span { "原始大小" }
                                        }
                                    }
                                    div { class: "stat-card",
                                        span { class: "stat-icon amber", Icon { name: "volume" } }
                                        div { strong { "{archive.volume_count()}" } span { "分卷" } }
                                    }
                                }

                                div { class: "file-panel",
                                    div { class: "file-panel-head",
                                        div {
                                            strong { "{archive.name()}" }
                                            span { "选择一项后可从顶部工具栏解压" }
                                        }
                                        span { class: "result-count",
                                            if query.is_empty() {
                                                "{archive.entries().len()} 项"
                                            } else {
                                                "搜索结果"
                                            }
                                        }
                                    }
                                    div { class: "table-scroll",
                                        table { class: "file-table",
                                            thead {
                                                tr {
                                                    th { class: "name-column", "名称" }
                                                    th { "压缩方式" }
                                                    th { "原始大小" }
                                                    th { "分卷" }
                                                    th { class: "action-column", "操作" }
                                                }
                                            }
                                            tbody {
                                                for entry in archive.entries().iter().filter(|entry| {
                                                    query.is_empty() || entry.path.to_lowercase().contains(&query)
                                                }) {
                                                    tr {
                                                        key: "{entry.id.0}",
                                                        class: if selected_entry() == Some(entry.id) { "selected" } else { "" },
                                                        onclick: {
                                                            let id = entry.id;
                                                            move |_| selected_entry.set(Some(id))
                                                        },
                                                        td { class: "file-name-cell",
                                                            span { class: "file-badge", "{file_extension(&entry.path)}" }
                                                            div {
                                                                strong { title: "{entry.path}", "{file_name(&entry.path)}" }
                                                                span { title: "{entry.path}", "{parent_path(&entry.path)}" }
                                                                span { class: "mobile-file-meta",
                                                                    "{entry.compression} · {format_bytes(entry.size)} · 卷 {entry.volume}"
                                                                }
                                                            }
                                                        }
                                                        td { span { class: "codec-pill", "{entry.compression}" } }
                                                        td { class: "numeric", "{format_bytes(entry.size)}" }
                                                        td { class: "numeric", "{entry.volume}" }
                                                        td { class: "row-action",
                                                            button {
                                                                disabled: busy(),
                                                                title: "保存此文件",
                                                                onclick: {
                                                                    let id = entry.id;
                                                                    move |event| {
                                                                        event.stop_propagation();
                                                                        selected_entry.set(Some(id));
                                                                        spawn(save_archive_entry(
                                                                            loaded,
                                                                            id,
                                                                            status,
                                                                            status_is_error,
                                                                            busy,
                                                                        ));
                                                                    }
                                                                },
                                                                Icon { name: "extract" }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                div { class: "welcome-card",
                                    div { class: "welcome-illustration",
                                        div { class: "archive-stack back" }
                                        div { class: "archive-stack middle" }
                                        div { class: "archive-stack front", Icon { name: "archive" } }
                                    }
                                    h2 { "打开一个 Dzip 归档" }
                                    p { "选择主卷 .dz 文件；分卷归档请一次选中全部卷。" }
                                    button {
                                        class: "primary-button large",
                                        disabled: busy(),
                                        onclick: move |_| {
                                            spawn(open_archive_dialog(loaded, status, status_is_error, busy));
                                        },
                                        Icon { name: "open" }
                                        "选择归档文件"
                                    }
                                    div { class: "feature-line",
                                        span { Icon { name: "shield" } "本地处理" }
                                        span { Icon { name: "volume" } "支持分卷" }
                                        span { Icon { name: "verify" } "完整校验" }
                                    }
                                }
                            }
                        }
                    } else {
                        section { class: "workspace-page create-page",
                            header { class: "content-header",
                                div {
                                    div { class: "breadcrumb", "归档 / 新建" }
                                    h1 { "创建 Dzip 归档" }
                                    p { "添加文件、选择压缩策略，然后生成归档" }
                                }
                                button {
                                    class: "secondary-button",
                                    disabled: busy(),
                                    onclick: move |_| {
                                        spawn(add_input_files(inputs, status, status_is_error, busy));
                                    },
                                    Icon { name: "add" }
                                    "添加文件"
                                }
                            }

                            div { class: "builder-layout",
                                section { class: "queue-panel",
                                    div { class: "section-title",
                                        div {
                                            h2 { "文件队列" }
                                            p { "{inputs.read().len()} 个文件 · {format_bytes(queued_bytes)}" }
                                        }
                                        if !inputs.read().is_empty() {
                                            button {
                                                class: "text-action danger",
                                                onclick: move |_| {
                                                    inputs.write().clear();
                                                    selected_input.set(None);
                                                },
                                                "清空"
                                            }
                                        }
                                    }

                                    if inputs.read().is_empty() {
                                        button {
                                            class: "drop-zone",
                                            disabled: busy(),
                                            onclick: move |_| {
                                                spawn(add_input_files(inputs, status, status_is_error, busy));
                                            },
                                            span { class: "drop-icon", Icon { name: "add-file" } }
                                            strong { "添加需要压缩的文件" }
                                            span { "点击选择文件，可一次选择多个" }
                                        }
                                    } else {
                                        div { class: "queue-table-wrap",
                                            table { class: "file-table queue-table",
                                                thead {
                                                    tr {
                                                        th { "归档内路径" }
                                                        th { "大小" }
                                                        th { "目标卷" }
                                                        th { class: "action-column", "" }
                                                    }
                                                }
                                                tbody {
                                                    for (index, input) in inputs.read().iter().enumerate() {
                                                        tr {
                                                            key: "{index}-{input.path}",
                                                            class: if selected_input() == Some(index) { "selected" } else { "" },
                                                            onclick: move |_| selected_input.set(Some(index)),
                                                            td { class: "editable-path",
                                                                span { class: "file-badge", "{file_extension(&input.path)}" }
                                                                input {
                                                                    value: "{input.path}",
                                                                    aria_label: "归档内路径",
                                                                    oninput: move |event| {
                                                                        if let Some(input) = inputs.write().get_mut(index) {
                                                                            input.path = event.value();
                                                                        }
                                                                    },
                                                                }
                                                            }
                                                            td { class: "numeric", "{format_bytes(input.bytes.len() as u64)}" }
                                                            td {
                                                                input {
                                                                    class: "volume-field",
                                                                    r#type: "number",
                                                                    min: "0",
                                                                    max: "65534",
                                                                    value: "{input.volume}",
                                                                    aria_label: "目标卷编号",
                                                                    oninput: move |event| {
                                                                        if let Ok(volume) = event.value().parse()
                                                                            && let Some(input) = inputs.write().get_mut(index)
                                                                        {
                                                                            input.volume = volume;
                                                                        }
                                                                    },
                                                                }
                                                            }
                                                            td { class: "row-action",
                                                                button {
                                                                    class: "delete-button",
                                                                    aria_label: "移除文件",
                                                                    title: "移除文件",
                                                                    onclick: move |event| {
                                                                        event.stop_propagation();
                                                                        if index < inputs.read().len() {
                                                                            inputs.write().remove(index);
                                                                            selected_input.set(None);
                                                                        }
                                                                    },
                                                                    Icon { name: "trash" }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                aside { class: "settings-panel",
                                    div { class: "section-title",
                                        div {
                                            h2 { "归档设置" }
                                            p { "输出和编码参数" }
                                        }
                                        Icon { name: "settings" }
                                    }

                                    div { class: "settings-form",
                                        label { class: "field full",
                                            span { "输出名称" }
                                            input {
                                                value: "{archive_name}",
                                                oninput: move |event| archive_name.set(event.value()),
                                            }
                                        }
                                        label { class: "field full",
                                            span { "压缩算法" }
                                            select {
                                                value: "{compression}",
                                                onchange: move |event| compression.set(event.value()),
                                                option { value: "dz", "DZ · 原生高压缩" }
                                                option { value: "zlib", "Zlib · 快速兼容" }
                                                option { value: "bzip", "Bzip · 文本压缩" }
                                                option { value: "lzma", "LZMA · 高压缩率" }
                                                option { value: "copy", "Copy · 不压缩" }
                                                option { value: "zero", "Zero · 全零数据" }
                                            }
                                        }
                                        div { class: "field-grid",
                                            label { class: "field",
                                                span { "分卷数量" }
                                                input {
                                                    r#type: "number",
                                                    min: "1",
                                                    max: "65535",
                                                    value: "{volume_count}",
                                                    oninput: move |event| volume_count.set(event.value()),
                                                }
                                            }
                                            label { class: "field",
                                                span { "字节对齐" }
                                                input {
                                                    r#type: "number",
                                                    min: "0",
                                                    value: "{alignment}",
                                                    oninput: move |event| alignment.set(event.value()),
                                                }
                                            }
                                        }
                                        label { class: "field full",
                                            span { "输出兼容策略" }
                                            select {
                                                value: "{compatibility}",
                                                onchange: move |event| compatibility.set(event.value()),
                                                option { value: "dzip113", "复刻 Dzip 1.1.3" }
                                                option { value: "strict", "规范格式（严格）" }
                                            }
                                        }
                                        if compatibility() == "strict" {
                                            p { class: "setting-note warning",
                                                Icon { name: "info" }
                                                "写入真实物理长度；旧版 dzip.exe 的 Bzip 解包存在限制。"
                                            }
                                        } else {
                                            p { class: "setting-note",
                                                Icon { name: "info" }
                                                "保留原版写入特征，适合追求兼容和逐字节一致。"
                                            }
                                        }
                                        label { class: "field full",
                                            span { "内容提示" }
                                            select {
                                                value: "{hint}",
                                                onchange: move |event| hint.set(event.value()),
                                                option { value: "auto", "自动识别 MP3 / JPEG" }
                                                option { value: "none", "无" }
                                                option { value: "mp3", "MP3" }
                                                option { value: "jpeg", "JPEG" }
                                            }
                                        }
                                        label { class: "switch-row",
                                            div {
                                                strong { "随机访问" }
                                                span { "写入 RANDOMACCESS 标志" }
                                            }
                                            input {
                                                r#type: "checkbox",
                                                checked: random_access(),
                                                onchange: move |event| random_access.set(event.checked()),
                                            }
                                        }
                                    }

                                    button {
                                        class: "primary-button build-button",
                                        disabled: busy() || inputs.read().is_empty(),
                                        onclick: move |_| {
                                            match pack_request(
                                                archive_name(),
                                                &volume_count(),
                                                &alignment(),
                                                &compression(),
                                                &compatibility(),
                                                random_access(),
                                                &hint(),
                                            ) {
                                                Ok(request) => {
                                                    spawn(build_and_save(
                                                        inputs,
                                                        request,
                                                        status,
                                                        status_is_error,
                                                        busy,
                                                    ));
                                                }
                                                Err(error) => set_status(
                                                    status,
                                                    status_is_error,
                                                    true,
                                                    error,
                                                ),
                                            }
                                        },
                                        Icon { name: "package" }
                                        if busy() { "正在处理…" } else { "生成并保存归档" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            footer { class: "app-statusbar",
                div { class: if status_is_error() { "status-message error" } else { "status-message" },
                    if busy() {
                        span { class: "activity-dot" }
                    } else if status_is_error() {
                        Icon { name: "warning" }
                    } else {
                        Icon { name: "check" }
                    }
                    span { "{status}" }
                }
                div { class: "status-meta",
                    span { "dzip-rs" }
                    span { class: "status-separator" }
                    span { "Web / Desktop" }
                    span { class: "status-separator" }
                    span { "AGPL-3.0-or-later" }
                }
            }
        }
    }
}

#[component]
fn Icon(name: &'static str) -> Element {
    let path = match name {
        "open" => "M3 8.5h6l2-2h10v12H3z M3 8.5V5h6l2 2",
        "add" => "M12 5v14M5 12h14",
        "add-file" => "M7 3h7l4 4v14H7z M14 3v5h5 M12 11v6M9 14h6",
        "extract" => "M12 3v11m0 0 4-4m-4 4-4-4 M5 17v4h14v-4",
        "verify" => "M5 4h14v17H5z M8 9l2 2 5-5 M8 16h8",
        "new" => "M5 3h10l4 4v14H5z M15 3v5h5 M8 14h8M12 10v8",
        "archive" => "M4 3h16v5H4z M5 8h14v13H5z M9 12h6",
        "package" => "M4 7l8-4 8 4-8 4z M4 7v10l8 4 8-4V7 M12 11v10",
        "folder" => "M3 6h7l2 2h9v11H3z",
        "volume" => "M5 4h14v5H5z M5 11h14v9H5z M9 15h6",
        "shield" => "M12 3l7 3v5c0 4.6-2.8 8-7 10-4.2-2-7-5.4-7-10V6z M9 12l2 2 4-5",
        "file" => "M6 3h8l4 4v14H6z M14 3v5h5",
        "database" => {
            "M5 6c0-1.7 3.1-3 7-3s7 1.3 7 3-3.1 3-7 3-7-1.3-7-3z M5 6v6c0 1.7 3.1 3 7 3s7-1.3 7-3V6 M5 12v6c0 1.7 3.1 3 7 3s7-1.3 7-3v-6"
        }
        "search" => "M11 18a7 7 0 1 1 0-14 7 7 0 0 1 0 14z M16 16l5 5",
        "settings" => "M4 6h8 M16 6h4 M12 3v6 M4 18h4 M12 18h8 M8 15v6 M4 12h2 M10 12h10 M6 9v6",
        "trash" => "M5 7h14 M9 7V4h6v3 M7 7l1 14h8l1-14 M10 11v6 M14 11v6",
        "info" => "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z M12 10v6 M12 7h.01",
        "warning" => "M12 3 2 21h20z M12 9v5 M12 17h.01",
        "check" => "M5 12l4 4L19 6",
        _ => "M4 4h16v16H4z",
    };
    rsx! {
        svg {
            class: "icon",
            view_box: "0 0 24 24",
            fill: "none",
            path {
                d: "{path}",
                stroke: "currentColor",
                stroke_width: "1.8",
                stroke_linecap: "round",
                stroke_linejoin: "round",
            }
        }
    }
}

async fn open_archive_dialog(
    mut loaded: Signal<Option<LoadedArchive>>,
    status: Signal<String>,
    status_is_error: Signal<bool>,
    mut busy: Signal<bool>,
) {
    let Some(handles) = AsyncFileDialog::new()
        .add_filter("Dzip archive", &["dz"])
        .set_title("打开 Dzip 归档（分卷请一次全部选中）")
        .pick_files()
        .await
    else {
        return;
    };

    busy.set(true);
    let mut files = Vec::with_capacity(handles.len());
    for handle in handles {
        files.push(NamedBytes {
            name: handle.file_name(),
            bytes: handle.read().await,
        });
    }
    match LoadedArchive::open(files) {
        Ok(archive) => {
            let message = format!(
                "已打开 {}：{} 个文件，{} 个卷",
                archive.name(),
                archive.entries().len(),
                archive.volume_count()
            );
            loaded.set(Some(archive));
            set_status(status, status_is_error, false, message);
        }
        Err(error) => set_status(status, status_is_error, true, error),
    }
    busy.set(false);
}

async fn add_input_files(
    mut inputs: Signal<Vec<PackInput>>,
    status: Signal<String>,
    status_is_error: Signal<bool>,
    mut busy: Signal<bool>,
) {
    let Some(handles) = AsyncFileDialog::new()
        .set_title("选择需要打包的文件")
        .pick_files()
        .await
    else {
        return;
    };

    busy.set(true);
    let mut additions = Vec::with_capacity(handles.len());
    let mut reserved_names = inputs
        .read()
        .iter()
        .map(|input| input.path.clone())
        .collect::<Vec<_>>();
    for handle in handles {
        let path = unique_input_name(&reserved_names, &handle.file_name());
        reserved_names.push(path.clone());
        additions.push(PackInput {
            path,
            bytes: handle.read().await,
            volume: 0,
        });
    }
    let added = additions.len();
    inputs.write().extend(additions);
    set_status(
        status,
        status_is_error,
        false,
        format!("已添加 {added} 个文件到队列"),
    );
    busy.set(false);
}

async fn save_archive_entry(
    loaded: Signal<Option<LoadedArchive>>,
    id: EntryId,
    status: Signal<String>,
    status_is_error: Signal<bool>,
    mut busy: Signal<bool>,
) {
    busy.set(true);
    let result = loaded
        .read()
        .as_ref()
        .ok_or_else(|| "归档已经关闭".to_string())
        .and_then(|archive| archive.extract_entry(id));
    match result {
        Ok(file) => match save_file(file).await {
            Ok(Some(name)) => {
                set_status(status, status_is_error, false, format!("已保存 {name}"));
            }
            Ok(None) => {}
            Err(error) => set_status(status, status_is_error, true, error),
        },
        Err(error) => set_status(status, status_is_error, true, error),
    }
    busy.set(false);
}

async fn verify_archive(
    loaded: Signal<Option<LoadedArchive>>,
    status: Signal<String>,
    status_is_error: Signal<bool>,
    mut busy: Signal<bool>,
) {
    busy.set(true);
    let result = loaded
        .read()
        .as_ref()
        .ok_or_else(|| "归档已经关闭".to_string())
        .and_then(LoadedArchive::verify);
    match result {
        Ok((count, bytes)) => set_status(
            status,
            status_is_error,
            false,
            format!("校验通过：{count} 个文件，共 {}", format_bytes(bytes)),
        ),
        Err(error) => set_status(status, status_is_error, true, error),
    }
    busy.set(false);
}

async fn build_and_save(
    inputs: Signal<Vec<PackInput>>,
    request: PackRequest,
    status: Signal<String>,
    status_is_error: Signal<bool>,
    mut busy: Signal<bool>,
) {
    busy.set(true);
    let result = archive_io::pack(&inputs.read(), &request);
    match result {
        Ok(packed) => {
            let report = packed.report;
            let mut saved = 0usize;
            for file in packed.files {
                match save_file(file).await {
                    Ok(Some(_)) => saved += 1,
                    Ok(None) => break,
                    Err(error) => {
                        set_status(status, status_is_error, true, error);
                        busy.set(false);
                        return;
                    }
                }
            }
            set_status(
                status,
                status_is_error,
                false,
                format!(
                    "已保存 {saved}/{} 个卷：{} 个文件，输入 {}，压缩后 {}",
                    report.volumes,
                    report.entries,
                    format_bytes(report.input_bytes),
                    format_bytes(report.stored_bytes)
                ),
            );
        }
        Err(error) => set_status(status, status_is_error, true, error),
    }
    busy.set(false);
}

async fn save_file(file: NamedBytes) -> Result<Option<String>, String> {
    let Some(handle) = AsyncFileDialog::new()
        .set_title("保存文件")
        .set_file_name(&file.name)
        .save_file()
        .await
    else {
        return Ok(None);
    };
    handle
        .write(&file.bytes)
        .await
        .map_err(|error| format!("保存 {} 失败：{error}", file.name))?;
    Ok(Some(file.name))
}

fn pack_request(
    archive_name: String,
    volume_count: &str,
    alignment: &str,
    compression: &str,
    compatibility: &str,
    random_access: bool,
    hint: &str,
) -> Result<PackRequest, String> {
    let volume_count = volume_count
        .parse::<u16>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| "分卷数量必须是 1 到 65535 的整数".to_string())?;
    let alignment = alignment
        .parse::<u32>()
        .map_err(|_| "字节对齐必须是 0 到 4294967295 的整数".to_string())?;
    Ok(PackRequest {
        archive_name,
        volume_count,
        alignment,
        compression: parse_compression(compression),
        compatibility: if compatibility == "strict" {
            Compatibility::Strict
        } else {
            Compatibility::Dzip113
        },
        random_access,
        hint: parse_hint(hint),
    })
}

fn set_status(
    mut status: Signal<String>,
    mut status_is_error: Signal<bool>,
    is_error: bool,
    message: impl Into<String>,
) {
    status.set(message.into());
    status_is_error.set(is_error);
}

fn parse_compression(value: &str) -> Compression {
    match value {
        "copy" => Compression::Copy,
        "zero" => Compression::Zero,
        "bzip" => Compression::Bzip,
        "zlib" => Compression::Zlib,
        "lzma" => Compression::Lzma,
        _ => Compression::Dz,
    }
}

fn parse_hint(value: &str) -> HintChoice {
    match value {
        "none" => HintChoice::None,
        "mp3" => HintChoice::Mp3,
        "jpeg" => HintChoice::Jpeg,
        _ => HintChoice::Auto,
    }
}

fn unique_input_name(existing: &[String], file_name: &str) -> String {
    if !existing
        .iter()
        .any(|path| path.eq_ignore_ascii_case(file_name))
    {
        return file_name.to_string();
    }

    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let extension = path.extension().and_then(|value| value.to_str());
    for suffix in 2.. {
        let candidate = match extension {
            Some(extension) => format!("{stem}-{suffix}.{extension}"),
            None => format!("{stem}-{suffix}"),
        };
        if !existing
            .iter()
            .any(|path| path.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
    }
    unreachable!()
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn parent_path(path: &str) -> String {
    let name = file_name(path);
    let parent = path
        .strip_suffix(name)
        .unwrap_or("")
        .trim_end_matches(['/', '\\']);
    if parent.is_empty() {
        "归档根目录".to_string()
    } else {
        parent.replace('\\', "/")
    }
}

fn file_extension(path: &str) -> String {
    Path::new(file_name(path))
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.chars().take(4).collect::<String>().to_uppercase())
        .unwrap_or_else(|| "FILE".to_string())
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
