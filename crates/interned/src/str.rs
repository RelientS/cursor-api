//! 组合字符串类型，统一编译期和运行时字符串
//!
//! # 设计理念
//!
//! Rust 中常见两类字符串：
//! - **字面量** (`&'static str`): 编译期确定，零成本，永不释放
//! - **动态字符串** (`String`, `ArcStr`): 运行时构造，需要内存管理
//!
//! `Str` 通过枚举将两者统一，提供一致的 API，同时保留各自的性能优势。
//!
//! # 内存布局
//!
//! ```text
//! enum Str {
//!     Static(&'static str)  // 16 bytes (fat pointer)
//!     Counted(ArcStr)        // 8 bytes (NonNull)
//! }
//!
//! 总大小: 17-24 bytes (取决于编译器优化)
//! - Discriminant: 1 byte
//! - Padding: 0-7 bytes
//! - Data: 16 bytes (最大变体)
//! ```
//!
//! # 性能对比
//!
//! | 操作 | Static | Counted |
//! |------|--------|---------|
//! | 创建 | 0 ns | ~100 ns (首次) / ~20 ns (池命中) |
//! | Clone | ~1 ns | ~5 ns (atomic inc) |
//! | Drop | 0 ns | ~5 ns (atomic dec) + 可能的清理 |
//! | as_str() | 0 ns | 0 ns (直接访问) |
//! | len() | 0 ns | 0 ns (直接读字段) |
//!
//! # 使用场景
//!
//! ## ✅ 使用 Static 变体
//!
//! ```rust
//! use interned::Str;
//!
//! // 常量表
//! static KEYWORDS: &[Str] = &[
//!     Str::from_static("fn"),
//!     Str::from_static("let"),
//!     Str::from_static("match"),
//! ];
//!
//! // 编译期字符串
//! const ERROR_MSG: Str = Str::from_static("error occurred");
//! ```
//!
//! ## ✅ 使用 Counted 变体
//!
//! ```rust
//! use interned::Str;
//!
//! // 运行时字符串（去重）
//! let user_input = Str::new(get_user_input());
//!
//! // 跨线程共享
//! let shared = Str::new("config");
//! std::thread::spawn(move || {
//!     process(shared);
//! });
//! ```
//!
//! ## ⚠️ 常见陷阱
//!
//! ```rust
//! use interned::Str;
//!
//! // ❌ 字面量不要用 new()
//! let bad = Str::new("literal");  // 创建 Counted，进入池
//!
//! // ✅ 应该用 from_static
//! const GOOD: Str = Str::from_static("literal");  // Static 变体，零成本
//! ```

use super::arc_str::ArcStr;
use alloc::borrow::Cow;
use core::{
    cmp::Ordering,
    hash::{Hash, Hasher},
};

// ============================================================================
// Core Type Definition
// ============================================================================

