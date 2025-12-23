use alloc::borrow::Cow;

use super::parser::{ParseResult, Pattern};

#[inline(always)]
pub fn format_output(result: ParseResult) -> String {
    let mut output = String::with_capacity(64);

    // 格式化主体部分
    for (i, pattern) in result.main_parts.iter().enumerate() {
        if i > 0 {
            output.push(' ');
        }

        match pattern {
            Pattern::GPT => output.push_str("GPT"),
            Pattern::O(n) => {
                output.push('O');
                output.push((b'0' + n) as char);
            }
            Pattern::Version(v) => output.push_str(v.as_ref()),
            Pattern::Word(w) => output.push_str(capitalize_word(w).as_ref()),
            _ => {} // 日期相关不应该在主体部分
        }
    }

    // 格式化日期相关部分（括号内）
    for date_item in result.date_parts.iter() {
        output.push_str(" (");
        match date_item {
            Pattern::Date(d) => output.push_str(d.as_ref()),
            Pattern::DateMarker(m) => output.push_str(m), // latest, legacy
            _ => unreachable!(),                          // 其他不应该在日期部分
        }
        output.push(')');
    }

    output
}

#[inline(always)]
fn capitalize_word(word: &str) -> Cow<'_, str> {
    // 特殊情况处理 - 需要完全替换
    if word == "default" {
        return Cow::Borrowed("Default");
    }

    let bytes = word.as_bytes();
    if bytes.is_empty() {
        return Cow::Borrowed(word);
    }

    // 快速检查第一个字符是否已经是大写
    let first_byte = bytes[0];

    // 对于 ASCII 字符的快速路径
    if first_byte.is_ascii() {
        if first_byte.is_ascii_uppercase() {
            // 已经是大写，直接返回
            return Cow::Borrowed(word);
        }

        if first_byte.is_ascii_lowercase() {
            // ASCII 小写转大写，直接操作字节
            let mut result = String::with_capacity(word.len());
            result.push((first_byte - b'a' + b'A') as char);
            result.push_str(&word[1..]);
            return Cow::Owned(result);
        }

        // ASCII 但不是字母（如数字），保持原样
        return Cow::Borrowed(word);
    }

    // 非 ASCII 字符的处理（虽然在 AI 模型名中很少见）
    let mut chars = word.chars();
    match chars.next() {
        None => Cow::Borrowed(word),
        Some(first) if first.is_uppercase() => Cow::Borrowed(word),
        Some(first) => {
            // 预分配足够的空间（假设最坏情况下大写后长度翻倍）
            let mut result = String::with_capacity(word.len() + 4);
            for ch in first.to_uppercase() {
                result.push(ch);
            }
            result.push_str(chars.as_str());
            Cow::Owned(result)
        }
    }
}
