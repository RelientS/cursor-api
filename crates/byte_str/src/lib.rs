#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(
    feature = "nightly",
    feature(
        core_intrinsics,
        portable_simd,
        pattern,
        const_precise_live_drops,
        const_convert,
        const_default,
        const_trait_impl,
        slice_index_methods,
        ptr_metadata
    )
)]
#![allow(internal_features)]

extern crate alloc;

extern crate bytes;

#[cfg(feature = "serde")]
extern crate serde_core;

#[macro_use]
extern crate cfg_if;

#[cfg(feature = "serde")]
mod serde_impls;
mod utf8;
mod view;

#[cfg(not(feature = "std"))]
use alloc::string::String;
use bytes::Bytes;
#[cfg(feature = "nightly")]
use core::str::pattern::{Pattern, ReverseSearcher, Searcher as _};
use core::{borrow::Borrow, fmt, ops, str};
pub use utf8::{is_valid_ascii, is_valid_utf8};
use view::BytesUnsafeView;

#[repr(transparent)]
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteStr {
    // Invariant: bytes contains valid UTF-8
    bytes: Bytes,
}

impl ByteStr {
    #[inline]
    pub const fn new() -> ByteStr {
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
            match utf8::validate_utf8(&bytes.as_ref()) {
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
        utf8::validate_utf8(&bytes)?;
        // Invariant: just checked is utf8
        Ok(ByteStr { bytes })
    }

    #[inline]
    pub const fn len(&self) -> usize { self.bytes.len() }

    #[must_use]
    #[inline(always)]
    pub const fn as_bytes(&self) -> &Bytes { &self.bytes }

    #[must_use]
    #[inline(always)]
    pub const fn into_bytes(self) -> Bytes { self.bytes }

    #[cfg(feature = "nightly")]
    #[must_use]
    #[inline]
    pub unsafe fn slice_unchecked<I>(&self, index: I) -> Self
    where
        I: core::slice::SliceIndex<[u8], Output = [u8]>,
    {
        let slice = self.as_ref();
        let ptr = index.get_unchecked(slice);
        
        let len = core::ptr::metadata(ptr);

        if len == 0 {
            return ByteStr::new();
        }

        let mut ret = BytesUnsafeView::from(self.bytes.clone());
        ret.ptr = ptr.cast();
        ret.len = len;

        Self { bytes: ret.to() }
    }

    #[cfg(feature = "nightly")]
    #[inline]
    pub fn split_once<P: Pattern>(&self, delimiter: P) -> Option<(ByteStr, ByteStr)> {
        let (start, end) = delimiter.into_searcher(self).next_match()?;
        // SAFETY: `Searcher` is known to return valid indices.
        unsafe { Some((self.slice_unchecked(..start), self.slice_unchecked(end..))) }
    }

    #[cfg(feature = "nightly")]
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

impl const ops::Deref for ByteStr {
    type Target = str;

    #[inline]
    fn deref(&self) -> &str {
        let b: &[u8] = self.as_ref();
        // Safety: the invariant of `bytes` is that it contains valid UTF-8.
        unsafe { str::from_utf8_unchecked(b) }
    }
}

impl const AsRef<str> for ByteStr {
    #[inline]
    fn as_ref(&self) -> &str { self }
}

impl const AsRef<[u8]> for ByteStr {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        let view = BytesUnsafeView::from_ref(&self.bytes);
        unsafe { &*::core::ptr::slice_from_raw_parts(view.ptr, view.len) }
    }
}

impl core::hash::Hash for ByteStr {
    #[inline]
    fn hash<H>(&self, state: &mut H)
    where H: core::hash::Hasher {
        ops::Deref::deref(self).hash(state)
    }
}

impl const Borrow<str> for ByteStr {
    #[inline]
    fn borrow(&self) -> &str { self }
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

impl const Default for ByteStr {
    #[inline]
    fn default() -> ByteStr { ByteStr::new() }
}

impl From<String> for ByteStr {
    #[inline]
    fn from(src: String) -> ByteStr {
        ByteStr {
            // Invariant: src is a String so contains valid UTF-8.
            bytes: Bytes::from(src.into_bytes()),
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

impl const From<ByteStr> for Bytes {
    #[inline(always)]
    fn from(src: ByteStr) -> Self { src.bytes }
}