/// 组合字符串类型，支持编译期字面量和运行时引用计数字符串
///
/// # Variants
///
/// ## Static
///
/// - 包装 `&'static str`
/// - 零分配成本
/// - 零运行时开销
/// - Clone 是简单的指针复制
/// - 永不释放
///
/// ## Counted
///
/// - 包装 `ArcStr`
/// - 堆分配，通过全局字符串池去重
/// - 原子引用计数管理
/// - 线程安全共享
/// - 最后一个引用释放时回收
///
/// # Method Shadowing
///
/// `Str` 提供了与 `str` 同名的方法（如 `len()`, `is_empty()`），
/// 这些方法会覆盖（shadow）`Deref` 提供的版本，以便：
///
/// - 对 `Static` 变体：直接访问 `&'static str`
/// - 对 `Counted` 变体：使用 `ArcStr` 的优化实现（直接读取内部字段）
///
/// ```rust
/// use interned::Str;
///
/// let s = Str::new("hello");
/// // 调用 Str::len()，而不是 <Str as Deref>::deref().len()
/// // 对于 Counted 变体，这避免了构造 &str 的开销
/// assert_eq!(s.len(), 5);
/// ```
///
/// # Examples
///
/// ```rust
/// use interned::Str;
///
/// // 编译期字符串
/// let s1 = Str::from_static("hello");
/// assert!(s1.is_static());
/// assert_eq!(s1.ref_count(), None);
///
/// // 运行时字符串
/// let s2 = Str::new("world");
/// assert!(!s2.is_static());
/// assert_eq!(s2.ref_count(), Some(1));
///
/// // 统一接口
/// assert_eq!(s1.len(), 5);
/// assert_eq!(s2.len(), 5);
/// ```
///
/// # Thread Safety
///
/// `Str` 是 `Send + Sync`，可以安全地在线程间传递：
///
/// ```rust
/// use interned::Str;
/// use std::thread;
///
/// let s = Str::new("shared");
/// thread::spawn(move || {
///     println!("{}", s);
/// });
/// ```
#[derive(Clone)]
pub enum Str {
    /// 编译期字符串字面量
    ///
    /// - 零成本创建和访问
    /// - Clone 是指针复制（~1ns）
    /// - 永不释放内存
    /// - 适合常量表和配置
    Static(&'static str),

    /// 运行时引用计数字符串
    ///
    /// - 通过字符串池自动去重
    /// - 原子引用计数（线程安全）
    /// - Clone 增加引用计数（~5ns）
    /// - 最后一个引用释放时回收
    Counted(ArcStr),
}

// SAFETY: 两个变体都是 Send + Sync
unsafe impl Send for Str {}
unsafe impl Sync for Str {}

// ============================================================================
// Construction
// ============================================================================

impl Str {
    /// 创建静态字符串变体（编译期字面量）
    ///
    /// 这是创建零成本字符串的**推荐方式**。
    ///
    /// # Const Context
    ///
    /// 此函数是 `const fn`，可在编译期求值：
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// const GREETING: Str = Str::from_static("Hello");
    ///
    /// static KEYWORDS: &[Str] = &[
    ///     Str::from_static("fn"),
    ///     Str::from_static("let"),
    /// ];
    /// ```
    ///
    /// # Performance
    ///
    /// - 编译期：零成本（字符串嵌入二进制）
    /// - 运行期：零成本（只是指针）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::from_static("constant");
    /// assert!(s.is_static());
    /// assert_eq!(s.as_static(), Some("constant"));
    /// assert_eq!(s.ref_count(), None);
    /// ```
    #[inline]
    pub const fn from_static(s: &'static str) -> Self { Self::Static(s) }

    /// 创建或复用运行时字符串
    ///
    /// 字符串会进入全局字符串池，相同内容的字符串会复用同一内存。
    ///
    /// # Performance
    ///
    /// - **首次创建**：堆分配 + HashMap 插入 ≈ 100-200ns
    /// - **池命中**：HashMap 查找 + 引用计数递增 ≈ 10-20ns
    ///
    /// # Thread Safety
    ///
    /// 字符串池使用 `RwLock` 保护，支持并发访问：
    /// - 多个线程可以同时读取（查找已有字符串）
    /// - 创建新字符串时需要独占写锁
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::new("dynamic");
    /// let s2 = Str::new("dynamic");
    ///
    /// // 两个字符串共享同一内存
    /// assert_eq!(s1.ref_count(), s2.ref_count());
    /// assert!(s1.ref_count().unwrap() >= 2);
    /// ```
    ///
    /// # Use Cases
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// // ✅ 编译器：标识符去重
    /// let ident = Str::new(token.text);
    ///
    /// // ✅ 配置系统：键名复用
    /// let key = Str::new("database.host");
    ///
    /// // ✅ 跨线程共享
    /// let shared = Str::new("data");
    /// std::thread::spawn(move || {
    ///     process(shared);
    /// });
    /// # fn token() -> Token { Token { text: "x" } }
    /// # struct Token { text: &'static str }
    /// # fn process(_: Str) {}
    /// ```
    #[inline]
    pub fn new<S: AsRef<str>>(s: S) -> Self { Self::Counted(ArcStr::new(s)) }

    /// 检查是否为 Static 变体
    ///
    /// 用于判断字符串是否为编译期字面量。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::from_static("literal");
    /// let s2 = Str::new("dynamic");
    ///
    /// assert!(s1.is_static());
    /// assert!(!s2.is_static());
    /// ```
    ///
    /// # Use Cases
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// fn optimize_for_static(s: &Str) {
    ///     if s.is_static() {
    ///         // 可以安全地转换为 &'static str
    ///         let static_str = s.as_static().unwrap();
    ///         register_constant(static_str);
    ///     }
    /// }
    /// # fn register_constant(_: &'static str) {}
    /// ```
    #[inline]
    pub const fn is_static(&self) -> bool { matches!(self, Self::Static(_)) }

    /// 获取引用计数
    ///
    /// - **Static 变体**：返回 `None`（无引用计数概念）
    /// - **Counted 变体**：返回 `Some(count)`
    ///
    /// # Note
    ///
    /// 由于并发访问，返回的值可能在读取后立即过时。
    /// 主要用于调试和测试。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::from_static("static");
    /// let s2 = Str::new("counted");
    /// let s3 = s2.clone();
    ///
    /// assert_eq!(s1.ref_count(), None);
    /// assert_eq!(s2.ref_count(), Some(2));
    /// assert_eq!(s3.ref_count(), Some(2));
    /// ```
    #[inline]
    pub fn ref_count(&self) -> Option<usize> {
        match self {
            Self::Static(_) => None,
            Self::Counted(arc) => Some(arc.ref_count()),
        }
    }

    /// 尝试获取静态字符串引用
    ///
    /// 只有 Static 变体会返回 `Some`。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::from_static("literal");
    /// let s2 = Str::new("dynamic");
    ///
    /// assert_eq!(s1.as_static(), Some("literal"));
    /// assert_eq!(s2.as_static(), None);
    /// ```
    ///
    /// # Use Cases
    ///
    /// 某些 API 需要 `&'static str`：
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// fn register_global(name: &'static str) {
    ///     // 注册需要静态生命周期的字符串
    ///     # drop(name);
    /// }
    ///
    /// let s = Str::from_static("name");
    /// if let Some(static_str) = s.as_static() {
    ///     register_global(static_str);
    /// } else {
    ///     // Counted 变体无法转换为 'static
    ///     eprintln!("warning: not a static string");
    /// }
    /// ```
    #[inline]
    pub const fn as_static(&self) -> Option<&'static str> {
        match self {
            Self::Static(s) => Some(*s),
            Self::Counted(_) => None,
        }
    }

