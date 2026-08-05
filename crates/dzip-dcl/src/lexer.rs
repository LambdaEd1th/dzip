//! Byte-oriented emulation of the original DCL `fgets(256)` loop and its
//! small quoted-token grammar.

pub(super) fn dcl_fgets_lines(bytes: &[u8]) -> Vec<String> {
    let mut result = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let maximum_end = cursor.saturating_add(255).min(bytes.len());
        let end = bytes[cursor..maximum_end]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(maximum_end, |offset| cursor + offset + 1);
        let mut chunk = &bytes[cursor..end];
        if chunk.ends_with(b"\n") {
            chunk = &chunk[..chunk.len() - 1];
        }
        if chunk.ends_with(b"\r") {
            chunk = &chunk[..chunk.len() - 1];
        }
        if let Some(nul) = chunk.iter().position(|byte| *byte == 0) {
            chunk = &chunk[..nul];
        }
        result.push(String::from_utf8_lossy(chunk).into_owned());
        cursor = end;
    }
    result
}

pub(super) fn tokenize_dcl_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut token_started = false;
    let mut characters = line.chars().peekable();

    while let Some(character) = characters.next() {
        if in_quotes && character == '\\' && matches!(characters.peek(), Some('"' | '\\')) {
            current.push(characters.next().expect("peeked character exists"));
            token_started = true;
            continue;
        }
        if character == '"' {
            in_quotes = !in_quotes;
            token_started = true;
            continue;
        }
        if !in_quotes && matches!(character, ' ' | '\t' | '\n') {
            if token_started {
                tokens.push(std::mem::take(&mut current));
                token_started = false;
            }
            continue;
        }
        current.push(character);
        token_started = true;
    }
    if token_started {
        tokens.push(current);
    }
    tokens
}
