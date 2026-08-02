#![cfg_attr(feature = "bundle", windows_subsystem = "windows")]

#[cfg(all(feature = "desktop", feature = "web"))]
compile_error!("features `desktop` and `web` are mutually exclusive");
#[cfg(not(any(feature = "desktop", feature = "web")))]
compile_error!("enable either the `desktop` or `web` feature");

mod archive_ops;
mod model;
mod platform;
mod preferences;

use archive_ops::{build_archive, normalise_archive_name, open_archive, read_entries};
use dioxus::html::{FileData, HasFileData};
use dioxus::prelude::*;
use model::{
    CompressionChoice, DraftFile, DzCompressionOptions, EntryView, LoadedArchive, WorkspacePage,
    human_size, ratio_percent,
};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

const MAIN_CSS: Asset = asset!("/assets/main.css");

#[derive(Debug, Clone, PartialEq, Eq)]
struct FolderView {
    name: String,
    path: String,
    file_count: usize,
    size: u64,
    packed_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BrowserCrumb {
    name: String,
    path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppearanceMode {
    System,
    Light,
    Dark,
}

impl AppearanceMode {
    fn from_code(code: Option<&str>) -> Self {
        match code {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::System,
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocaleChoice {
    ZhCn,
    EnUs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenSelectMenu {
    Compression(u64),
    Alignment,
}

impl LocaleChoice {
    const ALL: [Self; 2] = [Self::ZhCn, Self::EnUs];

    fn from_code(code: Option<&str>) -> Self {
        match code {
            Some("en-US") => Self::EnUs,
            Some("zh-CN") => Self::ZhCn,
            _ => system_locale(),
        }
    }

    const fn code(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en-US",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::ZhCn => "简体中文",
            Self::EnUs => "English",
        }
    }

    const fn pick(self, zh_cn: &'static str, en_us: &'static str) -> &'static str {
        match self {
            Self::ZhCn => zh_cn,
            Self::EnUs => en_us,
        }
    }
}

fn system_locale() -> LocaleChoice {
    #[cfg(feature = "web")]
    let language = web_sys::window().and_then(|window| window.navigator().language());
    #[cfg(feature = "desktop")]
    let language = ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok());

    if language
        .as_deref()
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("en"))
    {
        LocaleChoice::EnUs
    } else {
        LocaleChoice::ZhCn
    }
}

fn append_log(mut logs: Signal<Vec<String>>, level: &str, message: impl Into<String>) {
    let mut entries = logs.write();
    entries.push(format!("[{level}] {}", message.into()));
    if entries.len() > 300 {
        entries.remove(0);
    }
}

fn compression_label(choice: CompressionChoice, locale: LocaleChoice) -> &'static str {
    match (choice, locale) {
        (CompressionChoice::Copy, LocaleChoice::EnUs) => "Store only",
        (CompressionChoice::Zero, LocaleChoice::EnUs) => "Zero fill",
        _ => choice.label(),
    }
}

fn compression_description(choice: CompressionChoice, locale: LocaleChoice) -> &'static str {
    if locale == LocaleChoice::ZhCn {
        return choice.description();
    }
    match choice {
        CompressionChoice::Dz => "Native compression",
        CompressionChoice::Zlib => "Best compatibility",
        CompressionChoice::Bzip => "Text and repeated data",
        CompressionChoice::Lzma => "High compression ratio",
        CompressionChoice::Copy => "No compression, fastest",
        CompressionChoice::Zero => "Only for all-zero files",
    }
}

fn alignment_option_label(value: u32, locale: LocaleChoice) -> &'static str {
    match value {
        512 => "512 bytes",
        2048 => "2 KB",
        4096 => "4 KB",
        _ => locale.pick("不对齐", "No alignment"),
    }
}

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut page = use_signal(|| WorkspacePage::Browse);
    let appearance = use_signal(|| {
        let stored = preferences::read_theme();
        AppearanceMode::from_code(stored.as_deref())
    });
    let locale = use_signal(|| {
        let stored = preferences::read_locale();
        LocaleChoice::from_code(stored.as_deref())
    });
    use_context_provider(|| locale);
    let mut settings_open = use_signal(|| false);
    let logs = use_signal(|| {
        vec![format!(
            "[INFO] Dzip Archive v{} started",
            env!("CARGO_PKG_VERSION")
        )]
    });
    let mut archive = use_signal(|| None::<LoadedArchive>);
    let mut selected = use_signal(HashSet::<usize>::new);
    let mut focused_entry = use_signal(|| None::<usize>);
    let mut search = use_signal(String::new);
    let mut browse_path = use_signal(String::new);
    let mut draft_files = use_signal(Vec::<DraftFile>::new);
    let mut compression = use_signal(|| CompressionChoice::Dz);
    let mut archive_name = use_signal(|| "game-assets.dz".to_string());
    let mut alignment = use_signal(|| 0u32);
    let mut random_access = use_signal(|| false);
    let mut dz_options = use_signal(DzCompressionOptions::default);
    let mut editing_source = use_signal(|| None::<String>);
    let next_id = use_signal(|| 1u64);
    let busy = use_signal(|| None::<String>);
    let mut toast = use_signal(|| None::<(bool, String)>);

    let appearance_class = match appearance() {
        AppearanceMode::System => "theme-system",
        AppearanceMode::Light => "theme-light",
        AppearanceMode::Dark => "theme-dark dark",
    };
    let theme_class = format!("app {appearance_class}");
    let current_locale = locale();
    let busy_hint = current_locale.pick("大文件可能需要一些时间", "Large files may take a moment");
    let dismiss_notification = current_locale.pick("关闭提示", "Dismiss notification");
    let primary_nav_label = current_locale.pick("主导航", "Primary navigation");
    let archive_nav = current_locale.pick("归档", "Archive");
    let archive_nav_hint = current_locale.pick("浏览与解压", "Browse and extract");
    let create_nav = current_locale.pick("新建", "Create");
    let create_nav_hint = current_locale.pick("压缩文件", "Compress files");
    let page_title = match page() {
        WorkspacePage::Browse => current_locale.pick("归档管理", "Archive manager"),
        WorkspacePage::Create if editing_source.read().is_some() => {
            current_locale.pick("编辑归档", "Edit archive")
        }
        WorkspacePage::Create => current_locale.pick("创建归档", "Create archive"),
    };
    let search_archive = current_locale.pick("搜索归档内容", "Search archive contents");
    let search_placeholder = current_locale.pick("搜索文件…", "Search files…");
    let clear_search = current_locale.pick("清除搜索", "Clear search");
    let open_settings = current_locale.pick("打开设置", "Open settings");
    let settings_label = current_locale.pick("设置", "Settings");
    let archive_label = archive
        .read()
        .as_ref()
        .map(|value| value.name.clone())
        .unwrap_or_else(|| {
            current_locale
                .pick("尚未打开归档", "No archive open")
                .to_string()
        });

    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        document::Title { "Dzip Archive" }

        div { class: "{theme_class}",
            div { class: "ambient ambient-one" }
            div { class: "ambient ambient-two" }
            div { class: "ambient ambient-three" }

            aside { class: "sidebar glass-panel",
                div { class: "brand",
                    div { class: "brand-mark", Icon { name: IconName::Archive, size: 22 } }
                    div { class: "brand-copy",
                        strong { "Dzip" }
                        span { "ARCHIVE" }
                    }
                }

                nav { class: "primary-nav", aria_label: primary_nav_label,
                    NavButton {
                        active: page() == WorkspacePage::Browse,
                        icon: IconName::FolderOpen,
                        label: archive_nav,
                        hint: archive_nav_hint,
                        onclick: move |_| page.set(WorkspacePage::Browse),
                    }
                    NavButton {
                        active: page() == WorkspacePage::Create,
                        icon: IconName::PackagePlus,
                        label: create_nav,
                        hint: create_nav_hint,
                        onclick: move |_| {
                            if page() != WorkspacePage::Create {
                                if editing_source.read().is_some() {
                                    draft_files.write().clear();
                                    compression.set(CompressionChoice::Dz);
                                    archive_name.set("game-assets.dz".to_string());
                                    alignment.set(0);
                                    random_access.set(false);
                                    dz_options.set(DzCompressionOptions::default());
                                    editing_source.set(None);
                                }
                                page.set(WorkspacePage::Create);
                            }
                        },
                    }
                }

            }

            main { class: "workbench",
                header { class: "topbar glass-panel",
                    div { class: "breadcrumb",
                        span { class: "eyebrow", "WORKSPACE" }
                        div { class: "breadcrumb-line",
                            strong { "{page_title}" }
                            span { class: "slash", "/" }
                            span { class: "current-file", "{archive_label}" }
                        }
                    }
                    div { class: "top-actions",
                        if page() == WorkspacePage::Browse && archive.read().is_some() {
                            label { class: "search-box",
                                Icon { name: IconName::Search, size: 17 }
                                input {
                                    aria_label: search_archive,
                                    placeholder: search_placeholder,
                                    value: "{search}",
                                    oninput: move |event| search.set(event.value()),
                                }
                                if !search.read().is_empty() {
                                    button {
                                        class: "clear-search",
                                        aria_label: clear_search,
                                        onclick: move |_| search.set(String::new()),
                                        Icon { name: IconName::X, size: 14 }
                                    }
                                }
                            }
                        }
                        button {
                            class: if settings_open() { "icon-button active" } else { "icon-button" },
                            r#type: "button",
                            aria_label: open_settings,
                            aria_haspopup: "dialog",
                            aria_expanded: if settings_open() { "true" } else { "false" },
                            aria_controls: "app-settings-dialog",
                            title: settings_label,
                            onclick: move |_| settings_open.set(!settings_open()),
                            Icon { name: IconName::Settings, size: 20 }
                        }
                    }
                }

                section { class: "page-content",
                    match page() {
                        WorkspacePage::Browse => rsx! {
                            BrowsePage {
                                archive: archive,
                                selected: selected,
                                focused_entry: focused_entry,
                                search: search,
                                browse_path: browse_path,
                                busy: busy,
                                toast: toast,
                                next_id: next_id,
                                draft_files: draft_files,
                                compression: compression,
                                archive_name: archive_name,
                                alignment: alignment,
                                random_access: random_access,
                                dz_options: dz_options,
                                editing_source: editing_source,
                                logs: logs,
                                page: page,
                                on_create: move |_| {
                                    draft_files.write().clear();
                                    compression.set(CompressionChoice::Dz);
                                    archive_name.set("game-assets.dz".to_string());
                                    alignment.set(0);
                                    random_access.set(false);
                                    dz_options.set(DzCompressionOptions::default());
                                    editing_source.set(None);
                                    page.set(WorkspacePage::Create);
                                },
                            }
                        },
                        WorkspacePage::Create => rsx! {
                            CreatePage {
                                draft_files: draft_files,
                                compression: compression,
                                archive_name: archive_name,
                                alignment: alignment,
                                random_access: random_access,
                                dz_options: dz_options,
                                busy: busy,
                                toast: toast,
                                next_id: next_id,
                                editing_source: editing_source,
                                logs: logs,
                                on_saved: move |value: LoadedArchive| {
                                    focused_entry.set(None);
                                    archive.set(Some(value));
                                    draft_files.write().clear();
                                    selected.write().clear();
                                    search.set(String::new());
                                    browse_path.set(String::new());
                                    editing_source.set(None);
                                    page.set(WorkspacePage::Browse);
                                },
                            }
                        },
                    }
                }
            }

            if settings_open() {
                SettingsModal {
                    open: settings_open,
                    appearance: appearance,
                    locale: locale,
                    logs: logs,
                }
            }

            if let Some(message) = busy.read().as_ref() {
                div { class: "busy-overlay",
                    div { class: "busy-card glass-panel",
                        div { class: "spinner" }
                        strong { "{message}" }
                        span { "{busy_hint}" }
                    }
                }
            }

            if let Some((success, message)) = toast.read().as_ref() {
                button {
                    class: if *success { "toast success" } else { "toast error" },
                    aria_label: dismiss_notification,
                    onclick: move |_| toast.set(None),
                    Icon {
                        name: if *success { IconName::CheckCircle } else { IconName::AlertCircle },
                        size: 19,
                    }
                    span { "{message}" }
                    Icon { name: IconName::X, size: 15 }
                }
            }
        }
    }
}

