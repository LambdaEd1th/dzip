use dzip::Compression;
use dzip::format::Chunk;
use dzip::reader::DzipReader;
use dzip::writer::compress_data;
use sha2::{Digest, Sha256};
use std::io::Cursor;

struct Fixture {
    name: &'static str,
    input: Vec<u8>,
    zlib_length: usize,
    zlib_sha256: &'static str,
    bzip_length: usize,
    bzip_sha256: &'static str,
}

fn random_bytes(length: usize, mut state: u32) -> Vec<u8> {
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn zlib_and_bzip_match_dzip_exe_byte_for_byte() {
    let fixtures = [
        Fixture {
            name: "empty",
            input: Vec::new(),
            zlib_length: 0,
            zlib_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            bzip_length: 0,
            bzip_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        },
        Fixture {
            name: "one",
            input: b"a".to_vec(),
            zlib_length: 13,
            zlib_sha256: "92e610dab04040b604dd5e60ba892d9550ea3ea8e1f3cd50119f9fa13b43ab3b",
            bzip_length: 37,
            bzip_sha256: "8fe0e8985113923f32f1e53c4908bb22717b7dee29f4d4b5ea0072d357c3f7e4",
        },
        Fixture {
            name: "text",
            input: b"dzip codec compatibility payload\n".repeat(1024),
            zlib_length: 157,
            zlib_sha256: "34450c29563deda7de82a7db6daf1b545d8fb2b8c54e2a758d88398b6d46409d",
            bzip_length: 109,
            bzip_sha256: "a69a7f24d3d0c98171bd7f1d4fa590f28bdb3879f40cd91e781741db6a84a5e5",
        },
        Fixture {
            name: "zeros100k",
            input: vec![0; 100_000],
            zlib_length: 124,
            zlib_sha256: "7940261a9a4aad093c66c076481fbdd42afc4d2eb68188be9cbd2d30055f1363",
            bzip_length: 47,
            bzip_sha256: "c7618be2d46329bae9e9ba209087cb2607fe9f7a8cf03dd0d3a96d85e4844a02",
        },
        Fixture {
            name: "random65535",
            input: random_bytes(65_535, 0x1234_5678),
            zlib_length: 65_565,
            zlib_sha256: "f12452543cabb729290e95afee1913854da2e6805824f0cbfe4da375b9c419e9",
            bzip_length: 66_164,
            bzip_sha256: "bf5ba9080609854e04199c52bb9d81594f5c1fd4e40c70e82fd49a9821def77d",
        },
        Fixture {
            name: "random100k",
            input: random_bytes(100_000, 0x9e37_79b9),
            zlib_length: 100_045,
            zlib_sha256: "d1d3ec860eba5bd8c636de5c75932648b3b30098d593cbd1cd572c372d91036e",
            bzip_length: 100_872,
            bzip_sha256: "8f18dfe95c4bc9f6ed4d4ecaa2f8fed02b691b54586c9ddbe35f19d713a45341",
        },
        Fixture {
            name: "pattern200k",
            input: (0..=255).cycle().take(200_000).collect(),
            zlib_length: 1_111,
            zlib_sha256: "0f8d2542c5d59b89ef2d336c049f43fa8e57bc192058d3b1126281212304e466",
            bzip_length: 1_925,
            bzip_sha256: "8ff4cb22925057066d921a26c5707b7f53e42dc9bf54283f51ed9ed5e1881926",
        },
        Fixture {
            name: "random200k",
            input: random_bytes(200_000, 0xa5a5_5a5a),
            zlib_length: 200_075,
            zlib_sha256: "424a6047933611f9586db40cb469a05f8398dab2cd74859746ae4c3a201d8918",
            bzip_length: 201_650,
            bzip_sha256: "6a52d03d2beb9dec4622092cb8e89edffce308026c617642b8d0956ae2a66de6",
        },
    ];

    for fixture in fixtures {
        for (method, expected_length, expected_sha256) in [
            (Compression::Zlib, fixture.zlib_length, fixture.zlib_sha256),
            (Compression::Bzip, fixture.bzip_length, fixture.bzip_sha256),
        ] {
            let (flags, encoded) = compress_data(&fixture.input, method).unwrap();
            assert_eq!(
                encoded.len(),
                expected_length,
                "{} {method:?}",
                fixture.name
            );
            assert_eq!(
                hex(&Sha256::digest(&encoded)),
                expected_sha256,
                "{} {method:?}",
                fixture.name
            );

            let chunk = Chunk {
                offset: 0,
                compressed_length: encoded.len() as u32,
                decompressed_length: fixture.input.len() as u32,
                flags,
                file: 0,
            };
            let decoded = DzipReader::new(Cursor::new(encoded))
                .read_chunk_data(&chunk)
                .unwrap();
            assert_eq!(decoded, fixture.input, "{} {method:?}", fixture.name);
        }
    }
}
