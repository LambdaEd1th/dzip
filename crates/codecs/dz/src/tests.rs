use super::*;

#[test]
fn dz_round_trip_literals_and_matches() {
    let data = b"the quick brown fox jumps over the quick brown fox\n".repeat(32);
    let settings = RangeSettings::default();
    let compressed = compress_chunk(&data, settings).unwrap();
    let decompressed = decompress_chunk(&compressed, data.len(), settings).unwrap();
    assert_eq!(decompressed, data);
    assert!(compressed.len() < data.len());
}

#[test]
fn dz_round_trip_small_inputs() {
    let settings = RangeSettings::default();
    for data in [
        Vec::new(),
        vec![0],
        vec![0, 0],
        vec![0, 1, 2],
        vec![255; 32],
    ] {
        let compressed = compress_chunk(&data, settings).unwrap();
        let decompressed = decompress_chunk(&compressed, data.len(), settings).unwrap();
        assert_eq!(decompressed, data);
    }
}

#[test]
fn empty_dz_stream_matches_dzip_original() {
    assert_eq!(
        compress_chunk(&[], RangeSettings::default()).unwrap(),
        [0xff, 0xc0]
    );
}

#[test]
fn dz_without_common_buffer_allows_zero_reference_settings() {
    let settings = RangeSettings {
        ref_length_table_size: 0,
        ref_length_tables: 0,
        ref_offset_table_size: 0,
        ref_offset_tables: 0,
        ..RangeSettings::default()
    };
    let data = b"local matches still work without external-reference models".repeat(8);
    let compressed = compress_chunk(&data, settings).unwrap();
    let decompressed = decompress_chunk(&compressed, data.len(), settings).unwrap();
    assert_eq!(decompressed, data);
}

#[test]
fn dz_rejects_original_memory_estimate_overflow() {
    let inputs = vec![vec![0u8; 1024]];
    let options = DzEncoderOptions {
        max_mem_usage: 1,
        ..DzEncoderOptions::default()
    };
    let error = compress_archive(&inputs, &options).unwrap_err();
    assert!(error.to_string().contains("max_mem_usage"));
}

#[test]
fn combuf_option_without_references_emits_original_placeholder() {
    let inputs = vec![b"alpha".to_vec(), b"omega".to_vec()];
    let options = DzEncoderOptions {
        use_combuf: true,
        ..DzEncoderOptions::default()
    };
    let encoded = compress_archive(&inputs, &options).unwrap();
    assert_eq!(encoded.common_buffer, Some(Vec::new()));
    let common = DzCommonBuffer::new(options.settings, vec![Vec::new()]).unwrap();
    for (input, compressed) in inputs.iter().zip(&encoded.chunks) {
        let decoded = decompress_chunk_with_common_buffer(
            compressed,
            input.len(),
            options.settings,
            Some(&common),
        )
        .unwrap();
        assert_eq!(&decoded, input);
    }
}

#[test]
fn dz_archive_round_trip_with_common_buffer() {
    let mut state = 0x9e37_79b9u32;
    let shared: Vec<u8> = (0..8192)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let mut shifted_once = vec![0x13];
    shifted_once.extend_from_slice(&shared);
    let mut shifted_twice = vec![0x37, 0x42];
    shifted_twice.extend_from_slice(&shared);
    let inputs = vec![shared, shifted_once, shifted_twice];
    let options = DzEncoderOptions {
        use_combuf: true,
        preprocess: false,
        ..DzEncoderOptions::default()
    };
    let encoded = compress_archive(&inputs, &options).unwrap();
    let common_bytes = encoded.common_buffer.as_ref().unwrap();
    assert!(!common_bytes.is_empty());
    let prefix_size = options.settings.combuf_static_prefix_size();
    assert!(
        common_bytes[..prefix_size]
            .iter()
            .any(|&frequency| frequency != 1),
        "COMBUF static model should contain analyzed frequencies"
    );
    let common = DzCommonBuffer::new(options.settings, vec![common_bytes.clone()]).unwrap();

    for (input, compressed) in inputs.iter().zip(&encoded.chunks) {
        let decoded = decompress_chunk_with_common_buffer(
            compressed,
            input.len(),
            options.settings,
            Some(&common),
        )
        .unwrap();
        assert_eq!(&decoded, input);
    }
}

#[test]
fn combuf_recent_bases_round_trip_with_and_without_reference_trimming() {
    let mut state = 0xa341_316cu32;
    let source: Vec<u8> = (0..4096)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect();
    let mut target = Vec::new();
    for index in 0..24usize {
        target.extend_from_slice(&[0xd3, index as u8, 0x71, (index as u8).wrapping_mul(29)]);
        let start = index * 137 % (source.len() - 48);
        target.extend_from_slice(&source[start..start + 48]);
    }
    let inputs = vec![source, target];

    for trim_reference_factor in [0, 20] {
        let options = DzEncoderOptions {
            use_combuf: true,
            preprocess: false,
            trim_reference_factor,
            max_common_match: 48,
            ..DzEncoderOptions::default()
        };
        let encoded = compress_archive(&inputs, &options).unwrap();
        let common_bytes = encoded.common_buffer.as_ref().unwrap();
        assert!(!common_bytes.is_empty());
        let common = DzCommonBuffer::new(options.settings, vec![common_bytes.clone()]).unwrap();

        for (input, compressed) in inputs.iter().zip(&encoded.chunks) {
            let decoded = decompress_chunk_with_common_buffer(
                compressed,
                input.len(),
                options.settings,
                Some(&common),
            )
            .unwrap();
            assert_eq!(&decoded, input);
        }
    }
}

#[test]
fn tiny_match_tokenization_matches_dzip_original() {
    let data = b"// Some text file...\r\n";
    let settings = RangeSettings::default();
    let expected = [
        0x17, 0x74, 0x23, 0xa8, 0x86, 0x9d, 0x5b, 0x36, 0x1e, 0xf0, 0xdc, 0xde, 0x86, 0x7a, 0xc0,
        0xde, 0xae, 0x88, 0x23, 0xf2, 0x28, 0x85, 0xe9, 0xb4,
    ];
    assert_eq!(compress_chunk(data, settings).unwrap(), expected);
}