#[component]
fn SettingsModal(
    mut open: Signal<bool>,
    mut appearance: Signal<AppearanceMode>,
    mut locale: Signal<LocaleChoice>,
    logs: Signal<Vec<String>>,
) -> Element {
    let mut language_open = use_signal(|| false);
    let mut logs_open = use_signal(|| false);
    let language_open_now = language_open();
    let current_locale = locale();
    let version = env!("CARGO_PKG_VERSION");
    let license = env!("CARGO_PKG_LICENSE");
    let settings_title = current_locale.pick("设置", "Settings");
    let settings_subtitle = current_locale.pick("配置 Dzip Archive", "Configure Dzip Archive");
    let close_settings = current_locale.pick("关闭设置", "Close settings");
    let appearance_heading = current_locale.pick("外观模式", "Appearance mode");
    let system_label = current_locale.pick("跟随系统", "System");
    let light_label = current_locale.pick("浅色", "Light");
    let dark_label = current_locale.pick("深色", "Dark");
    let language_heading = current_locale.pick("界面语言", "Interface language");
    let logs_heading = current_locale.pick("日志", "Logs");
    let view_logs = current_locale.pick("查看应用日志", "View application logs");
    let about_heading = current_locale.pick("关于", "About");
    let version_label = current_locale.pick("版本号", "Version");
    let author_label = current_locale.pick("作者", "Author");

    rsx! {
        div {
            class: "settings-overlay",
            div {
                class: "settings-backdrop",
                aria_hidden: "true",
                style: "backdrop-filter: blur(8px); -webkit-backdrop-filter: blur(8px);",
                onclick: move |_| open.set(false),
            }
            section {
                id: "app-settings-dialog",
                class: "app-settings-modal glass-panel",
                role: "dialog",
                aria_modal: "true",
                aria_labelledby: "settings-dialog-title",
                style: "backdrop-filter: var(--settings-modal-backdrop); -webkit-backdrop-filter: var(--settings-modal-backdrop);",
                header { class: "app-settings-header",
                    div {
                        span { class: "settings-title-icon", Icon { name: IconName::Settings, size: 16 } }
                        div {
                            h2 { id: "settings-dialog-title", "{settings_title}" }
                            span { "{settings_subtitle}" }
                        }
                    }
                    button {
                        class: "settings-close-button",
                        r#type: "button",
                        aria_label: close_settings,
                        onclick: move |_| open.set(false),
                        Icon { name: IconName::X, size: 16 }
                    }
                }

                div { class: "app-settings-content",
                    section { class: "settings-section",
                        div { class: "settings-section-heading",
                            span { Icon { name: IconName::Monitor, size: 16 } }
                            span { "{appearance_heading}" }
                        }
                        div { class: "appearance-segments",
                            button {
                                class: if appearance() == AppearanceMode::System { "appearance-option active" } else { "appearance-option" },
                                r#type: "button",
                                aria_pressed: if appearance() == AppearanceMode::System { "true" } else { "false" },
                                onclick: move |_| {
                                    if let Err(error) = preferences::save_theme(AppearanceMode::System.code()) {
                                        append_log(logs, "ERROR", error);
                                    }
                                    appearance.set(AppearanceMode::System);
                                },
                                Icon { name: IconName::Monitor, size: 15 }
                                "{system_label}"
                            }
                            button {
                                class: if appearance() == AppearanceMode::Light { "appearance-option active" } else { "appearance-option" },
                                r#type: "button",
                                aria_pressed: if appearance() == AppearanceMode::Light { "true" } else { "false" },
                                onclick: move |_| {
                                    if let Err(error) = preferences::save_theme(AppearanceMode::Light.code()) {
                                        append_log(logs, "ERROR", error);
                                    }
                                    appearance.set(AppearanceMode::Light);
                                },
                                Icon { name: IconName::Sun, size: 15 }
                                "{light_label}"
                            }
                            button {
                                class: if appearance() == AppearanceMode::Dark { "appearance-option active" } else { "appearance-option" },
                                r#type: "button",
                                aria_pressed: if appearance() == AppearanceMode::Dark { "true" } else { "false" },
                                onclick: move |_| {
                                    if let Err(error) = preferences::save_theme(AppearanceMode::Dark.code()) {
                                        append_log(logs, "ERROR", error);
                                    }
                                    appearance.set(AppearanceMode::Dark);
                                },
                                Icon { name: IconName::Moon, size: 15 }
                                "{dark_label}"
                            }
                        }
                    }

                    section { class: "settings-section",
                        div { class: "settings-section-heading",
                            span { Icon { name: IconName::Languages, size: 16 } }
                            span { "{language_heading}" }
                        }
                        div { class: if language_open_now { "settings-language open" } else { "settings-language" },
                            button {
                                class: "settings-language-control",
                                r#type: "button",
                                aria_label: language_heading,
                                aria_haspopup: "listbox",
                                aria_expanded: if language_open_now { "true" } else { "false" },
                                onclick: move |_| language_open.set(!language_open_now),
                                span { "{current_locale.label()}" }
                                span { class: "settings-language-caret", Icon { name: IconName::ChevronDown, size: 16 } }
                            }
                            div {
                                class: "settings-language-menu",
                                role: "listbox",
                                aria_label: language_heading,
                                aria_hidden: if language_open_now { "false" } else { "true" },
                                for choice in LocaleChoice::ALL {
                                    {
                                        let active = choice == current_locale;
                                        rsx! {
                                            button {
                                                class: if active { "active" } else { "" },
                                                r#type: "button",
                                                role: "option",
                                                tabindex: if language_open_now { "0" } else { "-1" },
                                                aria_selected: if active { "true" } else { "false" },
                                                onclick: move |_| {
                                                    language_open.set(false);
                                                    if let Err(error) = preferences::save_locale(choice.code()) {
                                                        append_log(logs, "ERROR", error);
                                                    }
                                                    locale.set(choice);
                                                    append_log(logs, "INFO", format!("Interface language: {}", choice.label()));
                                                },
                                                span { "{choice.label()}" }
                                                if active {
                                                    Icon { name: IconName::Check, size: 15 }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    section { class: "settings-section settings-divided-section",
                        div { class: "settings-section-heading",
                            span { Icon { name: IconName::ScrollText, size: 16 } }
                            span { "{logs_heading}" }
                        }
                        button {
                            class: "settings-action-button",
                            r#type: "button",
                            onclick: move |_| logs_open.set(true),
                            Icon { name: IconName::ScrollText, size: 17 }
                            span { "{view_logs}" }
                        }
                    }

                    section { class: "settings-section settings-divided-section",
                        div { class: "settings-section-heading",
                            span { Icon { name: IconName::Info, size: 16 } }
                            span { "{about_heading}" }
                        }
                        dl { class: "settings-about-list",
                            div { class: "settings-about-item",
                                dt { "{version_label}" }
                                dd { "{version}" }
                            }
                            div { class: "settings-about-item",
                                dt { "License" }
                                dd { "{license}" }
                            }
                            div { class: "settings-about-item",
                                dt { "{author_label}" }
                                dd {
                                    a {
                                        href: "https://space.bilibili.com/8217621",
                                        target: "_blank",
                                        rel: "noopener noreferrer",
                                        "LambdaEd1th"
                                    }
                                }
                            }
                        }
                        a {
                            class: "settings-github-link",
                            href: "https://github.com/LambdaEd1th/dzip-rs",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            Icon { name: IconName::Github, size: 17 }
                            span { "GitHub" }
                        }
                    }
                }
            }
        }

        if logs_open() {
            LogViewerModal { open: logs_open, logs: logs, locale: locale }
        }
    }
}

#[component]
fn LogViewerModal(
    mut open: Signal<bool>,
    mut logs: Signal<Vec<String>>,
    locale: Signal<LocaleChoice>,
) -> Element {
    let current_locale = locale();
    let log_text = logs.read().join("\n");
    let logs_title = current_locale.pick("应用日志", "Application logs");
    let logs_subtitle =
        current_locale.pick("当前会话中的运行记录", "Runtime records from this session");
    let close_logs = current_locale.pick("关闭日志", "Close logs");
    let empty_logs = current_locale.pick("暂无日志", "No log entries");
    let log_content = current_locale.pick("应用日志内容", "Application log content");
    let clear_label = current_locale.pick("清空", "Clear");
    let export_label = current_locale.pick("导出日志", "Export log");

    rsx! {
        div { class: "log-viewer-overlay",
            div {
                class: "settings-backdrop",
                aria_hidden: "true",
                style: "backdrop-filter: blur(8px); -webkit-backdrop-filter: blur(8px);",
                onclick: move |_| open.set(false),
            }
            section {
                class: "log-viewer-modal glass-panel",
                role: "dialog",
                aria_modal: "true",
                aria_label: logs_title,
                style: "backdrop-filter: var(--settings-modal-backdrop); -webkit-backdrop-filter: var(--settings-modal-backdrop);",
                header { class: "app-settings-header",
                    div {
                        span { class: "settings-title-icon", Icon { name: IconName::ScrollText, size: 17 } }
                        div {
                            h2 { "{logs_title}" }
                            span { "{logs_subtitle}" }
                        }
                    }
                    button {
                        class: "settings-close-button",
                        r#type: "button",
                        aria_label: close_logs,
                        onclick: move |_| open.set(false),
                        Icon { name: IconName::X, size: 16 }
                    }
                }
                div { class: "log-viewer-content",
                    textarea {
                        class: "log-viewer-textarea",
                        readonly: true,
                        spellcheck: "false",
                        value: "{log_text}",
                        placeholder: empty_logs,
                        aria_label: log_content,
                    }
                }
                footer { class: "log-viewer-actions",
                    button {
                        class: "button secondary compact",
                        r#type: "button",
                        onclick: move |_| logs.write().clear(),
                        Icon { name: IconName::Trash, size: 16 }
                        "{clear_label}"
                    }
                    button {
                        class: "button primary compact",
                        r#type: "button",
                        onclick: move |_| {
                            let text = logs.read().join("\n");
                            spawn(async move {
                                match platform::save_bytes("dzip-archive.log", text.into_bytes()).await {
                                    Ok(path) => append_log(logs, "INFO", format!("Log exported: {path}")),
                                    Err(error) => append_log(logs, "ERROR", error),
                                }
                            });
                        },
                        Icon { name: IconName::Download, size: 16 }
                        "{export_label}"
                    }
                }
            }
        }
    }
}

fn build_browser_listing(
    entries: &[EntryView],
    current_dir: &str,
    query: &str,
) -> (Vec<FolderView>, Vec<EntryView>) {
    if !query.is_empty() {
        let mut files: Vec<EntryView> = entries
            .iter()
            .filter(|entry| entry.path.to_lowercase().contains(query))
            .cloned()
            .collect();
        files.sort_by_key(|entry| entry.path.to_lowercase());
        return (Vec::new(), files);
    }

    let current_dir = current_dir.trim_matches('/');
    let prefix = if current_dir.is_empty() {
        String::new()
    } else {
        format!("{current_dir}/")
    };
    let mut folder_map = BTreeMap::<String, FolderView>::new();
    let mut files = Vec::new();

    for entry in entries {
        let path = entry.path.trim_matches('/');
        let Some(relative) = path.strip_prefix(&prefix) else {
            continue;
        };
        if let Some((folder_name, _)) = relative.split_once('/') {
            if folder_name.is_empty() {
                continue;
            }
            let folder_path = if current_dir.is_empty() {
                folder_name.to_string()
            } else {
                format!("{current_dir}/{folder_name}")
            };
            let folder = folder_map
                .entry(folder_path.clone())
                .or_insert_with(|| FolderView {
                    name: folder_name.to_string(),
                    path: folder_path,
                    file_count: 0,
                    size: 0,
                    packed_size: 0,
                });
            folder.file_count += 1;
            folder.size = folder.size.saturating_add(entry.size);
            folder.packed_size = folder.packed_size.saturating_add(entry.packed_size);
        } else if !relative.is_empty() {
            files.push(entry.clone());
        }
    }

    let mut folders: Vec<FolderView> = folder_map.into_values().collect();
    folders.sort_by_key(|folder| folder.name.to_lowercase());
    files.sort_by_key(|entry| entry.name.to_lowercase());
    (folders, files)
}

#[component]
fn BrowsePage(
    archive: Signal<Option<LoadedArchive>>,
    selected: Signal<HashSet<usize>>,
    focused_entry: Signal<Option<usize>>,
    search: Signal<String>,
    browse_path: Signal<String>,
    busy: Signal<Option<String>>,
    toast: Signal<Option<(bool, String)>>,
    next_id: Signal<u64>,
    draft_files: Signal<Vec<DraftFile>>,
    compression: Signal<CompressionChoice>,
    archive_name: Signal<String>,
    alignment: Signal<u32>,
    random_access: Signal<bool>,
    dz_options: Signal<DzCompressionOptions>,
    editing_source: Signal<Option<String>>,
    logs: Signal<Vec<String>>,
    page: Signal<WorkspacePage>,
    on_create: EventHandler<MouseEvent>,
) -> Element {
    let locale = use_context::<Signal<LocaleChoice>>();
    let current_locale = locale();
    let empty_title = current_locale.pick("打开一个 Dzip 归档", "Open a Dzip archive");
    let empty_description = current_locale.pick(
        "查看压缩详情、筛选文件并安全解压。分卷归档请一次选择主文件和所有分卷。",
        "Inspect compression details, filter files, and extract safely. Select the main file and all volumes together for split archives.",
    );
    let select_archive = current_locale.pick("选择 .dz 文件", "Choose .dz files");
    let create_archive = current_locale.pick("创建新归档", "Create new archive");
    let Some(archive_value) = archive.read().as_ref().cloned() else {
        return rsx! {
            div { class: "empty-state glass-card",
                div { class: "empty-visual",
                    div { class: "archive-stack back" }
                    div { class: "archive-stack middle" }
                    div { class: "archive-stack front",
                        Icon { name: IconName::Archive, size: 40 }
                        span { "DZ" }
                    }
                    div { class: "sparkle sparkle-one" }
                    div { class: "sparkle sparkle-two" }
                    div { class: "sparkle sparkle-three" }
                }
                h1 { "{empty_title}" }
                p { "{empty_description}" }
                div { class: "empty-actions",
                    label { class: "button primary large file-action",
                        Icon { name: IconName::FolderOpen, size: 19 }
                        "{select_archive}"
                        input {
                            class: "visually-hidden",
                            r#type: "file",
                            accept: ".dz,.dzip,.001,.002,.003,.004,.005",
                            multiple: true,
                            onchange: move |event| {
                                let files = event.files();
                                async move {
                                    if files.is_empty() { return; }
                                    let mut archive = archive;
                                    let mut focused_entry = focused_entry;
                                    let mut selected = selected;
                                    let mut search = search;
                                    let mut browse_path = browse_path;
                                    let mut busy = busy;
                                    let mut toast = toast;
                                    busy.set(Some(current_locale.pick("正在读取归档…", "Reading archive…").to_string()));
                                    let mut loaded = Vec::new();
                                    for file in files {
                                        let name = file.name();
                                        match file.read_bytes().await {
                                            Ok(bytes) => loaded.push((name, bytes.to_vec())),
                                            Err(error) => {
                                                let message = if current_locale == LocaleChoice::ZhCn {
                                                    format!("读取 {name} 失败：{error}")
                                                } else {
                                                    format!("Failed to read {name}: {error}")
                                                };
                                                append_log(logs, "ERROR", &message);
                                                toast.set(Some((false, message.clone())));
                                                busy.set(None);
                                                return;
                                            }
                                        }
                                    }
                                    let main_index = loaded.iter().position(|(name, _)| {
                                        let lower = name.to_ascii_lowercase();
                                        lower.ends_with(".dz") || lower.ends_with(".dzip")
                                    }).unwrap_or(0);
                                    let (main_name, main_bytes) = loaded.remove(main_index);
                                    match open_archive(main_name.clone(), main_bytes, loaded) {
                                        Ok(value) => {
                                            focused_entry.set(None);
                                            archive.set(Some(value));
                                            selected.write().clear();
                                            search.set(String::new());
                                            browse_path.set(String::new());
                                            let message = if current_locale == LocaleChoice::ZhCn {
                                                format!("已打开 {main_name}")
                                            } else {
                                                format!("Opened {main_name}")
                                            };
                                            append_log(logs, "INFO", &message);
                                            toast.set(Some((true, message)));
                                        }
                                        Err(message) => {
                                            append_log(logs, "ERROR", &message);
                                            toast.set(Some((false, message.clone())));
                                        }
                                    }
                                    busy.set(None);
                                }
                            }
                        }
                    }
                    button { class: "button secondary large", onclick: on_create,
                        Icon { name: IconName::PackagePlus, size: 19 }
                        "{create_archive}"
                    }
                }
            }
        };
    };

    let query = search().trim().to_lowercase();
    let current_dir = browse_path();
    let (folders, visible) =
        build_browser_listing(archive_value.entries.as_ref(), &current_dir, &query);
    let mut breadcrumb_path = String::new();
    let breadcrumbs: Vec<BrowserCrumb> = current_dir
        .split('/')
        .filter(|part| !part.is_empty())
        .map(|part| {
            if !breadcrumb_path.is_empty() {
                breadcrumb_path.push('/');
            }
            breadcrumb_path.push_str(part);
            BrowserCrumb {
                name: part.to_string(),
                path: breadcrumb_path.clone(),
            }
        })
        .collect();
    let parent_dir = current_dir
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default();
    let selected_count = selected.read().len();
    let selected_bytes: u64 = archive_value
        .entries
        .iter()
        .filter(|entry| selected.read().contains(&entry.id))
        .map(|entry| entry.size)
        .sum();
    let all_visible_selected = !visible.is_empty()
        && visible
            .iter()
            .all(|entry| selected.read().contains(&entry.id));
    let focused = focused_entry()
        .and_then(|id| archive_value.entries.iter().find(|entry| entry.id == id))
        .cloned();
    let archive_for_selected = archive_value.clone();
    let archive_for_all = archive_value.clone();
    let archive_for_edit = archive_value.clone();
    let overview_description = current_locale.pick(
        "已载入归档索引，可搜索、选择并解压条目。",
        "The archive index is loaded. Search, select, and extract entries.",
    );
    let edit_archive = current_locale.pick("编辑归档", "Edit archive");
    let extract_selected = current_locale.pick("解压所选", "Extract selected");
    let extract_all = current_locale.pick("全部解压", "Extract all");
    let files_label = current_locale.pick("文件", "Files");
    let archive_size_label = current_locale.pick("归档大小", "Archive size");
    let unpacked_label = current_locale.pick("解压后", "Unpacked");
    let ratio_label = current_locale.pick("压缩占比", "Compression ratio");
    let original_total = current_locale.pick("原始数据总量", "Original data total");
    let ratio_note = current_locale.pick("越低压缩越充分", "Lower means better compression");
    let archive_contents = current_locale.pick("归档内容", "Archive contents");
    let clear_label = current_locale.pick("清除", "Clear");
    let parent_folder = current_locale.pick("返回上级文件夹", "Go to parent folder");
    let parent_label = current_locale.pick("返回上级", "Go back");
    let archive_path_label = current_locale.pick("归档路径", "Archive path");
    let root_label = current_locale.pick("根目录", "Root");
    let search_results = current_locale.pick("搜索结果", "Search results");
    let select_visible = current_locale.pick("选择全部可见条目", "Select all visible entries");
    let name_label = current_locale.pick("名称", "Name");
    let original_size = current_locale.pick("原始大小", "Original size");
    let packed_size = current_locale.pick("压缩后", "Packed size");
    let algorithm_label = current_locale.pick("算法", "Algorithm");
    let empty_folder = current_locale.pick("此文件夹为空", "This folder is empty");
    let no_matches = current_locale.pick("没有匹配的文件", "No matching files");
    let empty_folder_hint =
        current_locale.pick("返回上级目录继续浏览", "Go back to continue browsing");
    let no_matches_hint = current_locale.pick("换一个关键词试试", "Try another search term");
    let browse_hint = current_locale.pick(
        "打开文件夹或查看文件详情",
        "Open a folder or inspect a file",
    );
    let searching_hint = current_locale.pick("正在搜索整个归档", "Searching the entire archive");
    let choose_entry = current_locale.pick("选择一个条目", "Select an entry");
    let details_hint = current_locale.pick("这里会显示文件详情", "File details will appear here");
    let chunk_note = if current_locale == LocaleChoice::ZhCn {
        format!("{} 个数据块", archive_value.chunk_count)
    } else {
        format!("{} chunks", archive_value.chunk_count)
    };
    let volume_note = if current_locale == LocaleChoice::ZhCn {
        format!("{} 个分卷", archive_value.volume_count)
    } else {
        format!("{} volumes", archive_value.volume_count)
    };
    let contents_summary = if current_locale == LocaleChoice::ZhCn {
        if query.is_empty() {
            format!("{} 个文件夹 · {} 个文件", folders.len(), visible.len())
        } else {
            format!("全归档找到 {} 个文件", visible.len())
        }
    } else if query.is_empty() {
        format!("{} folders · {} files", folders.len(), visible.len())
    } else {
        format!("{} files found across the archive", visible.len())
    };
    let selected_summary = if current_locale == LocaleChoice::ZhCn {
        format!("已选 {selected_count} 项")
    } else {
        format!("{selected_count} selected")
    };
    let directory_summary = if current_locale == LocaleChoice::ZhCn {
        format!(
            "当前目录：{} 个文件夹 · {} 个文件",
            folders.len(),
            visible.len()
        )
    } else {
        format!(
            "Current folder: {} folders · {} files",
            folders.len(),
            visible.len()
        )
    };
    let search_summary = if current_locale == LocaleChoice::ZhCn {
        format!(
            "搜索到 {} / {} 个文件",
            visible.len(),
            archive_value.entries.len()
        )
    } else {
        format!(
            "Found {} of {} files",
            visible.len(),
            archive_value.entries.len()
        )
    };

    rsx! {
        div { class: "archive-heading",
            div {
                span { class: "eyebrow accent", "ARCHIVE OVERVIEW" }
                h1 { "{archive_value.name}" }
                p { "{overview_description}" }
            }
            div { class: "heading-actions",
                button {
                    class: "button secondary",
                    onclick: move |_| {
                        let archive_value = archive_for_edit.clone();
                        async move {
                            let preparing = if current_locale == LocaleChoice::ZhCn {
                                format!("正在准备 {} 的编辑工作区…", archive_value.name)
                            } else {
                                format!("Preparing {} for editing…", archive_value.name)
                            };
                            busy.set(Some(preparing));
                            let ids: Vec<usize> = archive_value.entries.iter().map(|entry| entry.id).collect();
                            match read_entries(&archive_value, &ids) {
                                Ok(files) => {
                                    let method = archive_value
                                        .entries
                                        .first()
                                        .map(|entry| CompressionChoice::from_archive_label(&entry.compression))
                                        .unwrap_or(CompressionChoice::Dz);
                                    let mut drafts = Vec::with_capacity(files.len());
                                    for (path, bytes) in files {
                                        let id = next_id();
                                        next_id.set(id.saturating_add(1));
                                        let entry_compression = archive_value
                                            .entries
                                            .iter()
                                            .find(|entry| entry.path.eq_ignore_ascii_case(&path))
                                            .map(|entry| CompressionChoice::from_archive_label(&entry.compression))
                                            .unwrap_or(method);
                                        drafts.push(DraftFile {
                                            id,
                                            path,
                                            bytes: Arc::from(bytes),
                                            compression: entry_compression,
                                        });
                                    }
                                    let source_name = archive_value.name.clone();
                                    draft_files.set(drafts);
                                    compression.set(method);
                                    archive_name.set(source_name.clone());
                                    alignment.set(0);
                                    random_access.set(false);
                                    dz_options.set(archive_value.dz_options);
                                    editing_source.set(Some(source_name.clone()));
                                    page.set(WorkspacePage::Create);
                                    let message = if current_locale == LocaleChoice::ZhCn {
                                        format!("已载入 {source_name} 的编辑工作区")
                                    } else {
                                        format!("Loaded {source_name} for editing")
                                    };
                                    append_log(logs, "INFO", &message);
                                    toast.set(Some((true, message)));
                                }
                                Err(message) => {
                                    append_log(logs, "ERROR", &message);
                                    toast.set(Some((false, message.clone())));
                                }
                            }
                            busy.set(None);
                        }
                    },
                    Icon { name: IconName::Pencil, size: 17 }
                    "{edit_archive}"
                }
                button {
                    class: "button secondary",
                    disabled: selected_count == 0,
                    onclick: move |_| {
                        let archive_value = archive_for_selected.clone();
                        let ids: Vec<usize> = selected.read().iter().copied().collect();
                        async move {
                            extract_and_export(archive_value, ids, busy, toast, logs, current_locale).await;
                        }
                    },
                    Icon { name: IconName::Download, size: 17 }
                    if selected_count == 0 { "{extract_selected}" } else { "{extract_selected} ({selected_count})" }
                }
                button {
                    class: "button primary",
                    onclick: move |_| {
                        let archive_value = archive_for_all.clone();
                        let ids = archive_value.entries.iter().map(|entry| entry.id).collect();
                        async move {
                            extract_and_export(archive_value, ids, busy, toast, logs, current_locale).await;
                        }
                    },
                    Icon { name: IconName::ArchiveRestore, size: 17 }
                    "{extract_all}"
                }
            }
        }

        div { class: "stats-grid",
            StatCard { icon: IconName::Files, label: files_label, value: archive_value.entries.len().to_string(), note: chunk_note, tone: "mint" }
            StatCard { icon: IconName::HardDrive, label: archive_size_label, value: human_size(archive_value.source_size), note: volume_note, tone: "blue" }
            StatCard { icon: IconName::Expand, label: unpacked_label, value: human_size(archive_value.unpacked_size), note: original_total.to_string(), tone: "pink" }
            StatCard { icon: IconName::Gauge, label: ratio_label, value: format!("{}%", ratio_percent(archive_value.source_size, archive_value.unpacked_size)), note: ratio_note.to_string(), tone: "amber" }
        }

        div { class: "archive-layout",
            section { class: "file-panel glass-card",
                div { class: "panel-toolbar",
                    div { class: "panel-title",
                        div { class: "soft-icon", Icon { name: IconName::ListTree, size: 18 } }
                        div {
                            strong { "{archive_contents}" }
                            span { "{contents_summary}" }
                        }
                    }
                    if selected_count > 0 {
                        div { class: "selection-summary",
                            span { "{selected_summary}" }
                            strong { "{human_size(selected_bytes)}" }
                            button { onclick: move |_| selected.write().clear(), "{clear_label}" }
                        }
                    }
                }

                div { class: "file-browser-bar",
                    button {
                        class: "browser-back-button",
                        r#type: "button",
                        disabled: current_dir.is_empty() || !query.is_empty(),
                        aria_label: parent_folder,
                        title: parent_label,
                        onclick: {
                            let parent_dir = parent_dir.clone();
                            move |_| {
                                browse_path.set(parent_dir.clone());
                                focused_entry.set(None);
                            }
                        },
                        Icon { name: IconName::ArrowLeft, size: 16 }
                    }
                    nav { class: "archive-breadcrumbs", aria_label: archive_path_label,
                        button {
                            class: if current_dir.is_empty() { "archive-crumb active" } else { "archive-crumb" },
                            r#type: "button",
                            disabled: !query.is_empty(),
                            onclick: move |_| {
                                browse_path.set(String::new());
                                focused_entry.set(None);
                            },
                            Icon { name: IconName::Home, size: 14 }
                            span { if query.is_empty() { "{root_label}" } else { "{search_results}" } }
                        }
                        if query.is_empty() {
                            for (index, crumb) in breadcrumbs.iter().enumerate() {
                                span { class: "crumb-separator", Icon { name: IconName::ChevronRight, size: 13 } }
                                button {
                                    class: if index + 1 == breadcrumbs.len() { "archive-crumb active" } else { "archive-crumb" },
                                    r#type: "button",
                                    onclick: {
                                        let target = crumb.path.clone();
                                        move |_| {
                                            browse_path.set(target.clone());
                                            focused_entry.set(None);
                                        }
                                    },
                                    "{crumb.name}"
                                }
                            }
                        }
                    }
                }

                div { class: "file-table",
                    div { class: "file-row table-head",
                        label { class: "check-wrap",
                            input {
                                r#type: "checkbox",
                                checked: all_visible_selected,
                                disabled: visible.is_empty(),
                                aria_label: select_visible,
                                onchange: move |_| {
                                    let ids: Vec<usize> = visible.iter().map(|entry| entry.id).collect();
                                    let mut selected_set = selected.write();
                                    if all_visible_selected {
                                        for id in ids { selected_set.remove(&id); }
                                    } else {
                                        selected_set.extend(ids);
                                    }
                                }
                            }
                        }
                        span { "{name_label}" }
                        span { "{original_size}" }
                        span { "{packed_size}" }
                        span { "{algorithm_label}" }
                    }
                    if folders.is_empty() && visible.is_empty() {
                        div { class: "no-results",
                            Icon { name: if query.is_empty() { IconName::FolderOpen } else { IconName::SearchX }, size: 32 }
                            strong { if query.is_empty() { "{empty_folder}" } else { "{no_matches}" } }
                            span { if query.is_empty() { "{empty_folder_hint}" } else { "{no_matches_hint}" } }
                        }
                    }
                    for folder in folders.iter() {
                        FolderRow {
                            key: "{folder.path}",
                            folder: folder.clone(),
                            browse_path: browse_path,
                            focused_entry: focused_entry,
                        }
                    }
                    for entry in visible.iter() {
                        FileRow {
                            key: "{entry.id}",
                            entry: entry.clone(),
                            selected: selected,
                            focused_entry: focused_entry,
                            active: focused_entry() == Some(entry.id),
                        }
                    }
                }
                div { class: "table-footer",
                    if query.is_empty() {
                        span { "{directory_summary}" }
                        span { class: "desktop-hint", Icon { name: IconName::MousePointer, size: 14 } "{browse_hint}" }
                    } else {
                        span { "{search_summary}" }
                        span { class: "desktop-hint", Icon { name: IconName::Search, size: 14 } "{searching_hint}" }
                    }
                }
            }

            aside { class: "inspector glass-card",
                if let Some(entry) = focused {
                    EntryInspector { entry: entry }
                } else {
                    div { class: "inspector-empty",
                        Icon { name: IconName::MousePointer, size: 28 }
                        strong { "{choose_entry}" }
                        span { "{details_hint}" }
                    }
                }
            }
        }
    }
}

#[component]
fn CreatePage(
    draft_files: Signal<Vec<DraftFile>>,
    compression: Signal<CompressionChoice>,
    archive_name: Signal<String>,
    alignment: Signal<u32>,
    random_access: Signal<bool>,
    mut dz_options: Signal<DzCompressionOptions>,
    busy: Signal<Option<String>>,
    toast: Signal<Option<(bool, String)>>,
    next_id: Signal<u64>,
    editing_source: Signal<Option<String>>,
    logs: Signal<Vec<String>>,
    on_saved: EventHandler<LoadedArchive>,
) -> Element {
    let locale = use_context::<Signal<LocaleChoice>>();
    let current_locale = locale();
    let mut open_select_menu = use_signal(|| None::<OpenSelectMenu>);
    let mut dz_advanced_open = use_signal(|| false);
    let total_size: u64 = draft_files
        .read()
        .iter()
        .map(|file| file.bytes.len() as u64)
        .sum();
    let current_compression = compression();
    let current_dz_options = dz_options();
    let has_dz_entries = draft_files
        .read()
        .iter()
        .any(|file| file.compression == CompressionChoice::Dz)
        || (draft_files.read().is_empty() && current_compression == CompressionChoice::Dz);
    let editing_name = editing_source.read().as_ref().cloned();
    let is_editing = editing_name.is_some();
    let page_title = if let Some(source) = editing_name.as_ref() {
        if current_locale == LocaleChoice::ZhCn {
            format!("编辑 {source}")
        } else {
            format!("Edit {source}")
        }
    } else {
        current_locale
            .pick("创建 Dzip 归档", "Create a Dzip archive")
            .to_string()
    };
    let page_description = if is_editing {
        current_locale.pick(
            "可新增、删除或重命名条目；保存时会使用当前设置完整重建归档。",
            "Add, remove, or rename entries. Saving rebuilds the archive with the current settings.",
        )
    } else {
        current_locale.pick(
            "添加文件，为每个文件选择算法，然后保存为兼容 Dzip 1.1.3 的归档。",
            "Add files, choose an algorithm for each one, then save a Dzip 1.1.3-compatible archive.",
        )
    };
    let save_button = if is_editing {
        current_locale.pick("保存归档", "Save archive")
    } else {
        current_locale.pick("创建并保存", "Create and save")
    };
    let edit_notice = current_locale.pick(
        "原归档不会被直接修改；请选择保存位置生成重建后的归档。",
        "The source archive is not modified directly; choose where to save the rebuilt archive.",
    );
    let draft_heading = current_locale.pick("待压缩文件", "Files to compress");
    let draft_summary = if current_locale == LocaleChoice::ZhCn {
        format!(
            "{} 个文件 · {}",
            draft_files.read().len(),
            human_size(total_size)
        )
    } else {
        format!(
            "{} files · {}",
            draft_files.read().len(),
            human_size(total_size)
        )
    };
    let clear_label = current_locale.pick("清空", "Clear");
    let drop_title = current_locale.pick("把要压缩的文件拖到这里", "Drop files to compress here");
    let drop_hint = current_locale.pick("或点击选择多个文件", "or click to choose multiple files");
    let privacy_hint = current_locale.pick(
        "文件只在当前设备中处理，不会上传到服务器",
        "Files are processed only on this device and are never uploaded",
    );
    let archive_settings = current_locale.pick("归档设置", "Archive settings");
    let per_file_hint = current_locale.pick(
        "每个文件可以使用不同算法",
        "Each file can use a different algorithm",
    );
    let archive_name_label = current_locale.pick("归档名称", "Archive name");
    let default_algorithm =
        current_locale.pick("新增文件默认算法", "Default algorithm for new files");
    let apply_all = current_locale.pick("应用到全部", "Apply to all");
    let alignment_label = current_locale.pick("数据对齐", "Data alignment");
    let random_access_title = current_locale.pick("随机访问", "Random access");
    let random_access_description = current_locale.pick(
        "为条目标记随机访问优化",
        "Mark entries for random-access optimization",
    );
    let combuf_title = current_locale.pick("公共缓冲区", "Common buffer");
    let combuf_description = current_locale.pick(
        "DZ 文件间引用以提高压缩率",
        "Reference data across DZ files for better compression",
    );
    let dz_advanced_title = current_locale.pick("DZ 高级参数", "Advanced DZ parameters");
    let dz_advanced_description = current_locale.pick(
        "调整原生 Range/LZ 编码器与公共引用分析",
        "Tune the native Range/LZ encoder and common-reference analysis",
    );
    let reset_defaults = current_locale.pick("恢复默认", "Reset defaults");
    let analysis_parameters = current_locale.pick("编码与引用分析", "Encoding and references");
    let range_parameters = current_locale.pick("Range/LZ 模型", "Range/LZ model");
    let preprocess_title = current_locale.pick("预处理分析", "Preprocess analysis");
    let preprocess_description = current_locale.pick(
        "排除已能被本地匹配覆盖的数据",
        "Exclude data already covered by local matches",
    );
    let static_tables_title = current_locale.pick("COMBUF 静态表", "COMBUF static tables");
    let static_tables_description = current_locale.pick(
        "公共缓冲区启用时为兼容性必需",
        "Required for compatibility while the common buffer is enabled",
    );
    let unlimited_hint = current_locale.pick("-1 表示不限制", "-1 means unlimited");
    let zero_unlimited_hint = current_locale.pick("0 表示不限制", "0 means unlimited");
    let combuf_only_hint = current_locale.pick("仅影响公共缓冲区", "Common buffer only");
    let model_value_hint = current_locale.pick("写入归档全局设置", "Stored in archive settings");
    let local_processing = current_locale.pick("本地处理", "Local processing");
    let local_processing_description = if cfg!(feature = "web") {
        current_locale.pick(
            "WebAssembly 在浏览器中完成压缩",
            "WebAssembly compresses files in your browser",
        )
    } else {
        current_locale.pick(
            "原生进程直接处理本地数据",
            "The native process handles local data directly",
        )
    };

    rsx! {
        div { class: "archive-heading create-heading",
            div {
                span { class: "eyebrow accent", if is_editing { "EDIT ARCHIVE" } else { "NEW ARCHIVE" } }
                h1 { "{page_title}" }
                p { "{page_description}" }
            }
            button {
                class: "button primary large",
                disabled: draft_files.read().is_empty(),
                onclick: move |_| {
                    let files = draft_files.read().clone();
                    let name = normalise_archive_name(&archive_name());
                    let align = alignment();
                    let random = random_access();
                    let dz = dz_options();
                    async move {
                        busy.set(Some(current_locale.pick("正在压缩文件…", "Compressing files…").to_string()));
                        match build_archive(&files, &name, align, random, dz) {
                            Ok(bytes) => {
                                let reopened = open_archive(name.clone(), bytes.clone(), Vec::new());
                                busy.set(Some(current_locale.pick("正在保存归档…", "Saving archive…").to_string()));
                                match platform::save_bytes(&name, bytes).await {
                                    Ok(_) => {
                                        let message = if current_locale == LocaleChoice::ZhCn {
                                            let action = if is_editing { "保存" } else { "创建" };
                                            format!("{name} {action}成功")
                                        } else if is_editing {
                                            format!("Saved {name}")
                                        } else {
                                            format!("Created {name}")
                                        };
                                        append_log(logs, "INFO", &message);
                                        toast.set(Some((true, message)));
                                        if let Ok(value) = reopened {
                                            on_saved.call(value);
                                        }
                                    }
                                    Err(message) if message == "已取消保存" => {}
                                    Err(message) => {
                                        append_log(logs, "ERROR", &message);
                                        toast.set(Some((false, message.clone())));
                                    }
                                }
                            }
                            Err(message) => {
                                append_log(logs, "ERROR", &message);
                                toast.set(Some((false, message.clone())));
                            }
                        }
                        busy.set(None);
                    }
                },
                Icon { name: IconName::PackageCheck, size: 19 }
                "{save_button}"
            }
        }

        if let Some(source) = editing_name.as_ref() {
            div { class: "edit-mode-banner",
                div { class: "soft-icon", Icon { name: IconName::Pencil, size: 18 } }
                div {
                    strong {
                        if current_locale == LocaleChoice::ZhCn { "正在编辑 {source}" } else { "Editing {source}" }
                    }
                    span { "{edit_notice}" }
                }
            }
        }

        div {
            class: "create-layout",
            onclick: move |_| open_select_menu.set(None),
            section {
                class: if matches!(open_select_menu(), Some(OpenSelectMenu::Compression(_))) { "draft-panel glass-card menu-open" } else { "draft-panel glass-card" },
                div { class: "panel-toolbar",
                    div { class: "panel-title",
                        div { class: "soft-icon", Icon { name: IconName::Files, size: 18 } }
                        div {
                            strong { "{draft_heading}" }
                            span { "{draft_summary}" }
                        }
                    }
                    div { class: "draft-actions",
                        if !draft_files.read().is_empty() {
                            button { class: "text-button danger", onclick: move |_| draft_files.write().clear(),
                                Icon { name: IconName::Trash, size: 15 }
                                "{clear_label}"
                            }
                        }
                        UploadButton {
                            draft_files: draft_files,
                            next_id: next_id,
                            default_compression: current_compression,
                            directory: false,
                        }
                        UploadButton {
                            draft_files: draft_files,
                            next_id: next_id,
                            default_compression: current_compression,
                            directory: true,
                        }
                    }
                }

                if draft_files.read().is_empty() {
                    label { class: "drop-zone",
                        ondragover: move |event| event.prevent_default(),
                        ondrop: move |event| {
                            event.prevent_default();
                            let files = event.files();
                            async move {
                                add_uploaded_files(files, draft_files, next_id, current_compression, true).await;
                            }
                        },
                        div { class: "drop-icon", Icon { name: IconName::CloudUpload, size: 31 } }
                        strong { "{drop_title}" }
                        span { "{drop_hint}" }
                        small { "{privacy_hint}" }
                        input {
                            class: "visually-hidden",
                            r#type: "file",
                            multiple: true,
                            onchange: move |event| {
                                let files = event.files();
                                async move {
                                    add_uploaded_files(files, draft_files, next_id, current_compression, false).await;
                                }
                            }
                        }
                    }
                } else {
                    div {
                        class: if matches!(open_select_menu(), Some(OpenSelectMenu::Compression(_))) { "draft-list menu-open" } else { "draft-list" },
                        for file in draft_files.read().iter() {
                            div {
                                class: if open_select_menu() == Some(OpenSelectMenu::Compression(file.id)) { "draft-row dropdown-open" } else { "draft-row" },
                                key: "{file.id}",
                                div { class: "file-type-icon", Icon { name: icon_for_file(&file.path), size: 19 } }
                                div { class: "draft-name",
                                    input {
                                        class: "draft-path-input",
                                        value: "{file.path}",
                                        aria_label: if current_locale == LocaleChoice::ZhCn { format!("编辑归档路径 {}", file.path) } else { format!("Edit archive path {}", file.path) },
                                        title: current_locale.pick("编辑归档内路径", "Edit path inside archive"),
                                        oninput: {
                                            let id = file.id;
                                            move |event| {
                                                if let Some(item) = draft_files.write().iter_mut().find(|item| item.id == id) {
                                                    item.path = event.value();
                                                }
                                            }
                                        },
                                    }
                                    span { "{human_size(file.bytes.len() as u64)}" }
                                }
                                CompressionPicker {
                                    file_id: file.id,
                                    file_path: file.path.clone(),
                                    value: file.compression,
                                    draft_files,
                                    open_menu: open_select_menu,
                                }
                                button {
                                    class: "row-action danger",
                                    aria_label: if current_locale == LocaleChoice::ZhCn { format!("移除 {}", file.path) } else { format!("Remove {}", file.path) },
                                    onclick: {
                                        let id = file.id;
                                        move |_| draft_files.write().retain(|item| item.id != id)
                                    },
                                    Icon { name: IconName::X, size: 16 }
                                }
                            }
                        }
                    }
                }
            }

            aside { class: if open_select_menu() == Some(OpenSelectMenu::Alignment) { "settings-panel glass-card menu-open" } else { "settings-panel glass-card" },
                div { class: "settings-heading",
                    div { class: "soft-icon", Icon { name: IconName::Sliders, size: 18 } }
                    div {
                        strong { "{archive_settings}" }
                        span { "{per_file_hint}" }
                    }
                }

                label { class: "field-label", "{archive_name_label}" }
                label { class: "text-field",
                    Icon { name: IconName::Archive, size: 17 }
                    input {
                        value: "{archive_name}",
                        aria_label: archive_name_label,
                        oninput: move |event| archive_name.set(event.value()),
                    }
                }

                div { class: "field-label row-label",
                    span { "{default_algorithm}" }
                    if !draft_files.read().is_empty() {
                        button {
                            class: "apply-all-button",
                            onclick: move |_| {
                                for file in draft_files.write().iter_mut() {
                                    file.compression = current_compression;
                                }
                            },
                            "{apply_all}"
                        }
                    }
                }
                div { class: "compression-grid",
                    for choice in CompressionChoice::ALL {
                        button {
                            class: if choice == current_compression { "compression-option active" } else { "compression-option" },
                            onclick: move |_| compression.set(choice),
                            span { class: "radio-dot" }
                            strong { "{compression_label(choice, current_locale)}" }
                            small { "{compression_description(choice, current_locale)}" }
                        }
                    }
                }

                div { class: "settings-divider" }
                label { class: "field-label", "{alignment_label}" }
                AlignmentPicker {
                    alignment,
                    open_menu: open_select_menu,
                }

                ToggleRow {
                    title: random_access_title,
                    description: random_access_description,
                    checked: random_access(),
                    disabled: false,
                    onchange: move |value| random_access.set(value),
                }
                ToggleRow {
                    title: combuf_title,
                    description: combuf_description,
                    checked: current_dz_options.use_combuf,
                    disabled: !has_dz_entries,
                    onchange: move |value| {
                        let mut options = dz_options.write();
                        options.use_combuf = value;
                        if value {
                            options.combuf_static_tables = true;
                            options.ref_length_table_size = options.ref_length_table_size.max(1);
                            options.ref_length_tables = options.ref_length_tables.max(1);
                            options.ref_offset_table_size = options.ref_offset_table_size.max(1);
                            options.ref_offset_tables = options.ref_offset_tables.max(1);
                            options.big_min_match = options.big_min_match.max(1);
                        }
                    },
                }

                div {
                    class: if has_dz_entries { "dz-advanced" } else { "dz-advanced disabled" },
                    button {
                        class: if dz_advanced_open() { "dz-advanced-trigger open" } else { "dz-advanced-trigger" },
                        r#type: "button",
                        disabled: !has_dz_entries,
                        aria_expanded: if dz_advanced_open() { "true" } else { "false" },
                        onclick: move |_| dz_advanced_open.set(!dz_advanced_open()),
                        div {
                            strong { "{dz_advanced_title}" }
                            span { "{dz_advanced_description}" }
                        }
                        span { class: "dz-advanced-chevron",
                            Icon { name: IconName::ChevronDown, size: 16 }
                        }
                    }

                    if dz_advanced_open() && has_dz_entries {
                        div { class: "dz-advanced-content",
                            div { class: "dz-parameter-section-heading",
                                span { "{analysis_parameters}" }
                                button {
                                    class: "parameter-reset",
                                    r#type: "button",
                                    onclick: move |_| dz_options.set(DzCompressionOptions::default()),
                                    "{reset_defaults}"
                                }
                            }

                            ToggleRow {
                                title: preprocess_title,
                                description: preprocess_description,
                                checked: current_dz_options.preprocess,
                                disabled: !current_dz_options.use_combuf,
                                onchange: move |value| dz_options.write().preprocess = value,
                            }
                            ToggleRow {
                                title: static_tables_title,
                                description: static_tables_description,
                                checked: current_dz_options.combuf_static_tables,
                                disabled: current_dz_options.use_combuf,
                                onchange: move |value| dz_options.write().combuf_static_tables = value,
                            }

                            div { class: "dz-parameter-grid",
                                NumberParameter {
                                    label: "max_mem_usage",
                                    hint: unlimited_hint,
                                    value: i64::from(current_dz_options.max_mem_usage),
                                    min: -1,
                                    max: i64::from(i32::MAX),
                                    disabled: false,
                                    onchange: move |value| dz_options.write().max_mem_usage = value as i32,
                                }
                                NumberParameter {
                                    label: "trim_reference_factor",
                                    hint: combuf_only_hint,
                                    value: i64::from(current_dz_options.trim_reference_factor),
                                    min: i64::from(i32::MIN),
                                    max: i64::from(i32::MAX),
                                    disabled: !current_dz_options.use_combuf,
                                    onchange: move |value| dz_options.write().trim_reference_factor = value as i32,
                                }
                                NumberParameter {
                                    label: "max_common_match",
                                    hint: zero_unlimited_hint,
                                    value: i64::from(current_dz_options.max_common_match),
                                    min: 0,
                                    max: i64::from(u32::MAX),
                                    disabled: !current_dz_options.use_combuf,
                                    onchange: move |value| dz_options.write().max_common_match = value as u32,
                                }
                            }

                            div { class: "dz-parameter-section-heading model-heading",
                                span { "{range_parameters}" }
                            }
                            div { class: "dz-parameter-grid",
                                NumberParameter {
                                    label: "WinSize",
                                    hint: model_value_hint,
                                    value: i64::from(current_dz_options.win_size),
                                    min: 0,
                                    max: 30,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().win_size = value as u8,
                                }
                                NumberParameter {
                                    label: "OffsetTableSize",
                                    hint: "1–15",
                                    value: i64::from(current_dz_options.offset_table_size),
                                    min: 1,
                                    max: 15,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().offset_table_size = value as u8,
                                }
                                NumberParameter {
                                    label: "OffsetTables",
                                    hint: "1–255",
                                    value: i64::from(current_dz_options.offset_tables),
                                    min: 1,
                                    max: 255,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().offset_tables = value as u8,
                                }
                                NumberParameter {
                                    label: "OffsetContexts",
                                    hint: "1–8",
                                    value: i64::from(current_dz_options.offset_contexts),
                                    min: 1,
                                    max: 8,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().offset_contexts = value as u8,
                                }
                                NumberParameter {
                                    label: "RefLengthTableSize",
                                    hint: "0–15",
                                    value: i64::from(current_dz_options.ref_length_table_size),
                                    min: if current_dz_options.use_combuf { 1 } else { 0 },
                                    max: 15,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().ref_length_table_size = value as u8,
                                }
                                NumberParameter {
                                    label: "RefLengthTables",
                                    hint: "0–255",
                                    value: i64::from(current_dz_options.ref_length_tables),
                                    min: if current_dz_options.use_combuf { 1 } else { 0 },
                                    max: 255,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().ref_length_tables = value as u8,
                                }
                                NumberParameter {
                                    label: "RefOffsetTableSize",
                                    hint: "0–15",
                                    value: i64::from(current_dz_options.ref_offset_table_size),
                                    min: if current_dz_options.use_combuf { 1 } else { 0 },
                                    max: 15,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().ref_offset_table_size = value as u8,
                                }
                                NumberParameter {
                                    label: "RefOffsetTables",
                                    hint: "0–255",
                                    value: i64::from(current_dz_options.ref_offset_tables),
                                    min: if current_dz_options.use_combuf { 1 } else { 0 },
                                    max: 255,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().ref_offset_tables = value as u8,
                                }
                                NumberParameter {
                                    label: "BigMinMatch",
                                    hint: "0–255",
                                    value: i64::from(current_dz_options.big_min_match),
                                    min: if current_dz_options.use_combuf { 1 } else { 0 },
                                    max: 255,
                                    disabled: false,
                                    onchange: move |value| dz_options.write().big_min_match = value as u8,
                                }
                            }
                        }
                    }
                }

                div { class: "privacy-note",
                    Icon { name: IconName::ShieldCheck, size: 18 }
                    div {
                        strong { "{local_processing}" }
                        span { "{local_processing_description}" }
                    }
                }
            }
        }
    }
}

#[component]
fn CompressionPicker(
    file_id: u64,
    file_path: String,
    value: CompressionChoice,
    mut draft_files: Signal<Vec<DraftFile>>,
    mut open_menu: Signal<Option<OpenSelectMenu>>,
) -> Element {
    let locale = use_context::<Signal<LocaleChoice>>();
    let current_locale = locale();
    let is_open = open_menu() == Some(OpenSelectMenu::Compression(file_id));
    let menu_id = format!("compression-menu-{file_id}");
    let current_label = compression_label(value, current_locale);
    let trigger_label = if current_locale == LocaleChoice::ZhCn {
        format!("设置 {file_path} 的压缩算法，当前为 {current_label}")
    } else {
        format!("Set compression for {file_path}; current algorithm is {current_label}")
    };
    let menu_label = if current_locale == LocaleChoice::ZhCn {
        format!("{file_path} 的压缩算法")
    } else {
        format!("Compression algorithm for {file_path}")
    };

    rsx! {
        div {
            class: if is_open { "compression-picker open" } else { "compression-picker" },
            onclick: move |event| event.stop_propagation(),
            button {
                class: "compression-trigger",
                r#type: "button",
                aria_label: trigger_label,
                aria_haspopup: "listbox",
                aria_expanded: if is_open { "true" } else { "false" },
                aria_controls: "{menu_id}",
                onclick: move |_| {
                    if is_open {
                        open_menu.set(None);
                    } else {
                        open_menu.set(Some(OpenSelectMenu::Compression(file_id)));
                    }
                },
                span { "{current_label}" }
                span { class: "compression-trigger-chevron",
                    Icon { name: IconName::ChevronDown, size: 13 }
                }
            }
            if is_open {
                div {
                    id: "{menu_id}",
                    class: "compression-menu",
                    role: "listbox",
                    aria_label: menu_label,
                    for choice in CompressionChoice::ALL {
                        button {
                            class: if choice == value { "compression-menu-option active" } else { "compression-menu-option" },
                            r#type: "button",
                            role: "option",
                            aria_selected: if choice == value { "true" } else { "false" },
                            title: compression_description(choice, current_locale),
                            onclick: move |_| {
                                if let Some(item) = draft_files
                                    .write()
                                    .iter_mut()
                                    .find(|item| item.id == file_id)
                                {
                                    item.compression = choice;
                                }
                                open_menu.set(None);
                            },
                            span { class: "compression-option-mark",
                                if choice == value {
                                    Icon { name: IconName::Check, size: 13 }
                                }
                            }
                            span { "{compression_label(choice, current_locale)}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn AlignmentPicker(
    mut alignment: Signal<u32>,
    mut open_menu: Signal<Option<OpenSelectMenu>>,
) -> Element {
    const OPTIONS: [u32; 4] = [0, 512, 2048, 4096];

    let locale = use_context::<Signal<LocaleChoice>>();
    let current_locale = locale();
    let current_value = alignment();
    let current_label = alignment_option_label(current_value, current_locale);
    let is_open = open_menu() == Some(OpenSelectMenu::Alignment);
    let aria_label = current_locale.pick("选择数据对齐方式", "Choose data alignment");

    rsx! {
        div {
            class: if is_open { "alignment-picker open" } else { "alignment-picker" },
            onclick: move |event| event.stop_propagation(),
            button {
                class: "alignment-trigger",
                r#type: "button",
                aria_label,
                aria_haspopup: "listbox",
                aria_expanded: if is_open { "true" } else { "false" },
                aria_controls: "alignment-menu",
                onclick: move |_| {
                    if is_open {
                        open_menu.set(None);
                    } else {
                        open_menu.set(Some(OpenSelectMenu::Alignment));
                    }
                },
                span { "{current_label}" }
                span { class: "alignment-trigger-chevron",
                    Icon { name: IconName::ChevronDown, size: 16 }
                }
            }
            if is_open {
                div {
                    id: "alignment-menu",
                    class: "alignment-menu",
                    role: "listbox",
                    aria_label,
                    for option in OPTIONS {
                        {
                            let option_label = alignment_option_label(option, current_locale);
                            let active = option == current_value;
                            rsx! {
                                button {
                                    class: if active { "alignment-menu-option active" } else { "alignment-menu-option" },
                                    r#type: "button",
                                    role: "option",
                                    aria_selected: if active { "true" } else { "false" },
                                    onclick: move |_| {
                                        alignment.set(option);
                                        open_menu.set(None);
                                    },
                                    span { "{option_label}" }
                                    span { class: "alignment-option-mark",
                                        if active {
                                            Icon { name: IconName::Check, size: 14 }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn UploadButton(
    draft_files: Signal<Vec<DraftFile>>,
    next_id: Signal<u64>,
    default_compression: CompressionChoice,
    directory: bool,
) -> Element {
    let locale = use_context::<Signal<LocaleChoice>>();
    let current_locale = locale();
    let label = if directory {
        current_locale.pick("添加文件夹", "Add folder")
    } else {
        current_locale.pick("添加文件", "Add files")
    };
    rsx! {
        label { class: if directory { "button secondary compact file-action folder-upload" } else { "button secondary compact file-action" },
            Icon { name: if directory { IconName::FolderPlus } else { IconName::Plus }, size: 16 }
            "{label}"
            input {
                class: "visually-hidden",
                r#type: "file",
                multiple: true,
                directory: directory,
                onchange: move |event| {
                    let files = event.files();
                    async move {
                        add_uploaded_files(
                            files,
                            draft_files,
                            next_id,
                            default_compression,
                            directory,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

async fn add_uploaded_files(
    files: Vec<FileData>,
    mut draft_files: Signal<Vec<DraftFile>>,
    mut next_id: Signal<u64>,
    default_compression: CompressionChoice,
    preserve_tree: bool,
) {
    let common_root = preserve_tree.then(|| common_upload_root(&files)).flatten();
    for file in files {
        if let Ok(bytes) = file.read_bytes().await {
            let id = next_id();
            next_id.set(id.saturating_add(1));
            let path = upload_path(&file, common_root.as_deref());
            draft_files.write().push(DraftFile {
                id,
                path,
                bytes: Arc::from(bytes.to_vec()),
                compression: default_compression,
            });
        }
    }
}

fn upload_path(file: &FileData, common_root: Option<&std::path::Path>) -> String {
    let path = file.path();
    if let Some(root) = common_root
        && let Ok(relative) = path.strip_prefix(root)
        && !relative.as_os_str().is_empty()
    {
        relative.to_string_lossy().replace('\\', "/")
    } else if !path.is_absolute() && path.components().count() > 1 {
        path.to_string_lossy().replace('\\', "/")
    } else {
        file.name()
    }
}

fn common_upload_root(files: &[FileData]) -> Option<std::path::PathBuf> {
    let mut root = files.first()?.path().parent()?.to_path_buf();
    if !root.is_absolute() {
        return None;
    }
    for file in files.iter().skip(1) {
        while !file.path().starts_with(&root) {
            if !root.pop() {
                return None;
            }
        }
    }
    Some(root)
}

async fn extract_and_export(
    archive: LoadedArchive,
    entry_ids: Vec<usize>,
    mut busy: Signal<Option<String>>,
    mut toast: Signal<Option<(bool, String)>>,
    logs: Signal<Vec<String>>,
    locale: LocaleChoice,
) {
    if entry_ids.is_empty() {
        return;
    }
    let progress = if locale == LocaleChoice::ZhCn {
        format!("正在解压 {} 个文件…", entry_ids.len())
    } else {
        format!("Extracting {} files…", entry_ids.len())
    };
    busy.set(Some(progress));
    let result = match read_entries(&archive, &entry_ids) {
        Ok(files) => platform::export_files(&archive.name, files).await,
        Err(message) => Err(message),
    };
    match result {
        Ok(message) => {
            append_log(logs, "INFO", &message);
            toast.set(Some((true, message)));
        }
        Err(message) if message == "已取消解压" => {}
        Err(message) => {
            append_log(logs, "ERROR", &message);
            toast.set(Some((false, message.clone())));
        }
    }
    busy.set(None);
}

#[component]
fn NavButton(
    active: bool,
    icon: IconName,
    label: &'static str,
    hint: &'static str,
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
fn StatCard(
    icon: IconName,
    label: &'static str,
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
fn FolderRow(
    folder: FolderView,
    mut browse_path: Signal<String>,
    mut focused_entry: Signal<Option<usize>>,
) -> Element {
    let locale = use_context::<Signal<LocaleChoice>>();
    let current_locale = locale();
    let target = folder.path.clone();
    let open_label = if current_locale == LocaleChoice::ZhCn {
        format!("打开文件夹 {}", folder.name)
    } else {
        format!("Open folder {}", folder.name)
    };
    let file_count = if current_locale == LocaleChoice::ZhCn {
        format!("{} 个文件", folder.file_count)
    } else {
        format!("{} files", folder.file_count)
    };
    let original_label = current_locale.pick("原始", "Original");
    let packed_label = current_locale.pick("压缩后", "Packed");
    let type_label = current_locale.pick("类型", "Type");
    let folder_label = current_locale.pick("文件夹", "Folder");
    rsx! {
        button {
            class: "file-row folder-row",
            r#type: "button",
            aria_label: open_label,
            onclick: move |_| {
                browse_path.set(target.clone());
                focused_entry.set(None);
            },
            span { class: "check-wrap folder-disclosure", Icon { name: IconName::ChevronRight, size: 15 } }
            div { class: "file-cell name-cell",
                div { class: "file-type-icon folder", Icon { name: IconName::Folder, size: 19 } }
                div {
                    strong { "{folder.name}" }
                    span { "{file_count}" }
                }
            }
            span { class: "file-cell size-cell", "data-label": original_label, "{human_size(folder.size)}" }
            span { class: "file-cell size-cell", "data-label": packed_label, "{human_size(folder.packed_size)}" }
            span { class: "file-cell method-cell", "data-label": type_label, span { class: "folder-chip", "{folder_label}" } }
        }
    }
}

#[component]
fn FileRow(
    entry: EntryView,
    selected: Signal<HashSet<usize>>,
    focused_entry: Signal<Option<usize>>,
    active: bool,
) -> Element {
    let locale = use_context::<Signal<LocaleChoice>>();
    let current_locale = locale();
    let is_selected = selected.read().contains(&entry.id);
    let icon = icon_for_file(&entry.name);
    let select_label = if current_locale == LocaleChoice::ZhCn {
        format!("选择 {}", entry.path)
    } else {
        format!("Select {}", entry.path)
    };
    let original_label = current_locale.pick("原始", "Original");
    let packed_label = current_locale.pick("压缩后", "Packed");
    let algorithm_label = current_locale.pick("算法", "Algorithm");
    let folder_name = if entry.folder == "根目录" {
        current_locale.pick("根目录", "Root")
    } else {
        entry.folder.as_str()
    };
    rsx! {
        div {
            class: if active { "file-row active" } else { "file-row" },
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
            div { class: "file-cell name-cell",
                div { class: "file-type-icon", Icon { name: icon, size: 18 } }
                div {
                    strong { "{entry.name}" }
                    span { "{folder_name}" }
                }
            }
            span { class: "file-cell size-cell", "data-label": original_label, "{human_size(entry.size)}" }
            span { class: "file-cell size-cell", "data-label": packed_label, "{human_size(entry.packed_size)}" }
            span { class: "file-cell method-cell", "data-label": algorithm_label, span { class: "method-chip", "{entry.compression}" } }
        }
    }
}

#[component]
fn EntryInspector(entry: EntryView) -> Element {
    let locale = use_context::<Signal<LocaleChoice>>();
    let current_locale = locale();
    let ratio = ratio_percent(entry.packed_size, entry.size);
    let ratio_label = current_locale.pick("压缩占比", "Compression ratio");
    let saved_label = if current_locale == LocaleChoice::ZhCn {
        format!("节省 {}% 空间", 100u64.saturating_sub(ratio))
    } else {
        format!("{}% space saved", 100u64.saturating_sub(ratio))
    };
    let original_size = current_locale.pick("原始大小", "Original size");
    let packed_size = current_locale.pick("压缩后", "Packed size");
    let compression_algorithm = current_locale.pick("压缩算法", "Compression algorithm");
    let chunks_label = current_locale.pick("数据块", "Chunks");
    let volume_label = current_locale.pick("所在分卷", "Volume");
    let archive_path = current_locale.pick("归档路径", "Archive path");
    let folder_name = if entry.folder == "根目录" {
        current_locale.pick("根目录", "Root")
    } else {
        entry.folder.as_str()
    };
    rsx! {
        div { class: "inspector-header",
            span { class: "eyebrow", "FILE DETAILS" }
            div { class: "inspector-file-icon", Icon { name: icon_for_file(&entry.name), size: 28 } }
            h2 { "{entry.name}" }
            p { "{folder_name}" }
        }
        div { class: "ratio-block",
            div { class: "ratio-label", span { "{ratio_label}" } strong { "{ratio}%" } }
            div { class: "ratio-track", span { style: "width: {ratio.min(100)}%" } }
            small { "{saved_label}" }
        }
        dl { class: "detail-list",
            div { dt { "{original_size}" } dd { "{human_size(entry.size)}" } }
            div { dt { "{packed_size}" } dd { "{human_size(entry.packed_size)}" } }
            div { dt { "{compression_algorithm}" } dd { span { class: "method-chip", "{entry.compression}" } } }
            div { dt { "{chunks_label}" } dd { "{entry.chunks}" } }
            div { dt { "{volume_label}" } dd { "Volume {entry.volume}" } }
        }
        div { class: "path-box",
            span { "{archive_path}" }
            code { "{entry.path}" }
        }
    }
}

#[component]
fn NumberParameter(
    label: &'static str,
    hint: &'static str,
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
fn ToggleRow(
    title: &'static str,
    description: &'static str,
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

fn icon_for_file(path: &str) -> IconName {
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" => IconName::Image,
        "mp3" | "wav" | "ogg" | "flac" => IconName::Music,
        "txt" | "md" | "json" | "toml" | "xml" | "ini" => IconName::FileText,
        "exe" | "dll" | "bin" | "dat" => IconName::Binary,
        _ => IconName::File,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IconName {
    AlertCircle,
    Archive,
    ArchiveRestore,
    ArrowLeft,
    Binary,
    Check,
    CheckCircle,
    ChevronDown,
    ChevronRight,
    CloudUpload,
    Download,
    Expand,
    File,
    FileText,
    Files,
    Folder,
    FolderOpen,
    FolderPlus,
    Gauge,
    Github,
    HardDrive,
    Home,
    Image,
    Info,
    Languages,
    ListTree,
    Monitor,
    Moon,
    MousePointer,
    Music,
    PackageCheck,
    PackagePlus,
    Pencil,
    Plus,
    Search,
    SearchX,
    Settings,
    ShieldCheck,
    Sliders,
    ScrollText,
    Sun,
    Trash,
    X,
}

#[component]
fn Icon(name: IconName, #[props(default = 18)] size: u8) -> Element {
    let stroke_width = if name == IconName::Settings {
        "2"
    } else {
        "1.8"
    };
    let paths = match name {
        IconName::Archive => {
            rsx! { rect { x: "3", y: "4", width: "18", height: "5", rx: "2" } path { d: "M5 9v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9" } path { d: "M10 13h4" } }
        }
        IconName::FolderOpen => {
            rsx! { path { d: "m6 14 1.5-4h13l-2.1 7.4A2 2 0 0 1 16.5 19H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v2" } }
        }
        IconName::PackagePlus => {
            rsx! { path { d: "m7.5 4.27 9 5.15" } path { d: "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l3-1.72" } path { d: "M3.3 7 12 12l8.7-5" } path { d: "M12 22V12" } path { d: "M19 16v6" } path { d: "M16 19h6" } }
        }
        IconName::Github => {
            rsx! { path { d: "M15 22v-4a4.8 4.8 0 0 0-1-3.5c3.3-.4 6.8-1.6 6.8-7A5.4 5.4 0 0 0 19.3 4 5 5 0 0 0 19.1.5S17.9.1 15 2a13.4 13.4 0 0 0-7 0C5.1.1 3.9.5 3.9.5A5 5 0 0 0 3.7 4a5.4 5.4 0 0 0-1.5 3.7c0 5.4 3.5 6.6 6.8 7A4.8 4.8 0 0 0 8 18v4" } path { d: "M8 19c-3 .9-3-1.5-4-2" } }
        }
        IconName::Search => rsx! { circle { cx: "11", cy: "11", r: "7" } path { d: "m20 20-4-4" } },
        IconName::X => rsx! { path { d: "M18 6 6 18" } path { d: "m6 6 12 12" } },
        IconName::Sun => {
            rsx! { circle { cx: "12", cy: "12", r: "4" } path { d: "M12 2v2M12 20v2M4.93 4.93l1.42 1.42M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" } }
        }
        IconName::Moon => {
            rsx! { path { d: "M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5 8.5 8.5 0 1 0 20.5 14.2Z" } }
        }
        IconName::Monitor => {
            rsx! { rect { x: "3", y: "4", width: "18", height: "14", rx: "2" } path { d: "M8 21h8M12 18v3" } }
        }
        IconName::CheckCircle => {
            rsx! { circle { cx: "12", cy: "12", r: "9" } path { d: "m8 12 2.5 2.5L16 9" } }
        }
        IconName::AlertCircle => {
            rsx! { circle { cx: "12", cy: "12", r: "9" } path { d: "M12 8v4" } path { d: "M12 16h.01" } }
        }
        IconName::Download => {
            rsx! { path { d: "M12 3v12" } path { d: "m7 10 5 5 5-5" } path { d: "M5 21h14" } }
        }
        IconName::ArchiveRestore => {
            rsx! { rect { x: "3", y: "4", width: "18", height: "5", rx: "2" } path { d: "M5 9v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9" } path { d: "M9 14h6" } path { d: "m11 12-2 2 2 2" } }
        }
        IconName::ArrowLeft => rsx! { path { d: "m15 18-6-6 6-6" } },
        IconName::Files => {
            rsx! { path { d: "M15 2H6a2 2 0 0 0-2 2v12" } path { d: "M8 6h10a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2Z" } }
        }
        IconName::HardDrive => {
            rsx! { line { x1: "22", x2: "2", y1: "12", y2: "12" } path { d: "M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z" } line { x1: "6", x2: "6.01", y1: "16", y2: "16" } line { x1: "10", x2: "10.01", y1: "16", y2: "16" } }
        }
        IconName::Expand => {
            rsx! { path { d: "m15 3 6 6" } path { d: "M21 3v6h-6" } path { d: "m9 21-6-6" } path { d: "M3 21v-6h6" } }
        }
        IconName::Gauge => {
            rsx! { path { d: "m12 14 4-4" } path { d: "M3.34 19a10 10 0 1 1 17.32 0" } }
        }
        IconName::ListTree => {
            rsx! { path { d: "M21 12h-8" } path { d: "M21 6H8" } path { d: "M21 18h-8" } path { d: "M3 6h1v4h4v2" } path { d: "M3 10v8h5" } }
        }
        IconName::SearchX => {
            rsx! { circle { cx: "11", cy: "11", r: "7" } path { d: "m20 20-4-4" } path { d: "m8 8 6 6" } path { d: "m14 8-6 6" } }
        }
        IconName::Settings => {
            rsx! { path { d: "M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 0 0 2.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 0 0 1.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 0 0-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 0 0-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 0 0-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 0 0-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 0 0 1.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065Z" } path { d: "M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z" } }
        }
        IconName::MousePointer => {
            rsx! { path { d: "m3 3 7.1 17 2.5-7.4L20 10.1 3 3Z" } path { d: "m13 13 6 6" } }
        }
        IconName::PackageCheck => {
            rsx! { path { d: "m7.5 4.27 9 5.15" } path { d: "M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l3-1.72" } path { d: "M3.3 7 12 12l8.7-5" } path { d: "M12 22V12" } path { d: "m16 19 2 2 4-4" } }
        }
        IconName::Pencil => {
            rsx! { path { d: "M12 20h9" } path { d: "M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4Z" } }
        }
        IconName::Trash => {
            rsx! { path { d: "M3 6h18" } path { d: "M8 6V4h8v2" } path { d: "m19 6-1 15H6L5 6" } path { d: "M10 11v5M14 11v5" } }
        }
        IconName::Plus => rsx! { path { d: "M12 5v14M5 12h14" } },
        IconName::FolderPlus => {
            rsx! { path { d: "M12 10v6M9 13h6" } path { d: "M3 18V6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" } }
        }
        IconName::CloudUpload => {
            rsx! { path { d: "M12 13v8" } path { d: "m8 17 4-4 4 4" } path { d: "M5 16a4 4 0 0 1 .8-7.9A6 6 0 0 1 17.6 6 4.5 4.5 0 0 1 18 15" } }
        }
        IconName::Sliders => {
            rsx! { line { x1: "4", x2: "4", y1: "21", y2: "14" } line { x1: "4", x2: "4", y1: "10", y2: "3" } line { x1: "12", x2: "12", y1: "21", y2: "12" } line { x1: "12", x2: "12", y1: "8", y2: "3" } line { x1: "20", x2: "20", y1: "21", y2: "16" } line { x1: "20", x2: "20", y1: "12", y2: "3" } line { x1: "2", x2: "6", y1: "14", y2: "14" } line { x1: "10", x2: "14", y1: "8", y2: "8" } line { x1: "18", x2: "22", y1: "16", y2: "16" } }
        }
        IconName::ChevronDown => rsx! { path { d: "m6 9 6 6 6-6" } },
        IconName::ChevronRight => rsx! { path { d: "m9 18 6-6-6-6" } },
        IconName::ShieldCheck => {
            rsx! { path { d: "M20 13c0 5-3.5 7.5-8 9-4.5-1.5-8-4-8-9V5l8-3 8 3Z" } path { d: "m9 12 2 2 4-4" } }
        }
        IconName::Check => rsx! { path { d: "m5 12 4 4L19 6" } },
        IconName::Image => {
            rsx! { rect { x: "3", y: "3", width: "18", height: "18", rx: "2" } circle { cx: "9", cy: "9", r: "2" } path { d: "m21 15-5-5L5 21" } }
        }
        IconName::Music => {
            rsx! { path { d: "M9 18V5l12-2v13" } circle { cx: "6", cy: "18", r: "3" } circle { cx: "18", cy: "16", r: "3" } }
        }
        IconName::FileText => {
            rsx! { path { d: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" } path { d: "M14 2v6h6" } path { d: "M8 13h8M8 17h6" } }
        }
        IconName::Binary => {
            rsx! { rect { x: "3", y: "3", width: "18", height: "18", rx: "2" } path { d: "M8 7v10M16 7v10M6 9h4M14 15h4" } }
        }
        IconName::File => {
            rsx! { path { d: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" } path { d: "M14 2v6h6" } }
        }
        IconName::Folder => {
            rsx! { path { d: "M3 18V6a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" } }
        }
        IconName::Home => {
            rsx! { path { d: "m3 11 9-8 9 8" } path { d: "M5 10v10h14V10" } path { d: "M9 20v-6h6v6" } }
        }
        IconName::Info => {
            rsx! { circle { cx: "12", cy: "12", r: "10" } path { d: "M12 16v-4" } path { d: "M12 8h.01" } }
        }
        IconName::Languages => {
            rsx! { path { d: "m5 8 6 6" } path { d: "m4 14 6-6 2-3" } path { d: "M2 5h12" } path { d: "M7 2h1" } path { d: "m22 22-5-10-5 10" } path { d: "M14 18h6" } }
        }
        IconName::ScrollText => {
            rsx! { path { d: "M15 12h-5" } path { d: "M15 8h-5" } path { d: "M15 16h-5" } path { d: "M19 17V5a2 2 0 0 0-2-2H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10" } path { d: "M19 17h2a2 2 0 0 1-2 4h-2" } }
        }
    };
    rsx! {
        svg {
            width: "{size}",
            height: "{size}",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "{stroke_width}",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            {paths}
        }
    }
}

#[cfg(test)]
mod browser_tests {
    use super::*;

    fn entry(id: usize, path: &str, size: u64) -> EntryView {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let folder = path
            .rsplit_once('/')
            .map(|(folder, _)| folder.to_string())
            .unwrap_or_else(|| "根目录".to_string());
        EntryView {
            id,
            path: path.to_string(),
            name,
            folder,
            size,
            packed_size: size / 2,
            compression: "DZ".to_string(),
            volume: 0,
            chunks: 1,
        }
    }

    #[test]
    fn browser_listing_builds_virtual_folders_and_searches_globally() {
        let entries = vec![
            entry(0, "readme.txt", 20),
            entry(1, "Data/config.json", 40),
            entry(2, "Data/Images/logo.png", 60),
            entry(3, "Sounds/theme.ogg", 80),
        ];

        let (root_folders, root_files) = build_browser_listing(&entries, "", "");
        assert_eq!(
            root_folders
                .iter()
                .map(|folder| folder.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Data", "Sounds"]
        );
        assert_eq!(root_folders[0].file_count, 2);
        assert_eq!(root_folders[0].size, 100);
        assert_eq!(root_files[0].path, "readme.txt");

        let (data_folders, data_files) = build_browser_listing(&entries, "Data", "");
        assert_eq!(data_folders[0].path, "Data/Images");
        assert_eq!(data_files[0].path, "Data/config.json");

        let (search_folders, search_files) = build_browser_listing(&entries, "Data", "theme");
        assert!(search_folders.is_empty());
        assert_eq!(search_files[0].path, "Sounds/theme.ogg");
    }
}
