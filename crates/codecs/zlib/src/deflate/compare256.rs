#[cfg(test)]
const MAX_COMPARE_SIZE: usize = 256;

pub fn compare256_slice(src0: &[u8], src1: &[u8]) -> usize {
    let src0 = first_chunk::<_, 256>(src0).unwrap();
    let src1 = first_chunk::<_, 256>(src1).unwrap();

    compare256(src0, src1)
}

fn compare256(src0: &[u8; 256], src1: &[u8; 256]) -> usize {
    src0.iter().zip(src1).take_while(|(x, y)| x == y).count()
}

pub fn compare256_rle_slice(byte: u8, src: &[u8]) -> usize {
    assert!(src.len() >= 256, "too short {}", src.len());

    let repeated = u64::from_ne_bytes([byte; 8]);
    let mut len = 0;

    // Statically limiting the slice to 256 bytes lets LLVM unroll this
    // portable word-at-a-time comparison without architecture intrinsics.
    for chunk in src[..256].chunks_exact(8) {
        let value = u64::from_ne_bytes(chunk.try_into().unwrap());
        let diff = repeated ^ value;

        if diff != 0 {
            let byte_index = if cfg!(target_endian = "little") {
                diff.trailing_zeros()
            } else {
                diff.leading_zeros()
            } / 8;
            return len + byte_index as usize;
        }

        len += 8;
    }

    256
}

#[inline]
pub const fn first_chunk<T, const N: usize>(slice: &[T]) -> Option<&[T; N]> {
    if slice.len() < N {
        None
    } else {
        // SAFETY: The length check proves that the first N elements exist, and
        // the returned reference cannot outlive the source slice.
        Some(unsafe { &*(slice.as_ptr() as *const [T; N]) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare256_finds_first_difference() {
        let left = [b'a'; MAX_COMPARE_SIZE];
        let mut right = left;

        for i in 0..right.len() {
            right[i] = 0;
            assert_eq!(compare256(&left, &right), i);
            right[i] = b'a';
        }
    }

    #[test]
    fn compare256_rle_finds_first_difference() {
        let mut input = [b'a'; MAX_COMPARE_SIZE];

        for i in 0..input.len() {
            input[i] = 0;
            assert_eq!(compare256_rle_slice(b'a', &input), i);
            input[i] = b'a';
        }
    }
}
