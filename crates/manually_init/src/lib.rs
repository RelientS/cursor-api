//! # ManuallyInit - Zero-Cost Manual Memory Initialization
//!
//! A minimalist unsafe abstraction for manual memory initialization, designed for experts who need
//! direct memory control without runtime overhead.
//!
//! ## Design Philosophy
//!
//! `ManuallyInit` is not a safe alternative to `std::sync::OnceLock` or `std::cell::OnceCell`.
//! It is a deliberate choice for scenarios where:
//!
//! - You need zero runtime overhead
//! - You have full control over initialization timing and access patterns
//! - You explicitly choose to manage safety invariants manually
//! - You come from C/C++ and want familiar, direct memory semantics
//!
//! ## The Core Contract
//!
//! **By using this crate, you accept complete responsibility for:**
//!
//! 1. **Initialization state tracking** - You must know when values are initialized
//! 2. **Thread safety** - You must ensure no data races in concurrent environments
//! 3. **Aliasing rules** - You must uphold Rust's borrowing rules manually
//!
//! The library provides ergonomic APIs by marking methods as safe, but they are semantically unsafe.
//! This is a deliberate design choice to reduce syntax noise in controlled unsafe contexts.
//!
//! ## Primary Use Pattern: Single-Thread Init, Multi-Thread Read
//!
//! The most common and recommended pattern:
//! ```rust,no_run
//! use manually_init::ManuallyInit;
//!
//! static GLOBAL: ManuallyInit<Config> = ManuallyInit::new();
//!
//! // In main thread during startup
//! fn initialize() {
//!     GLOBAL.init(Config::load());
//! }
//!
//! // In any thread after initialization
//! fn use_config() {
//!     let config = GLOBAL.get();
//!     // Read-only access is safe
//! }
//! ```
//!
//! ## Choosing the Right Tool
//!
//! | Strategy | Category | Choose For | What You Get | Cost |
//! |----------|----------|------------|--------------|------|
//! | **`const` item** | Compile-time | True constants that can be inlined | Zero runtime cost | Must be const-evaluable |
//! | **`static` item** | Read-only global | Simple immutable data with fixed address | Zero read cost | Must be `'static` and const |
//! | **`std::sync::LazyLock`** | Lazy immutable | **Default choice** for lazy statics | Automatic thread-safe init | Atomic check per access |
//! | **`std::sync::OnceLock`** | Lazy immutable | Manual control over init timing | Thread-safe one-time init | Atomic read per access |
//! | **`std::sync::Mutex`** | Mutable global | Simple exclusive access | One thread at a time | OS-level lock per access |
//! | **`std::sync::RwLock`** | Mutable global | Multiple readers, single writer | Concurrent reads | Reader/writer lock overhead |
//! | **`core::sync::atomic`** | Lock-free | Primitive types without locks | Wait-free operations | Memory ordering complexity |
//! | **`thread_local!`** | Thread-local | Per-thread state | No synchronization needed | Per-thread initialization |
//! | **`parking_lot::Mutex`** | Mutable global | Faster alternative to std::sync::Mutex | Smaller, faster locks | No poisoning, custom features |
//! | **`parking_lot::RwLock`** | Mutable global | Faster alternative to std::sync::RwLock | Better performance | No poisoning, custom features |
//! | **`parking_lot::OnceCell`** | Lazy immutable | Backport/alternative to std version | Same as std, more features | Similar to std version |
//! | **`once_cell::sync::OnceCell`** | Lazy immutable | Pre-1.70 Rust compatibility | Same as std version | Similar to std version |
//! | **`once_cell::sync::Lazy`** | Lazy immutable | Pre-1.80 Rust compatibility | Same as std version | Similar to std version |
//! | **`lazy_static::lazy_static!`** | Lazy immutable | Macro-based lazy statics | Convenient syntax | Extra dependency, macro overhead |
//! | **`crossbeam::atomic::AtomicCell`** | Lock-free | Any `Copy` type atomically | Lock-free for small types | CAS loop overhead |
//! | **`dashmap::DashMap`** | Concurrent map | High-throughput key-value store | Per-shard locking | Higher memory usage |
//! | **`tokio::sync::Mutex`** | Async mutable | Async-aware exclusive access | Works across .await | Async runtime overhead |
//! | **`tokio::sync::RwLock`** | Async mutable | Async-aware read/write lock | Works across .await | Async runtime overhead |
//! | **`tokio::sync::OnceCell`** | Async init | Initialization in async context | Async-aware safety | Async runtime overhead |
//! | **`static_cell::StaticCell`** | Single-thread | no_std mutable statics | Safe mutable statics | Single-threaded only |
//! | **`conquer_once::OnceCell`** | Lock-free init | Wait-free reads after init | no_std compatible | Complex implementation |
//! | **`core::mem::MaybeUninit`** | Unsafe primitive | Building custom abstractions | Maximum control | `unsafe` for every operation |
//! | **`ManuallyInit`** | Unsafe ergonomic | Zero-cost with external safety proof | Safe-looking API, zero overhead | You handle all safety |
//!
//! ## When to Use ManuallyInit
//!
//! Choose `ManuallyInit` only when:
//! - You need absolute zero overhead (no runtime state tracking)
//! - You have complete control over access patterns
//! - You're interfacing with C/C++ code that expects raw memory
//! - You're implementing a custom synchronization primitive
//! - You can prove safety through external invariants (e.g., init-before-threads pattern)
//!
//! ## Types Requiring Extra Care
//!
//! When using `ManuallyInit`, be especially careful with:
//!
//! | Type Category | Examples | Risk | Mitigation |
//! |--------------|----------|------|------------|
//! | **Heap Owners** | `String`, `Vec<T>`, `Box<T>` | Memory leaks on re-init | Call `take()` before re-init |
//! | **Reference Counted** | `Rc<T>`, `Arc<T>` | Reference leaks | Proper cleanup required |
//! | **Interior Mutability** | `Cell<T>`, `RefCell<T>` | Complex aliasing rules | Avoid or handle carefully |
//! | **Async Types** | `Future`, `Waker` | Complex lifetime requirements | Not recommended |
//!
//! ## Feature Flags
//!
//! - `sync` - Enables `Sync` implementation. By enabling this feature, you explicitly accept
//!   responsibility for preventing data races in concurrent access patterns.

