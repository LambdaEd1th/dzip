//! Reusable presentational components for the archive workspace.

use crate::app::{CompressionPicker, VolumePicker};
use crate::browser::{
    BrowserCrumb, FolderView, draft_file_name, remove_draft_folder, replace_draft_file_name,
};
use crate::i18n::{I18n, Locale};
use crate::icons::{Icon, IconName, icon_for_file};
use crate::state::OpenSelectMenu;
use dioxus::prelude::*;
use dzip_gui::model::{DraftFile, EntryView, human_size, ratio_percent};
use std::collections::HashSet;

pub(crate) fn volume_option_label(value: u16, i18n: I18n) -> String {
    if value == 0 {
        i18n.t("main-volume")
    } else {
        i18n.t_args("volume-value", &[("number", value.to_string())])
    }
}

#[component]
pub(crate) fn NavButton(
    active: bool,
    icon: IconName,
    label: String,
    hint: String,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button { class: if active { "nav-button active" } else { "nav-button" }, onclick,
            span { class: "nav-icon", Icon { name: icon, size: 20 } }
            span { class: "nav-copy", strong { "{label}" } small { "{hint}" } }
        }
    }
}

#[component]
pub(crate) fn FileWorkspaceLayout(onclick: EventHandler<MouseEvent>, children: Element) -> Element {
    rsx! {
        div { class: "file-workspace-layout", onclick, {children} }
    }
}

#[component]
pub(crate) fn FileBrowserPanel(
    icon: IconName,
    title: String,
    summary: String,
    menu_open: bool,
    header_actions: Element,
    children: Element,
) -> Element {
    rsx! {
        section {
            class: if menu_open { "file-browser-panel glass-card menu-open" } else { "file-browser-panel glass-card" },
            div { class: "panel-toolbar",
                div { class: "panel-title",
                    div { class: "soft-icon", Icon { name: icon, size: 18 } }
                    div {
                        strong { "{title}" }
                        span { "{summary}" }
                    }
                }
                {header_actions}
            }
            {children}
        }
    }
}

#[component]
pub(crate) fn ContextPanel(class_name: String, menu_open: bool, children: Element) -> Element {
    rsx! {
        aside {
            class: if menu_open { "context-panel {class_name} glass-card menu-open" } else { "context-panel {class_name} glass-card" },
            {children}
        }
    }
}

