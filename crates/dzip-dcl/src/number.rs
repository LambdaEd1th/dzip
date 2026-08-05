//! CRT-compatible numeric parsing used by the DCL frontend.

pub(super) fn atoi_compat(value: &str) -> i32 {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    let negative = match bytes.first() {
        Some(b'-') => {
            index = 1;
            true
        }
        Some(b'+') => {
            index = 1;
            false
        }
        _ => false,
    };
    let mut parsed = 0i64;
    let mut found_digit = false;
    while let Some(byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        found_digit = true;
        parsed = parsed
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'));
        index += 1;
    }
    if !found_digit {
        return 0;
    }
    if negative {
        parsed = -parsed;
    }
    parsed.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

pub(super) fn strtoul_boundary_compat(value: &str) -> i32 {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    let negative = match bytes.first() {
        Some(b'-') => {
            index = 1;
            true
        }
        Some(b'+') => {
            index = 1;
            false
        }
        _ => false,
    };
    let mut parsed = 0u64;
    while let Some(byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        parsed = parsed
            .saturating_mul(10)
            .saturating_add(u64::from(byte - b'0'))
            .min(u64::from(u32::MAX));
        index += 1;
    }
    let mut raw = parsed as u32;
    if negative {
        raw = raw.wrapping_neg();
    }
    if bytes.get(index) == Some(&b'%') {
        raw = raw.wrapping_neg();
    }
    raw as i32
}
