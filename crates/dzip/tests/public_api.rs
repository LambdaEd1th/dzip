use dzip::volume::MemoryVolumeSource;
use dzip::{
    Archive, ArchiveBuilder, Codec, Compatibility, Compression, EntryOptions, ExtractOptions,
    MemoryVolumeSink, PackOptions,
};
use std::io::Cursor;

#[test]
fn codecs_are_distinct_from_storage_strategies() {
    assert_eq!(Compression::Copy.codec(), None);
    assert_eq!(Compression::Zero.codec(), None);
    assert_eq!(Compression::Bzip.codec(), Some(Codec::Bzip));
    assert_eq!(Compression::Zlib.codec(), Some(Codec::Zlib));
    assert_eq!(Compression::Lzma.codec(), Some(Codec::Lzma));
    assert_eq!(Compression::Dz.codec(), Some(Codec::Dz));
    assert_eq!("bzip2".parse(), Ok(Compression::Bzip));
    assert!("mp3".parse::<Compression>().is_err());
}
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn public_builder_and_archive_round_trip_all_codecs() {
    let mut builder = ArchiveBuilder::new();
    let fixtures = [
        ("copy.bin", Compression::Copy, b"copy payload".repeat(10)),
        ("bzip.bin", Compression::Bzip, b"bzip payload".repeat(100)),
        ("zlib.bin", Compression::Zlib, b"zlib payload".repeat(100)),
        ("lzma.bin", Compression::Lzma, b"lzma payload".repeat(100)),
        ("dz.bin", Compression::Dz, b"native dz payload".repeat(100)),
        ("zero.bin", Compression::Zero, vec![0; 1024]),
    ];
    for (path, codec, bytes) in &fixtures {
        builder
            .add_bytes(path, bytes.clone(), EntryOptions::new().compression(*codec))
            .unwrap();
    }

    let mut sink = MemoryVolumeSink::default();
    let report = builder.write_to_sink(&mut sink).unwrap();
    assert_eq!(report.entries, fixtures.len());
    let main = sink.into_volumes().remove(&0).unwrap();
    let mut archive =
        Archive::open_with_volumes(Cursor::new(main), MemoryVolumeSource::new([])).unwrap();

    for (path, _, expected) in fixtures {
        assert_eq!(archive.read_entry_by_path(path).unwrap(), expected);
    }
}

#[test]
fn split_volumes_and_segmented_entries_use_public_api() {
    let mut builder = ArchiveBuilder::with_options(PackOptions {
        volume_names: vec!["main.dz".to_string(), "main1.dz".to_string()],
        ..PackOptions::default()
    });
    builder
        .add_bytes(
            "joined.bin",
            b"first ".to_vec(),
            EntryOptions::new().compression(Compression::Copy).volume(1),
        )
        .unwrap()
        .add_bytes(
            "joined.bin",
            b"second".to_vec(),
            EntryOptions::new().compression(Compression::Copy).volume(1),
        )
        .unwrap();

    let mut sink = MemoryVolumeSink::default();
    builder.write_to_sink(&mut sink).unwrap();
    let mut volumes = sink.into_volumes();
    let main = volumes.remove(&0).unwrap();
    let auxiliary = volumes.remove(&1).unwrap();
    let mut archive =
        Archive::open_with_volumes(Cursor::new(main), MemoryVolumeSource::new([(1, auxiliary)]))
            .unwrap();
    assert_eq!(
        archive.read_entry_by_path("joined.bin").unwrap(),
        b"first second"
    );
}

#[test]
fn extraction_is_safe_and_strict_zero_mode_rejects_data_loss() {
    let mut unsafe_builder = ArchiveBuilder::new();
    assert!(
        unsafe_builder
            .add_bytes("../escape.bin", b"x".to_vec(), EntryOptions::new())
            .is_err()
    );

    let mut strict_builder = ArchiveBuilder::with_options(PackOptions {
        compatibility: Compatibility::Strict,
        ..PackOptions::default()
    });
    strict_builder
        .add_bytes(
            "not-zero.bin",
            b"x".to_vec(),
            EntryOptions::new().compression(Compression::Zero),
        )
        .unwrap();
    assert!(
        strict_builder
            .write_to_sink(&mut MemoryVolumeSink::default())
            .is_err()
    );

    let mut builder = ArchiveBuilder::new();
    builder
        .add_bytes(
            "safe/sub/file.bin",
            b"public extraction".to_vec(),
            EntryOptions::new().compression(Compression::Zlib),
        )
        .unwrap();
    let mut sink = MemoryVolumeSink::default();
    builder.write_to_sink(&mut sink).unwrap();
    let main = sink.into_volumes().remove(&0).unwrap();
    let mut archive =
        Archive::open_with_volumes(Cursor::new(main), MemoryVolumeSource::new([])).unwrap();

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let output =
        std::env::temp_dir().join(format!("dzip-public-api-{}-{unique}", std::process::id()));
    let report = archive
        .extract_to(&output, ExtractOptions::default())
        .unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(
        std::fs::read(output.join("safe/sub/file.bin")).unwrap(),
        b"public extraction"
    );
    std::fs::remove_dir_all(output).unwrap();
}