#[component]
pub(crate) fn BrowserBar(
    current_dir: String,
    breadcrumbs: Vec<BrowserCrumb>,
    root_label: String,
    root_override: Option<String>,
    path_label: String,
    back_aria_label: String,
    back_title: String,
    back_disabled: bool,
    navigation_disabled: bool,
    on_back: EventHandler<()>,
    on_navigate: EventHandler<String>,
) -> Element {
    let at_root = current_dir.is_empty();
    let root_text = root_override.unwrap_or(root_label);
    rsx! {
        div { class: "file-browser-bar",
            button {
                class: "browser-back-button",
                r#type: "button",
                disabled: back_disabled,
                aria_label: back_aria_label,
                title: back_title,
                onclick: move |_| on_back.call(()),
                Icon { name: IconName::ArrowLeft, size: 16 }
            }
            nav { class: "archive-breadcrumbs", aria_label: path_label,
                button {
                    class: if at_root { "archive-crumb active" } else { "archive-crumb" },
                    r#type: "button",
                    disabled: navigation_disabled,
                    onclick: move |_| on_navigate.call(String::new()),
                    Icon { name: IconName::Home, size: 14 }
                    span { "{root_text}" }
                }
                if !navigation_disabled {
                    for (index, crumb) in breadcrumbs.iter().enumerate() {
                        span { class: "crumb-separator", Icon { name: IconName::ChevronRight, size: 13 } }
                        button {
                            class: if index + 1 == breadcrumbs.len() { "archive-crumb active" } else { "archive-crumb" },
                            r#type: "button",
                            onclick: {
                                let target = crumb.path.clone();
                                move |_| on_navigate.call(target.clone())
                            },
                            "{crumb.name}"
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(crate) fn FileTableHeader(
    leading: Element,
    name: String,
    original: String,
    packed: String,
    algorithm: String,
    volume: String,
) -> Element {
    rsx! {
        div { class: "file-row table-head file-browser-row",
            {leading}
            span { "{name}" }
            span { "{original}" }
            span { "{packed}" }
            span { "{algorithm}" }
            span { "{volume}" }
            div { class: "row-action-slot" }
        }
    }
}

#[component]
pub(crate) fn BrowserNameCell(icon: IconName, folder: bool, children: Element) -> Element {
    rsx! {
        div { class: "file-cell name-cell",
            div {
                class: if folder { "file-type-icon folder" } else { "file-type-icon" },
                Icon { name: icon, size: 19 }
            }
            {children}
        }
    }
}

#[component]
pub(crate) fn BrowserValuePill(value: String, tone: String) -> Element {
    rsx! {
        span { class: "browser-value-pill {tone}", "{value}" }
    }
}

#[component]
pub(crate) fn StatCard(
    icon: IconName,
    label: String,
    value: String,
    note: String,
    tone: &'static str,
) -> Element {
    rsx! {
        div { class: "stat-card glass-card {tone}",
            div { class: "stat-icon", Icon { name: icon, size: 20 } }
            div { class: "stat-copy",
                span { "{label}" }
                strong { "{value}" }
                small { "{note}" }
            }
        }
    }
}

#[component]
pub(crate) fn FolderRow(
    folder: FolderView,
    selected: Signal<HashSet<usize>>,
    mut browse_path: Signal<String>,
    mut focused_entry: Signal<Option<usize>>,
) -> Element {
    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let target = folder.path.clone();
    let open_label = i18n.t_args("open-folder", &[("name", folder.name.clone())]);
    let file_count = i18n.t_args(
        "folder-file-count",
        &[("count", folder.file_count.to_string())],
    );
    let original_label = i18n.t("original");
    let packed_label = i18n.t("packed");
    let algorithm_label = i18n.t("algorithm");
    let folder_label = i18n.t("folder");
    let packed_value = folder
        .packed_size
        .map(human_size)
        .unwrap_or_else(|| i18n.t("pending-generation"));
    let select_label = i18n.t_args("select-folder", &[("name", folder.name.clone())]);
    let entry_ids = folder.entry_ids.clone();
    let selected_in_folder = entry_ids
        .iter()
        .filter(|id| selected.read().contains(id))
        .count();
    let all_selected = !entry_ids.is_empty() && selected_in_folder == entry_ids.len();
    let partially_selected = selected_in_folder > 0 && !all_selected;
    rsx! {
        div {
            class: "file-row file-browser-row folder-row",
            button {
                class: "folder-open-target",
                r#type: "button",
                aria_label: open_label,
                onclick: move |_| {
                    browse_path.set(target.clone());
                    focused_entry.set(None);
                },
            }
            label {
                class: if partially_selected { "check-wrap folder-check partial" } else { "check-wrap folder-check" },
                onclick: move |event| event.stop_propagation(),
                input {
                    r#type: "checkbox",
                    checked: all_selected,
                    aria_label: select_label,
                    aria_checked: if partially_selected { "mixed" } else if all_selected { "true" } else { "false" },
                    onchange: move |_| {
                        let mut selected_set = selected.write();
                        if all_selected {
                            for id in &entry_ids { selected_set.remove(id); }
                        } else {
                            selected_set.extend(entry_ids.iter().copied());
                        }
                    }
                }
            }
            BrowserNameCell { icon: IconName::Folder, folder: true,
                div {
                    strong { "{folder.name}" }
                    span { "{file_count}" }
                }
            }
            span { class: "file-cell size-cell original-cell", "data-label": original_label, "{human_size(folder.size)}" }
            span { class: "file-cell size-cell packed-cell", "data-label": packed_label, "{packed_value}" }
            span { class: "file-cell method-cell", "data-label": algorithm_label,
                BrowserValuePill { value: folder_label, tone: "folder" }
            }
            div { class: "file-cell volume-cell" }
            div { class: "row-action-slot" }
        }
    }
}

#[component]
pub(crate) fn DraftFolderRow(
    folder: FolderView,
    mut browse_path: Signal<String>,
    mut draft_files: Signal<Vec<DraftFile>>,
) -> Element {
    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let target = folder.path.clone();
    let open_label = i18n.t_args("open-folder", &[("name", folder.name.clone())]);
    let file_count = i18n.t_args(
        "folder-file-count",
        &[("count", folder.file_count.to_string())],
    );
    let original_label = i18n.t("original");
    let packed_label = i18n.t("packed");
    let algorithm_label = i18n.t("algorithm");
    let folder_label = i18n.t("folder");
    let pending_value = i18n.t("pending-generation");
    let remove_label = i18n.t_args("remove-folder-label", &[("path", folder.path.clone())]);
    let folder_path = folder.path.clone();
    rsx! {
        div {
            class: "file-row file-browser-row folder-row editable-folder-row",
            button {
                class: "folder-open-target",
                r#type: "button",
                aria_label: open_label,
                onclick: move |_| browse_path.set(target.clone()),
            }
            div { class: "check-wrap" }
            BrowserNameCell { icon: IconName::Folder, folder: true,
                div {
                    strong { "{folder.name}" }
                    span { "{file_count}" }
                }
            }
            span { class: "file-cell size-cell original-cell", "data-label": original_label, "{human_size(folder.size)}" }
            span { class: "file-cell size-cell packed-cell pending-value", "data-label": packed_label, "{pending_value}" }
            span { class: "file-cell method-cell", "data-label": algorithm_label,
                BrowserValuePill { value: folder_label, tone: "folder" }
            }
            div { class: "file-cell volume-cell" }
            button {
                class: "row-action danger row-action-slot draft-row-action",
                r#type: "button",
                aria_label: remove_label,
                onclick: move |_| remove_draft_folder(&mut draft_files.write(), &folder_path),
                Icon { name: IconName::X, size: 16 }
            }
        }
    }
}

#[component]
pub(crate) fn DraftFileRow(
    file: DraftFile,
    mut draft_files: Signal<Vec<DraftFile>>,
    open_menu: Signal<Option<OpenSelectMenu>>,
) -> Element {
    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let file_name = draft_file_name(&file.path).to_string();
    let full_path = file.path.clone();
    let location = file
        .path
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_else(|| i18n.t("root"));
    let original_label = i18n.t("original");
    let packed_label = i18n.t("packed");
    let algorithm_label = i18n.t("algorithm");
    let volume_label = i18n.t("volume");
    let pending_value = i18n.t("pending-generation");
    let edit_label = i18n.t_args("rename-archive-entry-label", &[("path", file.path.clone())]);
    let edit_title = i18n.t("rename-archive-entry");
    let remove_label = i18n.t_args("remove-file-label", &[("path", file.path.clone())]);

    rsx! {
        div {
            id: "draft-row-{file.id}",
            class: if matches!(open_menu(), Some(OpenSelectMenu::Compression(id) | OpenSelectMenu::Volume(id)) if id == file.id) { "file-row file-browser-row editable-file-row dropdown-open" } else { "file-row file-browser-row editable-file-row" },
            div { class: "check-wrap" }
            BrowserNameCell { icon: icon_for_file(&file.path), folder: false,
                div { class: "draft-name",
                    input {
                        class: "draft-path-input",
                        value: "{file_name}",
                        aria_label: edit_label,
                        title: edit_title,
                        oninput: {
                            let id = file.id;
                            let original_path = file.path.clone();
                            move |event| {
                                if let Some(item) = draft_files.write().iter_mut().find(|item| item.id == id) {
                                    item.path = replace_draft_file_name(&original_path, &event.value());
                                }
                            }
                        },
                    }
                    span { title: "{full_path}", "{location}" }
                }
            }
            span { class: "file-cell size-cell original-cell", "data-label": original_label, "{human_size(file.bytes.len() as u64)}" }
            span { class: "file-cell size-cell packed-cell pending-value", "data-label": packed_label, "{pending_value}" }
            div { class: "file-cell method-cell", "data-label": algorithm_label,
                CompressionPicker {
                    file_id: file.id,
                    file_path: file.path.clone(),
                    value: file.compression,
                    draft_files,
                    open_menu,
                }
            }
            div { class: "file-cell volume-cell", "data-label": volume_label,
                VolumePicker {
                    file_id: file.id,
                    file_path: file.path.clone(),
                    value: file.volume,
                    draft_files,
                    open_menu,
                }
            }
            button {
                class: "row-action danger row-action-slot draft-row-action",
                aria_label: remove_label,
                onclick: {
                    let id = file.id;
                    move |_| draft_files.write().retain(|item| item.id != id)
                },
                Icon { name: IconName::X, size: 16 }
            }
        }
    }
}

#[component]
pub(crate) fn FileRow(
    entry: EntryView,
    selected: Signal<HashSet<usize>>,
    focused_entry: Signal<Option<usize>>,
    active: bool,
) -> Element {
    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let is_selected = selected.read().contains(&entry.id);
    let icon = icon_for_file(&entry.name);
    let select_label = i18n.t_args("select-file", &[("name", entry.path.clone())]);
    let original_label = i18n.t("original");
    let packed_label = i18n.t("packed");
    let algorithm_label = i18n.t("algorithm");
    let volume_label = i18n.t("volume");
    let volume_value = volume_option_label(entry.volume, i18n);
    let packed_value = entry
        .packed_size
        .map(human_size)
        .unwrap_or_else(|| i18n.t("pending-generation"));
    let folder_name = if entry.folder.is_empty() || entry.folder == "根目录" {
        i18n.t("root")
    } else {
        entry.folder.clone()
    };
    rsx! {
        div {
            class: if active { "file-row file-browser-row active" } else { "file-row file-browser-row" },
            onclick: {
                let id = entry.id;
                move |_| {
                    let mut focused_entry = focused_entry;
                    focused_entry.set(Some(id));
                }
            },
            label { class: "check-wrap", onclick: move |event| event.stop_propagation(),
                input {
                    r#type: "checkbox",
                    checked: is_selected,
                    aria_label: select_label,
                    onchange: {
                        let id = entry.id;
                        move |_| {
                            let mut set = selected.write();
                            if !set.insert(id) { set.remove(&id); }
                        }
                    }
                }
            }
            BrowserNameCell { icon, folder: false,
                div {
                    strong { "{entry.name}" }
                    span { "{folder_name}" }
                }
            }
            span { class: "file-cell size-cell original-cell", "data-label": original_label, "{human_size(entry.size)}" }
            span { class: "file-cell size-cell packed-cell", "data-label": packed_label, "{packed_value}" }
            span { class: "file-cell method-cell", "data-label": algorithm_label,
                BrowserValuePill { value: entry.compression, tone: "algorithm" }
            }
            span { class: "file-cell volume-cell", "data-label": volume_label,
                BrowserValuePill { value: volume_value, tone: "volume" }
            }
            div { class: "row-action-slot" }
        }
    }
}

#[component]
pub(crate) fn EntryInspector(entry: EntryView) -> Element {
    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let ratio = entry
        .packed_size
        .map(|packed| ratio_percent(packed, entry.size));
    let ratio_label = i18n.t("compression-ratio");
    let saved_label = ratio.map_or_else(
        || i18n.t("pending-generation"),
        |ratio| {
            i18n.t_args(
                "saved-space",
                &[("percent", 100u64.saturating_sub(ratio).to_string())],
            )
        },
    );
    let ratio_value =
        ratio.map_or_else(|| i18n.t("pending-generation"), |ratio| format!("{ratio}%"));
    let ratio_width = ratio.unwrap_or(0).min(100);
    let packed_value = entry
        .packed_size
        .map(human_size)
        .unwrap_or_else(|| i18n.t("pending-generation"));
    let original_size = i18n.t("original-size");
    let packed_size = i18n.t("packed-size");
    let compression_algorithm = i18n.t("compression-algorithm");
    let chunks_label = i18n.t("chunks");
    let volume_label = i18n.t("volume");
    let archive_path = i18n.t("archive-path");
    let file_details = i18n.t("file-details");
    let volume_value = volume_option_label(entry.volume, i18n);
    let folder_name = if entry.folder.is_empty() || entry.folder == "根目录" {
        i18n.t("root")
    } else {
        entry.folder.clone()
    };
    rsx! {
        div { class: "inspector-header",
            span { class: "eyebrow", "{file_details}" }
            div { class: "inspector-file-icon", Icon { name: icon_for_file(&entry.name), size: 28 } }
            h2 { "{entry.name}" }
            p { "{folder_name}" }
        }
        div { class: "ratio-block",
            div { class: "ratio-label", span { "{ratio_label}" } strong { "{ratio_value}" } }
            div { class: "ratio-track", span { style: "width: {ratio_width}%" } }
            small { "{saved_label}" }
        }
        dl { class: "detail-list",
            div { dt { "{original_size}" } dd { "{human_size(entry.size)}" } }
            div { dt { "{packed_size}" } dd { "{packed_value}" } }
            div { dt { "{compression_algorithm}" } dd { span { class: "method-chip", "{entry.compression}" } } }
            div { dt { "{chunks_label}" } dd { "{entry.chunks}" } }
            div { dt { "{volume_label}" } dd { "{volume_value}" } }
        }
        div { class: "path-box",
            span { "{archive_path}" }
            code { "{entry.path}" }
        }
    }
}

#[component]
pub(crate) fn NumberParameter(
    label: String,
    hint: String,
    value: i64,
    min: i64,
    max: i64,
    disabled: bool,
    onchange: EventHandler<i64>,
) -> Element {
    rsx! {
        label { class: if disabled { "number-parameter disabled" } else { "number-parameter" },
            span { class: "number-parameter-label", "{label}" }
            input {
                r#type: "number",
                inputmode: "numeric",
                value: "{value}",
                min: "{min}",
                max: "{max}",
                step: "1",
                disabled,
                oninput: move |event| {
                    if let Ok(next) = event.value().parse::<i64>() {
                        onchange.call(next.clamp(min, max));
                    }
                },
            }
            small { "{hint}" }
        }
    }
}

#[component]
pub(crate) fn ToggleRow(
    title: String,
    description: String,
    checked: bool,
    disabled: bool,
    onchange: EventHandler<bool>,
) -> Element {
    rsx! {
        label { class: if disabled { "toggle-row disabled" } else { "toggle-row" },
            div { strong { "{title}" } span { "{description}" } }
            input {
                class: "visually-hidden",
                r#type: "checkbox",
                checked,
                disabled,
                onchange: move |event| onchange.call(event.checked()),
            }
            span { class: if checked { "switch on" } else { "switch" }, span {} }
        }
    }
}
