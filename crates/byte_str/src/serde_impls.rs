#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use super::*;

impl serde_core::Serialize for ByteStr {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde_core::Serializer {
        serializer.serialize_str(self)
    }
}

struct ByteStrVisitor;

impl<'de> serde_core::de::Visitor<'de> for ByteStrVisitor {
    type Value = ByteStr;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a UTF-8 string")
    }

    #[inline]
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where E: serde_core::de::Error {
        Ok(ByteStr::from(v))
    }

    #[inline]
    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where E: serde_core::de::Error {
        Ok(ByteStr::from(v))
    }

    #[inline]
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where E: serde_core::de::Error {
        match str::from_utf8(v) {
            Ok(s) => Ok(ByteStr::from(s)),
            Err(e) => Err(E::custom(format_args!("invalid UTF-8: {e}"))),
        }
    }

    #[inline]
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where E: serde_core::de::Error {
        match String::from_utf8(v) {
            Ok(s) => Ok(ByteStr::from(s)),
            Err(e) => Err(E::custom(format_args!("invalid UTF-8: {}", e.utf8_error()))),
        }
    }

    #[inline]
    fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
    where V: serde_core::de::SeqAccess<'de> {
        use serde_core::de::Error as _;
        let len = core::cmp::min(seq.size_hint().unwrap_or(0), 4096);
        let mut bytes: Vec<u8> = Vec::with_capacity(len);

        while let Some(value) = seq.next_element()? {
            bytes.push(value);
        }

        match String::from_utf8(bytes) {
            Ok(s) => Ok(ByteStr::from(s)),
            Err(e) => Err(V::Error::custom(format_args!("invalid UTF-8: {}", e.utf8_error()))),
        }
    }
}

impl<'de> serde_core::Deserialize<'de> for ByteStr {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<ByteStr, D::Error>
    where D: serde_core::Deserializer<'de> {
        deserializer.deserialize_string(ByteStrVisitor)
    }
}
