// use ::bytes::{Buf as _, BytesMut};

use super::{
    decompress_gzip,
    types::{DecodedMessage, DecoderError, ProtobufMessage},
};
use alloc::borrow::Cow;

// #[derive(Clone)]
// pub struct DirectDecoder<T: ProtobufMessage> {
//     buf: BytesMut,
//     _phantom: std::marker::PhantomData<T>,
// }

// impl<T: ProtobufMessage> DirectDecoder<T> {
//     pub fn new() -> Self {
//         Self {
//             buf: BytesMut::new(),
//             _phantom: std::marker::PhantomData,
//         }
//     }

//     pub fn decode(&mut self, data: &[u8]) -> Result<Option<DecodedMessage<T>>, DecoderError> {
//         self.buf.extend_from_slice(data);

//         // 首先尝试按带头部的格式处理
//         if self.buf.len() >= 5 && self.buf[0] <= 1 {
//             // 检查头部
//             let is_compressed = data[0] == 1;
//             let msg_len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;

//             // 如果数据完整，按带头部格式处理
//             if self.buf.len() == 5 + msg_len {
//                 self.buf.advance(5);

//                 if is_compressed {
//                     match decompress_gzip(&self.buf) {
//                         Some(data) => {
//                             self.buf = data.as_slice().into();
//                         }
//                         None => return Err(DecoderError::Internal("decompress error")),
//                     }
//                 };

//                 if let Ok(msg) = T::decode(&self.buf[..]) {
//                     return Ok(Some(DecodedMessage::Protobuf(msg)));
//                 } else if let Ok(text) = String::from_utf8(self.buf.to_vec()) {
//                     return Ok(Some(DecodedMessage::Text(text)));
//                 }
//             }
//         }

//         // 如果不是带头部的格式，尝试直接处理数据
//         // 首先尝试解压（可能是压缩的直接数据）
//         if let Some(decompressed) = decompress_gzip(&self.buf) {
//             self.buf = decompressed.as_slice().into();
//         };

//         // 尝试解析
//         if let Ok(msg) = T::decode(&self.buf[..]) {
//             self.buf.clear();
//             Ok(Some(DecodedMessage::Protobuf(msg)))
//         } else if let Ok(text) = String::from_utf8(self.buf.to_vec()) {
//             self.buf.clear();
//             Ok(Some(DecodedMessage::Text(text)))
//         } else {
//             Ok(None)
//         }
//     }
// }

pub fn decode<T: ProtobufMessage>(data: &[u8]) -> Result<DecodedMessage<T>, DecoderError> {
    // 首先尝试按带头部的格式处理
    if data.len() >= 5 && data[0] <= 1 {
        // 检查头部
        let is_compressed = data[0] == 1;
        let msg_len = u32::from_be_bytes([data[1], data[2], data[3], data[4]]) as usize;

        // 如果数据完整，按带头部格式处理
        if data.len() == 5 + msg_len {
            let payload = &data[5..];

            let decompressed = if is_compressed {
                match decompress_gzip(payload) {
                    Some(data) => Cow::Owned(data),
                    None => return Err(DecoderError::Internal("decompress error")),
                }
            } else {
                Cow::Borrowed(payload)
            };

            if let Ok(msg) = T::decode(&*decompressed) {
                return Ok(DecodedMessage::Protobuf(msg));
            } else if let Some(text) = super::utils::string_from_utf8(decompressed) {
                return Ok(DecodedMessage::Text(text));
            }
        }
    }

    // 尝试解析
    if let Ok(msg) = T::decode(data) {
        Ok(DecodedMessage::Protobuf(msg))
    } else if let Some(text) = super::utils::string_from_utf8(data) {
        Ok(DecodedMessage::Text(text))
    } else {
        Err(DecoderError::Internal("decode error"))
    }
}
