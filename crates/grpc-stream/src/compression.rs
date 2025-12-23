//! 压缩数据处理

use std::io::Read as _;

use flate2::read::GzDecoder;

use crate::MAX_DECOMPRESSED_SIZE_BYTES;

/// 解压 gzip 数据
///
/// # 参数
/// - `data`: gzip 压缩的数据
///
/// # 返回
/// - `Some(Vec<u8>)`: 解压成功
/// - `None`: 不是有效的 gzip 数据或解压失败
///
/// # 最小 GZIP 文件结构
///
/// ```text
/// +----------+-------------+----------+
/// | Header   | DEFLATE     | Footer   |
/// | 10 bytes | 2+ bytes    | 8 bytes  |
/// +----------+-------------+----------+
/// 最小: 10 + 2 + 8 = 20 字节
/// ```
///
/// # 安全性
/// - 限制解压后大小不超过 `MAX_DECOMPRESSED_SIZE_BYTES`
/// - 防止 gzip 炸弹攻击
pub fn decompress_gzip(data: &[u8]) -> Option<Vec<u8>> {
    // 快速路径：拒绝明显无效的数据
    // 最小有效 gzip 文件为 20 字节（头10 + 数据2 + 尾8）
    if data.len() < 20 {
        return None;
    }

    // SAFETY: 上面已验证 data.len() >= 20，保证索引 0, 1, 2 有效
    // 检查 gzip 魔数（0x1f 0x8b）和压缩方法（0x08 = DEFLATE）
    if unsafe {
        *data.get_unchecked(0) != 0x1f
            || *data.get_unchecked(1) != 0x8b
            || *data.get_unchecked(2) != 0x08
    } {
        return None;
    }

    // 读取 gzip footer 中的 ISIZE（原始大小，最后 4 字节，小端序）
    // SAFETY: 已验证 data.len() >= 20，末尾 4 字节必然有效
    let capacity = unsafe {
        let ptr = data.as_ptr().add(data.len() - 4) as *const [u8; 4];
        u32::from_le_bytes(ptr.read()) as usize
    };

    // 防止解压炸弹攻击
    if capacity > MAX_DECOMPRESSED_SIZE_BYTES {
        return None;
    }

    // 执行实际解压
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::with_capacity(capacity);

    decoder.read_to_end(&mut decompressed).ok()?;

    Some(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_too_short() {
        // 小于 20 字节的数据应该直接拒绝
        assert!(decompress_gzip(&[]).is_none());
        assert!(decompress_gzip(&[0x1f, 0x8b, 0x08]).is_none());
        assert!(decompress_gzip(&[0u8; 19]).is_none());
    }

    #[test]
    fn test_invalid_magic() {
        // 长度足够但魔数错误
        let mut data = vec![0u8; 20];
        data[0] = 0x00; // 错误的魔数
        data[1] = 0x8b;
        data[2] = 0x08;
        assert!(decompress_gzip(&data).is_none());

        // 正确的第一字节，错误的第二字节
        data[0] = 0x1f;
        data[1] = 0x00;
        assert!(decompress_gzip(&data).is_none());

        // 前两字节正确，压缩方法错误
        data[1] = 0x8b;
        data[2] = 0x09; // 非 DEFLATE
        assert!(decompress_gzip(&data).is_none());
    }

    #[test]
    fn test_gzip_bomb_protection() {
        // 构造声称解压后为 2MB 的假 gzip 数据
        let mut fake_gzip = vec![0x1f, 0x8b, 0x08]; // 正确的魔数
        fake_gzip.extend_from_slice(&[0u8; 14]); // 填充到 17 字节

        // ISIZE 字段（最后 4 字节）：2MB
        let size_2mb = 2 * 1024 * 1024u32;
        fake_gzip.extend_from_slice(&size_2mb.to_le_bytes());

        assert_eq!(fake_gzip.len(), 21); // 17 + 4
        assert!(decompress_gzip(&fake_gzip).is_none());
    }

    #[test]
    fn test_valid_gzip() {
        // 使用标准库压缩一些数据
        use std::io::Write;

        use flate2::write::GzEncoder;
        use flate2::Compression;

        let original = b"Hello, GZIP!";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        // 验证：压缩数据 >= 20 字节
        assert!(compressed.len() >= 20);

        // 解压并验证
        let decompressed = decompress_gzip(&compressed).unwrap();
        assert_eq!(&decompressed, original);
    }

    #[test]
    fn test_empty_gzip() {
        // 压缩空数据（最小有效 gzip）
        use std::io::Write;

        use flate2::write::GzEncoder;
        use flate2::Compression;

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&[]).unwrap();
        let compressed = encoder.finish().unwrap();

        // 验证：最小 gzip 文件 ~20 字节
        assert!(compressed.len() >= 20);

        let decompressed = decompress_gzip(&compressed).unwrap();
        assert_eq!(decompressed.len(), 0);
    }
}