    /// 尝试获取内部 `ArcStr` 的引用
    ///
    /// 只有 Counted 变体会返回 `Some`。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::from_static("literal");
    /// let s2 = Str::new("dynamic");
    ///
    /// assert!(s1.as_arc_str().is_none());
    /// assert!(s2.as_arc_str().is_some());
    /// ```
    #[inline]
    pub const fn as_arc_str(&self) -> Option<&ArcStr> {
        match self {
            Self::Static(_) => None,
            Self::Counted(arc) => Some(arc),
        }
    }

    /// 尝试将 Counted 变体转换为 `ArcStr`
    ///
    /// - **Counted**：返回 `Some(ArcStr)`，零成本转换
    /// - **Static**：返回 `None`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::new("counted");
    /// let s2 = Str::from_static("static");
    ///
    /// assert!(s1.into_arc_str().is_some());
    /// assert!(s2.into_arc_str().is_none());
    /// ```
    #[inline]
    pub fn into_arc_str(self) -> Option<ArcStr> {
        match self {
            Self::Static(_) => None,
            Self::Counted(arc) => Some(arc),
        }
    }
}

// ============================================================================
// Optimized str Methods (Method Shadowing)
// ============================================================================

impl Str {
    /// 获取字符串切片
    ///
    /// 这个方法覆盖了 `Deref` 提供的 `as_str()`，以便：
    /// - 对 `Static` 变体：直接返回 `&'static str`
    /// - 对 `Counted` 变体：使用 `ArcStr::as_str()` 的优化实现
    ///
    /// # Performance
    ///
    /// - **Static**：零成本（只是返回指针）
    /// - **Counted**：零成本（直接访问内部字段）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::new("hello");
    /// assert_eq!(s.as_str(), "hello");
    /// ```
    #[inline(always)]
    pub const fn as_str(&self) -> &str {
        match self {
            Self::Static(s) => s,
            Self::Counted(arc) => arc.as_str(),
        }
    }

