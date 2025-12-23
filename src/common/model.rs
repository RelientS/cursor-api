pub mod config;
pub mod error;
pub mod health;
pub mod token;
pub mod tri;
pub mod userinfo;
pub mod stringify;
pub mod ntp;

use alloc::borrow::Cow;

use serde::{Serialize, Serializer};
use serde::ser::SerializeStruct;
use http::StatusCode;

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiStatus {
    Success,
    Error,
}

pub struct GenericError {
    pub status: ApiStatus,
    pub code: Option<StatusCode>,
    pub error: Option<Cow<'static, str>>,
    pub message: Option<Cow<'static, str>>,
}

impl Serialize for GenericError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: Serializer {
        let field_count = 1 // status 总是存在
            + self.code.is_some() as usize
            + self.error.is_some() as usize
            + self.message.is_some() as usize;

        let mut state = serializer.serialize_struct("GenericError", field_count)?;

        state.serialize_field("status", &self.status)?;

        if let Some(ref code) = self.code {
            state.serialize_field("code", &code.as_u16())?;
        }

        if let Some(ref error) = self.error {
            state.serialize_field("error", error)?;
        }

        if let Some(ref message) = self.message {
            state.serialize_field("message", message)?;
        }

        state.end()
    }
}

#[allow(unused)]
#[derive(Clone)]
pub struct HeaderValue {
    pub inner: bytes::Bytes,
    pub is_sensitive: bool,
}

impl HeaderValue {
    #[inline(always)]
    pub const fn into(self) -> http::HeaderValue { unsafe { core::mem::transmute(self) } }
    #[inline]
    pub const fn from_static(src: &'static str) -> HeaderValue {
        HeaderValue { inner: bytes::Bytes::from_static(src.as_bytes()), is_sensitive: false }
    }
}

#[inline]
pub fn is_default<T>(v: &T) -> bool
where T: Default + PartialEq {
    *v == T::default()
}
