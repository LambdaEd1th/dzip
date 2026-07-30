# Compression engines

These crates are source dependencies of `dzip`; no C compiler or system
compression library is used.

- `zlib-rs` starts from upstream `zlib-rs` 0.5.5 and restores the zlib 1.1.3 level-6
  parser choices used by `dzip.exe`: the slow parser, rolling three-byte hash,
  the `TOO_FAR=4096` length-three match rule, and matching hash initialization.
  Its portable scalar backend is used on every target; it has no architecture-
  specific instruction requirements or runtime CPU-feature dispatch.
  `dzip` supplies the exact Windows gzip header and intentionally omits the
  CRC32/ISIZE trailer, as the original program does.
- `bzip-rs` starts from `libbz2-rs-sys` 0.2.2, a Rust translation of
  libbzip2. `dzip` uses the reverse-engineered 1.0.3 parameters
  `blockSize100k=1`, `verbosity=0`, and `workFactor=30`.
- `lzma-rs` is the repository's Rust implementation of the LZMA SDK 9.20
  path.
- `dz-rs` implements the native range/LZ stream, archive-wide COMBUF
  selection, static models, and common-reference encoding. Its implementation
  is split by model, chunk, match-finder, and archive-analysis responsibilities.

The integration test `crates/dzip/tests/codec_compatibility.rs` freezes the
lengths and SHA-256 hashes of blocks produced by the checked-in original
`dzip.exe`.
