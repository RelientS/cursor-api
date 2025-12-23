use alloc::borrow::Cow;

use crate::core::model::{anthropic, openai};
use super::GenericError;

pub enum ChatError {
    ModelNotSupported(String),
    EmptyMessages,
    RequestFailed(Cow<'static, str>),
    ProcessingFailed(Cow<'static, str>),
}

impl ChatError {
    #[inline]
    pub fn error_type(&self) -> &'static str {
        match self {
            Self::ModelNotSupported(_) => "model_not_supported",
            Self::EmptyMessages => "empty_messages",
            Self::RequestFailed(_) => "request_failed",
            Self::ProcessingFailed(_) => "processing_failed",
        }
    }
}

impl core::fmt::Display for ChatError {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ModelNotSupported(model) => write!(f, "Model '{model}' is not supported"),
            Self::EmptyMessages => write!(f, "Message array cannot be empty"),
            Self::RequestFailed(err) => write!(f, "Request failed: {err}"),
            Self::ProcessingFailed(err) => write!(f, "Processing failed: {err}"),
        }
    }
}

impl ChatError {
    #[inline]
    pub fn to_generic(&self) -> GenericError {
        GenericError {
            status: super::ApiStatus::Error,
            code: None,
            error: Some(Cow::Borrowed(self.error_type())),
            message: Some(Cow::Owned(self.to_string())),
        }
    }

    #[inline]
    pub fn to_openai(&self) -> openai::OpenAiError {
        openai::OpenAiErrorInner {
            code: Some(Cow::Borrowed(self.error_type())),
            message: Cow::Owned(self.to_string()),
        }
        .wrapped()
    }

    #[inline]
    pub fn to_anthropic(&self) -> anthropic::AnthropicError {
        anthropic::AnthropicErrorInner {
            r#type: self.error_type(),
            message: Cow::Owned(self.to_string()),
        }
        .wrapped()
    }
}
