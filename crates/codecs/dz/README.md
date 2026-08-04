# dz

A dependency-free `no_std + alloc` implementation of Dzip's native adaptive
range/LZ codec and archive-wide common-buffer (COMBUF) analysis.

Unlike the three standard codecs, its encoder intentionally preserves the
reverse-engineered `dzip.exe` tokenization, static models, recent offsets,
reference trimming, and stream layout. Integration tests rebuild the checked-in
native fixtures and compare every DZ stream and COMBUF record byte for byte.

The chunk façade now matches the three standard codecs with `EncoderOptions`,
`DecoderOptions`, reusable `Encoder` / `Decoder` values, typed `ErrorKind`
failures, and explicit input/output/model-workspace limits. Archive-wide
`DzEncoderOptions` remains separate because COMBUF analysis is inherently a
multi-chunk operation.
