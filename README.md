# dzip-rs

[![Rust CI](https://github.com/LambdaEd1th/dzip-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/LambdaEd1th/dzip-rs/actions/workflows/ci.yml)

Pure-Rust support for reading, extracting, creating, and inspecting Dzip
archives. The workspace contains the reusable `dzip` library and the
`dzip-cli` command-line application.

The implementation is compatible with Dzip 1.1.3, including split volumes,
native DZ compression and COMBUF references, Bzip, LZMA SDK 9.20 output, and
the original program's unusual truncated-gzip framing.

## Library

Add the public crate to a Rust project:

```toml
[dependencies]
dzip = "0.4"
```

Open, inspect, and extract an archive:

```rust,no_run
use dzip::{Archive, ExtractOptions};

fn main() -> dzip::Result<()> {
    let mut archive = Archive::open_path("game.dz")?;

    for entry in archive.entries() {
        println!(
            "{}: {} bytes, {}",
            entry.path().display(),
            entry.decompressed_size(),
            entry.compression(),
        );
    }

    let config = archive.read_entry_by_path("Data/config.bin")?;
    println!("config contains {} bytes", config.len());

    archive.extract_to("output", ExtractOptions::default())?;
    Ok(())
}
```

Create a deterministic archive:

```rust,no_run
use dzip::{ArchiveBuilder, Compression, EntryOptions};

fn main() -> dzip::Result<()> {
    let mut builder = ArchiveBuilder::new();
    builder.add_path(
        "Data/config.bin",
        "input/config.bin",
        EntryOptions::new().compression(Compression::Dz),
    )?;
    builder.add_bytes(
        "readme.txt",
        b"hello from dzip".to_vec(),
        EntryOptions::new().compression(Compression::Zlib),
    )?;
    builder.write_to_path("game.dz")?;
    Ok(())
}
```

For split or embedded archives, use `VolumeSource`/`VolumeSink`,
`MemoryVolumeSource`, and `MemoryVolumeSink`. The lower-level format reader and
writer remain available for inspection and reverse-engineering tools.

### Compatibility and safety

`Compatibility::Dzip113` is the default. It reproduces original writer quirks
and repairs known incorrect physical-length fields while reading.
`Compatibility::Strict` rejects those malformed fields and unsafe zero-chunk
requests.

High-level extraction rejects absolute paths, parent traversal, existing
symlink parents, and symlink output targets. `ReadLimits` bounds metadata and
decompressed output when processing untrusted archives.

### Features

- `encode` and `decode` enable the corresponding public workflows.
- `bzip`, `dz`, `zlib`, and `lzma` select codec engines independently.
- `all-codecs` enables all four engines and is enabled by default.
- `parallel` parallelizes source preparation.
- `serde` adds serialization support to public configuration enums.

Archive metadata, `RangeSettings`, `Compression`, and builder options remain
available without codec features. Attempting to read or write a chunk whose
engine is disabled returns `CodecError::Unavailable`.

## CLI

Build from source:

```bash
git clone https://github.com/LambdaEd1th/dzip-rs.git
cd dzip-rs
cargo build --release -p dzip-cli
```

The executable is written to `target/release/dzip-cli`.

```bash
dzip-cli list game_data.dz
dzip-cli extract game_data.dz --output extracted
dzip-cli build build/game.dcl --output rebuilt \
  --command "align 2048"
dzip-cli create game_data.dz Data/config.bin Images/logo.png \
  --dir assets --type zlib --output rebuilt
```

The CLI is defined entirely with Clap derive. `build` accepts TOML manifests
and the legacy Dzip 1.1.3 `.dcl` syntax, while `create` provides a typed direct
creation workflow without the original executable's order-dependent argument
state. `extract` writes files only; it does not generate a repack manifest.

The DCL
frontend supports quoted paths, `master` includes, the original global
`basedir` search order, combined file flags, byte/percentage ranges, DZ option
blocks, and repeatable `-c`/`--command` overrides. Compatibility parsing keeps
the original case-insensitive and permissive numeric behavior; unsafe archive
entry traversal and recursive include cycles are still rejected.

## GUI

`dzip-gui` is a Dioxus archive manager that shares one responsive interface
between native desktop builds and the browser. It can open and inspect archives,
search and select entries, extract files, and create new archives with DZ, Zlib,
Bzip, LZMA, copy, or zero-fill encoding.

Install the Dioxus CLI, then start the desktop app:

```bash
cargo install dioxus-cli --version 0.7.10 --locked
cd crates/dzip-gui
dx serve --desktop
```

Start the WebAssembly version:

```bash
cd crates/dzip-gui
dx serve --web
```

The desktop build saves archives with the native file dialog and extracts to a
chosen directory. The web build processes files entirely in WebAssembly,
downloads created `.dz` archives directly, and bundles extracted entries into a
browser-friendly ZIP download. Select the main `.dz` file and its auxiliary
volumes together when opening a split archive.

## Workspace

```text
crates/
├── dzip/                 Public archive library
│   └── src/
│       ├── archive.rs    Indexed reading
│       ├── builder.rs    Deterministic creation
│       ├── extract.rs    Safe filesystem extraction
│       ├── codec/        Unified codec and chunk-flag façade
│       └── format/       On-disk structures and constants
├── dzip-cli/             CLI and manifest adapter
├── dzip-gui/             Dioxus desktop and WebAssembly archive manager
└── codecs/
    ├── bzip/             Pure-Rust Bzip engine
    ├── lzma/             LZMA SDK 9.20-compatible engine
    ├── zlib/             zlib 1.1.3-compatible DEFLATE engine
    └── dz/               Native DZ/COMBUF engine
```

The project uses Rust 2024 and has an MSRV of Rust 1.85.

### Publishing

The crates.io publication order is:

1. `bzip`, `lzma`, `zlib`, and `dz`
2. `dzip`
3. `dzip-cli`

The path dependencies also carry exact registry versions, so repository and
crates.io builds use the same codec releases.

## License

This repository is distributed under AGPL-3.0-or-later. Consumers embedding the
library should review the license terms and the codec-specific license files.