    /// 获取字符串的字节切片
    ///
    /// 覆盖 `Deref` 版本以传播 `ArcStr::as_bytes()` 的优化。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::new("hello");
    /// assert_eq!(s.as_bytes(), b"hello");
    /// ```
    #[inline(always)]
    pub const fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Static(s) => s.as_bytes(),
            Self::Counted(arc) => arc.as_bytes(),
        }
    }

    /// 获取字符串长度（字节数）
    ///
    /// 覆盖 `Deref` 版本以传播 `ArcStr::len()` 的优化（直接读取字段）。
    ///
    /// # Performance
    ///
    /// - **Static**：读取 fat pointer 的 len 字段
    /// - **Counted**：读取 `ArcStrInner::string_len` 字段（无需构造 `&str`）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::new("hello");
    /// assert_eq!(s.len(), 5);
    /// ```
    #[inline(always)]
    pub const fn len(&self) -> usize {
        match self {
            Self::Static(s) => s.len(),
            Self::Counted(arc) => arc.len(),
        }
    }

    /// 检查字符串是否为空
    ///
    /// 覆盖 `Deref` 版本以传播 `ArcStr::is_empty()` 的优化。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::new("");
    /// let s2 = Str::new("not empty");
    ///
    /// assert!(s1.is_empty());
    /// assert!(!s2.is_empty());
    /// ```
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        match self {
            Self::Static(s) => s.is_empty(),
            Self::Counted(arc) => arc.is_empty(),
        }
    }

    /// 获取内部指针（用于调试和测试）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::new("ptr");
    /// let ptr = s.as_ptr();
    /// assert!(!ptr.is_null());
    /// ```
    #[inline(always)]
    pub const fn as_ptr(&self) -> *const u8 {
        match self {
            Self::Static(s) => s.as_ptr(),
            Self::Counted(arc) => arc.as_ptr(),
        }
    }
}

// ============================================================================
// From Conversions
// ============================================================================

impl const From<&'static str> for Str {
    /// 从字面量创建 Static 变体
    ///
    /// ⚠️ **注意**：只有真正的 `&'static str` 才会自动推断为 Static。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// // ✅ 字面量自动推断为 Static
    /// let s: Str = "literal".into();
    /// assert!(s.is_static());
    ///
    /// // ❌ 但这不会工作（编译错误）：
    /// // let owned = String::from("not static");
    /// // let s: Str = owned.as_str().into();  // 生命周期不是 'static
    /// ```
    #[inline]
    fn from(s: &'static str) -> Self { Self::Static(s) }
}

impl From<String> for Str {
    /// 从 `String` 创建 Counted 变体
    ///
    /// 字符串会进入字符串池，如果已存在相同内容则复用。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s: Str = String::from("owned").into();
    /// assert!(!s.is_static());
    /// assert_eq!(s.as_str(), "owned");
    /// ```
    #[inline]
    fn from(s: String) -> Self { Self::Counted(ArcStr::from(s)) }
}

impl From<&String> for Str {
    /// 从 `&String` 创建 Counted 变体
    #[inline]
    fn from(s: &String) -> Self { Self::Counted(ArcStr::from(s)) }
}

