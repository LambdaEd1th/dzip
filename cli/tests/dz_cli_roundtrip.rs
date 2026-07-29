use dzip_core::format::CHUNK_LZMA;
use dzip_core::reader::DzipReader;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn official_sample_repack_round_trip() {
    let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test_data/sample");
    if !sample.join("DerbhCLI.txt").exists() {
        return;
    }
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

    run([
        "pack",
        sample.join("DerbhCLI.txt").to_str().unwrap(),
        "--output",
        packed.to_str().unwrap(),
    ]);
    run(["verify", packed.join("testnew.dz").to_str().unwrap()]);
    run([
        "unpack",
        packed.join("testnew.dz").to_str().unwrap(),
        "--output",
        unpacked.to_str().unwrap(),
    ]);

    for relative in [
        "Image16b.bmp",
        "BMP/Image16.bmp",
        "BMP/Image4.bmp",
        "BMP/Image8.bmp",
        "TXT/Text1.txt",
        "TXT/Text3.txt",
    ] {
        assert_same(&sample.join(relative), &unpacked.join(relative));
    }
    let config = std::fs::read_to_string(unpacked.join("testnew.toml")).unwrap();
    assert!(config.contains("use_combuf = true"));

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
        "pack",
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
        "unpack",
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

fn assert_same(expected: &Path, actual: &Path) {
    assert_eq!(
        std::fs::read(actual).unwrap(),
        std::fs::read(expected).unwrap(),
        "{}",
        expected.display()
    );
}
