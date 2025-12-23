//! # ManuallyInit - Manual Memory Initialization
//! 
//! ⚠️ **WARNING: This is an unsafe crate**
//! 
//! This crate provides `ManuallyInit<T>`, a type for managing memory with manual initialization control.
//! While most APIs are marked as safe for better ergonomics, they are semantically unsafe.
//! 
//! ## Design Philosophy
//! 
//! This crate adopts an "unsafe but unmarked" design philosophy:
//! 
//! 1. **Reduced syntax noise**: Avoids requiring `unsafe` blocks everywhere
//! 2. **Domain-specific usage**: Intended for low-level systems programming where users understand the risks
//! 3. **Holistic unsafe semantics**: The entire type requires an unsafe mindset to use correctly
//! 
//! ## Safety Declaration
//! 
//! **When using this crate, you must understand:**
//! 
//! - Almost all methods are semantically unsafe
//! - You must manually track initialization state
//! - Incorrect usage leads to undefined behavior
//! - Safe API signatures are provided for convenience only
//! 
//! ## Typical Use Cases
//! 
//! - Lazy initialization of global static variables
//! - Fixed memory layouts in embedded systems
//! - Custom memory allocator implementations
//! - Memory management for C FFI
//! - Scenarios requiring Drop bypass
//! 
//! ## Comparison with Standard Library
//! 
//! - `std::cell::OnceCell`: Safe one-time initialization with runtime overhead
//! - `std::mem::MaybeUninit`: Manual management requiring `unsafe` everywhere
//! - `ManuallyInit`: Compromise offering convenient API with caller-guaranteed correctness
//! 
//! ## Disclaimer
//! 
//! **By using this crate, you acknowledge and accept all associated risks. 
//! The authors are not responsible for any issues arising from misuse.
//! Only use this crate if you fully understand Rust's memory safety model and unsafe semantics.**

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

/// A cell that requires manual initialization management.
/// 
/// `ManuallyInit<T>` wraps a possibly-uninitialized value and provides methods to
/// initialize, access, and manipulate it. The key difference from `MaybeUninit` is
/// that methods are marked safe for ergonomics, but **you must uphold the safety
/// invariants manually**.
/// 
/// # Core Invariants
/// 
/// When using `ManuallyInit<T>`, you must maintain these invariants:
/// 
/// - Values must be initialized before calling `get()`, `get_mut()`, `deref()`, etc.
/// - `init()` can be called multiple times but will **overwrite** without **dropping** the old value
/// - After `take()`, the value becomes uninitialized and must not be accessed
/// 
/// # Memory Leaks
/// 
/// The `init()` method uses `MaybeUninit::write`, which means:
/// - It directly overwrites the memory
/// - **Does not call the old value's `Drop` implementation**
/// - For heap-owning types (`String`, `Vec<T>`, etc.), repeated initialization causes memory leaks
/// - For `Copy` types or types without `Drop`, repeated initialization is relatively safe
/// 
/// If you need proper handling of old values, call `take()` first or use types with correct Drop semantics.
/// 
/// # Examples
/// 
/// ## Basic Usage
/// ```rust,no_run
/// use manually_init::ManuallyInit;
/// 
/// let cell = ManuallyInit::new();
/// cell.init(42);
/// assert_eq!(*cell.get(), 42);
/// ```
/// 
/// ## Global Static Variable
/// ```rust,no_run
/// use manually_init::ManuallyInit;
/// 
/// static mut GLOBAL: ManuallyInit<String> = ManuallyInit::new();
/// 
/// unsafe {
///     GLOBAL.init(String::from("Hello"));
///     println!("{}", GLOBAL.get());
/// }
/// ```
/// 
/// # Common Pitfalls
/// 
/// ## Memory Leak from Repeated Initialization
/// ```rust,no_run
/// # use manually_init::ManuallyInit;
/// let cell = ManuallyInit::new();
/// cell.init(String::from("first"));
/// cell.init(String::from("second")); // ⚠️ "first" is leaked!
/// ```
/// 
/// ## Accessing Uninitialized Value
/// ```rust,no_run
/// # use manually_init::ManuallyInit;
/// let cell = ManuallyInit::<i32>::new();
/// let value = cell.get(); // UB! Not initialized
/// ```
/// 
/// ## Access After Take
/// ```rust,no_run
/// # use manually_init::ManuallyInit;
/// let mut cell = ManuallyInit::new_with(42);
/// let value = cell.take();
/// let ref_value = cell.get(); // UB! Value was taken
/// ```
#[repr(transparent)]
pub struct ManuallyInit<T> {
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> ManuallyInit<T> {
    /// Creates a new uninitialized cell.
    /// 
    /// The cell starts in an uninitialized state. You must call `init()` before
    /// accessing the value.
    /// 
    /// # Examples
    /// ```rust
    /// # use manually_init::ManuallyInit;
    /// let cell: ManuallyInit<i32> = ManuallyInit::new();
    /// // cell is uninitialized, don't access it yet!
    /// cell.init(42);
    /// // Now safe to access
    /// assert_eq!(*cell.get(), 42);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new() -> ManuallyInit<T> {
        ManuallyInit { value: UnsafeCell::new(MaybeUninit::uninit()) }
    }

