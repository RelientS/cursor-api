#![allow(unsafe_op_in_unsafe_fn)]

use ::bytes::{Buf, BufMut};
use ::core::intrinsics::{assume, likely, unchecked_shl, unchecked_shr, unlikely};

use crate::error::DecodeError;

/// ZigZag 编码 32 位整数
#[inline(always)]
pub const fn encode_zigzag32(value: i32) -> u32 {
    unsafe { (unchecked_shl(value, 1u8) ^ unchecked_shr(value, 31u8)) as u32 }
}

/// ZigZag 解码 32 位整数
#[inline(always)]
pub const fn decode_zigzag32(value: u32) -> i32 {
    unsafe { (unchecked_shr(value, 1u8) as i32) ^ (-((value & 1) as i32)) }
}

/// ZigZag 编码 64 位整数
#[inline(always)]
pub const fn encode_zigzag64(value: i64) -> u64 {
    unsafe { (unchecked_shl(value, 1u8) ^ unchecked_shr(value, 63u8)) as u64 }
}

/// ZigZag 解码 64 位整数
#[inline(always)]
pub const fn decode_zigzag64(value: u64) -> i64 {
    unsafe { (unchecked_shr(value, 1u8) as i64) ^ (-((value & 1) as i64)) }
}

/// The maximum number of bytes a Protobuf Varint can occupy.
const VARINT64_MAX_LEN: usize = 10;

/// Encodes an integer value into LEB128 variable length format, and writes it to the buffer.
///
/// Dispatches to a fast path if the buffer has enough contiguous space,
/// otherwise falls back to a slower, byte-by-byte write.
#[inline]
pub fn encode_varint64(value: u64, buf: &mut impl BufMut) -> usize {
    let len = encoded_len_varint64(value);

    // If there is enough contiguous space, use the optimized path.
    if likely(buf.chunk_mut().len() >= len) {
        // Safety: The check above guarantees `buf.chunk_mut()` has at least `len` bytes.
        unsafe { encode_varint64_fast(value, len, buf) };
    } else {
        encode_varint64_slow(value, len, buf);
    }

    len
}

/// Fast-path for encoding to a contiguous buffer slice.
///
/// ## Safety
///
/// The caller must ensure `buf.chunk_mut().len() >= len`.
#[inline(always)]
unsafe fn encode_varint64_fast(mut value: u64, len: usize, buf: &mut impl BufMut) {
    let ptr = buf.chunk_mut().as_mut_ptr();

    for i in 0..(len - 1) {
        *ptr.add(i) = (value & 0x7F) as u8 | 0x80;
        value >>= 7;
    }

    // After the loop, `value` holds the last byte, which must not have the continuation bit.
    // The `encoded_len_varint` logic guarantees this.
    assume(value < 0x80);
    *ptr.add(len - 1) = value as u8;

    // Notify the buffer that `len` bytes have been written.
    buf.advance_mut(len);
}

/// Slow-path encoding for buffers that may not be contiguous.
#[cold]
#[inline(never)]
fn encode_varint64_slow(mut value: u64, len: usize, buf: &mut impl BufMut) {
    for _ in 0..(len - 1) {
        buf.put_u8((value & 0x7F) as u8 | 0x80);
        value >>= 7;
    }
    // After the loop, `value` holds the last byte, which must not have the continuation bit.
    // The `encoded_len_varint` logic guarantees this.
    unsafe { assume(value < 0x80) };

    buf.put_u8(value as u8);
}

/// Returns the encoded length of the value in LEB128 variable length format.
/// The returned value will be between 1 and 10, inclusive.
#[inline]
pub const fn encoded_len_varint64(value: u64) -> usize {
    unsafe {
        let value = value
            .bit_width()
            .unchecked_mul(9)
            .unbounded_shr(6)
            .unchecked_add(1);
        assume(value >= 1 && value <= VARINT64_MAX_LEN as u32);
        value as usize
    }
}