#![no_std]
#![cfg_attr(
    feature = "sync",
    doc = "**⚠️ The `sync` feature is enabled. You are responsible for thread safety.**"
)]

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;

/// A zero-cost wrapper for manually managed initialization.
///
/// This type provides direct memory access with no runtime checks. It's designed for
/// experts who need precise control over initialization and memory layout.
///
/// # Safety Invariants You Must Uphold
///
/// 1. **Never access uninitialized memory** - Calling `get()`, `deref()`, etc. on an
///    uninitialized instance is undefined behavior.
///
/// 2. **Track initialization state** - The type does not track whether it's initialized.
///    This is entirely your responsibility.
///
/// 3. **Handle concurrent access** - If `sync` feature is enabled, you must ensure:
///    - Initialization happens before any concurrent access
///    - No concurrent mutations occur
///    - Memory barriers are properly established
///
/// 4. **Manage memory lifecycle** - `init()` overwrites without dropping. For types
///    that own heap memory, this causes leaks.
///
/// # Example: Global Configuration
///
/// ```rust,no_run
/// use manually_init::ManuallyInit;
///
/// #[derive(Copy, Clone)]
/// struct Config {
///     max_connections: usize,
///     timeout_ms: u64,
/// }
///
/// static CONFIG: ManuallyInit<Config> = ManuallyInit::new();
///
/// // Called once during startup
/// fn initialize_config() {
///     CONFIG.init(Config {
///         max_connections: 100,
///         timeout_ms: 5000,
///     });
/// }
///
/// // Called from any thread after initialization
/// fn get_timeout() -> u64 {
///     CONFIG.get().timeout_ms
/// }
/// ```
///
/// # Example: FFI Pattern
///
/// ```rust,no_run
/// use manually_init::ManuallyInit;
/// use core::ffi::c_void;
///
/// static FFI_CONTEXT: ManuallyInit<*mut c_void> = ManuallyInit::new();
///
/// extern "C" fn init_library(ctx: *mut c_void) {
///     FFI_CONTEXT.init(ctx);
/// }
///
/// extern "C" fn use_library() {
///     let ctx = *FFI_CONTEXT.get();
///     // Use ctx with FFI functions
/// }
/// ```
#[repr(transparent)]
pub struct ManuallyInit<T> {
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> ManuallyInit<T> {
    /// Creates a new uninitialized instance.
    ///
    /// The memory is not initialized. You must call `init()` before any access.
    ///
    /// # Example
    /// ```rust
    /// use manually_init::ManuallyInit;
    ///
    /// static DATA: ManuallyInit<i32> = ManuallyInit::new();
    /// ```
    #[inline]
    #[must_use]
    #[allow(clippy::new_without_default)]
    pub const fn new() -> ManuallyInit<T> {
        ManuallyInit { value: UnsafeCell::new(MaybeUninit::uninit()) }
    }

    /// Creates a new instance initialized with the given value.
    ///
    /// The instance is immediately ready for use.
    ///
    /// # Example
    /// ```rust
    /// use manually_init::ManuallyInit;
    ///
    /// let cell = ManuallyInit::new_with(42);
    /// assert_eq!(*cell.get(), 42);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new_with(value: T) -> ManuallyInit<T> {
        ManuallyInit { value: UnsafeCell::new(MaybeUninit::new(value)) }
    }

