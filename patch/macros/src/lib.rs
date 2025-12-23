/// Batch define constants of the same type with shared attributes.
///
/// # Examples
///
/// ```
/// define_typed_constants! {
///     pub u32 => {
///         MAX_CONNECTIONS = 1024,
///         DEFAULT_TIMEOUT = 30,
///         MIN_BUFFER_SIZE = 256,
///     }
///
///     #[allow(dead_code)]
///     &'static str => {
///         APP_NAME = "server",
///         VERSION = "1.0.0",
///     }
/// }
/// ```
#[macro_export]
macro_rules! define_typed_constants {
    // Entry point: process type group with first constant
    (
        $(#[$group_attr:meta])*
        $vis:vis $ty:ty => {
            $(#[$attr:meta])*
            $name:ident = $value:expr,
            $($inner_rest:tt)*
        }
        $($rest:tt)*
    ) => {
        $(#[$attr])*
        $(#[$group_attr])*
        $vis const $name: $ty = $value;

        $crate::define_typed_constants! {
            @same_type
            $(#[$group_attr])*
            $vis $ty => {
                $($inner_rest)*
            }
        }

        $crate::define_typed_constants! {
            $($rest)*
        }
    };

    // Process remaining constants of the same type
    (
        @same_type
        $(#[$group_attr:meta])*
        $vis:vis $ty:ty => {
            $(#[$attr:meta])*
            $name:ident = $value:expr,
            $($rest:tt)*
        }
    ) => {
        $(#[$attr])*
        $(#[$group_attr])*
        $vis const $name: $ty = $value;

        $crate::define_typed_constants! {
            @same_type
            $(#[$group_attr])*
            $vis $ty => {
                $($rest)*
            }
        }
    };

    // Last constant in type group (no trailing comma)
    (
        @same_type
        $(#[$group_attr:meta])*
        $vis:vis $ty:ty => {
            $(#[$attr:meta])*
            $name:ident = $value:expr
        }
    ) => {
        $(#[$attr])*
        $(#[$group_attr])*
        $vis const $name: $ty = $value;
    };

    // Empty type group
    (@same_type $(#[$group_attr:meta])* $vis:vis $ty:ty => {}) => {};

    // Terminal case
    () => {};
}

#[macro_export]
macro_rules! transmute_unchecked {
    ($x:expr) => {
        unsafe { ::core::intrinsics::transmute_unchecked($x) }
    };
}

#[macro_export]
macro_rules! unwrap_unchecked {
    ($x:expr) => {
        unsafe { $x.unwrap_unchecked() }
    };
}
