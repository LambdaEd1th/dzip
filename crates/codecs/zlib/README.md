# zlib

A dependency-free, safe Rust implementation of RFC 1950 zlib and RFC 1951
DEFLATE streams. The decoder supports stored, fixed-Huffman, and
dynamic-Huffman blocks; the encoder performs LZ77 matching and emits
fixed-Huffman blocks.

The format baseline is zlib 1.3.2. Encoded bytes are deterministic but are not
expected to match zlib's encoder byte for byte.

`StreamFormat` selects raw DEFLATE or RFC 1950 framing. The public façade
provides `EncoderOptions`, `DecoderOptions`, reusable `Encoder` / `Decoder`
values, typed `ErrorKind` failures, and explicit resource limits. The crate is
always `no_std + alloc`.
