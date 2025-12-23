#[derive(Debug, Clone, Copy)]
pub struct Token {
    pub content: &'static str,
    pub meta: TokenMeta,
}

#[derive(Debug, Clone, Copy)]
pub struct TokenMeta {
    pub is_digit_only: bool,
    pub digit_count: u8,
    pub has_dot: bool,
    pub first_char: u8,
    pub len: u8,
}

#[inline(always)]
pub fn tokenize(identifier: &'static str) -> Vec<Token> {
    identifier
        .split('-')
        .map(|segment| Token { content: segment, meta: analyze_segment(segment) })
        .collect()
}

#[inline(always)]
fn analyze_segment(segment: &str) -> TokenMeta {
    let bytes = segment.as_bytes();
    let len = bytes.len() as u8;

    if len == 0 {
        return TokenMeta {
            is_digit_only: false,
            digit_count: 0,
            has_dot: false,
            first_char: 0,
            len: 0,
        };
    }

    let first_char = bytes[0];
    let mut is_digit_only = true;
    let mut digit_count = 0u8;
    let mut has_dot = false;

    for &byte in bytes {
        match byte {
            b'0'..=b'9' => digit_count += 1,
            b'.' => has_dot = true,
            _ => is_digit_only = false,
        }
    }

    TokenMeta { is_digit_only: is_digit_only && !has_dot, digit_count, has_dot, first_char, len }
}
