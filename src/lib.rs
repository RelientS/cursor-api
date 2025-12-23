#![feature(const_trait_impl)]

pub const trait UnwrapUnchecked<T: Sized>: Sized { fn unwrap_unchecked(self) -> T; }

impl<T> const UnwrapUnchecked<T> for Option<T> { #[inline(always)] fn unwrap_unchecked(self) -> T { unsafe { self.unwrap_unchecked() } } }

impl<T, E> UnwrapUnchecked<T> for Result<T, E> { #[inline(always)] fn unwrap_unchecked(self) -> T { unsafe { self.unwrap_unchecked() } } }

#[cfg(debug_assertions)]
#[macro_export]
#[doc(hidden)]
macro_rules! __unwrap { ($expr:expr) => { $expr.unwrap() } }

#[cfg(not(debug_assertions))]
#[macro_export]
#[doc(hidden)]
macro_rules! __unwrap { ($expr:expr) => { $crate::UnwrapUnchecked::unwrap_unchecked($expr) } }

#[macro_export]
#[doc(hidden)]
macro_rules! __unwrap_panic { ($result:expr) => { match $result { ::core::result::Result::Ok(t) => t, ::core::result::Result::Err(e) => $crate::__panic(&e) } } }

#[inline(never)]
#[cold]
#[doc(hidden)]
pub fn __panic(error: &dyn ::core::fmt::Display) -> ! { ::core::panic!("{error}") }

#[macro_export]
#[doc(hidden)]
macro_rules! __unreachable { () => {{ #[cfg(debug_assertions)] { unreachable!() } #[cfg(not(debug_assertions))] unsafe { ::core::hint::unreachable_unchecked() } }} }

#[macro_export]
#[doc(hidden)]
macro_rules! __cold_path { () => { ::core::hint::cold_path() } }

#[cfg(unix)]
pub const LF: &[u8] = b"\n";

#[cfg(windows)]
pub const LF: &[u8] = b"\r\n";

#[macro_export]
#[doc(hidden)]
macro_rules! __print { ($expr:expr) => { $crate::_print($expr.as_bytes()) } }

#[macro_export]
#[doc(hidden)]
macro_rules! __println { () => { $crate::_print($crate::LF) }; ($expr:expr) => { $crate::_print(concat!($expr, "\n").as_bytes()) } }

#[macro_export]
#[doc(hidden)]
macro_rules! __eprint { ($expr:expr) => { $crate::_eprint($expr.as_bytes()) } }

#[macro_export]
#[doc(hidden)]
macro_rules! __eprintln { () => { $crate::_eprint($crate::LF) }; ($expr:expr) => { $crate::_eprint(concat!($expr, "\n").as_bytes()) } }

fn print_to<T>(bytes: &'_ [u8], global_s: fn() -> T, label: &str) where T: std::io::Write { if let Err(e) = global_s().write_all(bytes) { panic!("failed printing to {label}: {e}"); } }

#[doc(hidden)]
#[inline]
pub fn _print(bytes: &'_ [u8]) { print_to(bytes, std::io::stdout, "stdout"); }

#[doc(hidden)]
#[inline]
pub fn _eprint(bytes: &'_ [u8]) { print_to(bytes, std::io::stderr, "stderr"); }