/// Decodes a LEB128-encoded variable length integer from the buffer.
#[inline]
pub fn decode_varint64(buf: &mut impl Buf) -> Result<u64, DecodeError> {
    fn inner(buf: &mut impl Buf) -> Option<u64> {
        let bytes = buf.chunk();
        let len = bytes.len();
        if unlikely(len == 0) {
            return None;
        }

        // Fast path for single-byte varints.
        let first = unsafe { *bytes.get_unchecked(0) };
        if likely(first < 0x80) {
            buf.advance(1);
            return Some(first as _);
        }

        // If the chunk is large enough or the varint is known to terminate within it,
        // use the fast path which operates on a slice.
        if likely(len >= VARINT64_MAX_LEN || bytes[len - 1] < 0x80) {
            return decode_varint64_fast(bytes).map(|(value, advance)| {
                buf.advance(advance);
                value
            });
        }

        // Fallback for varints that cross chunk boundaries.
        decode_varint64_slow(buf)
    }
    inner(buf).ok_or(DecodeError::new("invalid varint64"))
}

/// Fast-path decoding of a varint from a contiguous memory slice.
///
/// ## Safety
///
/// Assumes `bytes` contains a complete varint or is at least `VARINT64_MAX_LEN` bytes long.
#[inline(always)]
fn decode_varint64_fast(bytes: &[u8]) -> Option<(u64, usize)> {
    let ptr = bytes.as_ptr();
    let mut value = 0u64;

    for i in 0..VARINT64_MAX_LEN {
        let byte = unsafe { *ptr.add(i) };
        value |= ((byte & 0x7F) as u64) << (i * 7);

        if byte < 0x80 {
            // Check for overlong encoding on the 10th byte.
            if unlikely(i == 9 && byte > 1) {
                return None;
            }
            return Some((value, i + 1));
        }
    }

    // A varint must not be longer than 10 bytes.
    None
}

/// Slow-path decoding for varints that may cross `Buf` chunk boundaries.
#[cold]
#[inline(never)]
fn decode_varint64_slow(buf: &mut impl Buf) -> Option<u64> {
    // Safety: The dispatcher `decode_varint` only calls this function if `bytes[0] >= 0x80`.
    // This hint allows the compiler to optimize the first loop iteration.
    unsafe { assume(buf.chunk().len() > 0 && buf.chunk()[0] >= 0x80) };

    let mut value = 0u64;
    for i in 0..VARINT64_MAX_LEN {
        if unlikely(!buf.has_remaining()) {
            return None; // Unexpected end of buffer.
        }
        let byte = buf.get_u8();
        value |= ((byte & 0x7F) as u64) << (i * 7);

        if byte < 0x80 {
            // Check for overlong encoding on the 10th byte.
            if unlikely(i == 9 && byte > 1) {
                return None;
            }
            return Some(value);
        }
    }

    // A varint must not be longer than 10 bytes.
    None
}

/// The maximum number of bytes a Protobuf Varint can occupy.
const VARINT32_MAX_LEN: usize = 5;

/// Encodes an integer value into LEB128 variable length format, and writes it to the buffer.
///
/// Dispatches to a fast path if the buffer has enough contiguous space,
/// otherwise falls back to a slower, byte-by-byte write.
#[inline]
pub fn encode_varint32(value: u32, buf: &mut impl BufMut) -> usize {
    let len = encoded_len_varint32(value);

    // If there is enough contiguous space, use the optimized path.
    if likely(buf.chunk_mut().len() >= len) {
        // Safety: The check above guarantees `buf.chunk_mut()` has at least `len` bytes.
        unsafe { encode_varint32_fast(value, len, buf) };
    } else {
        encode_varint32_slow(value, len, buf);
    }

    len
}

