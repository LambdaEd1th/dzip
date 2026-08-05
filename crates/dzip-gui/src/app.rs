use crate::archive_input::{
    DroppedArchiveError, prepare_archive_files, prepare_dropped_archive, rebase_extracted_files,
};
use crate::background::{
    build_archive, close_archive as close_archive_session, editable_entries, open_archive,
    read_entries,
};
use crate::browser::{
    browser_breadcrumbs, build_browser_listing, build_draft_browser_listing,
    entry_ids_in_directory, join_archive_path, parent_browser_directory,
};
use crate::components::*;
use crate::i18n::{self, I18n, Locale};
use crate::icons::{Icon, IconName};
use crate::state::{ArchiveEditorMode, OpenSelectMenu, WorkspaceState};
use crate::{app_i18n, file_drop, platform, preferences};
use dioxus::html::FileData;
use dioxus::prelude::*;
use dzip_gui::model::{
    CompressionChoice, DraftFile, DzCompressionOptions, LoadedArchive, WorkspacePage, human_size,
    ratio_percent,
};
use dzip_workflow::normalize_archive_name;
use std::collections::HashSet;
use std::sync::Arc;

const MAIN_CSS: Asset = asset!("/assets/main.css");
#[cfg(feature = "web")]
const _: Asset = asset!("/assets/worker", AssetOptions::folder());

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

fn append_log(mut logs: Signal<Vec<String>>, level: &str, message: impl Into<String>) {
    let mut entries = logs.write();
    entries.push(format!("[{level}] {}", message.into()));
    if entries.len() > 300 {
        entries.remove(0);
    }
}

fn compression_label(choice: CompressionChoice, i18n: I18n) -> String {
    i18n.t(match choice {
        CompressionChoice::Dz => "compression-dz",
        CompressionChoice::Zlib => "compression-zlib",
        CompressionChoice::Bzip => "compression-bzip",
        CompressionChoice::Lzma => "compression-lzma",
        CompressionChoice::Copy => "compression-copy",
        CompressionChoice::Zero => "compression-zero",
    })
}

fn compression_description(choice: CompressionChoice, i18n: I18n) -> String {
    i18n.t(match choice {
        CompressionChoice::Dz => "compression-dz-description",
        CompressionChoice::Zlib => "compression-zlib-description",
        CompressionChoice::Bzip => "compression-bzip-description",
        CompressionChoice::Lzma => "compression-lzma-description",
        CompressionChoice::Copy => "compression-copy-description",
        CompressionChoice::Zero => "compression-zero-description",
    })
}

fn alignment_option_label(value: u32, i18n: I18n) -> String {
    i18n.t(match value {
        512 => "alignment-512",
        2048 => "alignment-2048",
        4096 => "alignment-4096",
        _ => "alignment-none",
    })
}

fn reveal_draft_menu(file_id: u64) {
    dioxus::document::eval(&format!(
        r#"
        requestAnimationFrame(() => requestAnimationFrame(() => {{
            const row = document.getElementById('draft-row-{file_id}');
            if (!row) return;

            row.scrollIntoView({{ block: 'nearest', inline: 'nearest' }});
            const picker = row.querySelector('.compression-picker.open');
            const menu = picker?.querySelector('.compression-menu');
            const trigger = picker?.querySelector('.compression-trigger');
            if (!menu || !trigger) return;

            menu.classList.remove('drop-up');
            const triggerRect = trigger.getBoundingClientRect();
            const menuHeight = Math.min(menu.scrollHeight, 226);
            const viewportPadding = 12;
            const spaceBelow = window.innerHeight - triggerRect.bottom - viewportPadding;
            const spaceAbove = triggerRect.top - viewportPadding;
            menu.classList.toggle(
                'drop-up',
                spaceBelow < menuHeight && spaceAbove > spaceBelow,
            );
        }}));
        "#
    ));
}

pub fn launch() {
    dioxus::launch(App);
}

#[cfg(feature = "web")]
fn use_web_file_drop_guard() {
    use_effect(|| {
        dioxus::document::eval(
            r#"
            (() => {
                if (window.__dzipFileDropGuard) return;
                const blocksNativeFileDrop = (event) => {
                    const transfer = event.dataTransfer;
                    if (!transfer) return;
                    const hasFiles = Array.from(transfer.types || []).includes('Files')
                        || Array.from(transfer.items || []).some((item) => item.kind === 'file');
                    if (hasFiles) event.preventDefault();
                };
                window.__dzipFileDropGuard = blocksNativeFileDrop;
                document.addEventListener('dragover', blocksNativeFileDrop, true);
                document.addEventListener('drop', blocksNativeFileDrop, true);
            })();
            "#,
        );
    });
}

fn reset_archive_editor(
    mut draft_files: Signal<Vec<DraftFile>>,
    mut compression: Signal<CompressionChoice>,
    mut archive_name: Signal<String>,
    mut alignment: Signal<u32>,
    mut random_access: Signal<bool>,
    mut dz_options: Signal<DzCompressionOptions>,
    mut editor_mode: Signal<ArchiveEditorMode>,
) {
    let stored = preferences::read_archive_preferences();
    draft_files.write().clear();
    compression.set(stored.compression);
    archive_name.set("game-assets.dz".to_string());
    alignment.set(stored.alignment);
    random_access.set(stored.random_access);
    dz_options.set(stored.dz_options);
    editor_mode.set(ArchiveEditorMode::New);
}

async fn replace_loaded_archive(mut archive: Signal<Option<LoadedArchive>>, value: LoadedArchive) {
    let previous = archive
        .peek()
        .as_ref()
        .map(|archive| archive.session_id)
        .filter(|session| *session != value.session_id);
    archive.set(Some(value));
    if let Some(session_id) = previous {
        let _ = close_archive_session(session_id).await;
    }
}

