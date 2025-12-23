#![allow(unsafe_op_in_unsafe_fn)]

//! 高性能 Base64 编解码实现
//!
//! 本模块提供了一个优化的 Base64 编解码器，使用自定义字符集：
//! - 字符集：`-AaBbCcDdEeFfGgHhIiJjKkLlMmNnOoPpQqRrSsTtUuVvWwXxYyZz1032547698_`
//! - 特点：URL 安全，无需填充字符

/// Base64 字符集
const BASE64_CHARS: &[u8; 64] = b"-AaBbCcDdEeFfGgHhIiJjKkLlMmNnOoPpQqRrSsTtUuVvWwXxYyZz1032547698_";

/// Base64 解码查找表
const BASE64_DECODE_TABLE: [u8; 256] = {
    let mut table = [0xFF_u8; 256];
    let mut i = 0;
    while i < BASE64_CHARS.len() {
        table[BASE64_CHARS[i] as usize] = i as u8;
        i += 1;
    }
    table
};

/// 计算编码后的精确长度
#[inline]
pub const fn encoded_len(input_len: usize) -> usize {
    let d = input_len / 3;
    let r = input_len % 3;

    (if r > 0 { d + 1 } else { d }) * 4
        - match r {
            1 => 2, // 1字节编码为2个字符
            2 => 1, // 2字节编码为3个字符
            0 => 0, // 3字节编码为4个字符
            _ => unreachable!(),
        }
}

/// 计算解码后的精确长度
#[inline]
pub const fn decoded_len(encoded_len: usize) -> Option<usize> {
    match encoded_len % 4 {
        0 => Some((encoded_len / 4) * 3),
        2 => Some((encoded_len / 4) * 3 + 1),
        3 => Some((encoded_len / 4) * 3 + 2),
        1 => None, // 无效长度（% 4 == 1）
        _ => unreachable!(),
    }
}

/// 将字节数据编码到提供的缓冲区
///
/// # Safety
///
/// 调用者必须确保：
/// - input.len() 字节可读
/// - output 有 encoded_len(input.len()) 字节可写
#[inline]
pub unsafe fn encode_to_slice_unchecked(input: &[u8], output: &mut [u8]) {
    let chunks_exact = input.chunks_exact(3);
    let remainder = chunks_exact.remainder();
    let mut j = 0;

    // 主循环：使用 chunks_exact 让编译器更好地优化
    for chunk in chunks_exact {
        let b1 = *chunk.get_unchecked(0);
        let b2 = *chunk.get_unchecked(1);
        let b3 = *chunk.get_unchecked(2);

        let n = ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);

        *output.get_unchecked_mut(j) = BASE64_CHARS[(n >> 18) as usize];
        *output.get_unchecked_mut(j + 1) = BASE64_CHARS[((n >> 12) & 0x3F) as usize];
        *output.get_unchecked_mut(j + 2) = BASE64_CHARS[((n >> 6) & 0x3F) as usize];
        *output.get_unchecked_mut(j + 3) = BASE64_CHARS[(n & 0x3F) as usize];

        j += 4;
    }

    // 处理剩余字节
    match remainder.len() {
        1 => {
            let b1 = *remainder.get_unchecked(0);
            let n = (b1 as u32) << 16;

            *output.get_unchecked_mut(j) = BASE64_CHARS[(n >> 18) as usize];
            *output.get_unchecked_mut(j + 1) = BASE64_CHARS[((n >> 12) & 0x3F) as usize];
        }
        2 => {
            let b1 = *remainder.get_unchecked(0);
            let b2 = *remainder.get_unchecked(1);
            let n = ((b1 as u32) << 16) | ((b2 as u32) << 8);

            *output.get_unchecked_mut(j) = BASE64_CHARS[(n >> 18) as usize];
            *output.get_unchecked_mut(j + 1) = BASE64_CHARS[((n >> 12) & 0x3F) as usize];
            *output.get_unchecked_mut(j + 2) = BASE64_CHARS[((n >> 6) & 0x3F) as usize];
        }
        0 => {}
        _ => ::core::hint::unreachable_unchecked(),
    }
}

