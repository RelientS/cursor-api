use alloc::format;

use crate::DecodeError;

/// Represent the wire type for protobuf encoding.
///
/// The integer value is equvilant with the encoded value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WireType {
    Varint = 0,
    SixtyFourBit = 1,
    LengthDelimited = 2,
    StartGroup = 3,
    EndGroup = 4,
    ThirtyTwoBit = 5,
}

impl WireType {
    #[inline]
    const fn try_from(value: u8) -> Option<Self> {
        match value {
            0 => Some(WireType::Varint),
            1 => Some(WireType::SixtyFourBit),
            2 => Some(WireType::LengthDelimited),
            3 => Some(WireType::StartGroup),
            4 => Some(WireType::EndGroup),
            5 => Some(WireType::ThirtyTwoBit),
            _ => None,
        }
    }

    #[inline]
    pub fn try_from_tag(tag: u32) -> Result<(Self, u32), DecodeError> {
        let value = (tag & super::WireTypeMask) as u8;
        match Self::try_from(value) {
            Some(wire_type) => Ok((wire_type, tag >> super::WireTypeBits)),
            None => Err(DecodeError::new(format!("invalid wire type value: {value}"))),
        }
    }
}

impl TryFrom<u32> for WireType {
    type Error = DecodeError;

    #[inline]
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(WireType::Varint),
            1 => Ok(WireType::SixtyFourBit),
            2 => Ok(WireType::LengthDelimited),
            3 => Ok(WireType::StartGroup),
            4 => Ok(WireType::EndGroup),
            5 => Ok(WireType::ThirtyTwoBit),
            _ => Err(DecodeError::new(format!("invalid wire type value: {value}"))),
        }
    }
}

/// Checks that the expected wire type matches the actual wire type,
/// or returns an error result.
#[inline]
pub fn check_wire_type(expected: WireType, actual: WireType) -> Result<(), DecodeError> {
    if expected != actual {
        return Err(DecodeError::new(format!(
            "invalid wire type: {actual:?} (expected {expected:?})",
        )));
    }
    Ok(())
}
