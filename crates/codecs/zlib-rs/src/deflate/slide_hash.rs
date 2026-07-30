pub fn slide_hash(state: &mut crate::deflate::State) {
    let wsize = state.w_size as u16;

    slide_hash_chain(state.head.as_mut_slice(), wsize);
    slide_hash_chain(state.prev.as_mut_slice(), wsize);
}

fn slide_hash_chain(table: &mut [u16], wsize: u16) {
    for entry in table {
        *entry = entry.saturating_sub(wsize);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WSIZE: u16 = 32768;

    const INPUT: [u16; 64] = [
        0, 0, 28790, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 43884, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 64412, 0, 0, 0, 0, 0, 21043, 0, 0, 0, 0, 0, 23707, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 64026, 0, 0, 20182,
    ];

    const OUTPUT: [u16; 64] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 11116, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 31644, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 31258, 0, 0, 0,
    ];

    #[test]
    fn slide_hash_is_saturating_subtraction() {
        let mut input = INPUT;
        slide_hash_chain(&mut input, WSIZE);
        assert_eq!(input, OUTPUT);
    }

    quickcheck::quickcheck! {
        fn slide_hash_matches_element_wise_reference(input: Vec<u16>, wsize: u16) -> bool {
            let mut expected = input.clone();
            for entry in &mut expected {
                *entry = entry.saturating_sub(wsize);
            }

            let mut actual = input;
            slide_hash_chain(&mut actual, wsize);
            actual == expected
        }
    }
}