/// Fast-path for encoding to a contiguous buffer slice.
///
/// ## Safety
///
/// The caller must ensure `buf.chunk_mut().len() >= len`.
#[inline(always)]
unsafe fn encode_varint32_fast(mut value: u32, len: usize, buf: &mut impl BufMut) {
    let ptr = buf.chunk_mut().as_mut_ptr();

    for i in 0..(len - 1) {
        *ptr.add(i) = (value & 0x7F) as u8 | 0x80;
        value >>= 7;
    }

    // After the loop, `value` holds the last byte, which must not have the continuation bit.
    // The `encoded_len_varint` logic guarantees this.
    assume(value < 0x80);
    *ptr.add(len - 1) = value as u8;

    // Notify the buffer that `len` bytes have been written.
    buf.advance_mut(len);
}

/// Slow-path encoding for buffers that may not be contiguous.
#[cold]
#[inline(never)]
fn encode_varint32_slow(mut value: u32, len: usize, buf: &mut impl BufMut) {
    for _ in 0..(len - 1) {
        buf.put_u8((value & 0x7F) as u8 | 0x80);
        value >>= 7;
    }
    // After the loop, `value` holds the last byte, which must not have the continuation bit.
    // The `encoded_len_varint` logic guarantees this.
    unsafe { assume(value < 0x80) };

    buf.put_u8(value as u8);
}

/// Returns the encoded length of the value in LEB128 variable length format.
/// The returned value will be between 1 and 5, inclusive.
#[inline]
pub const fn encoded_len_varint32(value: u32) -> usize {
    unsafe {
        let value = value
            .bit_width()
            .unchecked_mul(9)
            .unbounded_shr(6)
            .unchecked_add(1);
        assume(value >= 1 && value <= VARINT32_MAX_LEN as u32);
        value as usize
    }
}

/// Decodes a LEB128-encoded variable length integer from the buffer.
#[inline]
pub fn decode_varint32(buf: &mut impl Buf) -> Result<u32, DecodeError> {
    #[inline(always)]
    fn inner(buf: &mut impl Buf) -> Option<u32> {
        let bytes = buf.chunk();
        let len = bytes.len();
        if unlikely(len == 0) {
            return None;
        }

        // Fast path for single-byte varints.
        let first = unsafe { *bytes.get_unchecked(0) };
        if likely(first < 0x80) {
            buf.advance(1);
            return Some(first as _);
        }

        // If the chunk is large enough or the varint is known to terminate within it,
        // use the fast path which operates on a slice.
        if likely(len >= VARINT32_MAX_LEN || bytes[len - 1] < 0x80) {
            return decode_varint32_fast(bytes).map(|(value, advance)| {
                buf.advance(advance);
                value
            });
        }

        // Fallback for varints that cross chunk boundaries.
        decode_varint32_slow(buf)
    }
    inner(buf).ok_or(DecodeError::new("invalid varint32"))
}

/// Fast-path decoding of a varint from a contiguous memory slice.
///
/// ## Safety
///
/// Assumes `bytes` contains a complete varint or is at least `VARINT32_MAX_LEN` bytes long.
#[inline(always)]
fn decode_varint32_fast(bytes: &[u8]) -> Option<(u32, usize)> {
    let ptr = bytes.as_ptr();
    let mut value = 0u32;

    for i in 0..VARINT32_MAX_LEN {
        let byte = unsafe { *ptr.add(i) };
        value |= ((byte & 0x7F) as u32) << (i * 7);

        if byte < 0x80 {
            // Check for overlong encoding on the 5th byte.
            if unlikely(i == 4 && byte > 4) {
                return None;
            }
            return Some((value, i + 1));
        }
    }

    // A varint must not be longer than 5 bytes.
    None
}

/// Slow-path decoding for varints that may cross `Buf` chunk boundaries.
#[cold]
#[inline(never)]
fn decode_varint32_slow(buf: &mut impl Buf) -> Option<u32> {
    // Safety: The dispatcher `decode_varint` only calls this function if `bytes[0] >= 0x80`.
    // This hint allows the compiler to optimize the first loop iteration.
    unsafe { assume(buf.chunk().len() > 0 && buf.chunk()[0] >= 0x80) };

    let mut value = 0u32;
    for i in 0..VARINT32_MAX_LEN {
        if unlikely(!buf.has_remaining()) {
            return None; // Unexpected end of buffer.
        }
        let byte = buf.get_u8();
        value |= ((byte & 0x7F) as u32) << (i * 7);

        if byte < 0x80 {
            // Check for overlong encoding on the 5th byte.
            if unlikely(i == 4 && byte > 4) {
                return None;
            }
            return Some(value);
        }
    }

    // A varint must not be longer than 5 bytes.
    None
}

