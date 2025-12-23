//! Utility functions and types for encoding and decoding Protobuf types.
//!
//! This module contains the encoding and decoding primatives for Protobuf as described in
//! <https://protobuf.dev/programming-guides/encoding/>.
//!
//! This module is `pub`, but is only for prost internal use. The `prost-derive` crate needs access for its `Message` implementations.

#![allow(clippy::implicit_hasher, clippy::ptr_arg)]

use alloc::collections::BTreeMap;
#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};
use core::any::{Any, TypeId};
use core::num::NonZeroU32;

use ::bytes::{Buf, BufMut, Bytes};

use crate::{DecodeError, Message, ByteStr};

pub mod varint;
pub use varint::usize::{decode_varint, encode_varint, encoded_len_varint};

pub mod length_delimiter;
pub use length_delimiter::{
    decode_length_delimiter, encode_length_delimiter, length_delimiter_len,
};

pub mod wire_type;
pub use wire_type::{WireType, check_wire_type};

pub mod fixed_width;

pub mod utf8;
pub use utf8::is_vaild_utf8;

#[macro_export]
macro_rules! field_ {
    (0) => {};
    ($n:expr) => {
        unsafe { ::core::num::NonZeroU32::new_unchecked($n) }
    };
}

define_typed_constants!(
    #[allow(non_upper_case_globals)]
    u32 => {
        WireTypeBits = 3,
        WireTypeMask = 7,
    }
    #[allow(non_upper_case_globals)]
    pub NonZeroU32 => {
        MaxFieldNumber = field_!((1 << 29) - 1),
        FieldNumber1 = field_!(1),
        FieldNumber2 = field_!(2),
    }
    #[allow(non_upper_case_globals)]
    TypeId => {
        __bytes__BytesMut = TypeId::of::<::bytes::BytesMut>(),
        __alloc__vec__Vec_u8_ = TypeId::of::<::alloc::vec::Vec<u8>>(),
    }
);

/// Retrieves the `TypeId` of a potentially non-'static type `T`.
#[inline]
fn type_id_of<T: ?Sized>() -> TypeId {
    use ::core::marker::PhantomData;

    trait NonStaticAny {
        fn get_type_id(&self) -> TypeId
        where
            Self: 'static;
    }

    impl<T: ?Sized> NonStaticAny for PhantomData<T> {
        fn get_type_id(&self) -> TypeId
        where
            Self: 'static,
        {
            TypeId::of::<T>()
        }
    }

    let phantom_data = PhantomData::<T>;
    // Safety: `TypeId` is a function of the type structure, not its data or lifetime.
    // Transmuting to satisfy the `'static` bound for this specific purpose is sound.
    NonStaticAny::get_type_id(unsafe {
        ::core::intrinsics::transmute_unchecked::<&dyn NonStaticAny, &(dyn NonStaticAny + 'static)>(
            &phantom_data,
        )
    })
}

/// Performs a downcast from `&mut V` to `&mut T`, relying on a pre-computed type equality check.
///
/// This is an optimized internal helper that avoids performing the type check itself. Its safety
/// depends on the caller upholding the `_eq` parameter contract.
#[inline(always)]
unsafe fn downcast_mut_prechecked<T: Any, V>(_val: &mut V, _eq: bool) -> Option<&mut T> {
    if _eq {
        // Safety: The caller guarantees via the `_eq` parameter that `V` is the same type as `T`.
        // This makes the pointer type cast valid.
        unsafe { Some(::core::mem::transmute(_val)) }
    } else {
        None
    }
}

/// Additional information passed to every decode/merge function.
///
/// The context should be passed by value and can be freely cloned. When passing
/// to a function which is decoding a nested object, then use `enter_recursion`.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "no-recursion-limit", derive(Default))]
pub struct DecodeContext {
    /// How many times we can recurse in the current decode stack before we hit
    /// the recursion limit.
    ///
    /// The recursion limit is defined by `RECURSION_LIMIT` and cannot be
    /// customized. The recursion limit can be ignored by building the Prost
    /// crate with the `no-recursion-limit` feature.
    #[cfg(not(feature = "no-recursion-limit"))]
    recurse_count: u32,
}

#[cfg(not(feature = "no-recursion-limit"))]
impl Default for DecodeContext {
    #[inline]
    fn default() -> DecodeContext {
        DecodeContext {
            recurse_count: crate::RECURSION_LIMIT,
        }
    }
}

impl DecodeContext {
    /// Call this function before recursively decoding.
    ///
    /// There is no `exit` function since this function creates a new `DecodeContext`
    /// to be used at the next level of recursion. Continue to use the old context
    // at the previous level of recursion.
    #[cfg(not(feature = "no-recursion-limit"))]
    #[inline]
    pub(crate) fn enter_recursion(&self) -> DecodeContext {
        DecodeContext {
            recurse_count: self.recurse_count - 1,
        }
    }

    #[cfg(feature = "no-recursion-limit")]
    #[inline]
    pub(crate) fn enter_recursion(&self) -> DecodeContext { DecodeContext {} }

    /// Checks whether the recursion limit has been reached in the stack of
    /// decodes described by the `DecodeContext` at `self.ctx`.
    ///
    /// Returns `Ok<()>` if it is ok to continue recursing.
    /// Returns `Err<DecodeError>` if the recursion limit has been reached.
    #[cfg(not(feature = "no-recursion-limit"))]
    #[inline]
    pub(crate) fn limit_reached(&self) -> Result<(), DecodeError> {
        if self.recurse_count == 0 {
            Err(DecodeError::new("recursion limit reached"))
        } else {
            Ok(())
        }
    }

    #[cfg(feature = "no-recursion-limit")]
    #[inline]
    pub(crate) fn limit_reached(&self) -> Result<(), DecodeError> { Ok(()) }
}

