use crate::task::run_archive_task;
use dzip_gui::model::{DraftFile, DzCompressionOptions, LoadedArchive};
#[cfg(feature = "web")]
use dzip_gui::worker_protocol::FileBytes;
use dzip_gui::worker_protocol::{
    ArchivePayload, ArchiveTask, ArchiveTaskResponse, DraftFilePayload, NamedBytes,
};

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
        _ => Err("后台任务返回了错误的归档响应".to_string()),
    }
}

pub async fn read_entries(
    archive: &LoadedArchive,
    entry_ids: &[usize],
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let response = run_archive_task(ArchiveTask::ReadEntries {
        archive: ArchivePayload::from(archive),
        entry_ids: entry_ids.to_vec(),
    })
    .await?;
    match response {
        ArchiveTaskResponse::Files(files) => Ok(files
            .into_iter()
            .map(|file| (file.path, file.bytes))
            .collect()),
        _ => Err("后台任务返回了错误的解压响应".to_string()),
    }
}

pub async fn build_archive(
    files: &[DraftFile],
    archive_name: &str,
    alignment: u32,
    random_access: bool,
    dz_options: DzCompressionOptions,
) -> Result<(Vec<(String, Vec<u8>)>, LoadedArchive), String> {
    let response = run_archive_task(ArchiveTask::Build {
        files: files.iter().map(DraftFilePayload::from).collect(),
        archive_name: archive_name.to_string(),
        alignment,
        random_access,
        dz_options,
    })
    .await?;
    match response {
        ArchiveTaskResponse::Built { volumes, archive } => Ok((
            volumes
                .into_iter()
                .map(|volume| (volume.name, volume.bytes))
                .collect(),
            LoadedArchive::from(archive),
        )),
        _ => Err("后台任务返回了错误的压缩响应".to_string()),
    }
}

#[cfg(feature = "web")]
pub async fn make_store_zip(files: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, String> {
    let response = run_archive_task(ArchiveTask::MakeStoreZip {
        files: files
            .into_iter()
            .map(|(path, bytes)| FileBytes { path, bytes })
            .collect(),
    })
    .await?;
    match response {
        ArchiveTaskResponse::Bytes(bytes) => Ok(bytes),
        _ => Err("后台任务返回了错误的 ZIP 响应".to_string()),
    }
}
