macro_rules! generate_static_variable {
  (
    $variable_name: ident
    $variable_type: ty
  ) => {
    pub(self) static $variable_name: $crate::_marco::Variable<$variable_type> =
      $crate::_marco::uninit_variable();
    #[inline]
    pub(crate) unsafe fn get_unchecked() -> &'static $variable_type {
      unsafe { (&*$variable_name.0.get()).assume_init_ref() }
    }
    #[inline]
    pub(crate) unsafe fn initialize() {
      unsafe { (&mut *$variable_name.0.get()).write(_initialize()) };
    }
  };
}
pub(crate) struct Variable<T>(pub(crate) ::core::cell::UnsafeCell<::core::mem::MaybeUninit<T>>);
unsafe impl<T> Send for Variable<T> {}
unsafe impl<T> Sync for Variable<T> {}
#[inline(always)]
pub(crate) const fn uninit_variable<T>() -> Variable<T> {
  Variable(::core::cell::UnsafeCell::new(::core::mem::MaybeUninit::uninit()))
}

macro_rules! generate_variable_get {
  (
    $variable_name: ident
    $fn_result: ty
    |$x:ident| $map:expr
  ) => {
    pub(crate) mod $variable_name;
    #[inline]
    pub fn $variable_name() -> $fn_result {
      if !$crate::once::initialized() {
        $crate::variables::initialize();
      }
      let $x = unsafe { $variable_name::get_unchecked() };
      $map
    }
  };
}

macro_rules! reexport_info_types {
  {
    $(
      $info_name: ident
      $($info_type: ty)+,
    )*
  } => {
    $(
      mod $info_name;
    )*
    #[allow(unused_braces)]
    pub(crate) mod prelude {
      $(
        pub use super::$info_name::{$($info_type,)+};
      )*
    }
  };
}
