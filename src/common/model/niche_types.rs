use ::core::marker::ConstParamTy;

define_valid_range_type! {
    #[derive(ConstParamTy)]
    pub struct NonNegativeI8(i8 as u8 in 0..=0x7f);
    #[derive(ConstParamTy)]
    pub struct NonNegativeI16(i16 as u16 in 0..=0x7f_ff);
}
