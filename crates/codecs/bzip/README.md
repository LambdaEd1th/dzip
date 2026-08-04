# bzip

A dependency-free, safe Rust encoder and decoder for standard BZip2 streams.
It implements RLE, Burrows-Wheeler transform, move-to-front coding, selectors,
canonical Huffman coding, and BZip2 block/stream CRC validation directly.

The format baseline is bzip2 1.0.8. Encoded bytes are deterministic but are not
expected to match the reference encoder; decoded data is interoperable.

The implementation is original AGPL-3.0-or-later code. Its fixed compatibility
table for obsolete randomized blocks retains the bzip2 notice in `LICENSE`.

The public façade provides `EncoderOptions`, `DecoderOptions`, reusable
`Encoder` / `Decoder` values, typed `ErrorKind` failures, and explicit resource
limits. The crate is always `no_std + alloc`.
