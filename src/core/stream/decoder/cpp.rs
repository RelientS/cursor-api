//! StreamCpp 协议转换器
//!
//! 将 protobuf 的 StreamCppResponse 转换为结构化事件流。
//!
//! 核心特性：
//! - 单个 protobuf 消息可能生成多个事件（并行检查）
//! - ModelInfo/RangeReplace 与 Text 互斥（协议层语义）

use crate::core::{
    aiserver::v1::StreamCppResponse,
    error::{CppError, CursorError},
};
use alloc::borrow::Cow;
use grpc_stream::{Buffer, decompress_gzip};
use prost::Message as _;

#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamMessage {
    ModelInfo {
        is_fused_cursor_prediction_model: bool,
        is_multidiff_model: bool,
    },
    BeginEdit,
    RangeReplace {
        start_line_number: i32,
        end_line_number_inclusive: i32,
        #[serde(skip_serializing_if = "Option::is_none")]
        binding_id: Option<prost::ByteStr>,
        #[serde(skip_serializing_if = "Option::is_none")]
        should_remove_leading_eol: Option<bool>,
    },
    Text {
        text: String,
    },
    CursorPrediction {
        relative_path: String,
        line_number_one_indexed: i32,
        expected_content: String,
        should_retrigger_cpp: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        binding_id: Option<prost::ByteStr>,
    },
    DoneEdit,
    DoneStream,
    Debug {
        #[serde(skip_serializing_if = "Option::is_none")]
        model_input: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_output: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stream_time: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total_time: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ttft_time: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        server_timing: Option<String>,
    },
    // Suggestion 未在 JS 实现中处理，保留但不生成
    // #[deprecated]
    // Suggestion { start_line: i32, confidence: i32 },
    Error {
        error: CppError,
    },
    StreamEnd,
}

impl StreamMessage {
    #[inline(always)]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::ModelInfo { .. } => "model_info",
            Self::BeginEdit => "begin_edit",
            Self::RangeReplace { .. } => "range_replace",
            Self::Text { .. } => "text",
            Self::CursorPrediction { .. } => "cursor_prediction",
            Self::DoneEdit => "done_edit",
            Self::DoneStream => "done_stream",
            Self::Debug { .. } => "debug",
            Self::Error { .. } => "error",
            Self::StreamEnd => "stream_end",
        }
    }
}

pub struct StreamDecoder {
    buffer: Buffer,
    empty_stream_count: usize,
}

impl StreamDecoder {
    #[inline]
    pub fn new() -> Self { Self { buffer: Buffer::with_capacity(64), empty_stream_count: 0 } }

    #[inline]
    pub fn get_empty_stream_count(&self) -> usize { self.empty_stream_count }

    #[inline]
    pub fn reset_empty_stream_count(&mut self) {
        if self.empty_stream_count > 0 {
            self.empty_stream_count = 0;
        }
    }

    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<StreamMessage>, ()> {
        if data.is_empty() || {
            self.buffer.extend_from_slice(data);
            self.buffer.len() < 5
        } {
            self.empty_stream_count += 1;
            return Err(());
        }

        self.reset_empty_stream_count();

        let mut iter = (&self.buffer).into_iter();
        let mut events = Vec::with_capacity(1);

        for raw_msg in &mut iter {
            if raw_msg.data.is_empty() {
                continue;
            }

            let is_compressed = raw_msg.r#type & 1 != 0;
            let msg_type = raw_msg.r#type >> 1;

            let data = if is_compressed {
                match decompress_gzip(raw_msg.data) {
                    Some(d) => Cow::Owned(d),
                    None => continue,
                }
            } else {
                Cow::Borrowed(raw_msg.data)
            };
            let data = &*data;

            match msg_type {
                0 => {
                    if let Ok(response) = StreamCppResponse::decode(data) {
                        process_protobuf_message(&mut events, response);
                    }
                }
                1 => {
                    if data.len() == 2 {
                        events.push(StreamMessage::StreamEnd);
                    } else if let Ok(error) = CursorError::from_slice(data) {
                        events.push(StreamMessage::Error { error: error.canonical().into() });
                    }
                }
                _ => {
                    eprintln!("收到未知消息类型: {}", raw_msg.r#type);
                    crate::debug!(
                        "消息类型: {}, 内容: {}",
                        raw_msg.r#type,
                        hex::encode(raw_msg.data)
                    );
                }
            }
        }

        self.buffer.advance(iter.offset());
        Ok(events)
    }
}

impl Default for StreamDecoder {
    fn default() -> Self { Self::new() }
}

/// 将单个 protobuf 消息转换为事件
fn process_protobuf_message(events: &mut Vec<StreamMessage>, response: StreamCppResponse) {
    let mut is_plain_text = true;

    // 1. ModelInfo
    if let Some(info) = response.model_info {
        events.push(StreamMessage::ModelInfo {
            is_fused_cursor_prediction_model: info.is_fused_cursor_prediction_model,
            is_multidiff_model: info.is_multidiff_model,
        });
        is_plain_text = false;
    }

    // 2. RangeReplace
    if let Some(range) = response.range_to_replace {
        events.push(StreamMessage::RangeReplace {
            start_line_number: range.start_line_number,
            end_line_number_inclusive: range.end_line_number_inclusive,
            binding_id: response.binding_id.clone(),
            should_remove_leading_eol: response.should_remove_leading_eol,
        });
        is_plain_text = false;
    }

    // 3. Text (only if is_plain_text)
    if is_plain_text && !response.text.is_empty() {
        events.push(StreamMessage::Text { text: response.text });
    }

    // 4. CursorPrediction
    if let Some(cursor) = response.cursor_prediction_target {
        events.push(StreamMessage::CursorPrediction {
            relative_path: cursor.relative_path,
            line_number_one_indexed: cursor.line_number_one_indexed,
            expected_content: cursor.expected_content,
            should_retrigger_cpp: cursor.should_retrigger_cpp,
            binding_id: response.binding_id,
        });
    }

    // 5. DoneEdit
    if response.done_edit.unwrap_or(false) {
        events.push(StreamMessage::DoneEdit);
    }

    // 6. BeginEdit
    if response.begin_edit.unwrap_or(false) {
        events.push(StreamMessage::BeginEdit);
    }

    // 7. DoneStream
    if response.done_stream.unwrap_or(false) {
        events.push(StreamMessage::DoneStream);
    }

    // 8. Debug
    if response.debug_model_input.is_some()
        || response.debug_model_output.is_some()
        || response.debug_stream_time.is_some()
        || response.debug_total_time.is_some()
        || response.debug_ttft_time.is_some()
        || response.debug_server_timing.is_some()
    {
        events.push(StreamMessage::Debug {
            model_input: response.debug_model_input,
            model_output: response.debug_model_output,
            stream_time: response.debug_stream_time,
            total_time: response.debug_total_time,
            ttft_time: response.debug_ttft_time,
            server_timing: response.debug_server_timing,
        });
    }

    // Suggestion: 未在 JS 实现中使用，不生成
    // if let (Some(start_line), Some(confidence)) =
    //     (response.suggestion_start_line, response.suggestion_confidence)
    // {
    //     events.push(StreamMessage::Suggestion { start_line, confidence });
    // }
}