impl From<ArcStr> for Str {
    /// 从 `ArcStr` 创建 Counted 变体
    ///
    /// 直接包装，不会额外增加引用计数。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::{Str, ArcStr};
    ///
    /// let arc = ArcStr::new("shared");
    /// let count_before = arc.ref_count();
    ///
    /// let s: Str = arc.into();
    /// assert_eq!(s.ref_count(), Some(count_before));
    /// ```
    #[inline]
    fn from(arc: ArcStr) -> Self { Self::Counted(arc) }
}

impl<'a> From<Cow<'a, str>> for Str {
    /// 从 `Cow<str>` 创建 Counted 变体
    ///
    /// 无论 Cow 是 Borrowed 还是 Owned，都会进入字符串池。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    /// use std::borrow::Cow;
    ///
    /// let borrowed: Cow<str> = Cow::Borrowed("borrowed");
    /// let owned: Cow<str> = Cow::Owned(String::from("owned"));
    ///
    /// let s1: Str = borrowed.into();
    /// let s2: Str = owned.into();
    ///
    /// assert!(!s1.is_static());
    /// assert!(!s2.is_static());
    /// ```
    #[inline]
    fn from(cow: Cow<'a, str>) -> Self { Self::Counted(ArcStr::from(cow)) }
}

impl From<alloc::boxed::Box<str>> for Str {
    /// 从 `Box<str>` 创建 Counted 变体
    #[inline]
    fn from(s: alloc::boxed::Box<str>) -> Self { Self::Counted(ArcStr::from(s)) }
}

impl From<Str> for String {
    /// 转换为 `String`（总是需要分配）
    ///
    /// # Performance
    ///
    /// 无论哪个变体，都需要分配并复制字符串内容。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::new("to_string");
    /// let string: String = s.into();
    /// assert_eq!(string, "to_string");
    /// ```
    #[inline]
    fn from(s: Str) -> Self { s.as_str().to_owned() }
}

impl From<Str> for alloc::boxed::Box<str> {
    /// 转换为 `Box<str>`（需要分配）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::new("boxed");
    /// let boxed: Box<str> = s.into();
    /// assert_eq!(&*boxed, "boxed");
    /// ```
    #[inline]
    fn from(s: Str) -> Self { s.as_str().into() }
}

impl<'a> From<Str> for Cow<'a, str> {
    /// 转换为 `Cow`
    ///
    /// - **Static 变体**：转换为 `Cow::Borrowed`（零成本）
    /// - **Counted 变体**：转换为 `Cow::Owned`（需要分配）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    /// use std::borrow::Cow;
    ///
    /// let s1 = Str::from_static("static");
    /// let cow1: Cow<str> = s1.into();
    /// assert!(matches!(cow1, Cow::Borrowed(_)));
    ///
    /// let s2 = Str::new("counted");
    /// let cow2: Cow<str> = s2.into();
    /// assert!(matches!(cow2, Cow::Owned(_)));
    /// ```
    #[inline]
    fn from(s: Str) -> Self {
        match s {
            Str::Static(s) => Cow::Borrowed(s),
            Str::Counted(arc) => Cow::Owned(arc.into()),
        }
    }
}

impl<'a> const From<&'a Str> for Cow<'a, str> {
    /// 转换为 `Cow::Borrowed`（零成本）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    /// use std::borrow::Cow;
    ///
    /// let s = Str::from_static("cow");
    /// let cow: Cow<str> = (&s).into();
    ///
    /// assert!(matches!(cow, Cow::Borrowed(_)));
    /// assert_eq!(cow, "cow");
    /// ```
    #[inline]
    fn from(s: &'a Str) -> Self { Cow::Borrowed(s.as_str()) }
}

impl core::str::FromStr for Str {
    type Err = core::convert::Infallible;

