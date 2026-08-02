use dzip::format::{
    CHUNK_BZIP, CHUNK_COMBUF, CHUNK_COPYCOMP, CHUNK_DZ, CHUNK_JPEG, CHUNK_LZMA, CHUNK_RANDOMACCESS,
    CHUNK_ZLIB,
};
use dzip::reader::DzipReader;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn native_fixtures_repack_byte_exact_and_extract() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test_data/native");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "dzip-rs-cli-roundtrip-{}-{}",
        std::process::id(),
        unique
    ));
    let packed = root.join("packed");
    let unpacked = root.join("unpacked");
    std::fs::create_dir_all(&packed).unwrap();
    std::fs::create_dir_all(&unpacked).unwrap();

    for (dcl, archives) in [
        ("codecs.dcl", &["codecs.dz", "codecs-1.dz"][..]),
        (
            "ranges.dcl",
            &["ranges.dz", "ranges-1.dz", "ranges-2.dz"][..],
        ),
        ("tiny.dcl", &["tiny.dz"][..]),
    ] {
        run([
            "build",
            fixtures.join(dcl).to_str().unwrap(),
            "--output",
            packed.to_str().unwrap(),
        ]);
        for archive in archives {
            assert_same(&fixtures.join(archive), &packed.join(archive));
        }
    }

    run(["list", packed.join("codecs.dz").to_str().unwrap()]);
    let codecs_unpacked = unpacked.join("codecs");
    run([
        "extract",
        packed.join("codecs.dz").to_str().unwrap(),
        "--output",
        codecs_unpacked.to_str().unwrap(),
    ]);

    for relative in [
        "common/base.bin",
        "common/variant-1.bin",
        "common/variant-2.bin",
        "common/variant-3.bin",
        "local/random.bin",
        "local/text.txt",
        "local/periodic.bin",
        "local/runs.bin",
        "local/zero.bin",
    ] {
        assert_same(
            &fixtures.join("corpus").join(relative),
            &codecs_unpacked.join(relative),
        );
    }

    let ranges_unpacked = unpacked.join("ranges");
    run([
        "extract",
        packed.join("ranges.dz").to_str().unwrap(),
        "--output",
        ranges_unpacked.to_str().unwrap(),
    ]);
    for relative in ["common/base.bin", "common/variant-1.bin"] {
        assert_same(
            &fixtures.join("corpus").join(relative),
            &ranges_unpacked.join(relative),
        );
    }
    let periodic = std::fs::read(fixtures.join("corpus/local/periodic.bin")).unwrap();
    assert_eq!(
        std::fs::read(ranges_unpacked.join("local/periodic.bin")).unwrap(),
        periodic[123..12001]
    );

    let tiny_unpacked = unpacked.join("tiny");
    run([
        "extract",
        packed.join("tiny.dz").to_str().unwrap(),
        "--output",
        tiny_unpacked.to_str().unwrap(),
    ]);
    for relative in ["local/empty.bin", "local/one.bin", "local/two.bin"] {
        assert_same(
            &fixtures.join("corpus").join(relative),
            &tiny_unpacked.join(relative),
        );
    }

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn lzma_chunk_table_matches_dzip_1_1_3() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "dzip-rs-lzma-compat-{}-{}",
        std::process::id(),
        unique
    ));
    let packed = root.join("packed");
    let unpacked = root.join("unpacked");
    std::fs::create_dir_all(&packed).unwrap();
    std::fs::create_dir_all(&unpacked).unwrap();

    let original = b"dzip lzma compatibility payload\n".repeat(1024);
    std::fs::write(root.join("payload.bin"), &original).unwrap();
    std::fs::write(
        root.join("pack.toml"),
        r#"
archives = ["lzma.dz"]
base_dir = "."

[[files]]
path = "payload.bin"
archive_file_index = 0
compression = "Lzma"
"#,
    )
    .unwrap();

    run([
        "build",
        root.join("pack.toml").to_str().unwrap(),
        "--output",
        packed.to_str().unwrap(),
    ]);

    let archive_path = packed.join("lzma.dz");
    let mut reader = DzipReader::new(std::fs::File::open(&archive_path).unwrap());
    let archive = reader.read_archive_settings().unwrap();
    reader
        .read_strings((archive.num_user_files + archive.num_directories - 1) as usize)
        .unwrap();
    reader
        .read_file_chunk_map(archive.num_user_files as usize)
        .unwrap();
    let chunk_settings = reader.read_chunk_settings().unwrap();
    let chunks = reader
        .read_chunks(chunk_settings.num_chunks as usize)
        .unwrap();

    assert_eq!(chunks.len(), 1);
    assert_ne!(chunks[0].flags & CHUNK_LZMA, 0);
    assert_eq!(chunks[0].compressed_length, original.len() as u32);
    assert_eq!(chunks[0].decompressed_length, original.len() as u32);
    let physical_length =
        std::fs::metadata(&archive_path).unwrap().len() - u64::from(chunks[0].offset);
    assert!(
        physical_length < original.len() as u64,
        "test payload should produce a physically smaller LZMA stream"
    );

    run([
        "extract",
        archive_path.to_str().unwrap(),
        "--output",
        unpacked.to_str().unwrap(),
    ]);
    assert_eq!(
        std::fs::read(unpacked.join("payload.bin")).unwrap(),
        original
    );

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn framed_codec_chunk_lengths_match_dzip_1_1_3() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "dzip-rs-framed-codecs-{}-{}",
        std::process::id(),
        unique
    ));
    let packed = root.join("packed");
    std::fs::create_dir_all(&packed).unwrap();

    let original = b"dzip framed codec compatibility payload\n".repeat(1024);
    for name in ["zlib.bin", "bzip.bin", "lzma.bin"] {
        std::fs::write(root.join(name), &original).unwrap();
    }
    std::fs::write(
        root.join("pack.toml"),
        r#"
archives = ["codecs.dz"]
base_dir = "."

[[files]]
path = "zlib.bin"
archive_file_index = 0
compression = "Zlib"

[[files]]
path = "bzip.bin"
archive_file_index = 0
compression = "Bzip"

[[files]]
path = "lzma.bin"
archive_file_index = 0
compression = "Lzma"
"#,
    )
    .unwrap();

    run([
        "build",
        root.join("pack.toml").to_str().unwrap(),
        "--output",
        packed.to_str().unwrap(),
    ]);

    let archive_path = packed.join("codecs.dz");
    let mut reader = DzipReader::new(std::fs::File::open(&archive_path).unwrap());
    let archive = reader.read_archive_settings().unwrap();
    reader
        .read_strings((archive.num_user_files + archive.num_directories - 1) as usize)
        .unwrap();
    reader
        .read_file_chunk_map(archive.num_user_files as usize)
        .unwrap();
    let chunk_settings = reader.read_chunk_settings().unwrap();
    let chunks = reader
        .read_chunks(chunk_settings.num_chunks as usize)
        .unwrap();

    assert_eq!(chunks.len(), 3);
    for (chunk, flag) in chunks.iter().zip([CHUNK_ZLIB, CHUNK_BZIP, CHUNK_LZMA]) {
        assert_ne!(chunk.flags & flag, 0);
        assert_eq!(chunk.compressed_length, original.len() as u32);
        assert_eq!(chunk.decompressed_length, original.len() as u32);
    }
    assert!(
        chunks[1].offset < chunks[0].offset && chunks[0].offset < chunks[2].offset,
        "dzip.exe writes BZip, Zlib, then LZMA while retaining logical chunk IDs"
    );

    let archive_bytes = std::fs::read(&archive_path).unwrap();
    let zlib_offset = chunks[0].offset as usize;
    assert_eq!(
        &archive_bytes[zlib_offset..zlib_offset + 10],
        &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 11],
        "dzip.exe emits a zero-time gzip header with OS_NTFS"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn directory_ids_are_case_insensitive_like_dzip_1_1_3() {
    let root = unique_temp_dir("case-fold");
    let packed = root.join("packed");
    std::fs::create_dir_all(root.join("Foo")).unwrap();
    std::fs::create_dir_all(root.join("foo")).unwrap();
    std::fs::create_dir_all(&packed).unwrap();
    std::fs::write(root.join("Foo/a.bin"), b"a").unwrap();
    std::fs::write(root.join("foo/b.bin"), b"b").unwrap();
    std::fs::write(
        root.join("pack.toml"),
        r#"
archives = ["case.dz"]
base_dir = "."

[[files]]
path = "Foo/a.bin"
archive_file_index = 0
compression = "Copy"

[[files]]
path = "foo/b.bin"
archive_file_index = 0
compression = "Copy"
"#,
    )
    .unwrap();

    run([
        "build",
        root.join("pack.toml").to_str().unwrap(),
        "--output",
        packed.to_str().unwrap(),
    ]);

    let mut reader = DzipReader::new(std::fs::File::open(packed.join("case.dz")).unwrap());
    let archive = reader.read_archive_settings().unwrap();
    assert_eq!(archive.num_user_files, 2);
    assert_eq!(archive.num_directories, 2);
    let strings = reader
        .read_strings((archive.num_user_files + archive.num_directories - 1) as usize)
        .unwrap();
    assert_eq!(strings[2], "Foo");

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn combuf_is_written_before_dz_chunks() {
    let root = unique_temp_dir("combuf-order");
    let packed = root.join("packed");
    let unpacked = root.join("unpacked");
    std::fs::create_dir_all(&packed).unwrap();
    std::fs::create_dir_all(&unpacked).unwrap();

    let mut state = 0x9e37_79b9u32;
    let shared: Vec<u8> = (0..8192)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let mut shifted_once = vec![0x13];
    shifted_once.extend_from_slice(&shared);
    let mut shifted_twice = vec![0x37, 0x42];
    shifted_twice.extend_from_slice(&shared);
    let originals = [shared, shifted_once, shifted_twice];
    for (name, original) in ["a.bin", "b.bin", "c.bin"].iter().zip(&originals) {
        std::fs::write(root.join(name), original).unwrap();
    }
    std::fs::write(
        root.join("pack.toml"),
        r#"
archives = ["combuf.dz"]
base_dir = "."

[options]
use_combuf = true
preprocess = false

[[files]]
path = "a.bin"
archive_file_index = 0
compression = "Dz"

[[files]]
path = "b.bin"
archive_file_index = 0
compression = "Dz"

[[files]]
path = "c.bin"
archive_file_index = 0
compression = "Dz"
"#,
    )
    .unwrap();

    run([
        "build",
        root.join("pack.toml").to_str().unwrap(),
        "--output",
        packed.to_str().unwrap(),
    ]);

    let archive_path = packed.join("combuf.dz");
    let mut reader = DzipReader::new(std::fs::File::open(&archive_path).unwrap());
    let archive = reader.read_archive_settings().unwrap();
    reader
        .read_strings((archive.num_user_files + archive.num_directories - 1) as usize)
        .unwrap();
    reader
        .read_file_chunk_map(archive.num_user_files as usize)
        .unwrap();
    let chunk_settings = reader.read_chunk_settings().unwrap();
    let chunks = reader
        .read_chunks(chunk_settings.num_chunks as usize)
        .unwrap();
    let common = chunks
        .iter()
        .find(|chunk| chunk.flags & CHUNK_COMBUF != 0)
        .unwrap();
    let first_dz = chunks
        .iter()
        .filter(|chunk| chunk.flags & CHUNK_DZ != 0)
        .map(|chunk| chunk.offset)
        .min()
        .unwrap();
    let physical_common_length = first_dz.checked_sub(common.offset).unwrap();
    assert!(
        physical_common_length > 0,
        "COMBUF payload length is determined by the following physical chunk offset"
    );

    run([
        "extract",
        archive_path.to_str().unwrap(),
        "--output",
        unpacked.to_str().unwrap(),
    ]);
    for (name, original) in ["a.bin", "b.bin", "c.bin"].iter().zip(&originals) {
        assert_eq!(std::fs::read(unpacked.join(name)).unwrap(), *original);
    }

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn legacy_dcl_quotes_global_basedirs_flags_and_commands_match_dzip() {
    let root = unique_temp_dir("dcl-parity");
    let packed = root.join("packed");
    let assets = root.join("assets with spaces");
    std::fs::create_dir_all(&packed).unwrap();
    std::fs::create_dir_all(&assets).unwrap();
    let payload: Vec<u8> = (0..200).map(|value| value as u8).collect();
    std::fs::write(assets.join("payload file.bin"), &payload).unwrap();
    std::fs::write(
        root.join("pack.dcl"),
        r#"file "payload file.bin" 99 copy jpeg random-access from 25% to 75%
basedir missing
basedir "assets with spaces"
"#,
    )
    .unwrap();

    run([
        "build",
        root.join("pack.dcl").to_str().unwrap(),
        "--command",
        "archive nested/command.dz",
        "--output",
        packed.to_str().unwrap(),
    ]);

    let archive_path = packed.join("nested/command.dz");
    let mut reader = DzipReader::new(std::fs::File::open(&archive_path).unwrap());
    let archive = reader.read_archive_settings().unwrap();
    reader
        .read_strings((archive.num_user_files + archive.num_directories - 1) as usize)
        .unwrap();
    reader
        .read_file_chunk_map(archive.num_user_files as usize)
        .unwrap();
    let chunk_settings = reader.read_chunk_settings().unwrap();
    let chunks = reader
        .read_chunks(chunk_settings.num_chunks as usize)
        .unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].file, 0);
    assert_eq!(
        chunks[0].flags,
        CHUNK_COPYCOMP | CHUNK_JPEG | CHUNK_RANDOMACCESS
    );

    let mut archive = dzip::Archive::open_path(&archive_path).unwrap();
    assert_eq!(
        archive.read_entry_by_path("payload file.bin").unwrap(),
        payload[50..150]
    );

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn modern_create_list_and_extract_workflow() {
    let root = unique_temp_dir("modern-create");
    let source = root.join("source");
    let output = root.join("output");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    let original: Vec<u8> = (0..200).map(|value| value as u8).collect();
    std::fs::write(source.join("payload.bin"), &original).unwrap();

    run([
        "create",
        "direct.dz",
        "payload.bin",
        "--output",
        output.to_str().unwrap(),
        "--dir",
        source.to_str().unwrap(),
        "--type",
        "zlib",
        "--start",
        "25%",
        "--end",
        "75%",
    ]);

    let archive = output.join("direct.dz");
    let listed = Command::new(env!("CARGO_BIN_EXE_dzip-cli"))
        .args(["list", archive.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(listed.status.success());
    assert!(String::from_utf8_lossy(&listed.stdout).contains("payload.bin"));

    run(["extract", archive.to_str().unwrap()]);
    assert_eq!(
        std::fs::read(output.join("direct/payload.bin")).unwrap(),
        original[50..150]
    );

    let quiet = Command::new(env!("CARGO_BIN_EXE_dzip-cli"))
        .args(["--quiet", "list", archive.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(quiet.status.success());
    assert!(quiet.stdout.is_empty());

    std::fs::remove_dir_all(&root).unwrap();
}

fn run<const N: usize>(arguments: [&str; N]) {
    let output = Command::new(env!("CARGO_BIN_EXE_dzip-cli"))
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("dzip-rs-{label}-{}-{}", std::process::id(), unique))
}

fn assert_same(expected: &Path, actual: &Path) {
    assert_eq!(
        std::fs::read(actual).unwrap(),
        std::fs::read(expected).unwrap(),
        "{}",
        expected.display()
    );
}
