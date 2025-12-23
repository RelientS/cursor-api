use alloc::borrow::Cow;

use crate::app::constant::EMPTY_STRING;
use crate::common::utils::parse_from_env;

#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum ModelIdSource {
    Id,
    ClientId,
    ServerId,
}

impl ModelIdSource {
    const ID: &'static str = "id";
    const CLIENT_ID: &'static str = "client_id";
    const SERVER_ID: &'static str = "server_id";

    #[inline]
    pub fn from_env() -> Self {
        let mut s = parse_from_env("MODEL_ID_SOURCE", EMPTY_STRING);
        let s = match s {
            Cow::Borrowed(s) => s,
            Cow::Owned(ref mut s) => {
                s.make_ascii_lowercase();
                s
            }
        };
        match s {
            Self::ID => Self::Id,
            Self::CLIENT_ID => Self::ClientId,
            Self::SERVER_ID => Self::ServerId,
            _ => Self::default(),
        }
    }

    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Id => Self::ID,
            Self::ClientId => Self::CLIENT_ID,
            Self::ServerId => Self::SERVER_ID,
        }
    }
}

impl const Default for ModelIdSource {
    #[inline(always)]
    fn default() -> Self {
        Self::ClientId
    }
}

impl ::serde::Serialize for ModelIdSource {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}