    /// 从字符串解析（总是成功，创建 Counted 变体）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    /// use std::str::FromStr;
    ///
    /// let s = Str::from_str("parsed").unwrap();
    /// assert!(!s.is_static());
    /// assert_eq!(s.as_str(), "parsed");
    /// ```
    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> { Ok(Self::new(s)) }
}

// ============================================================================
// Comparison & Hashing
// ============================================================================

impl PartialEq for Str {
    /// 比较字符串内容
    ///
    /// # Optimization
    ///
    /// - **Counted vs Counted**：首先比较指针（O(1)），然后比较内容
    /// - **Static vs Static**：直接比较内容（编译器可能优化为指针比较）
    /// - **Static vs Counted**：必须比较内容
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::from_static("test");
    /// let s2 = Str::new("test");
    ///
    /// assert_eq!(s1, s2);  // 内容相同即相等
    /// ```
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            // Counted vs Counted: 利用 ArcStr 的指针比较优化
            (Self::Counted(a), Self::Counted(b)) => a == b,
            // 其他情况：比较字符串内容
            _ => self.as_str() == other.as_str(),
        }
    }
}

impl Eq for Str {}

impl const PartialEq<str> for Str {
    #[inline]
    fn eq(&self, other: &str) -> bool { self.as_str() == other }
}

impl const PartialEq<&str> for Str {
    #[inline]
    fn eq(&self, other: &&str) -> bool { self.as_str() == *other }
}

impl const PartialEq<String> for Str {
    #[inline]
    fn eq(&self, other: &String) -> bool { self.as_str() == other.as_str() }
}

impl const PartialEq<Str> for str {
    #[inline]
    fn eq(&self, other: &Str) -> bool { self == other.as_str() }
}

impl const PartialEq<Str> for &str {
    #[inline]
    fn eq(&self, other: &Str) -> bool { *self == other.as_str() }
}

impl const PartialEq<Str> for String {
    #[inline]
    fn eq(&self, other: &Str) -> bool { self.as_str() == other.as_str() }
}

impl PartialEq<ArcStr> for Str {
    /// 优化的 `Str` 与 `ArcStr` 比较
    ///
    /// 如果 `Str` 是 Counted 变体，使用指针比较（快速路径）。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::{Str, ArcStr};
    ///
    /// let arc = ArcStr::new("test");
    /// let s1 = Str::from(arc.clone());
    /// let s2 = Str::from_static("test");
    ///
    /// assert_eq!(s1, arc);  // 指针比较
    /// assert_eq!(s2, arc);  // 内容比较
    /// ```
    #[inline]
    fn eq(&self, other: &ArcStr) -> bool {
        match self {
            Self::Counted(arc) => arc == other,
            Self::Static(s) => *s == other.as_str(),
        }
    }
}

impl PartialEq<Str> for ArcStr {
    #[inline]
    fn eq(&self, other: &Str) -> bool { other == self }
}

impl Hash for Str {
    /// 基于字符串内容的哈希，与变体类型无关
    ///
    /// 这确保了 `Static("a")` 和 `Counted(ArcStr::new("a"))`
    /// 有相同的哈希值，可以在 `HashMap` 中作为相同的 key。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    /// use std::collections::HashMap;
    ///
    /// let mut map = HashMap::new();
    /// let s1 = Str::from_static("key");
    /// let s2 = Str::new("key");
    ///
    /// map.insert(s1, "value");
    /// assert_eq!(map.get(&s2), Some(&"value"));  // s2 可以找到 s1 插入的值
    /// ```
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) { state.write_str(self.as_str()) }
}

// ============================================================================
// Ordering
// ============================================================================

