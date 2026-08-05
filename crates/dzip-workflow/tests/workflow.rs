use dzip::{ChunkEncoding, Compression, ContentHint};
use dzip_workflow::{ArchiveService, BuildEntry, BuildPlan, DzConfig, NamedBytes};
use std::time::{SystemTime, UNIX_EPOCH};

fn encoding(compression: Compression) -> ChunkEncoding {
    ChunkEncoding {
        compression,
        random_access: false,
        common_buffer: false,
        content_hint: None,
        unknown_flags: 0,
    }
}

#[test]
fn segmented_split_volume_edit_round_trip_preserves_metadata() {
    let mut service = ArchiveService::default();
    let plan = BuildPlan {
        archive_name: "segmented.dz".to_string(),
        volume_names: Vec::new(),
        alignment: 512,
        dz_options: DzConfig::default(),
        entries: vec![
            BuildEntry {
                path: "Data/mixed.bin".to_string(),
                bytes: b"first segment".to_vec(),
                encoding: ChunkEncoding {
                    random_access: true,
                    content_hint: Some(ContentHint::Jpeg),
                    ..encoding(Compression::Copy)
                },
                raw_flags: None,
                volume: 1,
            },
            BuildEntry {
                path: "data/MIXED.bin".to_string(),
                bytes: b" second segment".to_vec(),
                encoding: encoding(Compression::Lzma),
                raw_flags: None,
                volume: 2,
            },
        ],
    };

    let built = service.build(plan).unwrap();
    assert_eq!(built.volumes.len(), 3);
    assert_eq!(built.archive.summary.entries.len(), 1);
    let summary = &built.archive.summary.entries[0];
    assert_eq!(summary.chunks, 2);
    assert_eq!(
        summary.segments[0].encoding.content_hint,
        Some(ContentHint::Jpeg)
    );
    assert!(summary.segments[0].encoding.random_access);
    assert_eq!(summary.segments[1].volume, 2);

    let editable = service
        .editable_entries(built.archive.session_id, &[summary.id])
        .unwrap();
    assert_eq!(editable[0].bytes, b"first segment second segment");
    assert_eq!(editable[0].segments.len(), 2);
    let extracted = service
        .read_entries(built.archive.session_id, &[summary.id])
        .unwrap();
    assert_eq!(extracted[0].bytes, editable[0].bytes);
}

#[test]
fn auxiliary_volumes_are_matched_by_stored_name_not_upload_order() {
    let mut builder = ArchiveService::default();
    let built = builder
        .build(BuildPlan {
            archive_name: "names.dz".to_string(),
            volume_names: Vec::new(),
            alignment: 0,
            dz_options: DzConfig::default(),
            entries: vec![
                BuildEntry {
                    path: "one.bin".to_string(),
                    bytes: vec![1; 32],
                    encoding: encoding(Compression::Copy),
                    raw_flags: None,
                    volume: 1,
                },
                BuildEntry {
                    path: "two.bin".to_string(),
                    bytes: vec![2; 32],
                    encoding: encoding(Compression::Copy),
                    raw_flags: None,
                    volume: 2,
                },
            ],
        })
        .unwrap();
    let mut volumes = built.volumes;
    let main = volumes.remove(0);
    volumes.reverse();

    let mut reader = ArchiveService::default();
    let handle = reader.open(main.name, main.bytes, volumes).unwrap();
    let files = reader.read_entries(handle.session_id, &[0, 1]).unwrap();
    assert_eq!(files[0].bytes, vec![1; 32]);
    assert_eq!(files[1].bytes, vec![2; 32]);
}

#[test]
fn split_archive_opens_before_missing_volumes_are_supplied() {
    let mut builder = ArchiveService::default();
    let built = builder
        .build(BuildPlan {
            archive_name: "lazy.dz".to_string(),
            volume_names: Vec::new(),
            alignment: 0,
            dz_options: DzConfig::default(),
            entries: vec![BuildEntry {
                path: "payload.bin".to_string(),
                bytes: b"lazy auxiliary payload".to_vec(),
                encoding: encoding(Compression::Copy),
                raw_flags: None,
                volume: 1,
            }],
        })
        .unwrap();
    let mut volumes = built.volumes;
    let main = volumes.remove(0);
    let auxiliary = volumes.remove(0);

    let mut reader = ArchiveService::default();
    let opened = reader.open(main.name, main.bytes, Vec::new()).unwrap();
    assert!(!opened.summary.source_complete);
    assert_eq!(opened.summary.loaded_volume_count, 1);
    assert_eq!(opened.summary.entries[0].packed_size, None);
    assert!(reader.read_entries(opened.session_id, &[0]).is_err());

    let supplied = reader
        .supply_volumes(opened.session_id, vec![auxiliary])
        .unwrap();
    assert!(supplied.summary.source_complete);
    assert_eq!(supplied.summary.loaded_volume_count, 2);
    let files = reader.read_entries(opened.session_id, &[0]).unwrap();
    assert_eq!(files[0].bytes, b"lazy auxiliary payload");
}

#[test]
fn damaged_input_and_unknown_sessions_are_rejected() {
    let mut service = ArchiveService::default();
    assert!(
        service
            .open(
                "bad.dz".to_string(),
                b"not a dzip archive".to_vec(),
                Vec::<NamedBytes>::new(),
            )
            .is_err()
    );
    assert!(
        service
            .read_entries(dzip_workflow::SessionId(99), &[0])
            .is_err()
    );
}

#[test]
fn filesystem_export_rejects_traversal_before_writing_outside_root() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let parent = std::env::temp_dir().join(format!(
        "dzip-workflow-export-{}-{unique}",
        std::process::id()
    ));
    let root = parent.join("output");
    let escaped = parent.join("escaped.bin");
    let files = [dzip_workflow::ExtractedFile {
        path: "../escaped.bin".to_string(),
        bytes: b"must not escape".to_vec(),
    }];

    assert!(dzip_workflow::write_extracted_files(&root, &files).is_err());
    assert!(!escaped.exists());
    if parent.exists() {
        std::fs::remove_dir_all(parent).unwrap();
    }
}
