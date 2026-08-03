# Vendored LZMA SDK 9.20 encoder

This is a local pure-Rust fork of `lzma-sdk-rs` 0.2301.1, upstream commit
`c4ae72456e96c5c5062af14b5ca5ecfacdaa359b`. The original crate targeted LZMA
SDK 23.01. This fork retargets the match finder, optimal parser, price-refresh
cadence, and end-marker path to LZMA SDK 9.20.

For `dzip.exe`, the encoder is used with `lc=3`, `lp=0`, `pb=2`, a 64 KiB
dictionary, `fb=32`, `mc=32`, and `writeEndMark=1`. Its output was checked
byte-for-byte against the SDK 9.20 C encoder over empty, structured,
boundary-sized, and high-entropy inputs through 100,000 bytes.

The code has no runtime or build dependencies and does not compile or link C.

## Provenance and license

The Rust port retains its upstream BSD-3-Clause license in `LICENSE`. The
original LZMA SDK by Igor Pavlov is public domain.
