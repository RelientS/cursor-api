//! gRPC 流式消息解码器
//!
//! 提供高性能的 gRPC streaming 消息解析，支持 gzip 压缩。
//!
//! # 示例
//!
//! ```no_run
//! use grpc_stream_decoder::StreamDecoder;
//! use prost::Message;
//!
//! #[derive(Message, Default)]
//! struct MyMessage {
//!     #[prost(string, tag = "1")]
//!     content: String,
//! }
//!
//! let mut decoder = StreamDecoder::<MyMessage>::new();
//!
//! // 接收到的数据块
//! let chunk = receive_data();
//! let messages = decoder.decode(&chunk);
//!
//! for msg in messages {
//!     println!("{}", msg.content);
//! }
//! ```

#![allow(internal_features)]
#![feature(core_intrinsics)]

mod frame;
mod buffer;
mod compression;
mod decoder;

// 公开 API
pub use frame::RawMessage;
pub use buffer::Buffer;
pub use compression::decompress_gzip;
pub use decoder::StreamDecoder;

// 常量
/// 最大解压缩消息大小限制（4 MiB）
/// 
/// 对齐gRPC标准的默认最大消息大小，防止内存滥用攻击
pub const MAX_DECOMPRESSED_SIZE_BYTES: usize = 0x400000; // 4 * 1024 * 1024
