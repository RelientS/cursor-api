//! 原始消息帧定义

/// gRPC 流式消息的原始帧
///
/// 包含帧头信息和消息数据的引用。
///
/// # 帧格式
///
/// ```text
/// +------+----------+----------------+
/// | type | length   | data           |
/// | 1B   | 4B (BE)  | length bytes   |
/// +------+----------+----------------+
/// ```
///
/// - `type`: 消息类型
///   - `0`: 未压缩
///   - `1`: gzip 压缩
/// - `length`: 消息体长度（大端序）
/// - `data`: 消息体数据
///
/// # 字段说明
///
/// - `r#type`: 帧类型标志（0=未压缩, 1=gzip）
/// - `data`: 消息体数据切片，其长度可通过 `data.len()` 获取
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawMessage<'b> {
    /// 消息类型（0=未压缩, 1=gzip）
    pub r#type: u8,

    /// 消息体数据
    pub data: &'b [u8],
}

impl RawMessage<'_> {
    /// 计算该消息在缓冲区中占用的总字节数
    ///
    /// 包含 5 字节帧头 + 消息体长度
    ///
    /// # 示例
    ///
    /// ```
    /// # use grpc_stream_decoder::RawMessage;
    /// let msg = RawMessage {
    ///     r#type: 0,
    ///     data: &[1, 2, 3],
    /// };
    /// assert_eq!(msg.total_size(), 8); // 5 + 3
    /// ```
    #[inline]
    pub const fn total_size(&self) -> usize {
        5 + self.data.len()
    }
}