/// 将 Base64 数据解码到提供的缓冲区
///
/// # Safety
///
/// 调用者必须确保：
/// - input 是有效的 base64 数据（所有字符都在字符集中，长度 % 4 != 1）
/// - output 有 decoded_len(input.len()) 字节可写
#[inline]
pub unsafe fn decode_to_slice_unchecked(input: &[u8], output: &mut [u8]) {
    let chunks = input.chunks_exact(4);
    let remainder = chunks.remainder();
    let mut j = 0;

    // 主循环：使用 chunks_exact 优化
    for chunk in chunks {
        let c1 = BASE64_DECODE_TABLE[*chunk.get_unchecked(0) as usize];
        let c2 = BASE64_DECODE_TABLE[*chunk.get_unchecked(1) as usize];
        let c3 = BASE64_DECODE_TABLE[*chunk.get_unchecked(2) as usize];
        let c4 = BASE64_DECODE_TABLE[*chunk.get_unchecked(3) as usize];

        let n = ((c1 as u32) << 18) | ((c2 as u32) << 12) | ((c3 as u32) << 6) | (c4 as u32);

        *output.get_unchecked_mut(j) = (n >> 16) as u8;
        *output.get_unchecked_mut(j + 1) = (n >> 8) as u8;
        *output.get_unchecked_mut(j + 2) = n as u8;

        j += 3;
    }

    // 处理剩余的2或3个字符
    match remainder.len() {
        2 => {
            let c1 = BASE64_DECODE_TABLE[*remainder.get_unchecked(0) as usize];
            let c2 = BASE64_DECODE_TABLE[*remainder.get_unchecked(1) as usize];

            *output.get_unchecked_mut(j) = (c1 << 2) | (c2 >> 4);
        }
        3 => {
            let c1 = BASE64_DECODE_TABLE[*remainder.get_unchecked(0) as usize];
            let c2 = BASE64_DECODE_TABLE[*remainder.get_unchecked(1) as usize];
            let c3 = BASE64_DECODE_TABLE[*remainder.get_unchecked(2) as usize];

            *output.get_unchecked_mut(j) = (c1 << 2) | (c2 >> 4);
            *output.get_unchecked_mut(j + 1) = (c2 << 4) | (c3 >> 2);
        }
        0 => {}
        1 => ::core::hint::unreachable_unchecked(),
        _ => ::core::hint::unreachable_unchecked(),
    }
}

/// 编码到新分配的 String
#[inline]
pub fn to_base64(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    let output_len = encoded_len(bytes.len());
    let mut output: Vec<u8> = Vec::with_capacity(output_len);

    unsafe {
        encode_to_slice_unchecked(
            bytes,
            core::slice::from_raw_parts_mut(output.as_mut_ptr(), output_len),
        );
        output.set_len(output_len);
        String::from_utf8_unchecked(output)
    }
}

/// 解码到新分配的 Vec
#[inline]
pub fn from_base64(input: &str) -> Option<Vec<u8>> {
    let input = input.as_bytes();
    let len = input.len();

    // 长度检查
    if len == 0 {
        return Some(Vec::new());
    }

    let output_len = decoded_len(len)?;

    // 字符检查 - 使用迭代器方法
    if input.iter().any(|&b| BASE64_DECODE_TABLE[b as usize] == 0xFF) {
        return None;
    }

    let mut output: Vec<u8> = Vec::with_capacity(output_len);

    unsafe {
        decode_to_slice_unchecked(
            input,
            core::slice::from_raw_parts_mut(output.as_mut_ptr(), output_len),
        );
        output.set_len(output_len);
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert_eq!(to_base64(b""), "");
        assert_eq!(from_base64("").unwrap(), b"");
    }

    #[test]
    fn test_basic() {
        let test_cases = [
            (b"f" as &[u8], "Zg"),
            (b"fo", "Zm8"),
            (b"foo", "Zm8v"),
            (b"foob", "Zm8vYg"),
            (b"fooba", "Zm8vYmE"),
            (b"foobar", "Zm8vYmFy"),
        ];

        for (input, expected) in test_cases {
            let encoded = to_base64(input);
            assert_eq!(encoded, expected);
            assert_eq!(from_base64(&encoded).unwrap(), input);
        }
    }

    #[test]
    fn test_length_calculation() {
        assert_eq!(encoded_len(0), 0);
        assert_eq!(encoded_len(1), 2);
        assert_eq!(encoded_len(2), 3);
        assert_eq!(encoded_len(3), 4);
        assert_eq!(encoded_len(4), 6);
        assert_eq!(encoded_len(5), 7);
        assert_eq!(encoded_len(6), 8);

        assert_eq!(decoded_len(0), Some(0));
        assert_eq!(decoded_len(2), Some(1));
        assert_eq!(decoded_len(3), Some(2));
        assert_eq!(decoded_len(4), Some(3));
        assert_eq!(decoded_len(6), Some(4));
        assert_eq!(decoded_len(7), Some(5));
        assert_eq!(decoded_len(8), Some(6));
    }

    #[test]
    fn test_invalid_input() {
        assert!(from_base64("!@#$").is_none());
        assert!(from_base64("ABC").is_none()); // 长度 % 4 == 1
    }
}