/// Encodes a Protobuf field key, which consists of a wire type designator and
/// the field tag.
#[inline]
pub fn encode_tag(number: NonZeroU32, wire_type: WireType, buf: &mut impl BufMut) {
    debug_assert!(number <= MaxFieldNumber);
    let tag = (number.get() << WireTypeBits) | wire_type as u32;
    varint::encode_varint32(tag, buf);
}

/// Decodes a Protobuf field key, which consists of a wire type designator and
/// the field tag.
#[inline(always)]
pub fn decode_tag(buf: &mut impl Buf) -> Result<(NonZeroU32, WireType), DecodeError> {
    let tag = varint::decode_varint32(buf)?;
    let (wire_type, number) = WireType::try_from_tag(tag)?;
    if let Some(number) = NonZeroU32::new(number) {
        Ok((number, wire_type))
    } else {
        Err(DecodeError::new("invalid field number: 0"))
    }
}

/// Returns the width of an encoded Protobuf field tag with the given field number.
/// The returned width will be between 1 and 5 bytes (inclusive).
#[inline]
pub const fn tag_len(number: NonZeroU32) -> usize { varint::encoded_len_varint32(number.get() << WireTypeBits) }

/// Helper function which abstracts reading a length delimiter prefix followed
/// by decoding values until the length of bytes is exhausted.
pub fn merge_loop<T, M, B>(
    value: &mut T,
    buf: &mut B,
    ctx: DecodeContext,
    mut merge: M,
) -> Result<(), DecodeError>
where
    M: FnMut(&mut T, &mut B, DecodeContext) -> Result<(), DecodeError>,
    B: Buf,
{
    let len = decode_varint(buf)?;
    let remaining = buf.remaining();
    if len > remaining {
        return Err(DecodeError::new("buffer underflow"));
    }

    let limit = remaining - len;
    while buf.remaining() > limit {
        merge(value, buf, ctx.clone())?;
    }

    if buf.remaining() != limit {
        return Err(DecodeError::new("delimited length exceeded"));
    }
    Ok(())
}

pub fn skip_field(
    wire_type: WireType,
    number: NonZeroU32,
    buf: &mut impl Buf,
    ctx: DecodeContext,
) -> Result<(), DecodeError> {
    ctx.limit_reached()?;
    let len = match wire_type {
        WireType::Varint => decode_varint(buf).map(|_| 0)?,
        WireType::ThirtyTwoBit => 4,
        WireType::SixtyFourBit => 8,
        WireType::LengthDelimited => decode_varint(buf)?,
        WireType::StartGroup => loop {
            let (inner_number, inner_wire_type) = decode_tag(buf)?;
            match inner_wire_type {
                WireType::EndGroup => {
                    if inner_number != number {
                        return Err(DecodeError::new("unexpected end group tag"));
                    }
                    break 0;
                }
                _ => skip_field(inner_wire_type, inner_number, buf, ctx.enter_recursion())?,
            }
        },
        WireType::EndGroup => return Err(DecodeError::new("unexpected end group tag")),
    };

    if len > buf.remaining() {
        return Err(DecodeError::new("buffer underflow"));
    }

    buf.advance(len);
    Ok(())
}

/// Helper macro which emits an `encode_repeated` function for the type.
macro_rules! encode_repeated {
    ($ty:ty) => {
        pub fn encode_repeated(tag: NonZeroU32, values: &[$ty], buf: &mut impl BufMut) {
            for value in values {
                encode(tag, value, buf);
            }
        }
    };
}

/// Helper macro which emits a `merge_repeated` function for the numeric type.
macro_rules! merge_repeated_numeric {
    ($ty:ty, $wire_type:expr, $merge:ident) => {
        pub fn merge_repeated(
            wire_type: WireType,
            values: &mut Vec<$ty>,
            buf: &mut impl Buf,
            ctx: DecodeContext,
        ) -> Result<(), DecodeError> {
            if wire_type == WireType::LengthDelimited {
                // Packed.
                merge_loop(values, buf, ctx, |values, buf, _ctx| {
                    let mut value = Default::default();
                    $merge(&mut value, buf)?;
                    values.push(value);
                    Ok(())
                })
            } else {
                // Unpacked.
                check_wire_type($wire_type, wire_type)?;
                let mut value = Default::default();
                $merge(&mut value, buf)?;
                values.push(value);
                Ok(())
            }
        }
    };
}

