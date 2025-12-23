//! 流式消息解码器

use prost::Message;

use crate::buffer::Buffer;
use crate::compression::decompress_gzip;
use crate::frame::RawMessage;

/// gRPC 流式消息解码器
///
/// 处理增量数据块，解析完整的 Protobuf 消息。
///
/// # 示例
///
/// ```no_run
/// use grpc_stream_decoder::StreamDecoder;
/// use prost::Message;
///
/// #[derive(Message, Default)]
/// struct MyMessage {
///     #[prost(string, tag = "1")]
///     content: String,
/// }
///
/// let mut decoder = StreamDecoder::new();
///
/// // 使用默认处理器
/// loop {
///     let chunk = receive_network_data();
///     let messages: Vec<MyMessage> = decoder.decode_default(&chunk);
///     
///     for msg in messages {
///         process(msg);
///     }
/// }
///
/// // 使用自定义处理器
/// let messages = decoder.decode(&chunk, |raw_msg| {
///     // 自定义解码逻辑
///     match raw_msg.r#type {
///         0 => MyMessage::decode(raw_msg.data).ok(),
///         _ => None,
///     }
/// });
/// ```
pub struct StreamDecoder {
    buffer: Buffer,
}

impl StreamDecoder {
    /// 创建新的解码器
    #[inline]
    pub fn new() -> Self { Self { buffer: Buffer::new() } }

    /// 使用自定义处理器解码数据块
    ///
    /// # 类型参数
    /// - `T`: 目标消息类型
    /// - `F`: 处理函数，签名为 `Fn(RawMessage<'_>) -> Option<T>`
    ///
    /// # 参数
    /// - `data`: 接收到的数据块
    /// - `processor`: 自定义处理函数，接收原始消息并返回解码结果
    ///
    /// # 返回
    /// 解码成功的消息列表
    ///
    /// # 示例
    ///
    /// ```no_run
    /// // 自定义处理：只接受未压缩消息
    /// let messages = decoder.decode(&data, |raw_msg| {
    ///     if raw_msg.r#type == 0 {
    ///         MyMessage::decode(raw_msg.data).ok()
    ///     } else {
    ///         None
    ///     }
    /// });
    /// ```
    pub fn decode<T, F>(&mut self, data: &[u8], processor: F) -> Vec<T>
    where F: Fn(RawMessage<'_>) -> Option<T> {
        self.buffer.extend_from_slice(data);

        let mut iter = (&self.buffer).into_iter();
        let exact_count = iter.len();
        let mut messages = Vec::with_capacity(exact_count);

        for raw_msg in &mut iter {
            if let Some(msg) = processor(raw_msg) {
                messages.push(msg);
            }
        }

        self.buffer.advance(iter.offset());
        messages
    }

    /// 使用默认处理器解码数据块
    ///
    /// 默认行为：
    /// - 类型 0：直接解码 Protobuf 消息
    /// - 类型 1：先 gzip 解压，再解码
    /// - 其他类型：忽略
    ///
    /// # 类型参数
    /// - `T`: 实现 `prost::Message + Default` 的消息类型
    ///
    /// # 参数
    /// - `data`: 接收到的数据块
    ///
    /// # 返回
    /// 解码成功的消息列表
    pub fn decode_default<T: Message + Default>(&mut self, data: &[u8]) -> Vec<T> {
        self.decode(data, |raw_msg| match raw_msg.r#type {
            0 => Self::decode_message(raw_msg.data),
            1 => Self::decode_compressed_message(raw_msg.data),
            _ => None,
        })
    }

    /// 解码未压缩消息
    #[inline]
    fn decode_message<T: Message + Default>(data: &[u8]) -> Option<T> { T::decode(data).ok() }

    /// 解码 gzip 压缩消息
    #[inline]
    fn decode_compressed_message<T: Message + Default>(data: &[u8]) -> Option<T> {
        let decompressed = decompress_gzip(data)?;
        Self::decode_message(&decompressed)
    }
}

impl Default for StreamDecoder {
    fn default() -> Self { Self::new() }
}