pub mod usize {
    use super::*;

    #[cfg(target_pointer_width = "32")]
    pub(super) use super::VARINT32_MAX_LEN as VARINT_MAX_LEN;
    #[cfg(target_pointer_width = "64")]
    pub(super) use super::VARINT64_MAX_LEN as VARINT_MAX_LEN;

    #[inline(always)]
    pub fn encode_varint(value: usize, buf: &mut impl BufMut) -> usize {
        #[cfg(target_pointer_width = "32")]
        return encode_varint32(value as u32, buf);
        #[cfg(target_pointer_width = "64")]
        return encode_varint64(value as u64, buf);
    }

    #[inline(always)]
    pub const fn encoded_len_varint(value: usize) -> usize {
        #[cfg(target_pointer_width = "32")]
        return encoded_len_varint32(value as u32);
        #[cfg(target_pointer_width = "64")]
        return encoded_len_varint64(value as u64);
    }

    #[inline(always)]
    pub fn decode_varint(buf: &mut impl Buf) -> Result<usize, DecodeError> {
        #[cfg(target_pointer_width = "32")]
        return transmute_unchecked!(decode_varint32(buf));
        #[cfg(target_pointer_width = "64")]
        return transmute_unchecked!(decode_varint64(buf));
    }
}

pub mod bool {
    use super::*;

    #[inline(always)]
    pub fn encode_varint(value: bool, buf: &mut impl BufMut) -> usize {
        buf.put_u8(value as _);
        1
    }

    #[inline(always)]
    pub const fn encoded_len_varint(_value: bool) -> usize { 1 }

    #[inline(always)]
    pub fn decode_varint(buf: &mut impl Buf) -> Result<bool, DecodeError> {
        fn inner(buf: &mut impl Buf) -> Option<bool> {
            if unlikely(buf.remaining() == 0) {
                return None;
            }
            let byte = buf.get_u8();
            if byte <= 1 { Some(byte != 0) } else { None }
        }
        inner(buf).ok_or(DecodeError::new("invalid bool"))
    }

    #[inline]
    pub(in super::super) fn encode_packed_fast<B: ReservableBuf>(values: &[bool], buf: &mut B) {
        let start_ptr = buf.as_mut().as_mut_ptr();
        buf.reserve(usize::VARINT_MAX_LEN);
        unsafe {
            buf.set_len(buf.len() + usize::VARINT_MAX_LEN);
        }

        let mut length = 0;
        for &value in values {
            length += encode_varint(value, buf);
        }
        let mut length_slice = unsafe {
            &mut *(start_ptr as *mut [::core::mem::MaybeUninit<u8>; usize::VARINT_MAX_LEN])
                as &mut [::core::mem::MaybeUninit<u8>]
        };
        let len = usize::encode_varint(length, &mut length_slice);

        unsafe {
            let dst = start_ptr.add(len);
            let src = start_ptr.add(usize::VARINT_MAX_LEN);
            ::core::ptr::copy(src, dst, length);
            buf.set_len(
                buf.len()
                    .unchecked_sub(usize::VARINT_MAX_LEN)
                    .unchecked_add(len),
            );
        }
    }
}

