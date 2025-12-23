use core::sync::atomic::{AtomicU64, Ordering};

// Base64 URL_SAFE_NO_PAD 编码表
const B64_ENCODE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

// 全局缓存的时间戳头
static TIMESTAMP_HEADER: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
const fn obfuscate_bytes(bytes: &mut [u8; 6]) {
    let mut prev = 165u8;

    bytes[0] = (bytes[0] ^ prev).wrapping_add(0);
    prev = bytes[0];

    bytes[1] = (bytes[1] ^ prev).wrapping_add(1);
    prev = bytes[1];

    bytes[2] = (bytes[2] ^ prev).wrapping_add(2);
    prev = bytes[2];

    bytes[3] = (bytes[3] ^ prev).wrapping_add(3);
    prev = bytes[3];

    bytes[4] = (bytes[4] ^ prev).wrapping_add(4);
    prev = bytes[4];

    bytes[5] = (bytes[5] ^ prev).wrapping_add(5);
}

#[inline(always)]
const fn encode_base64(input: &[u8; 6]) -> [u8; 8] {
    let mut output = [0u8; 8];

    unsafe {
        encode_chunk(&*(input.as_ptr() as *const _), &mut *(output.as_mut_ptr() as *mut _));

        encode_chunk(
            &*(input.as_ptr().add(3) as *const _),
            &mut *(output.as_mut_ptr().add(4) as *mut _),
        );
    }

    output
}

#[inline(always)]
const fn encode_chunk(input: &[u8; 3], output: &mut [u8; 4]) {
    let b0 = input[0];
    let b1 = input[1];
    let b2 = input[2];

    output[0] = B64_ENCODE[(b0 >> 2) as usize];
    output[1] = B64_ENCODE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize];
    output[2] = B64_ENCODE[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize];
    output[3] = B64_ENCODE[(b2 & 0x3F) as usize];
}

// 从千秒创建
#[inline]
const fn new(kilo_seconds: u64) -> u64 {
    let mut timestamp_bytes = [
        ((kilo_seconds >> 8) & 0xFF) as u8,
        (kilo_seconds & 0xFF) as u8,
        ((kilo_seconds >> 24) & 0xFF) as u8,
        ((kilo_seconds >> 16) & 0xFF) as u8,
        ((kilo_seconds >> 8) & 0xFF) as u8,
        (kilo_seconds & 0xFF) as u8,
    ];

    obfuscate_bytes(&mut timestamp_bytes);
    u64::from_ne_bytes(encode_base64(&timestamp_bytes))
}

// 获取全局指针
#[inline(always)]
pub fn read() -> [u8; 8] { TIMESTAMP_HEADER.load(Ordering::Relaxed).to_ne_bytes() }

// 使用指定千秒更新全局原子变量
#[inline]
pub fn update_global_with(kilo_seconds: u64) {
    TIMESTAMP_HEADER.store(new(kilo_seconds), Ordering::Relaxed)
}