/// Macro which emits a module containing a set of encoding functions for a
/// variable width numeric type.
macro_rules! varint {
    ($ty:ty, $proto_ty:ident) => {
        pub mod $proto_ty {
            use crate::encoding::varint::usize;
            use crate::encoding::varint::$proto_ty::*;
            use crate::encoding::wire_type::{WireType, check_wire_type};
            use crate::encoding::{
                __alloc__vec__Vec_u8_, __bytes__BytesMut, downcast_mut_prechecked, encode_tag, merge_loop,
                tag_len, type_id_of, DecodeContext,
            };
            use crate::error::DecodeError;
            #[cfg(not(feature = "std"))]
            use ::alloc::vec::Vec;
            use ::bytes::{Buf, BufMut};
            use core::num::NonZeroU32;

            pub fn encode(number: NonZeroU32, value: &$ty, buf: &mut impl BufMut) {
                encode_tag(number, WireType::Varint, buf);
                encode_varint(*value, buf);
            }

            pub fn merge(
                wire_type: WireType,
                value: &mut $ty,
                buf: &mut impl Buf,
                _ctx: DecodeContext,
            ) -> Result<(), DecodeError> {
                check_wire_type(WireType::Varint, wire_type)?;
                merge_unchecked(value, buf)
            }

            #[inline(always)]
            fn merge_unchecked(value: &mut $ty, buf: &mut impl Buf) -> Result<(), DecodeError> {
                *value = decode_varint(buf)?;
                Ok(())
            }

            encode_repeated!($ty);

            pub fn encode_packed<B: BufMut>(number: NonZeroU32, values: &[$ty], buf: &mut B) {
                if values.is_empty() {
                    return;
                }

                encode_tag(number, WireType::LengthDelimited, buf);

                let _id = type_id_of::<B>();

                if let Some(buf) = unsafe { downcast_mut_prechecked::<::bytes::BytesMut, B>(buf, _id == __bytes__BytesMut) } {
                    encode_packed_fast(values, buf);
                } else if let Some(buf) = unsafe { downcast_mut_prechecked::<Vec<u8>, B>(buf, _id == __alloc__vec__Vec_u8_) }
                {
                    encode_packed_fast(values, buf);
                } else {
                    let len = values
                        .iter()
                        .map(|&value| encoded_len_varint(value))
                        .sum::<usize>();
                    usize::encode_varint(len, buf);

                    for &value in values {
                        encode_varint(value, buf);
                    }
                }
            }

            merge_repeated_numeric!($ty, WireType::Varint, merge_unchecked);

            #[inline]
            pub fn encoded_len(number: NonZeroU32, value: &$ty) -> usize {
                tag_len(number) + encoded_len_varint(*value)
            }

            #[inline]
            pub fn encoded_len_repeated(number: NonZeroU32, values: &[$ty]) -> usize {
                tag_len(number) * values.len()
                    + values
                        .iter()
                        .map(|&value| encoded_len_varint(value))
                        .sum::<usize>()
            }

            #[inline]
            pub fn encoded_len_packed(number: NonZeroU32, values: &[$ty]) -> usize {
                if values.is_empty() {
                    0
                } else {
                    let len = values
                        .iter()
                        .map(|&value| encoded_len_varint(value))
                        .sum::<usize>();
                    tag_len(number) + usize::encoded_len_varint(len) + len
                }
            }

            #[cfg(test)]
            mod test {
                use proptest::prelude::*;

                use crate::encoding::{
                    test::{check_collection_type, check_type},
                    $proto_ty::*,
                };

                proptest! {
                    #[test]
                    fn check(value: $ty, tag in MIN_TAG..=MAX_TAG) {
                        check_type(value, tag, WireType::Varint,
                                encode, merge, encoded_len)?;
                    }
                    #[test]
                    fn check_repeated(value: Vec<$ty>, tag in MIN_TAG..=MAX_TAG) {
                        check_collection_type(value, tag, WireType::Varint,
                                            encode_repeated, merge_repeated,
                                            encoded_len_repeated)?;
                    }
                    #[test]
                    fn check_packed(value: Vec<$ty>, tag in MIN_TAG..=MAX_TAG) {
                        check_type(value, tag, WireType::LengthDelimited,
                                encode_packed, merge_repeated,
                                encoded_len_packed)?;
                    }
                }
            }
        }
    };
}
varint!(bool, bool);
varint!(i32, int32);
varint!(i64, int64);
varint!(u32, uint32);
varint!(u64, uint64);
varint!(i32, sint32);
varint!(i64, sint64);

/// Macro which emits a module containing a set of encoding functions for a
/// fixed width numeric type.
macro_rules! fixed_size {
    ($ty:ty, $proto_ty:ident) => {
        pub mod $proto_ty {
            use crate::encoding::fixed_width::$proto_ty::*;
            use crate::encoding::varint::usize;
            use crate::encoding::wire_type::{WireType, check_wire_type};
            use crate::encoding::{encode_tag, merge_loop, tag_len, DecodeContext};
            use crate::error::DecodeError;
            #[cfg(not(feature = "std"))]
            use ::alloc::vec::Vec;
            use ::bytes::{Buf, BufMut};
            use core::num::NonZeroU32;

            pub fn encode(number: NonZeroU32, value: &$ty, buf: &mut impl BufMut) {
                encode_tag(number, WIRE_TYPE, buf);
                encode_fixed(*value, buf);
            }

            pub fn merge(wire_type: WireType, value: &mut $ty, buf: &mut impl Buf, _ctx: DecodeContext) -> Result<(), DecodeError> {
                check_wire_type(WIRE_TYPE, wire_type)?;
                merge_unchecked(value, buf)
            }

            #[inline(always)]
            fn merge_unchecked(value: &mut $ty, buf: &mut impl Buf) -> Result<(), DecodeError> {
                *value = decode_fixed(buf)?;
                Ok(())
            }

            encode_repeated!($ty);

            pub fn encode_packed(number: NonZeroU32, values: &[$ty], buf: &mut impl BufMut) {
                if values.is_empty() {
                    return;
                }

                encode_tag(number, WireType::LengthDelimited, buf);
                usize::encode_varint(values.len() * SIZE, buf);

                for &value in values {
                    encode_fixed(value, buf);
                }
            }

            merge_repeated_numeric!($ty, WIRE_TYPE, merge_unchecked);

            #[inline]
            pub fn encoded_len(number: NonZeroU32, _: &$ty) -> usize { tag_len(number) + SIZE }

            #[inline]
            pub fn encoded_len_repeated(number: NonZeroU32, values: &[$ty]) -> usize {
                (tag_len(number) + SIZE) * values.len()
            }

            #[inline]
            pub fn encoded_len_packed(number: NonZeroU32, values: &[$ty]) -> usize {
                if values.is_empty() {
                    0
                } else {
                    let len = SIZE * values.len();
                    tag_len(number) + usize::encoded_len_varint(len) + len
                }
            }
        }
    };
}
fixed_size!(f32, float);
fixed_size!(f64, double);
fixed_size!(u32, fixed32);
fixed_size!(u64, fixed64);
fixed_size!(i32, sfixed32);
fixed_size!(i64, sfixed64);