impl PartialOrd for Str {
    /// 字典序比较
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let a = Str::from_static("apple");
    /// let b = Str::new("banana");
    ///
    /// assert!(a < b);
    /// assert!(b > a);
    /// ```
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Str {
    /// 字典序比较（总序）
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let mut strs = vec![
    ///     Str::new("cherry"),
    ///     Str::from_static("apple"),
    ///     Str::new("banana"),
    /// ];
    ///
    /// strs.sort();
    ///
    /// assert_eq!(strs[0].as_str(), "apple");
    /// assert_eq!(strs[1].as_str(), "banana");
    /// assert_eq!(strs[2].as_str(), "cherry");
    /// ```
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering { self.as_str().cmp(other.as_str()) }
}

// ============================================================================
// Deref & AsRef
// ============================================================================

impl core::ops::Deref for Str {
    type Target = str;

    /// 支持自动解引用为 `&str`
    ///
    /// 这允许直接调用 `str` 的所有方法（如 `starts_with()`, `contains()` 等）。
    ///
    /// ⚠️ **Note**: 常用方法（如 `len()`, `is_empty()`）已被 `Str` 的同名方法覆盖，
    /// 以便传播 `ArcStr` 的优化。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::from_static("deref");
    ///
    /// // 可以直接调用 str 的方法
    /// assert!(s.starts_with("de"));
    /// assert!(s.contains("ref"));
    /// assert_eq!(s.to_uppercase(), "DEREF");
    /// ```
    #[inline]
    fn deref(&self) -> &Self::Target { self.as_str() }
}

impl const AsRef<str> for Str {
    #[inline]
    fn as_ref(&self) -> &str { self.as_str() }
}

impl const AsRef<[u8]> for Str {
    #[inline]
    fn as_ref(&self) -> &[u8] { self.as_bytes() }
}

impl const core::borrow::Borrow<str> for Str {
    /// 支持在 `HashMap<Str, V>` 中使用 `&str` 查找
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    /// use std::collections::HashMap;
    ///
    /// let mut map = HashMap::new();
    /// map.insert(Str::new("key"), "value");
    ///
    /// // 可以使用 &str 查找
    /// assert_eq!(map.get("key"), Some(&"value"));
    /// ```
    #[inline]
    fn borrow(&self) -> &str { self.as_str() }
}

// ============================================================================
// Display & Debug
// ============================================================================

impl core::fmt::Display for Str {
    /// 输出字符串内容
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::from_static("display");
    /// assert_eq!(format!("{}", s), "display");
    /// ```
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result { f.write_str(self.as_str()) }
}

impl core::fmt::Debug for Str {
    /// 调试输出，显示变体类型和内容
    ///
    /// # Output Format
    ///
    /// - **Static**: `Str::Static("content")`
    /// - **Counted**: `Str::Counted("content", refcount=N)`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s1 = Str::from_static("debug");
    /// let s2 = Str::new("counted");
    ///
    /// println!("{:?}", s1);  // Str::Static("debug")
    /// println!("{:?}", s2);  // Str::Counted("counted", refcount=1)
    /// ```
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Static(s) => f.debug_tuple("Str::Static").field(s).finish(),
            Self::Counted(arc) => f
                .debug_tuple("Str::Counted")
                .field(&arc.as_str())
                .field(&format_args!("refcount={}", arc.ref_count()))
                .finish(),
        }
    }
}

// ============================================================================
// Default
// ============================================================================

impl const Default for Str {
    /// 返回空字符串的 Static 变体
    ///
    /// 这是零成本的，不会分配任何内存。
    ///
    /// # Examples
    ///
    /// ```rust
    /// use interned::Str;
    ///
    /// let s = Str::default();
    /// assert!(s.is_empty());
    /// assert!(s.is_static());
    /// assert_eq!(s.as_str(), "");
    /// ```
    #[inline]
    fn default() -> Self { Self::Static(Default::default()) }
}

// ============================================================================
// Serde Support
// ============================================================================