#[component]
fn App() -> Element {
    app_i18n::load_i18n_sources();
    #[cfg(feature = "web")]
    use_web_file_drop_guard();
    let mut page = use_signal(|| WorkspacePage::Browse);
    let appearance = use_signal(|| {
        let stored = preferences::read_theme();
        AppearanceMode::from_code(stored.as_deref())
    });
    let locale = use_signal(app_i18n::initial_locale);
    use_context_provider(|| locale);
    let mut settings_open = use_signal(|| false);
    let logs = use_signal(|| {
        vec![format!(
            "[INFO] Dzip Archive v{} started",
            env!("CARGO_PKG_VERSION")
        )]
    });
    let archive = use_signal(|| None::<LoadedArchive>);
    let mut selected = use_signal(HashSet::<usize>::new);
    let mut focused_entry = use_signal(|| None::<usize>);
    let mut search = use_signal(String::new);
    let mut browse_path = use_signal(String::new);
    let draft_files = use_signal(Vec::<DraftFile>::new);
    let initial_archive_preferences = use_hook(preferences::read_archive_preferences);
    let initial_compression = initial_archive_preferences.compression;
    let initial_alignment = initial_archive_preferences.alignment;
    let initial_random_access = initial_archive_preferences.random_access;
    let initial_dz_options = initial_archive_preferences.dz_options;
    let compression = use_signal(move || initial_compression);
    let archive_name = use_signal(|| "game-assets.dz".to_string());
    let alignment = use_signal(move || initial_alignment);
    let random_access = use_signal(move || initial_random_access);
    let dz_options = use_signal(move || initial_dz_options);
    let editor_mode = use_signal(|| ArchiveEditorMode::New);
    let next_id = use_signal(|| 1u64);
    let busy = use_signal(|| None::<String>);
    let mut toast = use_signal(|| None::<(bool, String)>);
    let mut dragging_files = use_signal(|| false);
    let workspace = WorkspaceState {
        page,
        archive,
        selected,
        focused_entry,
        search,
        browse_path,
        draft_files,
        compression,
        archive_name,
        alignment,
        random_access,
        dz_options,
        editor_mode,
        next_id,
        busy,
        toast,
        logs,
    };

    use_effect(move || {
        let preferences = preferences::ArchivePreferences {
            compression: compression(),
            alignment: alignment(),
            random_access: random_access(),
            dz_options: dz_options(),
        };
        if page() == WorkspacePage::Editor
            && editor_mode.read().is_new()
            && let Err(error) = preferences::queue_archive_preferences(preferences)
        {
            append_log(logs, "ERROR", format!("Archive preferences: {error}"));
        }
    });

    let appearance_class = match appearance() {
        AppearanceMode::System => "theme-system",
        AppearanceMode::Light => "theme-light",
        AppearanceMode::Dark => "theme-dark dark",
    };
    let accepting_drop = dragging_files();
    let theme_class = format!(
        "app {appearance_class}{}",
        if accepting_drop {
            " dragging-files"
        } else {
            ""
        }
    );
    let current_locale = locale();
    let i18n = I18n::new(current_locale);
    let busy_hint = i18n.t("busy-large-files");
    let dismiss_notification = i18n.t("dismiss-notification");
    let primary_nav_label = i18n.t("primary-navigation");
    let archive_nav = i18n.t("nav-archive");
    let archive_nav_hint = i18n.t("nav-archive-hint");
    let editor_nav = i18n.t("nav-editor");
    let editor_nav_hint = i18n.t("nav-editor-hint");
    let page_title = match page() {
        WorkspacePage::Browse => i18n.t("page-archive-manager"),
        WorkspacePage::Editor => i18n.t("page-archive-editor"),
    };
    let search_archive = i18n.t("search-archive");
    let search_placeholder = i18n.t("search-placeholder");
    let clear_search = i18n.t("clear-search");
    let open_settings = i18n.t("open-settings");
    let settings_label = i18n.t("settings");
    let (drop_active_title, drop_active_hint) = if page() == WorkspacePage::Browse {
        (i18n.t("drop-open-title"), i18n.t("drop-open-hint"))
    } else {
        (i18n.t("drop-active-title"), i18n.t("drop-active-hint"))
    };
    let archive_label = match page() {
        WorkspacePage::Browse => archive
            .read()
            .as_ref()
            .map(|value| value.name.clone())
            .unwrap_or_else(|| i18n.t("no-archive-open")),
        WorkspacePage::Editor => archive_name(),
    };

    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        document::Title { "Dzip Archive" }

        div {
            class: "{theme_class}",
            ondragover: move |event| {
                if file_drop::drag_has_files(&event) {
                    event.prevent_default();
                    dragging_files.set(true);
                }
            },
            ondragleave: move |_| dragging_files.set(false),
            ondrop: move |event| {
                dragging_files.set(false);
                if !file_drop::drag_has_files(&event) {
                    return;
                }
                event.prevent_default();
                if page() == WorkspacePage::Editor {
                    let default_compression = compression();
                    spawn(async move {
                        match add_dropped_files(event, draft_files, next_id, default_compression).await {
                            Ok(0) => {
                                let message = i18n.t("drop-no-files");
                                append_log(logs, "WARN", &message);
                                toast.set(Some((false, message)));
                            }
                            Ok(count) => {
                                let message = i18n.t_args(
                                    "dropped-files",
                                    &[("count", count.to_string())],
                                );
                                append_log(logs, "INFO", &message);
                                toast.set(Some((true, message)));
                            }
                            Err(error) => {
                                let message = i18n.t_args("drop-error", &[("error", error)]);
                                append_log(logs, "ERROR", &message);
                                toast.set(Some((false, message)));
                            }
                        }
                    });
                } else {
                    let mut busy = busy;
                    busy.set(Some(i18n.t("reading-archive")));
                    spawn(async move {
                        match open_dropped_archive(event).await {
                            Ok((main_name, value)) => {
                                focused_entry.set(None);
                                replace_loaded_archive(archive, value).await;
                                selected.write().clear();
                                search.set(String::new());
                                browse_path.set(String::new());
                                let message = i18n.t_args(
                                    "opened-archive",
                                    &[("name", main_name)],
                                );
                                append_log(logs, "INFO", &message);
                                toast.set(Some((true, message)));
                            }
                            Err(DroppedArchiveError::NoMainArchive) => {
                                let message = i18n.t("drop-archive-no-main");
                                append_log(logs, "WARN", &message);
                                toast.set(Some((false, message)));
                            }
                            Err(DroppedArchiveError::MultipleMainArchives) => {
                                let message = i18n.t("drop-archive-multiple-main");
                                append_log(logs, "WARN", &message);
                                toast.set(Some((false, message)));
                            }
                            Err(DroppedArchiveError::Read(error) | DroppedArchiveError::Open(error)) => {
                                let message = i18n.t_args("drop-open-error", &[("error", error)]);
                                append_log(logs, "ERROR", &message);
                                toast.set(Some((false, message)));
                            }
                        }
                        busy.set(None);
                    });
                }
            },
            div { class: "ambient ambient-one" }
            div { class: "ambient ambient-two" }
            div { class: "ambient ambient-three" }

            if accepting_drop {
                div { class: "file-drop-indicator", aria_hidden: "true",
                    span { class: "file-drop-indicator-icon",
                        Icon { name: IconName::CloudUpload, size: 17 }
                    }
                    div {
                        strong { "{drop_active_title}" }
                        span { "{drop_active_hint}" }
                    }
                }
            }

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
                        active: page() == WorkspacePage::Editor,
                        icon: IconName::PackagePlus,
                        label: editor_nav,
                        hint: editor_nav_hint,
                        onclick: move |_| page.set(WorkspacePage::Editor),
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
                                state: workspace,
                                on_new: move |_| {
                                    reset_archive_editor(
                                        draft_files,
                                        compression,
                                        archive_name,
                                        alignment,
                                        random_access,
                                        dz_options,
                                        editor_mode,
                                    );
                                    page.set(WorkspacePage::Editor);
                                },
                            }
                        },
                        WorkspacePage::Editor => rsx! {
                            ArchiveEditorPage {
                                state: workspace,
                                on_new: move |_| {
                                    reset_archive_editor(
                                        draft_files,
                                        compression,
                                        archive_name,
                                        alignment,
                                        random_access,
                                        dz_options,
                                        editor_mode,
                                    );
                                },
                                on_saved: move |value: LoadedArchive| async move {
                                    focused_entry.set(None);
                                    replace_loaded_archive(archive, value).await;
                                    selected.write().clear();
                                    search.set(String::new());
                                    browse_path.set(String::new());
                                    reset_archive_editor(
                                        draft_files,
                                        compression,
                                        archive_name,
                                        alignment,
                                        random_access,
                                        dz_options,
                                        editor_mode,
                                    );
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
    mut locale: Signal<Locale>,
    logs: Signal<Vec<String>>,
) -> Element {
    let mut language_open = use_signal(|| false);
    let mut logs_open = use_signal(|| false);
    let language_open_now = language_open();
    let current_locale = locale();
    let i18n = I18n::new(current_locale);
    let language_options = i18n::language_options();
    let current_language_label = language_options
        .iter()
        .find(|option| option.locale == current_locale)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| current_locale.code().to_string());
    let version = env!("CARGO_PKG_VERSION");
    let license = env!("CARGO_PKG_LICENSE");
    let settings_title = i18n.t("settings");
    let settings_subtitle = i18n.t("settings-subtitle");
    let close_settings = i18n.t("close-settings");
    let appearance_heading = i18n.t("appearance-mode");
    let system_label = i18n.t("appearance-system");
    let light_label = i18n.t("appearance-light");
    let dark_label = i18n.t("appearance-dark");
    let language_heading = i18n.t("interface-language");
    let logs_heading = i18n.t("logs");
    let view_logs = i18n.t("view-application-logs");
    let about_heading = i18n.t("about");
    let version_label = i18n.t("version");
    let license_label = i18n.t("license");
    let author_label = i18n.t("author");

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
                                aria_label: language_heading.clone(),
                                aria_haspopup: "listbox",
                                aria_expanded: if language_open_now { "true" } else { "false" },
                                onclick: move |_| language_open.set(!language_open_now),
                                span { "{current_language_label}" }
                                span { class: "settings-language-caret", Icon { name: IconName::ChevronDown, size: 16 } }
                            }
                            div {
                                class: "settings-language-menu",
                                role: "listbox",
                                aria_label: language_heading,
                                aria_hidden: if language_open_now { "false" } else { "true" },
                                for choice in language_options {
                                    {
                                        let choice_locale = choice.locale;
                                        let choice_label = choice.label;
                                        let log_label = choice_label.clone();
                                        let active = choice_locale == current_locale;
                                        rsx! {
                                            button {
                                                class: if active { "active" } else { "" },
                                                r#type: "button",
                                                role: "option",
                                                tabindex: if language_open_now { "0" } else { "-1" },
                                                aria_selected: if active { "true" } else { "false" },
                                                onclick: move |_| {
                                                    language_open.set(false);
                                                    if let Err(error) = preferences::save_locale(choice_locale.code()) {
                                                        append_log(logs, "ERROR", error);
                                                    }
                                                    locale.set(choice_locale);
                                                    append_log(logs, "INFO", format!("Interface language: {log_label}"));
                                                },
                                                span { "{choice_label}" }
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
                                dt { "{license_label}" }
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
    locale: Signal<Locale>,
) -> Element {
    let current_locale = locale();
    let i18n = I18n::new(current_locale);
    let log_text = logs.read().join("\n");
    let logs_title = i18n.t("application-logs");
    let logs_subtitle = i18n.t("logs-subtitle");
    let close_logs = i18n.t("close-logs");
    let empty_logs = i18n.t("no-log-entries");
    let log_content = i18n.t("log-content");
    let clear_label = i18n.t("clear");
    let export_label = i18n.t("export-log");

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

#[component]
fn BrowsePage(state: WorkspaceState, on_new: EventHandler<MouseEvent>) -> Element {
    let WorkspaceState {
        mut archive,
        mut selected,
        mut focused_entry,
        mut search,
        mut browse_path,
        mut busy,
        mut toast,
        mut next_id,
        mut draft_files,
        mut compression,
        mut archive_name,
        mut alignment,
        mut random_access,
        mut dz_options,
        mut editor_mode,
        logs,
        mut page,
    } = state;
    let locale = use_context::<Signal<Locale>>();
    let current_locale = locale();
    let i18n = I18n::new(current_locale);
    let empty_title = i18n.t("empty-open-title");
    let empty_description = i18n.t("empty-open-description");
    let select_archive = i18n.t("choose-dz-files");
    let create_archive = i18n.t("create-new-archive");
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
                            multiple: true,
                            onchange: move |event| {
                                let files = event.files();
                                async move {
                                    if files.is_empty() { return; }
                                    let archive = archive;
                                    let mut focused_entry = focused_entry;
                                    let mut selected = selected;
                                    let mut search = search;
                                    let mut browse_path = browse_path;
                                    let mut busy = busy;
                                    let mut toast = toast;
                                    busy.set(Some(i18n.t("reading-archive")));
                                    let mut loaded = Vec::new();
                                    for file in files {
                                        let name = file.name();
                                        match file.read_bytes().await {
                                            Ok(bytes) => loaded.push((name, bytes.to_vec())),
                                            Err(error) => {
                                                let message = i18n.t_args("read-file-failed", &[
                                                    ("name", name.clone()),
                                                    ("error", error.to_string()),
                                                ]);
                                                append_log(logs, "ERROR", &message);
                                                toast.set(Some((false, message.clone())));
                                                busy.set(None);
                                                return;
                                            }
                                        }
                                    }
                                    let (main_name, main_bytes, auxiliary) =
                                        match prepare_archive_files(loaded) {
                                            Ok(prepared) => prepared,
                                            Err(DroppedArchiveError::NoMainArchive) => {
                                                let message = i18n.t("drop-archive-no-main");
                                                append_log(logs, "WARN", &message);
                                                toast.set(Some((false, message)));
                                                busy.set(None);
                                                return;
                                            }
                                            Err(DroppedArchiveError::MultipleMainArchives) => {
                                                let message = i18n.t("drop-archive-multiple-main");
                                                append_log(logs, "WARN", &message);
                                                toast.set(Some((false, message)));
                                                busy.set(None);
                                                return;
                                            }
                                            Err(DroppedArchiveError::Read(error)
                                                | DroppedArchiveError::Open(error)) => {
                                                let message = i18n.t_args(
                                                    "drop-open-error",
                                                    &[("error", error)],
                                                );
                                                append_log(logs, "ERROR", &message);
                                                toast.set(Some((false, message)));
                                                busy.set(None);
                                                return;
                                            }
                                        };
                                    match open_archive(main_name.clone(), main_bytes, auxiliary).await {
                                        Ok(value) => {
                                            focused_entry.set(None);
                                            replace_loaded_archive(archive, value).await;
                                            selected.write().clear();
                                            search.set(String::new());
                                            browse_path.set(String::new());
                                            let message = i18n.t_args(
                                                "opened-archive",
                                                &[("name", main_name.clone())],
                                            );
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
                    button { class: "button secondary large", onclick: on_new,
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
    let breadcrumbs = browser_breadcrumbs(&current_dir);
    let parent_dir = parent_browser_directory(&current_dir);
    let selected_count = selected.read().len();
    let selected_bytes: u64 = archive_value
        .entries
        .iter()
        .filter(|entry| selected.read().contains(&entry.id))
        .map(|entry| entry.size)
        .sum();
    let selectable_entry_ids = if query.is_empty() {
        entry_ids_in_directory(archive_value.entries.as_ref(), &current_dir)
    } else {
        visible.iter().map(|entry| entry.id).collect()
    };
    let all_visible_selected = !selectable_entry_ids.is_empty()
        && selectable_entry_ids
            .iter()
            .all(|id| selected.read().contains(id));
    let focused = focused_entry()
        .and_then(|id| archive_value.entries.iter().find(|entry| entry.id == id))
        .cloned();
    let archive_for_selected = archive_value.clone();
    let archive_for_all = archive_value.clone();
    let archive_for_edit = archive_value.clone();
    let selected_export_base = current_dir.clone();
    let overview_description = i18n.t("archive-overview-description");
    let overview_eyebrow = i18n.t("archive-overview");
    let edit_archive = i18n.t("edit-archive");
    let close_archive = i18n.t("close-archive");
    let extract_selected = i18n.t("extract-selected");
    let extract_all = i18n.t("extract-all");
    let files_label = i18n.t("files");
    let archive_size_label = i18n.t("archive-size");
    let unpacked_label = i18n.t("unpacked");
    let ratio_label = i18n.t("compression-ratio");
    let original_total = i18n.t("original-total");
    let ratio_note = i18n.t("ratio-note");
    let archive_contents = i18n.t("archive-contents");
    let clear_label = i18n.t("clear");
    let parent_folder = i18n.t("go-parent-folder");
    let parent_label = i18n.t("go-back");
    let archive_path_label = i18n.t("archive-path");
    let root_label = i18n.t("root");
    let search_results = i18n.t("search-results");
    let select_visible = i18n.t("select-visible");
    let name_label = i18n.t("name");
    let original_size = i18n.t("original-size");
    let packed_size = i18n.t("packed-size");
    let algorithm_label = i18n.t("algorithm");
    let volume_label = i18n.t("volume");
    let empty_folder = i18n.t("empty-folder");
    let no_matches = i18n.t("no-matches");
    let empty_folder_hint = i18n.t("empty-folder-hint");
    let no_matches_hint = i18n.t("no-matches-hint");
    let browse_hint = i18n.t("browse-hint");
    let searching_hint = i18n.t("searching-archive");
    let choose_entry = i18n.t("select-entry");
    let details_hint = i18n.t("details-hint");
    let chunk_note = i18n.t_args(
        "chunk-count",
        &[("count", archive_value.chunk_count.to_string())],
    );
    let volume_note = i18n.t_args(
        "volume-count",
        &[("count", archive_value.volume_count.to_string())],
    );
    let source_size_value = if archive_value.source_complete {
        human_size(archive_value.source_size)
    } else {
        format!(
            "{} ({}/{})",
            human_size(archive_value.source_size),
            archive_value.loaded_volume_count,
            archive_value.volume_count
        )
    };
    let ratio_value = if archive_value.source_complete {
        format!(
            "{}%",
            ratio_percent(archive_value.source_size, archive_value.unpacked_size)
        )
    } else {
        i18n.t("pending-generation")
    };
    let contents_summary = i18n.t_args(
        if query.is_empty() {
            "contents-summary"
        } else {
            "contents-search-summary"
        },
        &[
            ("folders", folders.len().to_string()),
            ("files", visible.len().to_string()),
        ],
    );
    let selected_summary =
        i18n.t_args("selected-summary", &[("count", selected_count.to_string())]);
    let directory_summary = i18n.t_args(
        "directory-summary",
        &[
            ("folders", folders.len().to_string()),
            ("files", visible.len().to_string()),
        ],
    );
    let search_summary = i18n.t_args(
        "search-summary",
        &[
            ("visible", visible.len().to_string()),
            ("total", archive_value.entries.len().to_string()),
        ],
    );
    let archive_name_for_close = archive_value.name.clone();
    let archive_session_for_close = archive_value.session_id;

    rsx! {
        div { class: "archive-heading",
            div {
                span { class: "eyebrow accent", "{overview_eyebrow}" }
                h1 { "{archive_value.name}" }
                p { "{overview_description}" }
            }
            div { class: "heading-actions",
                button {
                    class: "button secondary",
                    onclick: move |_| {
                        let archive_value = archive_for_edit.clone();
                        async move {
                            let preparing = i18n.t_args(
                                "preparing-edit",
                                &[("name", archive_value.name.clone())],
                            );
                            busy.set(Some(preparing));
                            let ids: Vec<usize> = archive_value.entries.iter().map(|entry| entry.id).collect();
                            match editable_entries(&archive_value, &ids).await {
                                Ok(files) => {
                                    let method = archive_value
                                        .entries
                                        .first()
                                        .map(|entry| entry.compression)
                                        .unwrap_or(CompressionChoice::Dz);
                                    let mut drafts = Vec::with_capacity(files.len());
                                    for file in files {
                                        let id = next_id();
                                        next_id.set(id.saturating_add(1));
                                        let entry_compression = file
                                            .segments
                                            .first()
                                            .map(|segment| segment.encoding.compression)
                                            .unwrap_or(method);
                                        let entry_volume = file
                                            .segments
                                            .first()
                                            .map(|segment| segment.volume)
                                            .unwrap_or(0);
                                        drafts.push(DraftFile {
                                            id,
                                            path: file.path,
                                            bytes: Arc::from(file.bytes),
                                            compression: entry_compression,
                                            volume: entry_volume,
                                            preserved_segments: Some(file.segments),
                                        });
                                    }
                                    let source_name = archive_value.name.clone();
                                    draft_files.set(drafts);
                                    compression.set(method);
                                    archive_name.set(source_name.clone());
                                    alignment.set(0);
                                    random_access.set(false);
                                    dz_options.set(archive_value.dz_options);
                                    editor_mode.set(ArchiveEditorMode::Existing {
                                        source_name: source_name.clone(),
                                    });
                                    page.set(WorkspacePage::Editor);
                                    let message = i18n.t_args(
                                        "loaded-edit",
                                        &[("name", source_name)],
                                    );
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
                        let export_base = selected_export_base.clone();
                        async move {
                            extract_and_export(
                                archive_value,
                                ids,
                                Some(export_base),
                                busy,
                                toast,
                                logs,
                                current_locale,
                            ).await;
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
                            extract_and_export(
                                archive_value,
                                ids,
                                None,
                                busy,
                                toast,
                                logs,
                                current_locale,
                            ).await;
                        }
                    },
                    Icon { name: IconName::ArchiveRestore, size: 17 }
                    "{extract_all}"
                }
                button {
                    class: "button secondary close-archive-button",
                    r#type: "button",
                    aria_label: close_archive.clone(),
                    title: close_archive.clone(),
                    onclick: move |_| {
                        let archive_name_for_close = archive_name_for_close.clone();
                        async move {
                            let _ = close_archive_session(archive_session_for_close).await;
                            archive.set(None);
                            selected.write().clear();
                            focused_entry.set(None);
                            search.set(String::new());
                            browse_path.set(String::new());
                            let message = i18n.t_args(
                                "closed-archive",
                                &[("name", archive_name_for_close)],
                            );
                            append_log(logs, "INFO", &message);
                            toast.set(Some((true, message)));
                        }
                    },
                    Icon { name: IconName::X, size: 17 }
                    span { "{close_archive}" }
                }
            }
        }

        div { class: "stats-grid",
            StatCard { icon: IconName::Files, label: files_label, value: archive_value.entries.len().to_string(), note: chunk_note, tone: "mint" }
            StatCard { icon: IconName::HardDrive, label: archive_size_label, value: source_size_value, note: volume_note, tone: "blue" }
            StatCard { icon: IconName::Expand, label: unpacked_label, value: human_size(archive_value.unpacked_size), note: original_total.to_string(), tone: "pink" }
            StatCard { icon: IconName::Gauge, label: ratio_label, value: ratio_value, note: ratio_note.to_string(), tone: "amber" }
        }

        FileWorkspaceLayout { onclick: move |_| {},
            FileBrowserPanel {
                icon: IconName::ListTree,
                title: archive_contents,
                summary: contents_summary,
                menu_open: false,
                header_actions: rsx! {
                    if selected_count > 0 {
                        div { class: "selection-summary",
                            span { "{selected_summary}" }
                            strong { "{human_size(selected_bytes)}" }
                            button { onclick: move |_| selected.write().clear(), "{clear_label}" }
                        }
                    }
                },
                BrowserBar {
                    current_dir: current_dir.clone(),
                    breadcrumbs,
                    root_label,
                    root_override: if query.is_empty() { None } else { Some(search_results) },
                    path_label: archive_path_label,
                    back_aria_label: parent_folder,
                    back_title: parent_label,
                    back_disabled: current_dir.is_empty() || !query.is_empty(),
                    navigation_disabled: !query.is_empty(),
                    on_back: {
                        let parent_dir = parent_dir.clone();
                        move |_| {
                            browse_path.set(parent_dir.clone());
                            focused_entry.set(None);
                        }
                    },
                    on_navigate: move |target: String| {
                        browse_path.set(target);
                        focused_entry.set(None);
                    },
                }

                div { class: "file-table",
                    FileTableHeader {
                        leading: rsx! {
                            label { class: "check-wrap",
                            input {
                                r#type: "checkbox",
                                checked: all_visible_selected,
                                disabled: selectable_entry_ids.is_empty(),
                                aria_label: select_visible,
                                onchange: move |_| {
                                    let ids = selectable_entry_ids.clone();
                                    let mut selected_set = selected.write();
                                    if all_visible_selected {
                                        for id in ids { selected_set.remove(&id); }
                                    } else {
                                        selected_set.extend(ids);
                                    }
                                }
                            }
                            }
                        },
                        name: name_label,
                        original: original_size,
                        packed: packed_size,
                        algorithm: algorithm_label,
                        volume: volume_label,
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
                            selected: selected,
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

            ContextPanel { class_name: "inspector", menu_open: false,
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
fn ArchiveEditorPage(
    state: WorkspaceState,
    on_new: EventHandler<MouseEvent>,
    on_saved: EventHandler<LoadedArchive>,
) -> Element {
    let WorkspaceState {
        mut draft_files,
        mut compression,
        mut archive_name,
        alignment,
        mut random_access,
        mut dz_options,
        mut busy,
        mut toast,
        next_id,
        editor_mode,
        logs,
        ..
    } = state;
    let locale = use_context::<Signal<Locale>>();
    let current_locale = locale();
    let i18n = I18n::new(current_locale);
    let mut open_select_menu = use_signal(|| None::<OpenSelectMenu>);
    let mut dz_advanced_open = use_signal(|| false);
    let mut draft_browse_path = use_signal(String::new);
    let total_size: u64 = draft_files
        .read()
        .iter()
        .map(|file| file.bytes.len() as u64)
        .sum();
    let draft_current_dir = draft_browse_path();
    let (draft_folders, visible_draft_files) =
        build_draft_browser_listing(&draft_files.read(), &draft_current_dir);
    let draft_breadcrumbs = browser_breadcrumbs(&draft_current_dir);
    let draft_parent_dir = parent_browser_directory(&draft_current_dir);
    let current_compression = compression();
    let current_dz_options = dz_options();
    let has_dz_entries = draft_files
        .read()
        .iter()
        .any(|file| file.compression == CompressionChoice::Dz)
        || (draft_files.read().is_empty() && current_compression == CompressionChoice::Dz);
    let current_mode = editor_mode();
    let editing_name = current_mode.source_name().map(str::to_owned);
    let is_editing = editing_name.is_some();
    let page_title = if let Some(source) = editing_name.as_ref() {
        i18n.t_args("edit-title", &[("name", source.clone())])
    } else {
        i18n.t("create-title")
    };
    let editing_label = editing_name
        .as_ref()
        .map(|source| i18n.t_args("editing-source", &[("name", source.clone())]));
    let page_description = if is_editing {
        i18n.t("edit-description")
    } else {
        i18n.t("create-description")
    };
    let save_button = if is_editing {
        i18n.t("save-archive")
    } else {
        i18n.t("create-and-save")
    };
    let create_archive = i18n.t("create-new-archive");
    let edit_notice = i18n.t("edit-notice");
    let draft_heading = i18n.t("draft-files");
    let draft_summary = i18n.t_args(
        "draft-summary",
        &[
            ("count", draft_files.read().len().to_string()),
            ("size", human_size(total_size)),
        ],
    );
    let clear_label = i18n.t("clear");
    let drop_title = i18n.t("drop-files");
    let drop_hint = i18n.t("drop-files-hint");
    let privacy_hint = i18n.t("privacy-hint");
    let archive_settings = i18n.t("archive-settings");
    let per_file_hint = i18n.t("per-file-hint");
    let archive_name_label = i18n.t("archive-name");
    let parent_folder = i18n.t("go-parent-folder");
    let parent_label = i18n.t("go-back");
    let archive_path_label = i18n.t("archive-path");
    let root_label = i18n.t("root");
    let name_label = i18n.t("name");
    let original_size = i18n.t("original-size");
    let packed_size = i18n.t("packed-size");
    let algorithm_label = i18n.t("algorithm");
    let volume_label = i18n.t("volume");
    let empty_folder = i18n.t("empty-folder");
    let empty_folder_hint = i18n.t("empty-folder-hint");
    let browse_hint = i18n.t("draft-browse-hint");
    let draft_directory_summary = i18n.t_args(
        "directory-summary",
        &[
            ("folders", draft_folders.len().to_string()),
            ("files", visible_draft_files.len().to_string()),
        ],
    );
    let default_algorithm = i18n.t("default-algorithm");
    let apply_all = i18n.t("apply-all");
    let alignment_label = i18n.t("data-alignment");
    let random_access_title = i18n.t("random-access");
    let random_access_description = i18n.t("random-access-description");
    let combuf_title = i18n.t("common-buffer");
    let combuf_description = i18n.t("common-buffer-description");
    let dz_advanced_title = i18n.t("dz-advanced");
    let dz_advanced_description = i18n.t("dz-advanced-description");
    let reset_defaults = i18n.t("reset-defaults");
    let analysis_parameters = i18n.t("encoding-references");
    let range_parameters = i18n.t("range-model");
    let preprocess_title = i18n.t("preprocess-analysis");
    let preprocess_description = i18n.t("preprocess-description");
    let static_tables_title = i18n.t("combuf-static-tables");
    let static_tables_description = i18n.t("combuf-static-description");
    let unlimited_hint = i18n.t("minus-one-unlimited");
    let zero_unlimited_hint = i18n.t("zero-unlimited");
    let combuf_only_hint = i18n.t("common-buffer-only");
    let model_value_hint = i18n.t("stored-archive-settings");
    let local_processing = i18n.t("local-processing");
    let local_processing_description = if cfg!(feature = "web") {
        i18n.t("local-processing-web")
    } else {
        i18n.t("local-processing-desktop")
    };
    let mode_eyebrow = i18n.t(if is_editing {
        "edit-archive-mode"
    } else {
        "new-archive"
    });

    rsx! {
        div { class: "archive-heading editor-heading",
            div {
                span { class: "eyebrow accent", "{mode_eyebrow}" }
                h1 { "{page_title}" }
                p { "{page_description}" }
            }
            div { class: "heading-actions",
                if is_editing {
                    button { class: "button secondary large", onclick: on_new,
                        Icon { name: IconName::PackagePlus, size: 18 }
                        "{create_archive}"
                    }
                }
                button {
                    class: "button primary large",
                    disabled: draft_files.read().is_empty(),
                    onclick: move |_| {
                        let files = draft_files.read().clone();
                        let name = normalize_archive_name(&archive_name());
                        let align = alignment();
                        let random = random_access();
                        let dz = dz_options();
                        async move {
                            busy.set(Some(i18n.t("compressing-files")));
                            match build_archive(
                                &files,
                                &name,
                                align,
                                random,
                                dz,
                            )
                            .await
                            {
                                Ok((volumes, reopened)) => {
                                    let volume_count = volumes.len();
                                    busy.set(Some(i18n.t("saving-archive")));
                                    match platform::save_archive_volumes(&name, volumes).await {
                                        Ok(_) => {
                                            let message = i18n.t_args(
                                                match (is_editing, volume_count > 1) {
                                                    (true, true) => "saved-split-archive",
                                                    (true, false) => "saved-archive",
                                                    (false, true) => "created-split-archive",
                                                    (false, false) => "created-archive",
                                                },
                                                &[
                                                    ("name", name.clone()),
                                                    ("count", volume_count.to_string()),
                                                ],
                                            );
                                            append_log(logs, "INFO", &message);
                                            toast.set(Some((true, message)));
                                            on_saved.call(reopened);
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
        }

        if editing_name.is_some() {
            div { class: "edit-mode-banner",
                div { class: "soft-icon", Icon { name: IconName::Pencil, size: 18 } }
                div {
                    strong { "{editing_label.as_deref().unwrap_or_default()}" }
                    span { "{edit_notice}" }
                }
            }
        }

        FileWorkspaceLayout { onclick: move |_| open_select_menu.set(None),
            FileBrowserPanel {
                icon: IconName::Files,
                title: draft_heading,
                summary: draft_summary,
                menu_open: matches!(open_select_menu(), Some(OpenSelectMenu::Compression(_) | OpenSelectMenu::Volume(_))),
                header_actions: rsx! {
                    div { class: "draft-actions",
                        if !draft_files.read().is_empty() {
                            button {
                                class: "text-button danger",
                                onclick: move |_| {
                                    draft_files.write().clear();
                                    draft_browse_path.set(String::new());
                                },
                                Icon { name: IconName::Trash, size: 15 }
                                "{clear_label}"
                            }
                        }
                        UploadButton {
                            draft_files: draft_files,
                            next_id: next_id,
                            default_compression: current_compression,
                            directory: false,
                            destination: draft_current_dir.clone(),
                        }
                        UploadButton {
                            draft_files: draft_files,
                            next_id: next_id,
                            default_compression: current_compression,
                            directory: true,
                            destination: draft_current_dir.clone(),
                        }
                    }
                },

                if draft_files.read().is_empty() {
                    label { class: "drop-zone",
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
                                let destination = draft_current_dir.clone();
                                async move {
                                    add_uploaded_files(
                                        files,
                                        draft_files,
                                        next_id,
                                        current_compression,
                                        false,
                                        &destination,
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                } else {
                    BrowserBar {
                        current_dir: draft_current_dir.clone(),
                        breadcrumbs: draft_breadcrumbs,
                        root_label,
                        root_override: None,
                        path_label: archive_path_label,
                        back_aria_label: parent_folder,
                        back_title: parent_label,
                        back_disabled: draft_current_dir.is_empty(),
                        navigation_disabled: false,
                        on_back: {
                            let parent = draft_parent_dir.clone();
                            move |_| draft_browse_path.set(parent.clone())
                        },
                        on_navigate: move |target: String| draft_browse_path.set(target),
                    }

                    div {
                        class: "file-table",
                        FileTableHeader {
                            leading: rsx! { div { class: "check-wrap" } },
                            name: name_label,
                            original: original_size,
                            packed: packed_size,
                            algorithm: algorithm_label,
                            volume: volume_label,
                        }
                        if draft_folders.is_empty() && visible_draft_files.is_empty() {
                            div { class: "no-results",
                                Icon { name: IconName::FolderOpen, size: 32 }
                                strong { "{empty_folder}" }
                                span { "{empty_folder_hint}" }
                            }
                        }
                        for folder in draft_folders.iter() {
                            DraftFolderRow {
                                key: "{folder.path}",
                                folder: folder.clone(),
                                browse_path: draft_browse_path,
                                draft_files,
                            }
                        }
                        for file in visible_draft_files.iter() {
                            DraftFileRow {
                                key: "{file.id}",
                                file: file.clone(),
                                draft_files,
                                open_menu: open_select_menu,
                            }
                        }
                    }
                    div { class: "table-footer",
                        span { "{draft_directory_summary}" }
                        span { class: "desktop-hint", Icon { name: IconName::MousePointer, size: 14 } "{browse_hint}" }
                    }
                }
            }

            ContextPanel {
                class_name: "settings-panel",
                menu_open: open_select_menu() == Some(OpenSelectMenu::Alignment),
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
                                    file.replace_compression(current_compression);
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
                            strong { "{compression_label(choice, i18n)}" }
                            small { "{compression_description(choice, i18n)}" }
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
                    onchange: move |value| {
                        random_access.set(value);
                        for file in draft_files.write().iter_mut() {
                            file.preserved_segments = None;
                        }
                    },
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
pub(crate) fn CompressionPicker(
    file_id: u64,
    file_path: String,
    value: CompressionChoice,
    mut draft_files: Signal<Vec<DraftFile>>,
    mut open_menu: Signal<Option<OpenSelectMenu>>,
) -> Element {
    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let is_open = open_menu() == Some(OpenSelectMenu::Compression(file_id));
    let menu_id = format!("compression-menu-{file_id}");
    let current_label = compression_label(value, i18n);
    let trigger_label = i18n.t_args(
        "set-compression-for",
        &[
            ("path", file_path.clone()),
            ("algorithm", current_label.clone()),
        ],
    );
    let menu_label = i18n.t_args("compression-menu-for", &[("path", file_path)]);

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
                        reveal_draft_menu(file_id);
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
                            title: compression_description(choice, i18n),
                            onclick: move |_| {
                                if let Some(item) = draft_files
                                    .write()
                                    .iter_mut()
                                    .find(|item| item.id == file_id)
                                {
                                    item.replace_compression(choice);
                                }
                                open_menu.set(None);
                            },
                            span { class: "compression-option-mark",
                                if choice == value {
                                    Icon { name: IconName::Check, size: 13 }
                                }
                            }
                            span { "{compression_label(choice, i18n)}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(crate) fn VolumePicker(
    file_id: u64,
    file_path: String,
    value: u16,
    mut draft_files: Signal<Vec<DraftFile>>,
    mut open_menu: Signal<Option<OpenSelectMenu>>,
) -> Element {
    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let is_open = open_menu() == Some(OpenSelectMenu::Volume(file_id));
    let menu_id = format!("volume-menu-{file_id}");
    let current_label = volume_option_label(value, i18n);
    let max_volume = draft_files
        .read()
        .iter()
        .map(|file| file.volume)
        .max()
        .unwrap_or(0);
    let trigger_label = i18n.t_args(
        "set-volume-for",
        &[
            ("path", file_path.clone()),
            ("volume", current_label.clone()),
        ],
    );
    let menu_label = i18n.t_args("volume-menu-for", &[("path", file_path)]);

    rsx! {
        div {
            class: if is_open { "compression-picker volume-picker open" } else { "compression-picker volume-picker" },
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
                        open_menu.set(Some(OpenSelectMenu::Volume(file_id)));
                        reveal_draft_menu(file_id);
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
                    class: "compression-menu volume-menu",
                    role: "listbox",
                    aria_label: menu_label,
                    for option in 0..=max_volume {
                        {
                            let option_label = volume_option_label(option, i18n);
                            let active = option == value;
                            rsx! {
                                button {
                                    class: if active { "compression-menu-option active" } else { "compression-menu-option" },
                                    r#type: "button",
                                    role: "option",
                                    aria_selected: if active { "true" } else { "false" },
                                    onclick: move |_| {
                                        if let Some(item) = draft_files
                                            .write()
                                            .iter_mut()
                                            .find(|item| item.id == file_id)
                                        {
                                            item.replace_volume(option);
                                        }
                                        open_menu.set(None);
                                    },
                                    span { class: "compression-option-mark",
                                        if active {
                                            Icon { name: IconName::Check, size: 13 }
                                        }
                                    }
                                    span { "{option_label}" }
                                }
                            }
                        }
                    }
                    if let Some(next_volume) = max_volume.checked_add(1) {
                        {
                            let new_volume_label = i18n.t_args(
                                "new-volume-option",
                                &[("number", next_volume.to_string())],
                            );
                            rsx! {
                                button {
                                    class: "compression-menu-option new-volume-option",
                                    r#type: "button",
                                    role: "option",
                                    aria_selected: "false",
                                    onclick: move |_| {
                                        if let Some(item) = draft_files
                                            .write()
                                            .iter_mut()
                                            .find(|item| item.id == file_id)
                                        {
                                            item.replace_volume(next_volume);
                                        }
                                        open_menu.set(None);
                                    },
                                    span { class: "compression-option-mark",
                                        Icon { name: IconName::Plus, size: 13 }
                                    }
                                    span { "{new_volume_label}" }
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
fn AlignmentPicker(
    mut alignment: Signal<u32>,
    mut open_menu: Signal<Option<OpenSelectMenu>>,
) -> Element {
    const OPTIONS: [u32; 4] = [0, 512, 2048, 4096];

    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let current_value = alignment();
    let current_label = alignment_option_label(current_value, i18n);
    let is_open = open_menu() == Some(OpenSelectMenu::Alignment);
    let aria_label = i18n.t("choose-alignment");

    rsx! {
        div {
            class: if is_open { "alignment-picker open" } else { "alignment-picker" },
            onclick: move |event| event.stop_propagation(),
            button {
                class: "alignment-trigger",
                r#type: "button",
                aria_label: aria_label.clone(),
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
                    aria_label: aria_label.clone(),
                    for option in OPTIONS {
                        {
                            let option_label = alignment_option_label(option, i18n);
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
    destination: String,
) -> Element {
    let locale = use_context::<Signal<Locale>>();
    let i18n = I18n::new(locale());
    let label = if directory {
        i18n.t("add-folder")
    } else {
        i18n.t("add-files")
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
                    let destination = destination.clone();
                    async move {
                        add_uploaded_files(
                            files,
                            draft_files,
                            next_id,
                            default_compression,
                            directory,
                            &destination,
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
    destination: &str,
) {
    let common_root = preserve_tree.then(|| common_upload_root(&files)).flatten();
    for file in files {
        if let Ok(bytes) = file.read_bytes().await {
            let id = next_id();
            next_id.set(id.saturating_add(1));
            let path = join_archive_path(destination, &upload_path(&file, common_root.as_deref()));
            draft_files.write().push(DraftFile {
                id,
                path,
                bytes: Arc::from(bytes.to_vec()),
                compression: default_compression,
                volume: 0,
                preserved_segments: None,
            });
        }
    }
}

async fn add_dropped_files(
    event: DragEvent,
    mut draft_files: Signal<Vec<DraftFile>>,
    mut next_id: Signal<u64>,
    default_compression: CompressionChoice,
) -> Result<usize, String> {
    let files = file_drop::read_dropped_files(&event).await?;
    let count = files.len();
    for file in files {
        let id = next_id();
        next_id.set(id.saturating_add(1));
        draft_files.write().push(DraftFile {
            id,
            path: file.path,
            bytes: Arc::from(file.bytes),
            compression: default_compression,
            volume: 0,
            preserved_segments: None,
        });
    }
    Ok(count)
}

async fn open_dropped_archive(
    event: DragEvent,
) -> Result<(String, LoadedArchive), DroppedArchiveError> {
    let files = file_drop::read_dropped_files(&event)
        .await
        .map_err(DroppedArchiveError::Read)?;
    let (main_name, main_bytes, auxiliary) = prepare_dropped_archive(files)?;
    let archive = open_archive(main_name.clone(), main_bytes, auxiliary)
        .await
        .map_err(DroppedArchiveError::Open)?;
    Ok((main_name, archive))
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
    export_base: Option<String>,
    mut busy: Signal<Option<String>>,
    mut toast: Signal<Option<(bool, String)>>,
    logs: Signal<Vec<String>>,
    locale: Locale,
) {
    if entry_ids.is_empty() {
        return;
    }
    let i18n = I18n::new(locale);
    let progress = i18n.t_args(
        "extracting-files",
        &[("count", entry_ids.len().to_string())],
    );
    busy.set(Some(progress));
    let result = match read_entries(&archive, &entry_ids).await {
        Ok(files) => {
            let files = rebase_extracted_files(files, export_base.as_deref());
            platform::export_files(&archive.name, files).await
        }
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

#[cfg(test)]
mod browser_tests {
    use super::*;
    use crate::browser::{remove_draft_folder, replace_draft_file_name};
    use dzip_gui::model::EntryView;

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
            packed_size: Some(size / 2),
            compression: CompressionChoice::Dz,
            volume: 0,
            chunks: 1,
        }
    }

    fn draft(id: u64, path: &str, size: usize) -> DraftFile {
        DraftFile {
            id,
            path: path.to_string(),
            bytes: Arc::from(vec![0u8; size]),
            compression: CompressionChoice::Dz,
            volume: 0,
            preserved_segments: None,
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
        assert_eq!(root_folders[0].packed_size, Some(50));
        assert_eq!(root_folders[0].volume, Some(0));
        assert_eq!(root_files[0].path, "readme.txt");

        let mut split_entries = entries.clone();
        split_entries[2].volume = 1;
        let (split_folders, _) = build_browser_listing(&split_entries, "", "");
        assert_eq!(split_folders[0].volume, None);

        let (data_folders, data_files) = build_browser_listing(&entries, "Data", "");
        assert_eq!(data_folders[0].path, "Data/Images");
        assert_eq!(data_files[0].path, "Data/config.json");

        let (search_folders, search_files) = build_browser_listing(&entries, "Data", "theme");
        assert!(search_folders.is_empty());
        assert_eq!(search_files[0].path, "Sounds/theme.ogg");

        assert_eq!(entry_ids_in_directory(&entries, ""), vec![0, 1, 2, 3]);
        assert_eq!(entry_ids_in_directory(&entries, "Data"), vec![1, 2]);
        assert_eq!(entry_ids_in_directory(&entries, "Data/Images"), vec![2]);
        assert!(entry_ids_in_directory(&entries, "Missing").is_empty());
    }

    #[test]
    fn draft_browser_listing_navigates_and_preserves_archive_paths() {
        let files = vec![
            draft(1, "readme.txt", 20),
            draft(2, "Data/config.json", 40),
            draft(3, "Data/Images/logo.png", 60),
            draft(4, "Sounds/theme.ogg", 80),
        ];

        let (root_folders, root_files) = build_draft_browser_listing(&files, "");
        assert_eq!(
            root_folders
                .iter()
                .map(|folder| folder.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Data", "Sounds"]
        );
        assert_eq!(root_folders[0].file_count, 2);
        assert_eq!(root_folders[0].size, 100);
        assert_eq!(root_folders[0].packed_size, None);
        assert_eq!(root_folders[0].volume, Some(0));
        assert_eq!(root_files[0].path, "readme.txt");

        let mut split_files = files.clone();
        split_files[2].volume = 1;
        let (split_folders, _) = build_draft_browser_listing(&split_files, "");
        assert_eq!(split_folders[0].volume, None);

        let (data_folders, data_files) = build_draft_browser_listing(&files, "Data");
        assert_eq!(data_folders[0].path, "Data/Images");
        assert_eq!(data_files[0].path, "Data/config.json");

        assert_eq!(
            browser_breadcrumbs("Data/Images")
                .iter()
                .map(|crumb| crumb.path.as_str())
                .collect::<Vec<_>>(),
            vec!["Data", "Data/Images"]
        );
        assert_eq!(parent_browser_directory("Data/Images"), "Data");
        assert_eq!(
            replace_draft_file_name("Data/config.json", "settings.json"),
            "Data/settings.json"
        );
        assert_eq!(
            join_archive_path("Data/Images", "icons/logo.png"),
            "Data/Images/icons/logo.png"
        );
    }

    #[test]
    fn removing_a_draft_folder_removes_all_descendants_only() {
        let mut files = vec![
            draft(1, "Data/config.json", 40),
            draft(2, "Data/Images/logo.png", 60),
            draft(3, "Database/index.bin", 20),
            draft(4, "readme.txt", 10),
        ];

        remove_draft_folder(&mut files, "Data");

        assert_eq!(
            files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["Database/index.bin", "readme.txt"]
        );
    }

    #[test]
    fn selected_extraction_removes_the_current_parent_path() {
        let files = vec![
            ("Data/Images/UI/icon.png".to_string(), vec![1]),
            ("Data/Images/UI/logo.png".to_string(), vec![2]),
        ];

        let rebased = rebase_extracted_files(files, Some("Data/Images"));

        assert_eq!(
            rebased
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>(),
            vec!["UI/icon.png", "UI/logo.png"]
        );
    }

    #[test]
    fn extraction_keeps_full_paths_for_root_or_mixed_selections() {
        let files = vec![
            ("Data/config.json".to_string(), vec![1]),
            ("Sounds/theme.ogg".to_string(), vec![2]),
        ];

        assert_eq!(rebase_extracted_files(files.clone(), None), files);
        assert_eq!(rebase_extracted_files(files.clone(), Some("Data")), files);
    }

    #[test]
    fn dropped_archive_finds_the_main_file_and_sorted_volumes() {
        let files = vec![
            file_drop::DroppedFile {
                path: "release/game.002".to_string(),
                bytes: vec![2],
            },
            file_drop::DroppedFile {
                path: "release/readme.txt".to_string(),
                bytes: vec![9],
            },
            file_drop::DroppedFile {
                path: "release/game.DZ".to_string(),
                bytes: vec![0],
            },
            file_drop::DroppedFile {
                path: "release/game.001".to_string(),
                bytes: vec![1],
            },
        ];

        let (main_name, main_bytes, auxiliary) = prepare_dropped_archive(files).unwrap();

        assert_eq!(main_name, "game.DZ");
        assert_eq!(main_bytes, vec![0]);
        assert_eq!(
            auxiliary,
            vec![
                ("game.001".to_string(), vec![1]),
                ("game.002".to_string(), vec![2]),
            ]
        );
    }

    #[test]
    fn dropped_archive_requires_exactly_one_main_file() {
        let volume_only = vec![file_drop::DroppedFile {
            path: "game.001".to_string(),
            bytes: vec![1],
        }];
        assert_eq!(
            prepare_dropped_archive(volume_only),
            Err(DroppedArchiveError::NoMainArchive)
        );

        let multiple = vec![
            file_drop::DroppedFile {
                path: "one.dz".to_string(),
                bytes: vec![1],
            },
            file_drop::DroppedFile {
                path: "two.dzip".to_string(),
                bytes: vec![2],
            },
        ];
        assert_eq!(
            prepare_dropped_archive(multiple),
            Err(DroppedArchiveError::MultipleMainArchives)
        );
    }
}