/// Macro which emits encoding functions for a length-delimited type.
macro_rules! length_delimited {
    ($ty:ty) => {
        encode_repeated!($ty);

        pub fn merge_repeated(
            wire_type: WireType,
            values: &mut Vec<$ty>,
            buf: &mut impl Buf,
            ctx: DecodeContext,
        ) -> Result<(), DecodeError> {
            check_wire_type(WireType::LengthDelimited, wire_type)?;
            let mut value = Default::default();
            merge(wire_type, &mut value, buf, ctx)?;
            values.push(value);
            Ok(())
        }

        #[inline]
        #[allow(clippy::ptr_arg)]
        pub fn encoded_len(number: NonZeroU32, value: &$ty) -> usize {
            tag_len(number) + encoded_len_varint(value.len()) + value.len()
        }

        #[inline]
        pub fn encoded_len_repeated(number: NonZeroU32, values: &[$ty]) -> usize {
            tag_len(number) * values.len()
                + values
                    .iter()
                    .map(|value| encoded_len_varint(value.len()) + value.len())
                    .sum::<usize>()
        }
    };
}

mod sealed {
    use super::{Buf, BufMut};

    pub trait BytesAdapter: Default + Sized + 'static {
        fn len(&self) -> usize;

        /// Replace contents of this buffer with the contents of another buffer.
        fn replace_with(&mut self, buf: impl Buf);

        /// Appends this buffer to the (contents of) other buffer.
        fn append_to(&self, buf: &mut impl BufMut);

        /// Merges a specified number of bytes from a buffer into `self`.
        ///
        /// This method encapsulates the type-specific optimal merge strategy.
        fn merge_from_buf(&mut self, buf: &mut impl Buf, len: usize);

        fn clear(&mut self);
    }

    pub trait StringAdapter: Default + Sized + 'static {
        type Inner: super::BytesAdapter + AsRef<[u8]>;

        fn len(&self) -> usize;

        fn as_bytes(&self) -> &[u8];

        unsafe fn as_mut(&mut self) -> &mut Self::Inner;
    }
}

pub trait StringAdapter: sealed::StringAdapter {}

impl StringAdapter for ByteStr {}

impl sealed::StringAdapter for ByteStr {
    type Inner = Bytes;

    #[inline]
    fn len(&self) -> usize { self.len() }

    #[inline]
    fn as_bytes(&self) -> &[u8] { &self.as_bytes() }

    #[inline]
    unsafe fn as_mut(&mut self) -> &mut Self::Inner { self.as_bytes_mut() }
}

impl StringAdapter for String {}

impl sealed::StringAdapter for String {
    type Inner = Vec<u8>;

    #[inline]
    fn len(&self) -> usize { self.len() }

    #[inline]
    fn as_bytes(&self) -> &[u8] { self.as_bytes() }

    #[inline]
    unsafe fn as_mut(&mut self) -> &mut Self::Inner { self.as_mut_vec() }
}

pub mod string {
    use super::*;

    pub fn encode(number: NonZeroU32, value: &impl StringAdapter, buf: &mut impl BufMut) {
        encode_tag(number, WireType::LengthDelimited, buf);
        encode_varint(value.len(), buf);
        buf.put_slice(value.as_bytes());
    }

    pub fn merge<S: StringAdapter>(
        wire_type: WireType,
        value: &mut S,
        buf: &mut impl Buf,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        // ## Unsafety
        //
        // `string::merge` reuses `bytes::merge`, with an additional check of utf-8
        // well-formedness. If the utf-8 is not well-formed, or if any other error occurs, then the
        // string is cleared, so as to avoid leaking a string field with invalid data.
        //
        // This implementation uses the `StringAdapter` trait which provides access to the underlying
        // byte storage through `as_mut()`. This allows for efficient in-place modification while
        // maintaining the invariant that the string must contain valid UTF-8.
        //
        // To ensure that invalid UTF-8 data is never exposed through the StringAdapter, even in the
        // event of a panic in `bytes::merge` or in the buf implementation, a drop guard is used
        // that will clear the underlying storage if the function exits abnormally.

        struct DropGuard<'a, S: StringAdapter>(&'a mut <S as super::sealed::StringAdapter>::Inner);
        impl<S: StringAdapter> Drop for DropGuard<'_, S> {
            #[inline]
            fn drop(&mut self) { super::sealed::BytesAdapter::clear(self.0) }
        }
        let drop_guard = unsafe { DropGuard::<S>(value.as_mut()) };
        super::bytes::merge(wire_type, drop_guard.0, buf, ctx)?;
        let s = drop_guard.0.as_ref();
        if super::utf8::utf8_valid_up_to(s) == s.len() {
            // Success; do not clear the bytes.
            ::core::mem::forget(drop_guard);
            Ok(())
        } else {
            Err(DecodeError::new(
                "invalid string value: data is not UTF-8 encoded",
            ))
        }
    }

    length_delimited!(impl StringAdapter);

    #[cfg(test)]
    mod test {
        use proptest::prelude::*;

        use super::{
            super::test::{check_collection_type, check_type},
            *,
        };

        proptest! {
            #[test]
            fn check(value: String, tag in MIN_TAG..=MAX_TAG) {
                super::test::check_type(value, tag, WireType::LengthDelimited,
                                        encode, merge, encoded_len)?;
            }
            #[test]
            fn check_repeated(value: Vec<String>, tag in MIN_TAG..=MAX_TAG) {
                super::test::check_collection_type(value, tag, WireType::LengthDelimited,
                                                   encode_repeated, merge_repeated,
                                                   encoded_len_repeated)?;
            }
        }
    }
}

pub trait BytesAdapter: sealed::BytesAdapter {}

impl BytesAdapter for Bytes {}

impl sealed::BytesAdapter for Bytes {
    #[inline]
    fn len(&self) -> usize { ::bytes::Bytes::len(self) }

