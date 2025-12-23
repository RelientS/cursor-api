use crate::app::{
    constant::{
        ASIA, CURSOR_GCPP_ASIA_HOST, CURSOR_GCPP_EU_HOST, CURSOR_GCPP_US_HOST, EU, HTTPS_PREFIX, US,
    },
    lazy::{
        asia_stream_cpp_url, asia_sync_file_url, asia_upload_file_url, eu_stream_cpp_url,
        eu_sync_file_url, eu_upload_file_url, us_stream_cpp_url, us_sync_file_url,
        us_upload_file_url,
    },
};

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
#[repr(u8)]
pub enum GcppHost {
    Asia,
    EU,
    US,
    // Ext(&'static str),
}

pub enum CppService {
    FSUploadFile,
    FSSyncFile,
    StreamCpp,
}

impl GcppHost {
    #[inline]
    pub fn from_str(s: &str) -> Option<Self> {
        match if let Some(s) = s.strip_prefix(HTTPS_PREFIX) { s } else { s } {
            CURSOR_GCPP_ASIA_HOST | ASIA => Some(Self::Asia),
            CURSOR_GCPP_EU_HOST | EU => Some(Self::EU),
            CURSOR_GCPP_US_HOST | US => Some(Self::US),
            _ => None,
            // _ => Self::Ext(crate::leak::intern_static(s)),
        }
    }

    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            Self::Asia => ASIA,
            Self::EU => EU,
            Self::US => US,
        }
    }

    #[inline]
    pub fn get_url(&self, i: CppService, use_pri: bool) -> &'static str {
        match (self, i) {
            (Self::Asia, CppService::FSUploadFile) => asia_upload_file_url(use_pri),
            (Self::Asia, CppService::FSSyncFile) => asia_sync_file_url(use_pri),
            (Self::Asia, CppService::StreamCpp) => asia_stream_cpp_url(use_pri),

            (Self::EU, CppService::FSUploadFile) => eu_upload_file_url(use_pri),
            (Self::EU, CppService::FSSyncFile) => eu_sync_file_url(use_pri),
            (Self::EU, CppService::StreamCpp) => eu_stream_cpp_url(use_pri),

            (Self::US, CppService::FSUploadFile) => us_upload_file_url(use_pri),
            (Self::US, CppService::FSSyncFile) => us_sync_file_url(use_pri),
            (Self::US, CppService::StreamCpp) => us_stream_cpp_url(use_pri),
        }
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(GcppHost::Asia),
            1 => Some(GcppHost::EU),
            2 => Some(GcppHost::US),
            _ => None,
        }
    }
}

impl ::core::fmt::Display for GcppHost {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ::serde::Serialize for GcppHost {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: ::serde::Serializer {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> ::serde::Deserialize<'de> for GcppHost {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: ::serde::Deserializer<'de> {
        Self::from_str(&String::deserialize(deserializer)?).ok_or(::serde::de::Error::custom("Invalid GCPP host value. Valid options are: 'us-asia.gcpp.cursor.sh', 'Asia', 'us-eu.gcpp.cursor.sh', 'EU', 'us-only.gcpp.cursor.sh', or 'US'"))
    }
}
