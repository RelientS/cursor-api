use alloc::borrow::Cow;

use serde::{Deserialize, Serialize, Serializer};
use serde::ser::{SerializeStruct as _, SerializeTuple as _};

use crate::app::constant::{ERROR, FINISH_REASON_STOP, TYPE};
use crate::common::model::tri::TriState;
use super::Role;

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    String(String),
    Array(Vec<MessageContentObject>),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContentObject {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

impl MessageContentObject {
    #[inline]
    pub fn into_text(self) -> Option<String> {
        match self {
            Self::Text { text } => Some(text),
            Self::ImageUrl { .. } => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
}

#[derive(Serialize)]
pub struct ChatResponse<'a> {
    pub id: &'a str,
    pub object: &'static str,
    pub created: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<&'static str>,
    #[serde(serialize_with = "serialize_option_choice")]
    pub choices: Option<Choice>,
    #[serde(skip_serializing_if = "TriState::is_undefined")]
    pub usage: TriState<Usage>,
}

fn serialize_option_choice<S>(option: &Option<Choice>, serializer: S) -> Result<S::Ok, S::Error>
where S: Serializer {
    let mut tup = serializer.serialize_tuple(option.is_some() as usize)?;
    match option {
        Some(choice) => {
            // 序列化为单元素数组 [choice]
            tup.serialize_element(choice)?;
        }
        None => {
            // 序列化为空数组 []
        }
    }
    tup.end()
}

pub struct Choice {
    pub index: i32,
    pub message: Option<Message>,
    pub delta: Option<Delta>,
    pub finish_reason: bool,
}

impl Serialize for Choice {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        let mut field_count = 3;

        if self.message.is_some() {
            field_count += 1;
        }
        if self.delta.is_some() {
            field_count += 1;
        }

        let mut state = serializer.serialize_struct("choice", field_count)?;

        state.serialize_field("index", &self.index)?;

        if let Some(ref message) = self.message {
            state.serialize_field("message", message)?;
        }

        if let Some(ref delta) = self.delta {
            state.serialize_field("delta", delta)?;
        }

        state.serialize_field("logprobs", &None::<bool>)?;
        state.serialize_field(
            "finish_reason",
            &if self.finish_reason { Some(FINISH_REASON_STOP) } else { None },
        )?;

        state.end()
    }
}

#[derive(Serialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Cow<'static, str>>,
}

#[derive(Default)]
pub struct PromptTokensDetails {
    pub cached_tokens: i32,
    // pub audio_tokens: i32,
}

impl PromptTokensDetails {
    #[inline]
    fn is_zero(&self) -> bool { self.cached_tokens == 0 }
}

impl Serialize for PromptTokensDetails {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        let mut state = serializer.serialize_struct("prompt_tokens_details", 1)?;
        state.serialize_field("cached_tokens", &self.cached_tokens)?;
        state.end()
    }
}

// #[derive(Default)]
// pub struct CompletionTokensDetails {
//   pub reasoning_tokens: i32,
//   // pub audio_tokens: i32,
//   // pub accepted_prediction_tokens: i32,
//   // pub rejected_prediction_tokens: i32,
// }

// impl Serialize for CompletionTokensDetails {
//   #[inline]
//   fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
//   where
//     S: Serializer,
//   {
//     let mut state = serializer.serialize_struct("completion_tokens_details", 1)?;
//     state.serialize_field("reasoning_tokens", &self.reasoning_tokens)?;
//     state.end()
//   }
// }

#[derive(Serialize, Default)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    #[serde(default, skip_serializing_if = "PromptTokensDetails::is_zero")]
    pub prompt_tokens_details: PromptTokensDetails,
    // pub completion_tokens_details: CompletionTokensDetails,
}

// 聊天请求
#[derive(Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    // #[serde(default)]
    // pub reasoning_effort: ReasoningEffort,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub stream_options: Option<StreamOptions>,
}

// #[derive(Deserialize, Default)]
// #[serde(rename_all = "lowercase")]
// pub enum ReasoningEffort {
//     Minimal,
//     Low,
//     #[default]
//     Medium,
//     High,
// }

#[derive(Deserialize)]
pub struct StreamOptions {
    pub include_usage: bool,
}

mod private {
    use super::*;

    pub struct ErrorDetail {
        pub code: Option<Cow<'static, str>>,
        pub message: Cow<'static, str>,
    }

    impl ErrorDetail {
        #[inline(always)]
        pub const fn wrapped(self) -> OpenAiError { OpenAiError(self) }
    }
}

pub type OpenAiErrorInner = private::ErrorDetail;

#[repr(transparent)]
pub struct OpenAiError(private::ErrorDetail);

impl Serialize for OpenAiError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        let mut state = serializer.serialize_struct("OpenAIError", 4)?;
        state.serialize_field(TYPE, ERROR)?;
        state.serialize_field("code", &self.0.code)?;
        state.serialize_field("message", &self.0.message)?;
        state.serialize_field("param", &None::<bool>)?;
        state.end()
    }
}
