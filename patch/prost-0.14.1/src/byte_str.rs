use core::borrow::Borrow;
use core::{fmt, ops, str};
use core::str::pattern::{Pattern, ReverseSearcher, Searcher as _};
#[cfg(not(feature = "std"))]
use alloc::string::String;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use bytes::Bytes;

#[allow(unused)]
struct BytesUnsafeView {
    ptr: *const u8,
    len: usize,
    // inlined "trait object"
    data: core::sync::atomic::AtomicPtr<()>,
    vtable: &'static Vtable,
}

#[allow(unused)]
struct Vtable {
    /// fn(data, ptr, len)
    clone: unsafe fn(&core::sync::atomic::AtomicPtr<()>, *const u8, usize) -> Bytes,
    /// fn(data, ptr, len)
    ///
    /// takes `Bytes` to value
    to_vec: unsafe fn(&core::sync::atomic::AtomicPtr<()>, *const u8, usize) -> Vec<u8>,
    to_mut: unsafe fn(&core::sync::atomic::AtomicPtr<()>, *const u8, usize) -> bytes::BytesMut,
    /// fn(data)
    is_unique: unsafe fn(&core::sync::atomic::AtomicPtr<()>) -> bool,
    /// fn(data, ptr, len)
    drop: unsafe fn(&mut core::sync::atomic::AtomicPtr<()>, *const u8, usize),
}

impl BytesUnsafeView {
    #[inline]
    const fn from(src: Bytes) -> Self { unsafe { ::core::intrinsics::transmute(src) } }
    #[inline]
    const fn to(self) -> Bytes { unsafe { ::core::intrinsics::transmute(self) } }
}

#[repr(transparent)]
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteStr {
    // Invariant: bytes contains valid UTF-8
    bytes: Bytes,
}

impl ByteStr {
    #[inline]
    pub fn new() -> ByteStr {
        ByteStr {
            // Invariant: the empty slice is trivially valid UTF-8.
            bytes: Bytes::new(),
        }
    }

    #[inline]
    pub const fn from_static(val: &'static str) -> ByteStr {
        ByteStr {
            // Invariant: val is a str so contains valid UTF-8.
            bytes: Bytes::from_static(val.as_bytes()),
        }
    }

    #[inline]
    /// ## Panics
    /// In a debug build this will panic if `bytes` is not valid UTF-8.
    ///
    /// ## Safety
    /// `bytes` must contain valid UTF-8. In a release build it is undefined
    /// behavior to call this with `bytes` that is not valid UTF-8.
    pub unsafe fn from_utf8_unchecked(bytes: Bytes) -> ByteStr {
        if cfg!(debug_assertions) {
            match str::from_utf8(&bytes.as_ref()) {
                Ok(_) => (),
                Err(err) => panic!(
                    "ByteStr::from_utf8_unchecked() with invalid bytes; error = {err}, bytes = {bytes:?}",
                ),
            }
        }
        // Invariant: assumed by the safety requirements of this function.
        ByteStr { bytes }
    }

    #[inline(always)]
    pub fn from_utf8(bytes: Bytes) -> Result<ByteStr, str::Utf8Error> {
        str::from_utf8(&bytes)?;
        // Invariant: just checked is utf8
        Ok(ByteStr { bytes })
    }

    #[inline]
    pub const fn len(&self) -> usize { self.bytes.len() }

    #[must_use]
    #[inline(always)]
    pub const fn as_bytes(&self) -> &Bytes { &self.bytes }

    #[must_use]
    #[inline]
    pub unsafe fn slice_unchecked(&self, range: impl core::ops::RangeBounds<usize>) -> Self {
        use core::ops::Bound;

        let len = self.len();

        let begin = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(&n) => n + 1,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => len,
        };

        if end == begin {
            return ByteStr::new();
        }

        let mut ret = BytesUnsafeView::from(self.bytes.clone());

        ret.len = end - begin;
        ret.ptr = unsafe { ret.ptr.add(begin) };

        Self { bytes: ret.to() }
    }

    #[inline]
    pub fn split_once<P: Pattern>(&self, delimiter: P) -> Option<(ByteStr, ByteStr)> {
        let (start, end) = delimiter.into_searcher(self).next_match()?;
        // SAFETY: `Searcher` is known to return valid indices.
        unsafe { Some((self.slice_unchecked(..start), self.slice_unchecked(end..))) }
    }

    #[inline]
    pub fn rsplit_once<P: Pattern>(&self, delimiter: P) -> Option<(ByteStr, ByteStr)>
    where for<'a> P::Searcher<'a>: ReverseSearcher<'a> {
        let (start, end) = delimiter.into_searcher(self).next_match_back()?;
        // SAFETY: `Searcher` is known to return valid indices.
        unsafe { Some((self.slice_unchecked(..start), self.slice_unchecked(end..))) }
    }

    #[must_use]
    #[inline(always)]
    pub const unsafe fn as_bytes_mut(&mut self) -> &mut Bytes { &mut self.bytes }

    #[inline]
    pub fn clear(&mut self) { self.bytes.clear() }
}

unsafe impl Send for ByteStr {}
unsafe impl Sync for ByteStr {}

