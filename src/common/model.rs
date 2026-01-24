pub mod cached;
pub mod error;
pub mod health;
pub mod niche_types;
pub mod ntp;
pub mod raw_json;
pub mod token;
pub mod tri;
pub mod userinfo;

use alloc::borrow::Cow;
use core::mem::transmute;
use http::status::StatusCode;
use serde::{Serialize, Serializer, ser::SerializeStruct as _};

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
    pub const unsafe fn into(self) -> http::header::HeaderValue { unsafe { transmute(self) } }
    #[inline]
    pub const fn from_static(src: &'static str) -> HeaderValue {
        HeaderValue { inner: bytes::Bytes::from_static(src.as_bytes()), is_sensitive: false }
    }
    #[inline]
    pub fn from_bytes(src: &[u8]) -> HeaderValue {
        HeaderValue { inner: bytes::Bytes::copy_from_slice(src), is_sensitive: false }
    }
    pub fn from_integer<I: ::itoa::Integer>(i: I) -> http::header::HeaderValue {
        unsafe { Self::from_bytes(::itoa::Buffer::new().format(i).as_bytes()).into() }
    }
    pub fn validate(src: &[u8]) -> Result<(), http::header::InvalidHeaderValue> {
        for &b in src {
            if !is_valid(b) {
                return Err(unsafe { transmute(()) });
            }
        }
        Ok(())
    }
}

impl const From<http::header::HeaderValue> for HeaderValue {
    fn from(value: http::header::HeaderValue) -> Self { unsafe { transmute(value) } }
}

impl const From<&http::header::HeaderValue> for &HeaderValue {
    fn from(value: &http::header::HeaderValue) -> Self { unsafe { transmute(value) } }
}

impl From<String> for HeaderValue {
    fn from(value: String) -> Self { Self { inner: value.into(), is_sensitive: false } }
}

#[inline]
fn is_valid(b: u8) -> bool { b >= 32 && b != 127 || b == b'\t' }

#[inline]
pub fn is_default<T>(v: &T) -> bool
where T: Default + PartialEq {
    *v == T::default()
}
