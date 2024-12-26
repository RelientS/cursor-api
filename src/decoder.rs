use crate::StreamChatResponse;
use brotli::Decompressor;
use flate2::read::GzDecoder;
use prost::Message as _;
use std::error::Error as StdError;
use std::fmt;
use std::io::{Cursor, Read};

pub struct StreamDecoder;

impl Default for StreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self
    }

    pub fn process_chunk(&self, data: &[u8]) -> Result<String, Box<dyn StdError + Send + Sync>> {
        // 1. 首先尝试 proto 解码
        let hex = hex::encode(data);
        let mut offset = 0;
        let mut results = Vec::new();

        while offset + 10 <= hex.len() {
            match i64::from_str_radix(&hex[offset..offset + 10], 16) {
                Ok(data_length) => {
                    offset += 10;
                    if offset + (data_length * 2) as usize > hex.len() {
                        break;
                    }

                    let message_hex = &hex[offset..offset + (data_length * 2) as usize];
                    offset += (data_length * 2) as usize;

                    if let Ok(message_buffer) = hex::decode(message_hex) {
                        if let Ok(message) = StreamChatResponse::decode(&message_buffer[..]) {
                            results.push(message.text);
                        }
                    }
                }
                _ => break,
            }
        }

        if !results.is_empty() {
            return Ok(results.join(""));
        }

        // 2. 如果 proto 解码失败，尝试 gzip 解压
        if data.len() > 5 && data[0] == 0x1f {
            let mut decoder = GzDecoder::new(&data[5..]);
            let mut text = String::new();
            if decoder.read_to_string(&mut text).is_ok() && !text.contains("<|BEGIN_SYSTEM|>") {
                return Ok(text);
            }
            return Ok(String::new());
        }

        // 3. 如果 gzip 失败，尝试 brotli 解压
        if data.len() > 5 && data[0] == 0x0b {
            let mut decoder = Decompressor::new(
                Cursor::new(&data[5..]),
                4096, // 默认的缓冲区大小
            );
            let mut text = String::new();
            if decoder.read_to_string(&mut text).is_ok() && !text.contains("<|BEGIN_SYSTEM|>") {
                return Ok(text);
            }
            return Ok(String::new());
        }

        // 4. 如果所有解码方式都失败，返回空字符串
        Ok(String::new())
    }
}

#[derive(Debug)]
pub enum DecoderError {
    InvalidLength,
    HexDecode(hex::FromHexError),
    ProtoDecode(prost::DecodeError),
    Decompress(std::io::Error),
    Utf8(std::string::FromUtf8Error),
}

// 实现 Display trait
impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength => write!(f, "Invalid message length"),
            Self::HexDecode(e) => write!(f, "Hex decode error: {}", e),
            Self::ProtoDecode(e) => write!(f, "Proto decode error: {}", e),
            Self::Decompress(e) => write!(f, "Decompression error: {}", e),
            Self::Utf8(e) => write!(f, "UTF-8 decode error: {}", e),
        }
    }
}

// 实现 Error trait
impl StdError for DecoderError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::InvalidLength => None,
            Self::HexDecode(e) => Some(e),
            Self::ProtoDecode(e) => Some(e),
            Self::Decompress(e) => Some(e),
            Self::Utf8(e) => Some(e),
        }
    }
}