    #[inline]
    fn replace_with(&mut self, mut buf: impl Buf) { *self = buf.copy_to_bytes(buf.remaining()); }

    #[inline]
    fn append_to(&self, buf: &mut impl BufMut) { buf.put(self.clone()) }

    #[inline]
    fn merge_from_buf(&mut self, buf: &mut impl Buf, len: usize) {
        // Strategy for Bytes: use `copy_to_bytes` for potential zero-copy.
        *self = buf.copy_to_bytes(len);
    }

    #[inline]
    fn clear(&mut self) { self.clear() }
}

impl BytesAdapter for Vec<u8> {}

impl sealed::BytesAdapter for Vec<u8> {
    #[inline]
    fn len(&self) -> usize { ::alloc::vec::Vec::len(self) }

    #[inline]
    fn replace_with(&mut self, buf: impl Buf) {
        self.clear();
        self.put(buf);
    }

    #[inline]
    fn append_to(&self, buf: &mut impl BufMut) { buf.put(self.as_slice()) }

    #[inline]
    fn merge_from_buf(&mut self, buf: &mut impl Buf, len: usize) {
        // Strategy for Vec<u8>: use `take` to ensure single-copy.
        self.clear();
        self.put(buf.take(len));
    }

    #[inline]
    fn clear(&mut self) { self.clear(); }
}

pub mod bytes {
    use super::*;

    pub fn encode(number: NonZeroU32, value: &impl BytesAdapter, buf: &mut impl BufMut) {
        encode_tag(number, WireType::LengthDelimited, buf);
        encode_varint(value.len(), buf);
        value.append_to(buf);
    }

    pub fn merge(
        wire_type: WireType,
        value: &mut impl BytesAdapter,
        buf: &mut impl Buf,
        _ctx: DecodeContext,
    ) -> Result<(), DecodeError> {
        check_wire_type(WireType::LengthDelimited, wire_type)?;
        let len = decode_varint(buf)?;
        if len > buf.remaining() {
            return Err(DecodeError::new(
                "insufficient bytes for length-delimited field",
            ));
        }

        // Clear the existing value. This follows from the following rule in the encoding guide[1]:
        //
        // > Normally, an encoded message would never have more than one instance of a non-repeated
        // > field. However, parsers are expected to handle the case in which they do. For numeric
        // > types and strings, if the same field appears multiple times, the parser accepts the
        // > last value it sees.
        //
        // [1]: https://developers.google.com/protocol-buffers/docs/encoding#optional
        //
        // This is intended for A and B both being Bytes so it is zero-copy.
        // Some combinations of A and B types may cause a double-copy,
        // in which case merge_one_copy() should be used instead.
        value.merge_from_buf(buf, len);
        Ok(())
    }

    length_delimited!(impl BytesAdapter);

    #[cfg(test)]
    mod test {
        use proptest::prelude::*;

        use super::{
            super::test::{check_collection_type, check_type},
            *,
        };

        proptest! {
            #[test]
            fn check_vec(value: Vec<u8>, tag in MIN_TAG..=MAX_TAG) {
                super::test::check_type::<Vec<u8>, Vec<u8>>(value, tag, WireType::LengthDelimited,
                                                            encode, merge, encoded_len)?;
            }

            #[test]
            fn check_bytes(value: Vec<u8>, tag in MIN_TAG..=MAX_TAG) {
                let value = Bytes::from(value);
                super::test::check_type::<Bytes, Bytes>(value, tag, WireType::LengthDelimited,
                                                        encode, merge, encoded_len)?;
            }

            #[test]
            fn check_repeated_vec(value: Vec<Vec<u8>>, tag in MIN_TAG..=MAX_TAG) {
                super::test::check_collection_type(value, tag, WireType::LengthDelimited,
                                                   encode_repeated, merge_repeated,
                                                   encoded_len_repeated)?;
            }

            #[test]
            fn check_repeated_bytes(value: Vec<Vec<u8>>, tag in MIN_TAG..=MAX_TAG) {
                let value = value.into_iter().map(Bytes::from).collect();
                super::test::check_collection_type(value, tag, WireType::LengthDelimited,
                                                   encode_repeated, merge_repeated,
                                                   encoded_len_repeated)?;
            }
        }
    }
}

pub mod message {
    use super::*;

    pub fn encode<M>(number: NonZeroU32, msg: &M, buf: &mut impl BufMut)
    where
        M: Message,
    {
        encode_tag(number, WireType::LengthDelimited, buf);
        encode_varint(msg.encoded_len(), buf);
        msg.encode_raw(buf);
    }