    /// Initializes or overwrites the value.
    ///
    /// **Critical**: This method does NOT drop the old value. For types that own
    /// heap memory (`String`, `Vec`, `Box`, etc.), this causes memory leaks.
    ///
    /// # Memory Safety
    ///
    /// - For `Copy` types: Safe to call repeatedly
    /// - For heap-owning types: Call `take()` first or track initialization manually
    ///
    /// # Example
    /// ```rust
    /// use manually_init::ManuallyInit;
    ///
    /// let cell = ManuallyInit::new();
    /// cell.init(42);
    ///
    /// // Safe for Copy types
    /// cell.init(100);
    /// assert_eq!(*cell.get(), 100);
    /// ```
    #[inline]
    pub const fn init(&self, value: T) {
        unsafe { (&mut *self.value.get()).write(value) };
    }

    /// Gets a shared reference to the value.
    ///
    /// # Safety Contract
    ///
    /// You must ensure the value is initialized. Calling this on uninitialized
    /// memory is undefined behavior.
    ///
    /// # Example
    /// ```rust
    /// use manually_init::ManuallyInit;
    ///
    /// let cell = ManuallyInit::new_with(42);
    /// let value: &i32 = cell.get();
    /// assert_eq!(*value, 42);
    /// ```
    #[inline]
    pub const fn get(&self) -> &T {
        unsafe { (&*self.value.get()).assume_init_ref() }
    }

    /// Gets a raw mutable pointer to the value.
    ///
    /// This returns a raw pointer to avoid aliasing rule violations. You are
    /// responsible for ensuring no aliasing occurs when dereferencing.
    ///
    /// # Safety Contract
    ///
    /// - The value must be initialized before dereferencing
    /// - You must ensure no other references exist when creating `&mut T`
    /// - You must follow Rust's aliasing rules manually
    ///
    /// # Example
    /// ```rust
    /// use manually_init::ManuallyInit;
    ///
    /// let cell = ManuallyInit::new_with(42);
    /// let ptr = cell.get_ptr();
    ///
    /// // You must ensure no other references exist
    /// unsafe {
    ///     *ptr = 100;
    /// }
    ///
    /// assert_eq!(*cell.get(), 100);
    /// ```
    #[inline]
    pub const fn get_ptr(&self) -> *mut T {
        unsafe { (&mut *self.value.get()).as_mut_ptr() }
    }

    /// Consumes the cell and returns the inner value.
    ///
    /// # Safety Contract
    ///
    /// The value must be initialized. Calling this on uninitialized memory
    /// is undefined behavior.
    ///
    /// # Example
    /// ```rust
    /// use manually_init::ManuallyInit;
    ///
    /// let cell = ManuallyInit::new_with(String::from("hello"));
    /// let s = cell.into_inner();
    /// assert_eq!(s, "hello");
    /// ```
    #[inline]
    pub const fn into_inner(self) -> T {
        unsafe { self.value.into_inner().assume_init() }
    }

    /// Takes the value out, leaving the cell uninitialized.
    ///
    /// After calling this method, the cell is uninitialized. You must not
    /// access it until calling `init()` again.
    ///
    /// # Safety Contract
    ///
    /// The value must be initialized. Calling this on uninitialized memory
    /// is undefined behavior.
    ///
    /// # Example
    /// ```rust
    /// use manually_init::ManuallyInit;
    ///
    /// let cell = ManuallyInit::new_with(String::from("hello"));
    /// let s = cell.take();
    /// assert_eq!(s, "hello");
    /// // cell is now uninitialized!
    ///
    /// // Must reinitialize before access
    /// cell.init(String::from("world"));
    /// ```
    #[inline]
    pub const fn take(&self) -> T {
        unsafe {
            let slot = &mut *self.value.get();
            let value = slot.assume_init_read();
            *slot = MaybeUninit::uninit();
            value
        }
    }
}

unsafe impl<T: Send> Send for ManuallyInit<T> {}

#[cfg(feature = "sync")]
unsafe impl<T: Sync> Sync for ManuallyInit<T> {}

impl<T> ::core::ops::Deref for ManuallyInit<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T> ::core::ops::DerefMut for ManuallyInit<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Safe because we have &mut self, ensuring exclusive access
        unsafe { (&mut *self.value.get()).assume_init_mut() }
    }
}