macro_rules! varint {
    ($ty:ty, $proto_ty:ident,32) => {
        pub mod $proto_ty {
            use super::*;

            #[inline(always)]
            pub fn encode_varint(value: $ty, buf: &mut impl BufMut) -> usize { encode_varint32(value as u32, buf) }

            #[inline(always)]
            pub const fn encoded_len_varint(value: $ty) -> usize { encoded_len_varint32(value as u32) }

            #[inline(always)]
            pub fn decode_varint(buf: &mut impl Buf) -> Result<$ty, DecodeError> {
                transmute_unchecked!(decode_varint32(buf))
            }

            #[inline]
            pub(in super::super) fn encode_packed_fast(values: &[$ty], buf: &mut impl ReservableBuf) {
                let start_ptr = buf.as_mut().as_mut_ptr();
                buf.reserve(usize::VARINT_MAX_LEN);
                unsafe {
                    buf.set_len(buf.len() + usize::VARINT_MAX_LEN);
                }

                let mut length = 0;
                for &value in values {
                    length += encode_varint(value, buf);
                }
                let mut length_slice = unsafe {
                    &mut *(start_ptr as *mut [::core::mem::MaybeUninit<u8>; usize::VARINT_MAX_LEN])
                        as &mut [::core::mem::MaybeUninit<u8>]
                };
                let len = usize::encode_varint(length, &mut length_slice);

                unsafe {
                    let dst = start_ptr.add(len);
                    let src = start_ptr.add(usize::VARINT_MAX_LEN);
                    ::core::ptr::copy(src, dst, length);
                    buf.set_len(
                        buf.len()
                            .unchecked_sub(usize::VARINT_MAX_LEN)
                            .unchecked_add(len),
                    );
                }
            }
        }
    };
    ($ty:ty, $proto_ty:ident,64) => {
        pub mod $proto_ty {
            use super::*;

            #[inline(always)]
            pub fn encode_varint(value: $ty, buf: &mut impl BufMut) -> usize { encode_varint64(value as u64, buf) }

            #[inline(always)]
            pub const fn encoded_len_varint(value: $ty) -> usize { encoded_len_varint64(value as u64) }

            #[inline(always)]
            pub fn decode_varint(buf: &mut impl Buf) -> Result<$ty, DecodeError> {
                transmute_unchecked!(decode_varint64(buf))
            }

            #[inline]
            pub(in super::super) fn encode_packed_fast(values: &[$ty], buf: &mut impl ReservableBuf) {
                let start_ptr = buf.as_mut().as_mut_ptr();
                buf.reserve(usize::VARINT_MAX_LEN);
                unsafe {
                    buf.set_len(buf.len() + usize::VARINT_MAX_LEN);
                }

                let mut length = 0;
                for &value in values {
                    length += encode_varint(value, buf);
                }
                let mut length_slice = unsafe {
                    &mut *(start_ptr as *mut [::core::mem::MaybeUninit<u8>; usize::VARINT_MAX_LEN])
                        as &mut [::core::mem::MaybeUninit<u8>]
                };
                let len = usize::encode_varint(length, &mut length_slice);

                unsafe {
                    let dst = start_ptr.add(len);
                    let src = start_ptr.add(usize::VARINT_MAX_LEN);
                    ::core::ptr::copy(src, dst, length);
                    buf.set_len(
                        buf.len()
                            .unchecked_sub(usize::VARINT_MAX_LEN)
                            .unchecked_add(len),
                    );
                }
            }
        }
    };
    ($ty:ty, $proto_ty:ident,32, $encode_fn:ident, $decode_fn:ident) => {
        pub mod $proto_ty {
            use super::*;

            #[inline(always)]
            pub fn encode_varint(value: $ty, buf: &mut impl BufMut) -> usize {
                encode_varint32($encode_fn(value), buf)
            }

            #[inline(always)]
            pub const fn encoded_len_varint(value: $ty) -> usize { encoded_len_varint32($encode_fn(value)) }

            #[inline(always)]
            pub fn decode_varint(buf: &mut impl Buf) -> Result<$ty, DecodeError> {
                decode_varint32(buf).map($decode_fn)
            }

            #[inline]
            pub(in super::super) fn encode_packed_fast(values: &[$ty], buf: &mut impl ReservableBuf) {
                let start_ptr = buf.as_mut().as_mut_ptr();
                buf.reserve(usize::VARINT_MAX_LEN);
                unsafe {
                    buf.set_len(buf.len() + usize::VARINT_MAX_LEN);
                }

                let mut length = 0;
                for &value in values {
                    length += encode_varint(value, buf);
                }
                let mut length_slice = unsafe {
                    &mut *(start_ptr as *mut [::core::mem::MaybeUninit<u8>; usize::VARINT_MAX_LEN])
                        as &mut [::core::mem::MaybeUninit<u8>]
                };
                let len = usize::encode_varint(length, &mut length_slice);

                unsafe {
                    let dst = start_ptr.add(len);
                    let src = start_ptr.add(usize::VARINT_MAX_LEN);
                    ::core::ptr::copy(src, dst, length);
                    buf.set_len(
                        buf.len()
                            .unchecked_sub(usize::VARINT_MAX_LEN)
                            .unchecked_add(len),
                    );
                }
            }
        }
    };
    ($ty:ty, $proto_ty:ident,64, $encode_fn:ident, $decode_fn:ident) => {
        pub mod $proto_ty {
            use super::*;

            #[inline(always)]
            pub fn encode_varint(value: $ty, buf: &mut impl BufMut) -> usize {
                encode_varint64($encode_fn(value), buf)
            }

            #[inline(always)]
            pub const fn encoded_len_varint(value: $ty) -> usize { encoded_len_varint64($encode_fn(value)) }

            #[inline(always)]
            pub fn decode_varint(buf: &mut impl Buf) -> Result<$ty, DecodeError> {
                decode_varint64(buf).map($decode_fn)
            }

            #[inline]
            pub(in super::super) fn encode_packed_fast(values: &[$ty], buf: &mut impl ReservableBuf) {
                let start_ptr = buf.as_mut().as_mut_ptr();
                buf.reserve(usize::VARINT_MAX_LEN);
                unsafe {
                    buf.set_len(buf.len() + usize::VARINT_MAX_LEN);
                }

                let mut length = 0;
                for &value in values {
                    length += encode_varint(value, buf);
                }
                let mut length_slice = unsafe {
                    &mut *(start_ptr as *mut [::core::mem::MaybeUninit<u8>; usize::VARINT_MAX_LEN])
                        as &mut [::core::mem::MaybeUninit<u8>]
                };
                let len = usize::encode_varint(length, &mut length_slice);

                unsafe {
                    let dst = start_ptr.add(len);
                    let src = start_ptr.add(usize::VARINT_MAX_LEN);
                    ::core::ptr::copy(src, dst, length);
                    buf.set_len(
                        buf.len()
                            .unchecked_sub(usize::VARINT_MAX_LEN)
                            .unchecked_add(len),
                    );
                }
            }
        }
    };
}

