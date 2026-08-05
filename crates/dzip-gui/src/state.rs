use dioxus::prelude::Signal;
use dzip_gui::model::{
    CompressionChoice, DraftFile, DzCompressionOptions, LoadedArchive, WorkspacePage,
};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenSelectMenu {
    Compression(u64),
    Volume(u64),
    Alignment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArchiveEditorMode {
    New,
    Existing { source_name: String },
}

impl ArchiveEditorMode {
    pub(crate) fn source_name(&self) -> Option<&str> {
        match self {
            Self::New => None,
            Self::Existing { source_name } => Some(source_name),
        }
    }

    pub(crate) fn is_new(&self) -> bool {
        matches!(self, Self::New)
    }
}

/// Signals shared by the archive browser and editor controllers.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct WorkspaceState {
    pub(crate) page: Signal<WorkspacePage>,
    pub(crate) archive: Signal<Option<LoadedArchive>>,
    pub(crate) selected: Signal<HashSet<usize>>,
    pub(crate) focused_entry: Signal<Option<usize>>,
    pub(crate) search: Signal<String>,
    pub(crate) browse_path: Signal<String>,
    pub(crate) draft_files: Signal<Vec<DraftFile>>,
    pub(crate) compression: Signal<CompressionChoice>,
    pub(crate) archive_name: Signal<String>,
    pub(crate) alignment: Signal<u32>,
    pub(crate) random_access: Signal<bool>,
    pub(crate) dz_options: Signal<DzCompressionOptions>,
    pub(crate) editor_mode: Signal<ArchiveEditorMode>,
    pub(crate) next_id: Signal<u64>,
    pub(crate) busy: Signal<Option<String>>,
    pub(crate) toast: Signal<Option<(bool, String)>>,
    pub(crate) logs: Signal<Vec<String>>,
}