    // /// Creates a new cell initialized with the given value.
    // /// 
    // /// The cell starts in an initialized state and can be immediately accessed.
    // /// 
    // /// # Examples
    // /// ```rust
    // /// # use manually_init::ManuallyInit;
    // /// let cell = ManuallyInit::new_with(42);
    // /// assert_eq!(*cell.get(), 42);
    // /// ```
    // #[inline]
    // #[must_use]
    // pub const fn new_with(value: T) -> ManuallyInit<T> {
    //     ManuallyInit { value: UnsafeCell::new(MaybeUninit::new(value)) }
    // }

    /// Initializes or reinitializes the cell with the given value.
    /// 
    /// This method can be called multiple times. If the cell already contains a value,
    /// it will be overwritten **without calling its destructor**, potentially causing
    /// memory leaks for types that own heap memory.
    ///
    /// # Safety Requirements
    /// 
    /// While this method is marked safe, you must ensure:
    /// - If reinitializing a heap-owning type, the old value should be taken out first to avoid leaks
    /// - The initialization state is properly tracked in your code
    /// 
    /// # Examples
    /// ```rust
    /// # use manually_init::ManuallyInit;
    /// let cell = ManuallyInit::new();
    /// cell.init(42);
    /// 
    /// // Can reinitialize (safe for Copy types)
    /// cell.init(100);
    /// assert_eq!(*cell.get(), 100);
    /// ```
    /// 
    /// # Memory Leak Warning
    /// ```rust
    /// # use manually_init::ManuallyInit;
    /// let cell = ManuallyInit::new();
    /// cell.init(String::from("first"));
    /// cell.init(String::from("second")); // ⚠️ "first" is leaked!
    /// ```
    #[inline]
    pub const fn init(&self, value: T) {
        unsafe { (&mut *self.value.get()).write(value) };
    }

    /// Gets a shared reference to the underlying value.
    ///
    /// # Safety Requirements
    /// 
    /// The cell **must** be initialized before calling this method.
    /// Calling this on an uninitialized cell causes undefined behavior.
    /// 
    /// # Examples
    /// ```rust
    /// # use manually_init::ManuallyInit;
    /// let cell = ManuallyInit::new_with(42);
    /// assert_eq!(*cell.get(), 42);
    /// ```
    #[inline]
    pub const fn get(&self) -> &T {
        unsafe { (&*self.value.get()).assume_init_ref() }
    }

    /// Gets a mutable reference to the underlying value.
    ///
    /// # Safety Requirements
    /// 
    /// The cell **must** be initialized before calling this method.
    /// Calling this on an uninitialized cell causes undefined behavior.
    /// 
    /// # Examples
    /// ```rust
    /// # use manually_init::ManuallyInit;
    /// let mut cell = ManuallyInit::new_with(42);
    /// *cell.get_mut() = 100;
    /// assert_eq!(*cell.get(), 100);
    /// ```
    #[inline]
    pub const fn get_mut(&self) -> &mut T {
        unsafe { (&mut *self.value.get()).assume_init_mut() }
    }

    // /// Consumes the cell and returns the wrapped value.
    // ///
    // /// # Safety Requirements
    // /// 
    // /// The cell **must** be initialized before calling this method.
    // /// Calling this on an uninitialized cell causes undefined behavior.
    // /// 
    // /// # Examples
    // /// ```rust
    // /// # use manually_init::ManuallyInit;
    // /// let cell = ManuallyInit::new_with(42);
    // /// let value = cell.into_inner();
    // /// assert_eq!(value, 42);
    // /// ```
    // #[inline]
    // pub const fn into_inner(self) -> T {
    //     unsafe { self.value.into_inner().assume_init() }
    // }

    // /// Takes the value out of the cell, leaving it uninitialized.
    // /// 
    // /// After calling this method, the cell is uninitialized and must not be
    // /// accessed until `init()` is called again.
    // ///
    // /// # Safety Requirements
    // /// 
    // /// The cell **must** be initialized before calling this method.
    // /// Calling this on an uninitialized cell causes undefined behavior.
    // /// 
    // /// # Examples
    // /// ```rust
    // /// # use manually_init::ManuallyInit;
    // /// let mut cell = ManuallyInit::new_with(42);
    // /// let value = cell.take();
    // /// assert_eq!(value, 42);
    // /// // cell is now uninitialized, don't access!
    // /// 
    // /// cell.init(100);  // Reinitialize before accessing
    // /// assert_eq!(*cell.get(), 100);
    // /// ```
    // #[inline]
    // pub const fn take(&self) -> T {
    //     unsafe {
    //         let me = &mut *self.value.get();
    //         let v = me.assume_init_read();
    //         *me = MaybeUninit::uninit();
    //         v
    //     }
    // }
}

unsafe impl<T: Send> Send for ManuallyInit<T> {}
unsafe impl<T: Sync> Sync for ManuallyInit<T> {}

impl<T> ::core::ops::Deref for ManuallyInit<T> {
    type Target = T;

    /// Dereferences to the inner value.
    /// 
    /// # Safety Requirements
    /// 
    /// The cell **must** be initialized. Dereferencing an uninitialized cell
    /// causes undefined behavior.
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T> ::core::ops::DerefMut for ManuallyInit<T> {
    /// Mutably dereferences to the inner value.
    /// 
    /// # Safety Requirements
    /// 
    /// The cell **must** be initialized. Dereferencing an uninitialized cell
    /// causes undefined behavior.
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}