#[cfg(feature = "serde")]
mod serde_impls {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for Str {
        /// 序列化为普通字符串，丢失变体信息
        ///
        /// ⚠️ **注意**：反序列化后总是 Counted 变体。
        ///
        /// # Examples
        ///
        /// ```rust
        /// use interned::Str;
        ///
        /// let s = Str::from_static("serialize");
        /// let json = serde_json::to_string(&s).unwrap();
        /// assert_eq!(json, r#""serialize""#);
        /// ```
        #[inline]
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer {
            self.as_str().serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for Str {
        /// 反序列化为 Counted 变体
        ///
        /// ⚠️ **注意**：无法恢复 Static 变体，因为反序列化的字符串
        /// 不具有 `'static` 生命周期。
        ///
        /// # Examples
        ///
        /// ```rust
        /// use interned::Str;
        ///
        /// let json = r#""deserialize""#;
        /// let s: Str = serde_json::from_str(json).unwrap();
        ///
        /// assert!(!s.is_static());  // 总是 Counted
        /// assert_eq!(s.as_str(), "deserialize");
        /// ```
        #[inline]
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de> {
            String::deserialize(deserializer).map(Str::from)
        }
    }
}

// ============================================================================
// Testing
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_shadowing() {
        let s1 = Str::from_static("hello");
        let s2 = Str::new("world");

        // 验证调用的是覆盖版本（通过编译即可）
        assert_eq!(s1.len(), 5);
        assert_eq!(s2.len(), 5);
        assert!(!s1.is_empty());
        assert_eq!(s1.as_bytes(), b"hello");
        assert_eq!(s1.as_str(), "hello");
    }

    #[test]
    fn test_static_vs_counted() {
        let s1 = Str::from_static("hello");
        let s2 = Str::new("hello");

        assert!(s1.is_static());
        assert!(!s2.is_static());
        assert_eq!(s1.ref_count(), None);
        assert!(s2.ref_count().is_some());
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_arcstr_conversions() {
        let arc = ArcStr::new("test");
        let count_before = arc.ref_count();

        // ArcStr -> Str
        let s: Str = arc.clone().into();
        assert!(!s.is_static());
        assert_eq!(s.ref_count(), Some(count_before + 1));

        // Str -> Option<ArcStr>
        let arc_back = s.into_arc_str();
        assert!(arc_back.is_some());
        assert_eq!(arc_back.unwrap(), arc);
    }

    #[test]
    fn test_arcstr_equality() {
        let arc = ArcStr::new("same");
        let s1 = Str::from(arc.clone());
        let s2 = Str::from_static("same");

        // Counted vs ArcStr: 指针比较
        assert_eq!(s1, arc);

        // Static vs ArcStr: 内容比较
        assert_eq!(s2, arc);
    }

    #[test]
    fn test_default() {
        let s = Str::default();
        assert!(s.is_empty());
        assert!(s.is_static());
        assert_eq!(s.len(), 0);
    }

    #[test]
    fn test_const_construction() {
        const GREETING: Str = Str::from_static("Hello");
        static KEYWORDS: &[Str] =
            &[Str::from_static("fn"), Str::from_static("let"), Str::from_static("match")];

        assert!(GREETING.is_static());
        assert_eq!(KEYWORDS.len(), 3);
        assert!(KEYWORDS[0].is_static());
    }

    #[test]
    fn test_deref() {
        let s = Str::from_static("deref");

        // 通过 Deref 访问 str 的方法
        assert!(s.starts_with("de"));
        assert!(s.contains("ref"));
        assert_eq!(s.to_uppercase(), "DEREF");
    }

    #[test]
    fn test_ordering() {
        let mut strs = vec![Str::new("cherry"), Str::from_static("apple"), Str::new("banana")];

        strs.sort();

        assert_eq!(strs[0], "apple");
        assert_eq!(strs[1], "banana");
        assert_eq!(strs[2], "cherry");
    }

    #[test]
    fn test_conversions() {
        // From implementations
        let s1: Str = "literal".into();
        let s2: Str = String::from("owned").into();
        let s3: Str = ArcStr::new("arc").into();

        assert!(s1.is_static());
        assert!(!s2.is_static());
        assert!(!s3.is_static());

        // Into implementations
        let string: String = s2.clone().into();
        assert_eq!(string, "owned");

        let boxed: alloc::boxed::Box<str> = s3.into();
        assert_eq!(&*boxed, "arc");
    }

    #[test]
    fn test_hash_consistency() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let s1 = Str::from_static("test");
        let s2 = Str::new("test");

        let mut h1 = DefaultHasher::new();
        let mut h2 = DefaultHasher::new();

        s1.hash(&mut h1);
        s2.hash(&mut h2);

        assert_eq!(h1.finish(), h2.finish());
    }
}