    pub fn merge<M, B>(
        wire_type: WireType,
        msg: &mut M,
        buf: &mut B,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError>
    where
        M: Message,
        B: Buf,
    {
        check_wire_type(WireType::LengthDelimited, wire_type)?;
        ctx.limit_reached()?;
        merge_loop(
            msg,
            buf,
            ctx.enter_recursion(),
            |msg: &mut M, buf: &mut B, ctx| {
                let (number, wire_type) = decode_tag(buf)?;
                msg.merge_field(number, wire_type, buf, ctx)
            },
        )
    }

    pub fn encode_repeated<M>(number: NonZeroU32, messages: &[M], buf: &mut impl BufMut)
    where
        M: Message,
    {
        for msg in messages {
            encode(number, msg, buf);
        }
    }

    pub fn merge_repeated<M>(
        wire_type: WireType,
        messages: &mut Vec<M>,
        buf: &mut impl Buf,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError>
    where
        M: Message + Default,
    {
        check_wire_type(WireType::LengthDelimited, wire_type)?;
        let mut msg = M::default();
        merge(WireType::LengthDelimited, &mut msg, buf, ctx)?;
        messages.push(msg);
        Ok(())
    }

    #[inline]
    pub fn encoded_len<M>(number: NonZeroU32, msg: &M) -> usize
    where
        M: Message,
    {
        let len = msg.encoded_len();
        tag_len(number) + encoded_len_varint(len) + len
    }

    #[inline]
    pub fn encoded_len_repeated<M>(number: NonZeroU32, messages: &[M]) -> usize
    where
        M: Message,
    {
        tag_len(number) * messages.len()
            + messages
                .iter()
                .map(Message::encoded_len)
                .map(|len| len + encoded_len_varint(len))
                .sum::<usize>()
    }
}

pub mod group {
    use super::*;

    pub fn encode<M>(number: NonZeroU32, msg: &M, buf: &mut impl BufMut)
    where
        M: Message,
    {
        encode_tag(number, WireType::StartGroup, buf);
        msg.encode_raw(buf);
        encode_tag(number, WireType::EndGroup, buf);
    }

    pub fn merge<M>(
        number: NonZeroU32,
        wire_type: WireType,
        msg: &mut M,
        buf: &mut impl Buf,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError>
    where
        M: Message,
    {
        check_wire_type(WireType::StartGroup, wire_type)?;

        ctx.limit_reached()?;
        loop {
            let (field_number, field_wire_type) = decode_tag(buf)?;
            if field_wire_type == WireType::EndGroup {
                if field_number != number {
                    return Err(DecodeError::new("unexpected end group tag"));
                }
                return Ok(());
            }

            M::merge_field(msg, field_number, field_wire_type, buf, ctx.enter_recursion())?;
        }
    }

    pub fn encode_repeated<M>(number: NonZeroU32, messages: &[M], buf: &mut impl BufMut)
    where
        M: Message,
    {
        for msg in messages {
            encode(number, msg, buf);
        }
    }

    pub fn merge_repeated<M>(
        number: NonZeroU32,
        wire_type: WireType,
        messages: &mut Vec<M>,
        buf: &mut impl Buf,
        ctx: DecodeContext,
    ) -> Result<(), DecodeError>
    where
        M: Message + Default,
    {
        check_wire_type(WireType::StartGroup, wire_type)?;
        let mut msg = M::default();
        merge(number, WireType::StartGroup, &mut msg, buf, ctx)?;
        messages.push(msg);
        Ok(())
    }

    #[inline]
    pub fn encoded_len<M>(number: NonZeroU32, msg: &M) -> usize
    where
        M: Message,
    {
        2 * tag_len(number) + msg.encoded_len()
    }

    #[inline]
    pub fn encoded_len_repeated<M>(number: NonZeroU32, messages: &[M]) -> usize
    where
        M: Message,
    {
        2 * tag_len(number) * messages.len() + messages.iter().map(Message::encoded_len).sum::<usize>()
    }
}

/// Rust doesn't have a `Map` trait, so macros are currently the best way to be
/// generic over `HashMap` and `BTreeMap`.
macro_rules! map {
    ($map_ty:ident) => {
        use crate::encoding::*;
        use core::hash::Hash;

        /// Generic protobuf map encode function.
        pub fn encode<K, V, B, KE, KL, VE, VL>(
            key_encode: KE,
            key_encoded_len: KL,
            val_encode: VE,
            val_encoded_len: VL,
            number: NonZeroU32,
            values: &$map_ty<K, V>,
            buf: &mut B,
        ) where
            K: Default + Eq + Hash + Ord,
            V: Default + PartialEq,
            B: BufMut,
            KE: Fn(NonZeroU32, &K, &mut B),
            KL: Fn(NonZeroU32, &K) -> usize,
            VE: Fn(NonZeroU32, &V, &mut B),
            VL: Fn(NonZeroU32, &V) -> usize,
        {
            encode_with_default(
                key_encode,
                key_encoded_len,
                val_encode,
                val_encoded_len,
                &V::default(),
                number,
                values,
                buf,
            )
        }

        /// Generic protobuf map merge function.
        pub fn merge<K, V, B, KM, VM>(
            key_merge: KM,
            val_merge: VM,
            values: &mut $map_ty<K, V>,
            buf: &mut B,
            ctx: DecodeContext,
        ) -> Result<(), DecodeError>
        where
            K: Default + Eq + Hash + Ord,
            V: Default,
            B: Buf,
            KM: Fn(WireType, &mut K, &mut B, DecodeContext) -> Result<(), DecodeError>,
            VM: Fn(WireType, &mut V, &mut B, DecodeContext) -> Result<(), DecodeError>,
        {
            merge_with_default(key_merge, val_merge, V::default(), values, buf, ctx)
        }

        /// Generic protobuf map encode function.
        pub fn encoded_len<K, V, KL, VL>(
            key_encoded_len: KL,
            val_encoded_len: VL,
            number: NonZeroU32,
            values: &$map_ty<K, V>,
        ) -> usize
        where
            K: Default + Eq + Hash + Ord,
            V: Default + PartialEq,
            KL: Fn(NonZeroU32, &K) -> usize,
            VL: Fn(NonZeroU32, &V) -> usize,
        {
            encoded_len_with_default(key_encoded_len, val_encoded_len, &V::default(), number, values)
        }

        /// Generic protobuf map encode function with an overridden value default.
        ///
        /// This is necessary because enumeration values can have a default value other
        /// than 0 in proto2.
        pub fn encode_with_default<K, V, B, KE, KL, VE, VL>(
            key_encode: KE,
            key_encoded_len: KL,
            val_encode: VE,
            val_encoded_len: VL,
            val_default: &V,
            number: NonZeroU32,
            values: &$map_ty<K, V>,
            buf: &mut B,
        ) where
            K: Default + Eq + Hash + Ord,
            V: PartialEq,
            B: BufMut,
            KE: Fn(NonZeroU32, &K, &mut B),
            KL: Fn(NonZeroU32, &K) -> usize,
            VE: Fn(NonZeroU32, &V, &mut B),
            VL: Fn(NonZeroU32, &V) -> usize,
        {
            for (key, val) in values.iter() {
                let skip_key = key == &K::default();
                let skip_val = val == val_default;

                let len = (if skip_key { 0 } else { key_encoded_len(FieldNumber1, key) })
                    + (if skip_val { 0 } else { val_encoded_len(FieldNumber2, val) });

                encode_tag(number, WireType::LengthDelimited, buf);
                encode_varint(len, buf);
                if !skip_key {
                    key_encode(FieldNumber1, key, buf);
                }
                if !skip_val {
                    val_encode(FieldNumber2, val, buf);
                }
            }
        }

        /// Generic protobuf map merge function with an overridden value default.
        ///
        /// This is necessary because enumeration values can have a default value other
        /// than 0 in proto2.
        pub fn merge_with_default<K, V, B, KM, VM>(
            key_merge: KM,
            val_merge: VM,
            val_default: V,
            values: &mut $map_ty<K, V>,
            buf: &mut B,
            ctx: DecodeContext,
        ) -> Result<(), DecodeError>
        where
            K: Default + Eq + Hash + Ord,
            B: Buf,
            KM: Fn(WireType, &mut K, &mut B, DecodeContext) -> Result<(), DecodeError>,
            VM: Fn(WireType, &mut V, &mut B, DecodeContext) -> Result<(), DecodeError>,
        {
            let mut key = Default::default();
            let mut val = val_default;
            ctx.limit_reached()?;
            merge_loop(
                &mut (&mut key, &mut val),
                buf,
                ctx.enter_recursion(),
                |&mut (ref mut key, ref mut val), buf, ctx| {
                    let (number, wire_type) = decode_tag(buf)?;
                    #[allow(non_upper_case_globals)]
                    match number {
                        FieldNumber1 => key_merge(wire_type, key, buf, ctx),
                        FieldNumber2 => val_merge(wire_type, val, buf, ctx),
                        _ => skip_field(wire_type, number, buf, ctx),
                    }
                },
            )?;
            values.insert(key, val);

            Ok(())
        }

        /// Generic protobuf map encode function with an overridden value default.
        ///
        /// This is necessary because enumeration values can have a default value other
        /// than 0 in proto2.
        pub fn encoded_len_with_default<K, V, KL, VL>(
            key_encoded_len: KL,
            val_encoded_len: VL,
            val_default: &V,
            number: NonZeroU32,
            values: &$map_ty<K, V>,
        ) -> usize
        where
            K: Default + Eq + Hash + Ord,
            V: PartialEq,
            KL: Fn(NonZeroU32, &K) -> usize,
            VL: Fn(NonZeroU32, &V) -> usize,
        {
            tag_len(number) * values.len()
                + values
                    .iter()
                    .map(|(key, val)| {
                        let len = (if key == &K::default() {
                            0
                        } else {
                            key_encoded_len(FieldNumber1, key)
                        }) + (if val == val_default {
                            0
                        } else {
                            val_encoded_len(FieldNumber2, val)
                        });
                        encoded_len_varint(len) + len
                    })
                    .sum::<usize>()
        }
    };
}

#[cfg(feature = "std")]
pub mod hash_map {
    use std::collections::HashMap;
    map!(HashMap);
}

pub mod btree_map {
    map!(BTreeMap);
}

#[cfg(feature = "indexmap")]
pub mod index_map {
    use indexmap::IndexMap;
    map!(IndexMap);
}

#[cfg(test)]
mod test {
    #[cfg(not(feature = "std"))]
    use alloc::string::ToString;
    use core::{borrow::Borrow, fmt::Debug};

    use ::bytes::BytesMut;
    use proptest::{prelude::*, test_runner::TestCaseResult};

    use super::*;

    pub fn check_type<T, B>(
        value: T,
        number: NonZeroU32,
        wire_type: WireType,
        encode: fn(u32, &B, &mut BytesMut),
        merge: fn(WireType, &mut T, &mut Bytes, DecodeContext) -> Result<(), DecodeError>,
        encoded_len: fn(u32, &B) -> usize,
    ) -> TestCaseResult
    where
        T: Debug + Default + PartialEq + Borrow<B>,
        B: ?Sized,
    {
        prop_assume!((MIN_TAG..=MAX_TAG).contains(&tag));

        let expected_len = encoded_len(tag, value.borrow());

        let mut buf = BytesMut::with_capacity(expected_len);
        encode(tag, value.borrow(), &mut buf);

        let mut buf = buf.freeze();

        prop_assert_eq!(
            buf.remaining(),
            expected_len,
            "encoded_len wrong; expected: {}, actual: {}",
            expected_len,
            buf.remaining()
        );

        if !buf.has_remaining() {
            // Short circuit for empty packed values.
            return Ok(());
        }

        let (decoded_number, decoded_wire_type) =
            decode_tag(&mut buf).map_err(|error| TestCaseError::fail(error.to_string()))?;
        prop_assert_eq!(
            tag,
            decoded_number,
            "decoded tag does not match; expected: {}, actual: {}",
            tag,
            decoded_number
        );

        prop_assert_eq!(
            wire_type,
            decoded_wire_type,
            "decoded wire type does not match; expected: {:?}, actual: {:?}",
            wire_type,
            decoded_wire_type,
        );

        match wire_type {
            WireType::SixtyFourBit if buf.remaining() != 8 => Err(TestCaseError::fail(format!(
                "64bit wire type illegal remaining: {}, tag: {}",
                buf.remaining(),
                tag
            ))),
            WireType::ThirtyTwoBit if buf.remaining() != 4 => Err(TestCaseError::fail(format!(
                "32bit wire type illegal remaining: {}, tag: {}",
                buf.remaining(),
                tag
            ))),
            _ => Ok(()),
        }?;

        let mut roundtrip_value = T::default();
        merge(
            wire_type,
            &mut roundtrip_value,
            &mut buf,
            DecodeContext::default(),
        )
        .map_err(|error| TestCaseError::fail(error.to_string()))?;

        prop_assert!(
            !buf.has_remaining(),
            "expected buffer to be empty, remaining: {}",
            buf.remaining()
        );

        prop_assert_eq!(value, roundtrip_value);

        Ok(())
    }

    pub fn check_collection_type<T, B, E, M, L>(
        value: T,
        number: NonZeroU32,
        wire_type: WireType,
        encode: E,
        mut merge: M,
        encoded_len: L,
    ) -> TestCaseResult
    where
        T: Debug + Default + PartialEq + Borrow<B>,
        B: ?Sized,
        E: FnOnce(u32, &B, &mut BytesMut),
        M: FnMut(WireType, &mut T, &mut Bytes, DecodeContext) -> Result<(), DecodeError>,
        L: FnOnce(u32, &B) -> usize,
    {
        prop_assume!((MIN_TAG..=MAX_TAG).contains(&tag));

        let expected_len = encoded_len(tag, value.borrow());

        let mut buf = BytesMut::with_capacity(expected_len);
        encode(tag, value.borrow(), &mut buf);

        let mut buf = buf.freeze();

        prop_assert_eq!(
            buf.remaining(),
            expected_len,
            "encoded_len wrong; expected: {}, actual: {}",
            expected_len,
            buf.remaining()
        );

        let mut roundtrip_value = Default::default();
        while buf.has_remaining() {
            let (decoded_number, decoded_wire_type) =
                decode_tag(&mut buf).map_err(|error| TestCaseError::fail(error.to_string()))?;

            prop_assert_eq!(
                tag,
                decoded_number,
                "decoded tag does not match; expected: {}, actual: {}",
                tag,
                decoded_number
            );

            prop_assert_eq!(
                wire_type,
                decoded_wire_type,
                "decoded wire type does not match; expected: {:?}, actual: {:?}",
                wire_type,
                decoded_wire_type
            );

            merge(
                wire_type,
                &mut roundtrip_value,
                &mut buf,
                DecodeContext::default(),
            )
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
        }

        prop_assert_eq!(value, roundtrip_value);

        Ok(())
    }

    #[test]
    fn string_merge_invalid_utf8() {
        let mut s = String::new();
        let buf = b"\x02\x80\x80";

        let r = string::merge(
            WireType::LengthDelimited,
            &mut s,
            &mut &buf[..],
            DecodeContext::default(),
        );
        r.expect_err("must be an error");
        assert!(s.is_empty());
    }

    /// This big bowl o' macro soup generates an encoding property test for each combination of map
    /// type, scalar map key, and value type.
    /// TODO: these tests take a long time to compile, can this be improved?
    #[cfg(feature = "std")]
    macro_rules! map_tests {
        (keys: $keys:tt,
         vals: $vals:tt) => {
            mod hash_map {
                map_tests!(@private HashMap, hash_map, $keys, $vals);
            }
            mod btree_map {
                map_tests!(@private BTreeMap, btree_map, $keys, $vals);
            }
        };

        (@private $map_type:ident,
                  $mod_name:ident,
                  [$(($key_ty:ty, $key_proto:ident)),*],
                  $vals:tt) => {
            $(
                mod $key_proto {
                    use std::collections::$map_type;

                    use proptest::prelude::*;

                    use crate::encoding::*;
                    use crate::encoding::test::check_collection_type;

                    map_tests!(@private $map_type, $mod_name, ($key_ty, $key_proto), $vals);
                }
            )*
        };

        (@private $map_type:ident,
                  $mod_name:ident,
                  ($key_ty:ty, $key_proto:ident),
                  [$(($val_ty:ty, $val_proto:ident)),*]) => {
            $(
                proptest! {
                    #[test]
                    fn $val_proto(values: $map_type<$key_ty, $val_ty>, tag in MIN_TAG..=MAX_TAG) {
                        check_collection_type(values, tag, WireType::LengthDelimited,
                                              |tag, values, buf| {
                                                  $mod_name::encode($key_proto::encode,
                                                                    $key_proto::encoded_len,
                                                                    $val_proto::encode,
                                                                    $val_proto::encoded_len,
                                                                    tag,
                                                                    values,
                                                                    buf)
                                              },
                                              |wire_type, values, buf, ctx| {
                                                  check_wire_type(WireType::LengthDelimited, wire_type)?;
                                                  $mod_name::merge($key_proto::merge,
                                                                   $val_proto::merge,
                                                                   values,
                                                                   buf,
                                                                   ctx)
                                              },
                                              |tag, values| {
                                                  $mod_name::encoded_len($key_proto::encoded_len,
                                                                         $val_proto::encoded_len,
                                                                         tag,
                                                                         values)
                                              })?;
                    }
                }
             )*
        };
    }

    #[cfg(feature = "std")]
    map_tests!(keys: [
        (i32, int32),
        (i64, int64),
        (u32, uint32),
        (u64, uint64),
        (i32, sint32),
        (i64, sint64),
        (u32, fixed32),
        (u64, fixed64),
        (i32, sfixed32),
        (i64, sfixed64),
        (bool, bool),
        (String, string)
    ],
    vals: [
        (f32, float),
        (f64, double),
        (i32, int32),
        (i64, int64),
        (u32, uint32),
        (u64, uint64),
        (i32, sint32),
        (i64, sint64),
        (u32, fixed32),
        (u64, fixed64),
        (i32, sfixed32),
        (i64, sfixed64),
        (bool, bool),
        (String, string),
        (Vec<u8>, bytes)
    ]);
}
