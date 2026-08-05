//! Frontend-independent Dzip application workflows.
//!
//! This crate owns typed build plans, archive summaries, in-memory sessions,
//! and export helpers. CLI, desktop, and web frontends should translate user
//! input once and then call this layer instead of reconstructing archive rules.

mod archive;
mod build;
#[cfg(feature = "dcl")]
mod dcl;
mod error;
mod filesystem;
mod model;
#[cfg(feature = "protocol")]
mod protocol;
mod service;
mod zip;

pub use archive::ArchiveSession;
pub use build::{
    archive_volume_names, build_archive, normalize_archive_name, write_archive_to_directory,
};
#[cfg(feature = "dcl")]
pub use dcl::build_plan_from_dcl;
pub use error::{Result, WorkflowError};
#[cfg(feature = "protocol")]
pub use error::{WorkflowErrorCode, WorkflowFailure};
pub use filesystem::{write_extracted_files, write_named_files};
pub use model::{
    ArchiveHandle, ArchiveSummary, BuildEntry, BuildPlan, BuiltArchive, DzConfig, EditableEntry,
    EditableSegment, EntrySummary, ExtractedFile, NamedBytes, SegmentSummary, SessionId,
};
#[cfg(feature = "protocol")]
pub use protocol::{ArchiveTask, ArchiveTaskResponse, execute_archive_task};
pub use service::ArchiveService;
pub use zip::make_store_zip;
