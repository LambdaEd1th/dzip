# lzma

A dependency-free, safe Rust LZMA1 range encoder and decoder. It implements
literal, matched-literal, normal-match, repeated-match, distance-slot, length,
and end-marker coding directly.

The behavioral baseline is the current LZMA SDK 26.02 model. The crate exposes
raw LZMA1 streams; the `dzip` crate supplies the LZMA-alone 13-byte header.
Encoded bytes need not match an SDK release byte for byte.

Properties are validated without panicking. The public façade provides
`EncoderOptions`, `DecoderOptions`, reusable `Encoder` / `Decoder` values,
typed `ErrorKind` failures, and explicit resource limits. The crate is always
`no_std + alloc`.
