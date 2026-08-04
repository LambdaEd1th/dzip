# Compression engines

The `bzip`, `dz`, `lzma`, and `zlib` crates are independent, safe Rust
implementations written for this workspace. They have no C, FFI, build-script,
or third-party compression dependencies and work with `no_std + alloc`.

Their format behavior follows the current official specifications and reference
implementations: LZMA SDK 26.02 for LZMA1, zlib 1.3.2/RFC 1950 and RFC 1951 for
zlib and DEFLATE, and bzip2 1.0.8 for BZip2. This is a compatibility baseline,
not a promise to reproduce a reference encoder byte for byte.

- `lzma` reads and writes raw LZMA1 streams. `dzip` adds the standard 13-byte
  LZMA-alone properties/size header.
- `zlib` implements raw DEFLATE and RFC 1950 zlib streams. `dzip` adds the
  original ten-byte gzip header and deliberately omits the gzip trailer.
- `bzip` reads and writes standard BZip2 streams with 100-KiB blocks.
- `dz` implements Dzip's native range/LZ stream, COMBUF selection, static
  models, and common-reference encoding.

Compatibility tests decode an archive produced by the original `dzip.exe`,
round-trip each new encoder through the independent decoder, and cover damaged
inputs and boundary sizes.

## Common architecture

Every engine exposes the same layers:

- one-shot `encode` / `decode` functions taking `EncoderOptions` and
  `DecoderOptions`;
- reusable `Encoder` / `Decoder` values whose output allocation is retained
  between calls;
- `ResourceLimits` for input, output, and estimated workspace ceilings;
- a stable `ErrorKind` plus human-readable error details;
- a private format engine isolated from Dzip archive framing.

All four crates are unconditionally `#![no_std]` and depend only on `alloc`.
The similar public shape is intentional, but their bit readers, match finders,
probability models, and transforms remain algorithm-specific.
