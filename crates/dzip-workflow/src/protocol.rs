//! Serializable transport used by both the desktop backend and Web Worker.

use crate::{
    ArchiveHandle, ArchiveService, BuildPlan, BuiltArchive, EditableEntry, ExtractedFile,
    NamedBytes, SessionId, WorkflowFailure,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveTask {
    Open {
        main_name: String,
        #[serde(with = "serde_bytes")]
        main_bytes: Vec<u8>,
        auxiliary_files: Vec<NamedBytes>,
    },
    SupplyVolumes {
        session_id: SessionId,
        files: Vec<NamedBytes>,
    },
    ReadEntries {
        session_id: SessionId,
        entry_ids: Vec<usize>,
    },
    EditEntries {
        session_id: SessionId,
        entry_ids: Vec<usize>,
    },
    Build {
        plan: BuildPlan,
    },
    Close {
        session_id: SessionId,
    },
    MakeStoreZip {
        files: Vec<ExtractedFile>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveTaskResponse {
    Archive(ArchiveHandle),
    Files(Vec<ExtractedFile>),
    Editable(Vec<EditableEntry>),
    Built(BuiltArchive),
    Closed(bool),
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
}

pub fn execute_archive_task(
    service: &mut ArchiveService,
    task: ArchiveTask,
) -> Result<ArchiveTaskResponse, WorkflowFailure> {
    match task {
        ArchiveTask::Open {
            main_name,
            main_bytes,
            auxiliary_files,
        } => service
            .open(main_name, main_bytes, auxiliary_files)
            .map(ArchiveTaskResponse::Archive),
        ArchiveTask::SupplyVolumes { session_id, files } => service
            .supply_volumes(session_id, files)
            .map(ArchiveTaskResponse::Archive),
        ArchiveTask::ReadEntries {
            session_id,
            entry_ids,
        } => service
            .read_entries(session_id, &entry_ids)
            .map(ArchiveTaskResponse::Files),
        ArchiveTask::EditEntries {
            session_id,
            entry_ids,
        } => service
            .editable_entries(session_id, &entry_ids)
            .map(ArchiveTaskResponse::Editable),
        ArchiveTask::Build { plan } => service.build(plan).map(ArchiveTaskResponse::Built),
        ArchiveTask::Close { session_id } => {
            Ok(ArchiveTaskResponse::Closed(service.close(session_id)))
        }
        ArchiveTask::MakeStoreZip { files } => {
            crate::make_store_zip(&files).map(ArchiveTaskResponse::Bytes)
        }
    }
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BuildEntry, DzConfig};
    use dzip::{ChunkEncoding, Compression};

    #[test]
    fn stateful_protocol_reuses_the_open_archive_session() {
        let mut service = ArchiveService::default();
        let plan = BuildPlan {
            archive_name: "worker.dz".to_string(),
            volume_names: Vec::new(),
            alignment: 0,
            dz_options: DzConfig::default(),
            entries: vec![BuildEntry {
                path: "Data/worker.txt".to_string(),
                bytes: b"worker payload".to_vec(),
                encoding: ChunkEncoding {
                    compression: Compression::Zlib,
                    random_access: false,
                    common_buffer: false,
                    content_hint: None,
                    unknown_flags: 0,
                },
                raw_flags: None,
                volume: 1,
            }],
        };
        let ArchiveTaskResponse::Built(built) =
            execute_archive_task(&mut service, ArchiveTask::Build { plan }).unwrap()
        else {
            panic!("unexpected worker response");
        };
        assert_eq!(built.volumes.len(), 2);
        let session_id = built.archive.session_id;
        assert_eq!(service.session_count(), 1);

        let ArchiveTaskResponse::Files(files) = execute_archive_task(
            &mut service,
            ArchiveTask::ReadEntries {
                session_id,
                entry_ids: vec![0],
            },
        )
        .unwrap() else {
            panic!("unexpected worker response");
        };
        assert_eq!(files[0].bytes, b"worker payload");
        assert!(matches!(
            execute_archive_task(&mut service, ArchiveTask::Close { session_id }).unwrap(),
            ArchiveTaskResponse::Closed(true)
        ));
    }
}
