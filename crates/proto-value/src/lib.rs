#![feature(const_trait_impl)]
#![feature(const_default)]
#![feature(const_destruct)]
#![feature(const_convert)]
#![feature(const_cmp)]
#![feature(partial_ord_chaining_methods)]
#![cfg_attr(feature = "alloc", feature(allocator_api))]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "serde")]
extern crate serde_core;

mod bytes_value;
mod enum_value;
mod is_default;

#[cfg(feature = "serde")]
pub mod stringify;

pub use bytes_value::Bytes;
pub use enum_value::Enum;

#[inline(always)]
pub const fn is_default<Value: [const] is_default::IsDefault>(value: &Value) -> bool { value.is_default() }
