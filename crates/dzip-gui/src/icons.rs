use dioxus::prelude::*;

pub(crate) fn icon_for_file(path: &str) -> IconName {
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
pub(crate) enum IconName {
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
pub(crate) fn Icon(name: IconName, #[props(default = 18)] size: u8) -> Element {
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
            width: "{size}", height: "{size}", view_box: "0 0 24 24", fill: "none",
            stroke: "currentColor", stroke_width: "{stroke_width}", stroke_linecap: "round",
            stroke_linejoin: "round", "aria-hidden": "true", {paths}
        }
    }
}
