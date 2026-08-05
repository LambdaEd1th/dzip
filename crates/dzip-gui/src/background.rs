use crate::task::run_archive_task as dispatch_archive_task;
use dzip::ArchivePathKey;
use dzip_gui::model::{DraftFile, DzCompressionOptions, LoadedArchive};
#[cfg(feature = "web")]
use dzip_workflow::ExtractedFile;
use dzip_workflow::{
    ArchiveTask, ArchiveTaskResponse, BuildPlan, EditableEntry, NamedBytes, SessionId,
};
use std::collections::HashSet;

async fn run_archive_task(task: ArchiveTask) -> Result<ArchiveTaskResponse, String> {
    dispatch_archive_task(task)
        .await
        .map_err(|failure| failure.message)
}

pub async fn open_archive(
    main_name: String,
    main_bytes: Vec<u8>,
    auxiliary_files: Vec<(String, Vec<u8>)>,
) -> Result<LoadedArchive, String> {
    let response = run_archive_task(ArchiveTask::Open {
        main_name,
        main_bytes,
        auxiliary_files: auxiliary_files
            .into_iter()
            .map(|(name, bytes)| NamedBytes { name, bytes })
            .collect(),
    })
    .await?;
    match response {
        ArchiveTaskResponse::Archive(archive) => Ok(LoadedArchive::from(archive)),
        _ => Err("archive backend returned an unexpected open response".to_string()),
    }
}

pub async fn read_entries(
    archive: &LoadedArchive,
    entry_ids: &[usize],
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let response = run_archive_task(ArchiveTask::ReadEntries {
        session_id: archive.session_id,
        entry_ids: entry_ids.to_vec(),
    })
    .await?;
    match response {
        ArchiveTaskResponse::Files(files) => Ok(files
            .into_iter()
            .map(|file| (file.path, file.bytes))
            .collect()),
        _ => Err("archive backend returned an unexpected extraction response".to_string()),
    }
}

pub async fn editable_entries(
    archive: &LoadedArchive,
    entry_ids: &[usize],
) -> Result<Vec<EditableEntry>, String> {
    let response = run_archive_task(ArchiveTask::EditEntries {
        session_id: archive.session_id,
        entry_ids: entry_ids.to_vec(),
    })
    .await?;
    match response {
        ArchiveTaskResponse::Editable(files) => Ok(files),
        _ => Err("archive backend returned an unexpected edit response".to_string()),
    }
}

#[allow(dead_code)]
pub async fn close_archive(session_id: SessionId) -> Result<(), String> {
    match run_archive_task(ArchiveTask::Close { session_id }).await? {
        ArchiveTaskResponse::Closed(_) => Ok(()),
        _ => Err("archive backend returned an unexpected close response".to_string()),
    }
}

pub async fn build_archive(
    files: &[DraftFile],
    archive_name: &str,
    alignment: u32,
    random_access: bool,
    dz_options: DzCompressionOptions,
) -> Result<(Vec<(String, Vec<u8>)>, LoadedArchive), String> {
    if files.is_empty() {
        return Err("add at least one file before creating an archive".to_string());
    }
    let mut paths = HashSet::with_capacity(files.len());
    for file in files {
        let path = file.path.trim();
        if path.is_empty() {
            return Err("archive entry paths cannot be empty".to_string());
        }
        let key = ArchivePathKey::from_archive_str(path);
        if !paths.insert(key) {
            return Err(format!("duplicate archive path: {}", file.path));
        }
    }
    let plan = BuildPlan {
        archive_name: archive_name.to_string(),
        volume_names: Vec::new(),
        alignment,
        dz_options,
        entries: files
            .iter()
            .flat_map(|file| file.build_entries(random_access))
            .collect(),
    };
    match run_archive_task(ArchiveTask::Build { plan }).await? {
        ArchiveTaskResponse::Built(built) => Ok((
            built
                .volumes
                .into_iter()
                .map(|volume| (volume.name, volume.bytes))
                .collect(),
            LoadedArchive::from(built.archive),
        )),
        _ => Err("archive backend returned an unexpected build response".to_string()),
    }
}

#[cfg(feature = "web")]
pub async fn make_store_zip(files: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, String> {
    let response = run_archive_task(ArchiveTask::MakeStoreZip {
        files: files
            .into_iter()
            .map(|(path, bytes)| ExtractedFile { path, bytes })
            .collect(),
    })
    .await?;
    match response {
        ArchiveTaskResponse::Bytes(bytes) => Ok(bytes),
        _ => Err("archive backend returned an unexpected ZIP response".to_string()),
    }
}
