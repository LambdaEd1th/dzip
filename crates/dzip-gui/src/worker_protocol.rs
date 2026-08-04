use crate::archive_ops::{build_archive, make_store_zip, open_archive, read_entries};
use crate::model::{CompressionChoice, DraftFile, DzCompressionOptions, EntryView, LoadedArchive};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedBytes {
    pub name: String,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeBytes {
    pub id: u16,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBytes {
    pub path: String,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DraftFilePayload {
    pub id: u64,
    pub path: String,
    #[serde(with = "serde_bytes")]
    pub bytes: Vec<u8>,
    pub compression: CompressionChoice,
}

impl From<&DraftFile> for DraftFilePayload {
    fn from(file: &DraftFile) -> Self {
        Self {
            id: file.id,
            path: file.path.clone(),
            bytes: file.bytes.as_ref().to_vec(),
            compression: file.compression,
        }
    }
}

impl From<DraftFilePayload> for DraftFile {
    fn from(file: DraftFilePayload) -> Self {
        Self {
            id: file.id,
            path: file.path,
            bytes: Arc::from(file.bytes),
            compression: file.compression,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchivePayload {
    pub name: String,
    #[serde(with = "serde_bytes")]
    pub main_bytes: Vec<u8>,
    pub auxiliary: Vec<VolumeBytes>,
    pub entries: Vec<EntryView>,
    pub dz_options: DzCompressionOptions,
    pub source_size: u64,
    pub unpacked_size: u64,
    pub chunk_count: usize,
    pub volume_count: usize,
}

impl From<&LoadedArchive> for ArchivePayload {
    fn from(archive: &LoadedArchive) -> Self {
        Self {
            name: archive.name.clone(),
            main_bytes: archive.main_bytes.as_ref().to_vec(),
            auxiliary: archive
                .auxiliary
                .iter()
                .map(|(id, bytes)| VolumeBytes {
                    id: *id,
                    bytes: bytes.clone(),
                })
                .collect(),
            entries: archive.entries.as_ref().clone(),
            dz_options: archive.dz_options,
            source_size: archive.source_size,
            unpacked_size: archive.unpacked_size,
            chunk_count: archive.chunk_count,
            volume_count: archive.volume_count,
        }
    }
}

impl From<ArchivePayload> for LoadedArchive {
    fn from(archive: ArchivePayload) -> Self {
        Self {
            name: archive.name,
            main_bytes: Arc::from(archive.main_bytes),
            auxiliary: Arc::new(
                archive
                    .auxiliary
                    .into_iter()
                    .map(|volume| (volume.id, volume.bytes))
                    .collect(),
            ),
            entries: Arc::new(archive.entries),
            dz_options: archive.dz_options,
            source_size: archive.source_size,
            unpacked_size: archive.unpacked_size,
            chunk_count: archive.chunk_count,
            volume_count: archive.volume_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveTask {
    Open {
        main_name: String,
        #[serde(with = "serde_bytes")]
        main_bytes: Vec<u8>,
        auxiliary_files: Vec<NamedBytes>,
    },
    ReadEntries {
        archive: ArchivePayload,
        entry_ids: Vec<usize>,
    },
    Build {
        files: Vec<DraftFilePayload>,
        archive_name: String,
        alignment: u32,
        random_access: bool,
        dz_options: DzCompressionOptions,
    },
    MakeStoreZip {
        files: Vec<FileBytes>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveTaskResponse {
    Archive(ArchivePayload),
    Files(Vec<FileBytes>),
    Built {
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
        archive: ArchivePayload,
    },
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
}

pub fn execute_archive_task(task: ArchiveTask) -> Result<ArchiveTaskResponse, String> {
    match task {
        ArchiveTask::Open {
            main_name,
            main_bytes,
            auxiliary_files,
        } => open_archive(
            main_name,
            main_bytes,
            auxiliary_files
                .into_iter()
                .map(|file| (file.name, file.bytes))
                .collect(),
        )
        .map(|archive| ArchiveTaskResponse::Archive(ArchivePayload::from(&archive))),
        ArchiveTask::ReadEntries { archive, entry_ids } => {
            let archive = LoadedArchive::from(archive);
            read_entries(&archive, &entry_ids).map(|files| {
                ArchiveTaskResponse::Files(
                    files
                        .into_iter()
                        .map(|(path, bytes)| FileBytes { path, bytes })
                        .collect(),
                )
            })
        }
        ArchiveTask::Build {
            files,
            archive_name,
            alignment,
            random_access,
            dz_options,
        } => {
            let files: Vec<DraftFile> = files.into_iter().map(DraftFile::from).collect();
            let bytes = build_archive(&files, &archive_name, alignment, random_access, dz_options)?;
            let archive = open_archive(archive_name, bytes.clone(), Vec::new())?;
            Ok(ArchiveTaskResponse::Built {
                bytes,
                archive: ArchivePayload::from(&archive),
            })
        }
        ArchiveTask::MakeStoreZip { files } => {
            let files: Vec<(String, Vec<u8>)> = files
                .into_iter()
                .map(|file| (file.path, file.bytes))
                .collect();
            make_store_zip(&files).map(ArchiveTaskResponse::Bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_protocol_round_trips_archive_workflow() {
        let build = ArchiveTask::Build {
            files: vec![DraftFilePayload {
                id: 1,
                path: "Data/worker.txt".to_string(),
                bytes: b"worker payload".to_vec(),
                compression: CompressionChoice::Zlib,
            }],
            archive_name: "worker.dz".to_string(),
            alignment: 0,
            random_access: false,
            dz_options: DzCompressionOptions::default(),
        };
        let ArchiveTaskResponse::Built { bytes, archive } = execute_archive_task(build).unwrap()
        else {
            panic!("unexpected worker response");
        };
        assert!(!bytes.is_empty());

        let ArchiveTaskResponse::Files(files) = execute_archive_task(ArchiveTask::ReadEntries {
            archive,
            entry_ids: vec![0],
        })
        .unwrap() else {
            panic!("unexpected worker response");
        };
        assert_eq!(files[0].path, "Data/worker.txt");
        assert_eq!(files[0].bytes, b"worker payload");
    }
}
