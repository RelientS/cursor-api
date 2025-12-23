use ::bytes::{Buf, BufMut};

use super::wire_type::WireType;
use crate::error::DecodeError;
use alloc::string::ToString as _;

macro_rules! fixed {
    ($ty:ty, $proto_ty:ident, $wire_type:ident, $put:ident, $try_get:ident) => {
        pub mod $proto_ty {
            use super::*;

            pub const WIRE_TYPE: WireType = WireType::$wire_type;
            pub const SIZE: usize = core::mem::size_of::<$ty>();

            #[inline(always)]
            pub fn encode_fixed(value: $ty, buf: &mut impl BufMut) { buf.$put(value); }

            #[inline(always)]
            pub fn decode_fixed(buf: &mut impl Buf) -> Result<$ty, DecodeError> {
                buf.$try_get().map_err(|e| DecodeError::new(e.to_string()))
            }
        }
    };
}

fixed!(f32, float, ThirtyTwoBit, put_f32_le, try_get_f32_le);
fixed!(f64, double, SixtyFourBit, put_f64_le, try_get_f64_le);
fixed!(u32, fixed32, ThirtyTwoBit, put_u32_le, try_get_u32_le);
fixed!(u64, fixed64, SixtyFourBit, put_u64_le, try_get_u64_le);
fixed!(i32, sfixed32, ThirtyTwoBit, put_i32_le, try_get_i32_le);
fixed!(i64, sfixed64, SixtyFourBit, put_i64_le, try_get_i64_le);
