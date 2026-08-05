use super::model::{DclRange, FileEntry, GlobalOptions};
use super::number::{atoi_compat, strtoul_boundary_compat};
use dzip::format::{
    CHUNK_BZIP, CHUNK_COMBUF, CHUNK_COPYCOMP, CHUNK_DZ, CHUNK_JPEG, CHUNK_LZMA, CHUNK_MP3,
    CHUNK_RANDOMACCESS, CHUNK_ZERO, CHUNK_ZLIB,
};
use std::path::PathBuf;

pub(super) fn parse_file_directive(parts: &[String]) -> FileEntry {
    let path = PathBuf::from(parts[1].replace('\\', "/"));
    let archive_file_index = atoi_compat(&parts[2]);
    let mut flags = 0u16;
    let mut range = DclRange { from: 0, to: -100 };
    let mut pending_boundary = None;

    for part in &parts[3..] {
        if let Some(is_from) = pending_boundary.take() {
            let boundary = strtoul_boundary_compat(part);
            if is_from {
                range.from = boundary;
            } else {
                range.to = boundary;
            }
            continue;
        }

        match part.to_ascii_lowercase().as_str() {
            "combuf" => flags |= CHUNK_COMBUF,
            "dz" => flags |= CHUNK_DZ,
            "zlib" => flags |= CHUNK_ZLIB,
            "bzip" => flags |= CHUNK_BZIP,
            "mp3" => flags |= CHUNK_MP3,
            "jpeg" => flags |= CHUNK_JPEG,
            "zero" => flags |= CHUNK_ZERO,
            "copy" => flags |= CHUNK_COPYCOMP,
            "lzma" => flags |= CHUNK_LZMA,
            "random-access" => flags |= CHUNK_RANDOMACCESS,
            "from" => pending_boundary = Some(true),
            "to" => pending_boundary = Some(false),
            _ => {}
        }
    }

    FileEntry {
        path,
        archive_file_index,
        range,
        flags,
    }
}

pub(super) fn set_dcl_option(options: &mut GlobalOptions, key: &str, value: &str) {
    let value = atoi_compat(value);
    match key {
        "isnotdefault" => options.is_not_default = value != 0,
        "max_mem_usage" => options.max_mem_usage = value,
        "use_combuf" => options.use_combuf = value != 0,
        "preprocess" => options.preprocess = value != 0,
        "trim_reference_factor" => options.trim_reference_factor = value,
        "winsize" => options.win_size = value as u8,
        "flags" => options.flags = value as u8,
        "offsettablesize" => options.offset_table_size = value as u8,
        "offsettables" => options.offset_tables = value as u8,
        "offsetcontexts" => options.offset_contexts = value as u8,
        "reflengthtablesize" => options.ref_length_table_size = value as u8,
        "reflengthtables" => options.ref_length_tables = value as u8,
        "refoffsettablesize" => options.ref_offset_table_size = value as u8,
        "refoffsettables" => options.ref_offset_tables = value as u8,
        "bigminmatch" => options.big_min_match = value as u8,
        _ => {}
    }
}
