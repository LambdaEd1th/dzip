use dzip::raw::{
    CHUNK_COPYCOMP, CHUNK_DZ, CHUNK_JPEG, CHUNK_MP3, CHUNK_RANDOMACCESS, CHUNK_ZERO, CHUNK_ZLIB,
};
use dzip::volume::MemoryVolumeSource;
use dzip::{
    Archive, ArchiveBuilder, Codec, Compression, EntryOptions, ExtractOptions, MemoryVolumeSink,
    PackOptions,
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
fn raw_flags_select_the_original_packer_coder() {
    let fixtures = [
        (
            "dz-mp3.bin",
            CHUNK_DZ | CHUNK_MP3,
            b"DZ data carrying an MP3 hint".repeat(100),
        ),
        (
            "zlib-jpeg.bin",
            CHUNK_ZLIB | CHUNK_JPEG | CHUNK_RANDOMACCESS,
            b"Zlib data carrying JPEG and random-access metadata".repeat(100),
        ),
        (
            "copy-jpeg.bin",
            CHUNK_COPYCOMP | CHUNK_JPEG | CHUNK_RANDOMACCESS,
            b"raw copy data".repeat(100),
        ),
    ];
    let mut builder = ArchiveBuilder::new();
    for (path, flags, data) in &fixtures {
        builder
            .add_bytes(
                path,
                data.clone(),
                EntryOptions::new()
                    .compression(Compression::Copy)
                    .raw_flags(*flags),
            )
            .unwrap();
    }

    let mut sink = MemoryVolumeSink::default();
    builder.write_to_sink(&mut sink).unwrap();
    let main = sink.into_volumes().remove(&0).unwrap();
    let mut archive =
        Archive::open_with_volumes(Cursor::new(main), MemoryVolumeSource::new([])).unwrap();

    for (path, _, expected) in fixtures {
        assert_eq!(archive.read_entry_by_path(path).unwrap(), expected);
    }
}

#[test]
fn metadata_only_raw_flags_do_not_create_copy_chunks() {
    let mut builder = ArchiveBuilder::new();
    builder
        .add_bytes(
            "hint-only.bin",
            b"not implicitly copied".to_vec(),
            EntryOptions::new().raw_flags(CHUNK_MP3 | CHUNK_RANDOMACCESS),
        )
        .unwrap();

    assert!(
        builder
            .write_to_sink(&mut MemoryVolumeSink::default())
            .is_err()
    );
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
fn alignment_applies_once_per_volume_and_zero_uses_the_payload_origin() {
    let mut builder = ArchiveBuilder::with_options(PackOptions {
        volume_names: vec!["main.dz".to_string(), "main1.dz".to_string()],
        alignment: 256,
        ..PackOptions::default()
    });
    builder
        .add_bytes(
            "zero.bin",
            vec![0; 16],
            EntryOptions::new().compression(Compression::Zero),
        )
        .unwrap()
        .add_bytes(
            "main.bin",
            b"main".to_vec(),
            EntryOptions::new().compression(Compression::Copy),
        )
        .unwrap()
        .add_bytes(
            "aux.bin",
            b"abc".to_vec(),
            EntryOptions::new().compression(Compression::Copy).volume(1),
        )
        .unwrap()
        .add_bytes(
            "aux-2.bin",
            b"defg".to_vec(),
            EntryOptions::new().compression(Compression::Copy).volume(1),
        )
        .unwrap();

    let mut sink = MemoryVolumeSink::default();
    builder.write_to_sink(&mut sink).unwrap();
    let mut volumes = sink.into_volumes();
    let main = volumes.remove(&0).unwrap();
    let auxiliary = volumes.remove(&1).unwrap();
    let archive =
        Archive::open_with_volumes(Cursor::new(main), MemoryVolumeSource::new([(1, auxiliary)]))
            .unwrap();
    let chunks = archive.index().chunks();

    assert_eq!(chunks[0].flags & CHUNK_ZERO, CHUNK_ZERO);
    assert_ne!(chunks[0].offset, 0);
    assert_eq!(chunks[0].offset % 256, 0);
    assert_eq!(chunks[1].offset, chunks[0].offset);
    assert_eq!(chunks[2].offset, 0);
    assert_eq!(chunks[3].offset, 3);
}

#[test]
fn extraction_is_safe_and_zero_uses_original_behavior() {
    let mut unsafe_builder = ArchiveBuilder::new();
    assert!(
        unsafe_builder
            .add_bytes("../escape.bin", b"x".to_vec(), EntryOptions::new())
            .is_err()
    );

    let mut zero_builder = ArchiveBuilder::new();
    zero_builder
        .add_bytes(
            "not-zero.bin",
            b"x".to_vec(),
            EntryOptions::new().compression(Compression::Zero),
        )
        .unwrap();
    let mut zero_sink = MemoryVolumeSink::default();
    zero_builder.write_to_sink(&mut zero_sink).unwrap();
    let zero_main = zero_sink.into_volumes().remove(&0).unwrap();
    let mut zero_archive =
        Archive::open_with_volumes(Cursor::new(zero_main), MemoryVolumeSource::new([])).unwrap();
    assert_eq!(
        zero_archive.read_entry_by_path("not-zero.bin").unwrap(),
        [0]
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