varint!(i32, int32, 32);
varint!(i64, int64, 64);
varint!(u32, uint32, 32);
varint!(u64, uint64, 64);
varint!(i32, sint32, 32, encode_zigzag32, decode_zigzag32);
varint!(i64, sint64, 64, encode_zigzag64, decode_zigzag64);

pub(super) trait ReservableBuf: Sized + BufMut + AsMut<[u8]> {
    fn reserve(&mut self, additional: usize);
    fn len(&self) -> usize;
    unsafe fn set_len(&mut self, len: usize);
}

impl ReservableBuf for ::bytes::BytesMut {
    #[inline(always)]
    fn reserve(&mut self, additional: usize) { Self::reserve(self, additional); }
    #[inline(always)]
    fn len(&self) -> usize { Self::len(self) }
    #[inline(always)]
    unsafe fn set_len(&mut self, len: usize) { Self::set_len(self, len); }
}

impl ReservableBuf for ::alloc::vec::Vec<u8> {
    #[inline(always)]
    fn reserve(&mut self, additional: usize) { Self::reserve(self, additional); }
    #[inline(always)]
    fn len(&self) -> usize { Self::len(self) }
    #[inline(always)]
    unsafe fn set_len(&mut self, len: usize) { Self::set_len(self, len); }
}
