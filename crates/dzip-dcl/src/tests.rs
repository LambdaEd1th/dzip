use super::lexer::{dcl_fgets_lines, tokenize_dcl_line};
use super::*;
use dzip::Compression;
use dzip::format::{CHUNK_BZIP, CHUNK_JPEG, CHUNK_RANDOMACCESS, CHUNK_ZLIB};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn dcl_tokenizer_supports_original_quoted_and_escaped_tokens() {
    assert_eq!(
        tokenize_dcl_line(r#"file "folder/a b\"c\\d.bin" 1 copy"#),
        ["file", "folder/a b\"c\\d.bin", "1", "copy"]
    );
}

#[test]
fn dcl_file_flags_ranges_and_atoi_semantics_match_original() {
    let root = unique_temp_dir("flags");
    std::fs::create_dir_all(&root).unwrap();
    let config_path = root.join("flags.dcl");
    std::fs::write(
        &config_path,
        r#"archive "output archive.dz"
basedir "assets root"
file "payload file.bin" 7 zlib bzip random-access jpeg from 25% # to 75%
options dz
use_combuf yes
use_combuf 2trailing
WinSize 260
"#,
    )
    .unwrap();

    let config = parse_config(&config_path).unwrap();
    assert_eq!(config.archives, ["output archive.dz"]);
    assert_eq!(config.dcl_search_dirs, [root.join("assets root")]);
    let file = &config.files[0];
    assert_eq!(file.path, Path::new("payload file.bin"));
    assert_eq!(file.archive_file_index, 7);
    assert_eq!(file.selected_compression(), Some(Compression::Bzip));
    assert_eq!(
        file.dcl_flags(),
        CHUNK_ZLIB | CHUNK_BZIP | CHUNK_RANDOMACCESS | CHUNK_JPEG
    );
    assert_eq!(file.byte_range(200).unwrap(), (50, 150));
    let options = config.options;
    assert!(options.use_combuf);
    assert_eq!(options.win_size, 4);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dcl_basedirs_are_global_and_nested_master_paths_use_root_directory() {
    let root = unique_temp_dir("master");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(
        root.join("root.dcl"),
        "file payload.bin 0 copy\nmaster sub/first.dcl\n",
    )
    .unwrap();
    std::fs::write(
        root.join("sub/first.dcl"),
        "basedir first\nmaster second.dcl\n",
    )
    .unwrap();
    std::fs::write(root.join("second.dcl"), "basedir second\nalign 64\n").unwrap();

    let config = parse_config(&root.join("root.dcl")).unwrap();
    assert_eq!(
        config.dcl_search_dirs,
        [root.join("first"), root.join("second")]
    );
    assert_eq!(config.align, 64);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dcl_options_require_selection_and_commands_run_last() {
    let root = unique_temp_dir("commands");
    std::fs::create_dir_all(&root).unwrap();
    let config_path = root.join("commands.dcl");
    std::fs::write(
        &config_path,
        "archive first.dz\nuse_combuf 1\noptions dz\npreprocess 0\n",
    )
    .unwrap();
    let commands = vec![
        "archive second.dz".to_string(),
        "use_combuf 1".to_string(),
        "align 32".to_string(),
    ];

    let config = parse_config_with_commands(&config_path, &commands).unwrap();
    assert_eq!(config.archives, ["first.dz", "second.dz"]);
    assert_eq!(config.align, 32);
    let options = config.options;
    assert!(options.use_combuf);
    assert!(!options.preprocess);

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn dcl_reader_splits_physical_lines_like_fgets_256() {
    let mut bytes = vec![b'x'; 300];
    bytes.push(b'\n');
    let lines = dcl_fgets_lines(&bytes);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].len(), 255);
    assert_eq!(lines[1].len(), 45);
}

#[test]
fn root_config_accepts_non_dcl_extensions_like_dzip_exe() {
    let root = unique_temp_dir("arbitrary-extension");
    std::fs::create_dir_all(&root).unwrap();
    let config_path = root.join("pack.txt");
    std::fs::write(&config_path, "archive output.dz\nfile payload.bin 0 copy\n").unwrap();

    let config = parse_config(&config_path).unwrap();
    assert_eq!(config.archives, ["output.dz"]);
    assert_eq!(config.files.len(), 1);

    std::fs::remove_dir_all(root).unwrap();
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dzip-tools-dcl-{label}-{}-{unique}",
        std::process::id()
    ))
}