impl Clone for ByteStr {
    #[inline]
    fn clone(&self) -> ByteStr { Self { bytes: self.bytes.clone() } }
}

impl bytes::Buf for ByteStr {
    #[inline]
    fn remaining(&self) -> usize { self.bytes.remaining() }

    #[inline]
    fn chunk(&self) -> &[u8] { self.bytes.chunk() }

    #[inline]
    fn advance(&mut self, cnt: usize) { self.bytes.advance(cnt) }

    #[inline]
    fn copy_to_bytes(&mut self, len: usize) -> Bytes { self.bytes.copy_to_bytes(len) }
}

impl fmt::Debug for ByteStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Debug::fmt(&**self, f) }
}

impl fmt::Display for ByteStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { fmt::Display::fmt(&**self, f) }
}

impl ops::Deref for ByteStr {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        let b: &[u8] = self.bytes.as_ref();
        // Safety: the invariant of `bytes` is that it contains valid UTF-8.
        unsafe { str::from_utf8_unchecked(b) }
    }
}

impl AsRef<str> for ByteStr {
    #[inline]
    fn as_ref(&self) -> &str { self }
}

impl AsRef<[u8]> for ByteStr {
    #[inline]
    fn as_ref(&self) -> &[u8] { self.bytes.as_ref() }
}

impl core::hash::Hash for ByteStr {
    #[inline]
    fn hash<H>(&self, state: &mut H)
    where H: core::hash::Hasher {
        self.bytes.hash(state)
    }
}

impl Borrow<str> for ByteStr {
    #[inline]
    fn borrow(&self) -> &str { &**self }
}

impl Borrow<[u8]> for ByteStr {
    #[inline]
    fn borrow(&self) -> &[u8] { self.bytes.borrow() }
}

impl PartialEq<str> for ByteStr {
    #[inline]
    fn eq(&self, other: &str) -> bool { &**self == other }
}

impl PartialEq<&str> for ByteStr {
    #[inline]
    fn eq(&self, other: &&str) -> bool { &**self == *other }
}

impl PartialEq<ByteStr> for str {
    #[inline]
    fn eq(&self, other: &ByteStr) -> bool { self == &**other }
}

impl PartialEq<ByteStr> for &str {
    #[inline]
    fn eq(&self, other: &ByteStr) -> bool { *self == &**other }
}

impl PartialEq<String> for ByteStr {
    #[inline]
    fn eq(&self, other: &String) -> bool { &**self == other.as_str() }
}

impl PartialEq<&String> for ByteStr {
    #[inline]
    fn eq(&self, other: &&String) -> bool { &**self == other.as_str() }
}

impl PartialEq<ByteStr> for String {
    #[inline]
    fn eq(&self, other: &ByteStr) -> bool { self.as_str() == &**other }
}

impl PartialEq<ByteStr> for &String {
    #[inline]
    fn eq(&self, other: &ByteStr) -> bool { self.as_str() == &**other }
}

// impl From

impl Default for ByteStr {
    #[inline]
    fn default() -> ByteStr { ByteStr::new() }
}

impl From<String> for ByteStr {
    #[inline]
    fn from(src: String) -> ByteStr {
        ByteStr {
            // Invariant: src is a String so contains valid UTF-8.
            bytes: Bytes::from(src),
        }
    }
}

impl<'a> From<&'a str> for ByteStr {
    #[inline]
    fn from(src: &'a str) -> ByteStr {
        ByteStr {
            // Invariant: src is a str so contains valid UTF-8.
            bytes: Bytes::copy_from_slice(src.as_bytes()),
        }
    }
}

impl From<ByteStr> for Bytes {
    #[inline(always)]
    fn from(src: ByteStr) -> Self { src.bytes }
}

impl serde::Serialize for ByteStr {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        serializer.serialize_str(&**self)
    }
}

struct ByteStrVisitor;

impl<'de> serde::de::Visitor<'de> for ByteStrVisitor {
    type Value = ByteStr;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a UTF-8 string")
    }

    #[inline]
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where E: serde::de::Error {
        Ok(ByteStr::from(v))
    }

    #[inline]
    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where E: serde::de::Error {
        Ok(ByteStr::from(v))
    }

    #[inline]
    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where E: serde::de::Error {
        match str::from_utf8(v) {
            Ok(s) => Ok(ByteStr::from(s)),
            Err(e) => Err(E::custom(format_args!("invalid UTF-8: {e}"))),
        }
    }

    #[inline]
    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where E: serde::de::Error {
        match String::from_utf8(v) {
            Ok(s) => Ok(ByteStr::from(s)),
            Err(e) => Err(E::custom(format_args!("invalid UTF-8: {}", e.utf8_error()))),
        }
    }

    #[inline]
    fn visit_seq<V>(self, mut seq: V) -> Result<Self::Value, V::Error>
    where V: serde::de::SeqAccess<'de> {
        use serde::de::Error as _;
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

impl<'de> serde::Deserialize<'de> for ByteStr {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<ByteStr, D::Error>
    where D: serde::Deserializer<'de> {
        deserializer.deserialize_string(ByteStrVisitor)
    }
}
