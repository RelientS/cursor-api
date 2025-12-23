#![allow(unsafe_op_in_unsafe_fn)]

use core::{
    alloc::Layout,
    hash::{Hash, Hasher},
    marker::PhantomData,
    mem::SizedTypeProperties as _,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};
use hashbrown::{Equivalent, HashSet};
use manually_init::ManuallyInit;
use parking_lot::RwLock;

/// 字符串内容的内部表示
///
/// # Memory Layout
/// ```text
/// +----------------+
/// | count: usize   |  引用计数
/// | string_len: usize | 字符串长度
/// +----------------+
/// | string data... |  UTF-8 字符串数据
/// +----------------+
/// ```
struct ArcStrInner {
    /// 原子引用计数
    count: AtomicUsize,
    /// 字符串的字节长度
    string_len: usize,
}

impl ArcStrInner {
    const MAX_LEN: usize = {
        let layout = Self::LAYOUT;
        isize::MAX as usize + 1 - layout.align() - layout.size()
    };

    /// 获取字符串数据的起始地址
    ///
    /// # Safety
    /// 调用者必须确保 self 是有效的指针
    #[inline(always)]
    const unsafe fn string_ptr(&self) -> *const u8 { (self as *const Self).add(1) as *const u8 }

    /// 获取字符串切片引用
    ///
    /// # Safety
    /// - self 必须是有效的指针
    /// - 字符串数据必须是有效的 UTF-8
    /// - string_len 必须正确反映实际字符串长度
    #[inline(always)]
    const unsafe fn as_str(&self) -> &str {
        let ptr = self.string_ptr();
        let slice = ::core::slice::from_raw_parts(ptr, self.string_len);
        ::core::str::from_utf8_unchecked(slice)
    }

    /// 计算存储指定长度字符串所需的内存布局
    fn layout_for_string(string_len: usize) -> Layout {
        if string_len > Self::MAX_LEN {
            __cold_path!();
            panic!("string is too long");
        }
        unsafe {
            Layout::new::<Self>()
                .extend(Layout::array::<u8>(string_len).unwrap_unchecked())
                .unwrap_unchecked()
                .0
                .pad_to_align()
        }
    }

    /// 在指定内存位置写入结构体和字符串数据
    ///
    /// # Safety
    /// - ptr 必须指向足够大的已分配内存
    /// - 内存必须正确对齐
    /// - string 必须是有效的 UTF-8 字符串
    unsafe fn write_with_string(ptr: NonNull<Self>, string: &str) {
        let inner = ptr.as_ptr();

        // 初始化结构体
        ::core::ptr::write(inner, Self { count: AtomicUsize::new(1), string_len: string.len() });

        // 复制字符串数据到紧跟结构体后的内存
        let string_ptr = (*inner).string_ptr() as *mut u8;
        ::core::ptr::copy_nonoverlapping(string.as_ptr(), string_ptr, string.len());
    }
}

/// 引用计数的不可变字符串，支持全局字符串池复用
///
/// # Examples
/// ```
/// let s1 = ArcStr::new("hello");
/// let s2 = ArcStr::new("hello");
/// assert!(std::ptr::eq(s1.as_str(), s2.as_str())); // 复用相同内容
/// ```
#[repr(transparent)]
pub struct ArcStr {
    ptr: NonNull<ArcStrInner>,
    _marker: PhantomData<ArcStrInner>,
}

// Safety: ArcStr 使用原子引用计数，可以安全地在线程间传递
unsafe impl Send for ArcStr {}
unsafe impl Sync for ArcStr {}

impl Clone for ArcStr {
    #[inline]
    fn clone(&self) -> Self {
        // Safety: ptr 始终指向有效的 ArcStrInner
        let count = unsafe { self.ptr.as_ref().count.fetch_add(1, Ordering::Relaxed) };

        // 防止引用计数溢出
        if count > isize::MAX as usize {
            __cold_path!();
            std::process::abort();
        }

        Self { ptr: self.ptr, _marker: PhantomData }
    }
}

/// 线程安全的内部指针包装，用于在 HashSet 中作为键
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
struct ThreadSafePtr(NonNull<ArcStrInner>);

// Safety: ThreadSafePtr 只是指针的包装，本身是 POD 类型
unsafe impl Send for ThreadSafePtr {}
unsafe impl Sync for ThreadSafePtr {}

impl ::core::ops::Deref for ThreadSafePtr {
    type Target = NonNull<ArcStrInner>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl Hash for ThreadSafePtr {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Safety: 通过 ThreadSafePtr 保证指针有效性
        unsafe {
            let inner = self.0.as_ref();
            state.write_str(inner.as_str());
        }
    }
}

impl PartialEq for ThreadSafePtr {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        if self.0 == other.0 {
            return true;
        }

        // Safety: ThreadSafePtr 保证指针有效
        unsafe {
            let self_inner = self.0.as_ref();
            let other_inner = other.0.as_ref();
            self_inner.string_len == other_inner.string_len
                && self_inner.as_str() == other_inner.as_str()
        }
    }
}

impl Eq for ThreadSafePtr {}

// 实现 str 与 ThreadSafePtr 的比较，用于 HashSet::get 操作
impl PartialEq<ThreadSafePtr> for str {
    #[inline]
    fn eq(&self, other: &ThreadSafePtr) -> bool {
        // Safety: ThreadSafePtr 保证指针有效
        unsafe {
            let inner = other.0.as_ref();
            inner.string_len == self.len() && inner.as_str() == self
        }
    }
}

impl Equivalent<ThreadSafePtr> for str {
    #[inline]
    fn equivalent(&self, key: &ThreadSafePtr) -> bool { self.eq(key) }
}

/// 全局字符串池，用于复用相同内容的字符串
///
/// 使用 ahash 提供更好的哈希性能
static ARC_STR_POOL: ManuallyInit<RwLock<HashSet<ThreadSafePtr, ::ahash::RandomState>>> =
    ManuallyInit::new();

#[inline(always)]
pub(super) fn __init() {
    ARC_STR_POOL
        .init(RwLock::new(HashSet::with_capacity_and_hasher(128, ::ahash::RandomState::new())))
}

impl ArcStr {
    /// 创建或复用字符串实例
    ///
    /// 如果池中已存在相同内容的字符串，则增加其引用计数并返回；
    /// 否则创建新实例并加入池中。
    ///
    /// # 并发安全性
    /// - 使用 read-write lock 保护全局字符串池
    /// - 快速路径（read lock）：尝试复用已有实例
    /// - 慢速路径（write lock）：双重检查后创建新实例，防止重复插入
    ///
    /// # 示例
    /// ```
    /// let s1 = ArcStr::new("hello");
    /// let s2 = ArcStr::new("hello"); // 复用 s1 的内存，增加引用计数
    /// assert_eq!(s1.as_ptr(), s2.as_ptr()); // 指向同一内存
    /// ```
    pub fn new<S: AsRef<str>>(s: S) -> Self {
        let string = s.as_ref();

        // 快速路径：尝试从池中查找并增加引用计数
        {
            let pool = ARC_STR_POOL.read();
            if let Some(ptr_ref) = pool.get(string) {
                let ptr = ptr_ref.0;
                // Safety: 池中的指针始终有效（只要池中存在，计数必定 > 0）
                unsafe {
                    let count = ptr.as_ref().count.fetch_add(1, Ordering::Relaxed);
                    // 防止引用计数溢出（理论上不可能，但作为安全检查）
                    if count > isize::MAX as usize {
                        __cold_path!();
                        std::process::abort();
                    }
                }
                return Self { ptr, _marker: PhantomData };
            }
        }

        // 慢速路径：创建新实例（需要独占访问池）
        let mut pool = ARC_STR_POOL.write();

        // 双重检查：防止在获取 write lock 前，其他线程已经创建了相同的字符串
        if let Some(ptr_ref) = pool.get(string) {
            let ptr = ptr_ref.0;
            // Safety: 池中的指针始终有效
            unsafe {
                let count = ptr.as_ref().count.fetch_add(1, Ordering::Relaxed);
                if count > isize::MAX as usize {
                    __cold_path!();
                    std::process::abort();
                }
            }
            return Self { ptr, _marker: PhantomData };
        }

        // 分配并初始化新实例（使用自定义 DST 布局）
        let layout = ArcStrInner::layout_for_string(string.len());
        let ptr = unsafe {
            let alloc = alloc::alloc::alloc(layout) as *mut ArcStrInner;
            if alloc.is_null() {
                __cold_path!();
                alloc::alloc::handle_alloc_error(layout);
            }
            let ptr = NonNull::new_unchecked(alloc);
            // 初始化 ArcStrInner，引用计数为 1，并拷贝字符串内容
            ArcStrInner::write_with_string(ptr, string);
            ptr
        };

        // 将新实例插入池中（持有 write lock，保证线程安全）
        pool.insert(ThreadSafePtr(ptr));

        Self { ptr, _marker: PhantomData }
    }

    /// 获取字符串切片
    #[inline(always)]
    pub fn as_str(&self) -> &str {
        // Safety: ptr 始终指向有效的 ArcStrInner
        unsafe { self.ptr.as_ref().as_str() }
    }

    /// 获取字符串长度（字节数）
    #[inline(always)]
    pub fn len(&self) -> usize {
        // Safety: ptr 始终指向有效的 ArcStrInner
        unsafe { self.ptr.as_ref().string_len }
    }

    /// 检查字符串是否为空
    #[inline(always)]
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// 获取当前引用计数
    ///
    /// 主要用于调试和测试
    #[inline(always)]
    pub fn ref_count(&self) -> usize {
        // Safety: ptr 始终指向有效的 ArcStrInner
        unsafe { self.ptr.as_ref().count.load(Ordering::Relaxed) }
    }
}

impl Drop for ArcStr {
    fn drop(&mut self) {
        // Safety: ptr 始终指向有效的 ArcStrInner（在其引用计数 > 0 时）
        unsafe {
            let inner = self.ptr.as_ref();

            // 递减引用计数，使用 Release ordering 确保之前的所有修改对后续操作可见
            if inner.count.fetch_sub(1, Ordering::Release) != 1 {
                // 不是最后一个引用，直接返回
                return;
            }

            // 最后一个引用：需要清理资源
            // 获取 write lock 以保护池操作，同时防止并发的 new() 操作干扰
            let mut pool = ARC_STR_POOL.write();

            // 双重检查引用计数：防止在等待 write lock 期间，其他线程通过 new() 增加了引用
            // 关键竞态场景：
            //   Thread A: fetch_sub 返回 1，认为自己是最后一个引用
            //   Thread B: 在 new() 中通过 pool.get() 找到相同字符串
            //   Thread B: fetch_add 增加计数到 1
            //   Thread A: 获取 write lock
            // 此时必须重新检查，否则会错误地释放正在使用的内存
            if inner.count.load(Ordering::Relaxed) != 0 {
                // 有新的引用产生，取消释放操作
                return;
            }

            // 确认是最后一个引用，执行清理：
            // 1. 从池中移除（防止后续 new() 找到已释放的指针）
            //    注意：remove 操作通过 Borrow<str> trait 使用字符串内容作为 key
            pool.remove(&ThreadSafePtr(self.ptr));

            // 2. 释放堆内存（包括 ArcStrInner 和内联的字符串数据）
            let layout = ArcStrInner::layout_for_string(inner.string_len);
            alloc::alloc::dealloc(self.ptr.cast().as_ptr(), layout);
        }
    }
}

// ===== Trait 实现 =====

impl PartialEq for ArcStr {
    #[inline]
    fn eq(&self, other: &Self) -> bool { self.ptr == other.ptr }
}

impl Eq for ArcStr {}

impl Hash for ArcStr {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) { state.write_str(self.as_str()); }
}

impl ::core::fmt::Display for ArcStr {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ::core::fmt::Debug for ArcStr {
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        f.debug_struct("ArcStr")
            .field("ptr", &self.ptr)
            .field("content", &self.as_str())
            .field("len", &self.len())
            .field("ref_count", &self.ref_count())
            .finish()
    }
}

impl AsRef<str> for ArcStr {
    #[inline]
    fn as_ref(&self) -> &str { self.as_str() }
}

impl ::core::ops::Deref for ArcStr {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target { self.as_str() }
}

// ===== 与其他字符串类型的相等性比较 =====

impl PartialEq<str> for ArcStr {
    #[inline]
    fn eq(&self, other: &str) -> bool { self.as_str() == other }
}

impl PartialEq<&str> for ArcStr {
    #[inline]
    fn eq(&self, other: &&str) -> bool { self.as_str() == *other }
}

impl PartialEq<ArcStr> for str {
    #[inline]
    fn eq(&self, other: &ArcStr) -> bool { self == other.as_str() }
}

impl PartialEq<ArcStr> for &str {
    #[inline]
    fn eq(&self, other: &ArcStr) -> bool { *self == other.as_str() }
}

impl PartialEq<String> for ArcStr {
    #[inline]
    fn eq(&self, other: &String) -> bool { self.as_str() == other.as_str() }
}

impl PartialEq<ArcStr> for String {
    #[inline]
    fn eq(&self, other: &ArcStr) -> bool { self.as_str() == other.as_str() }
}

// ===== From 转换实现 =====

impl<'a> From<&'a str> for ArcStr {
    #[inline]
    fn from(s: &'a str) -> Self { Self::new(s) }
}

impl From<String> for ArcStr {
    #[inline]
    fn from(s: String) -> Self { Self::new(s.as_str()) }
}

impl<'a> From<alloc::borrow::Cow<'a, str>> for ArcStr {
    #[inline]
    fn from(cow: alloc::borrow::Cow<'a, str>) -> Self { Self::new(cow.as_ref()) }
}

// ===== Serde 实现 =====

mod serde_impls {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for ArcStr {
        #[inline]
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer {
            self.as_str().serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for ArcStr {
        #[inline]
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de> {
            String::deserialize(deserializer).map(ArcStr::new)
        }
    }
}

// ===== 测试辅助函数 =====

#[cfg(test)]
/// 获取字符串池的统计信息
pub fn pool_stats() -> (usize, usize) {
    let pool = ARC_STR_POOL.read();
    (pool.len(), pool.capacity())
}

#[cfg(test)]
/// 清空字符串池（仅用于测试）
pub fn clear_pool_for_test() {
    // 等待可能的并发操作完成
    std::thread::sleep(std::time::Duration::from_millis(10));
    ARC_STR_POOL.write().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{thread, time::Duration};

    /// 运行隔离的测试，确保池状态不会相互影响
    fn run_isolated_test<F: FnOnce()>(f: F) {
        clear_pool_for_test();
        f();
        clear_pool_for_test();
    }

    #[test]
    fn test_basic_functionality() {
        run_isolated_test(|| {
            let s1 = ArcStr::new("hello");
            let s2 = ArcStr::new("hello");
            let s3 = ArcStr::new("world");

            assert_eq!(s1, s2);
            assert_ne!(s1, s3);
            assert_eq!(s1.ptr, s2.ptr);
            assert_ne!(s1.ptr, s3.ptr);
            assert_eq!(s1.as_str(), "hello");
            assert_eq!(s1.len(), 5);
            assert!(!s1.is_empty());

            let (count, _) = pool_stats();
            assert_eq!(count, 2);
        });
    }

    #[test]
    fn test_reference_counting() {
        run_isolated_test(|| {
            let s1 = ArcStr::new("test");
            assert_eq!(s1.ref_count(), 1);
            assert_eq!(pool_stats().0, 1);

            let s2 = s1.clone();
            assert_eq!(s1.ref_count(), 2);
            assert_eq!(s2.ref_count(), 2);
            assert_eq!(s1.ptr, s2.ptr);
            assert_eq!(pool_stats().0, 1);

            drop(s2);
            assert_eq!(s1.ref_count(), 1);
            assert_eq!(pool_stats().0, 1);

            drop(s1);
            thread::sleep(Duration::from_millis(1));
            assert_eq!(pool_stats().0, 0);
        });
    }

    #[test]
    fn test_pool_reuse() {
        run_isolated_test(|| {
            let s1 = ArcStr::new("reuse_test");
            let s2 = ArcStr::new("reuse_test");

            assert_eq!(s1.ptr, s2.ptr);
            assert_eq!(s1.ref_count(), 2);
            assert_eq!(pool_stats().0, 1);
        });
    }

    #[test]
    fn test_automatic_cleanup() {
        run_isolated_test(|| {
            assert_eq!(pool_stats().0, 0);

            {
                let s1 = ArcStr::new("cleanup_test");
                assert_eq!(pool_stats().0, 1);

                let _s2 = ArcStr::new("cleanup_test");
                assert_eq!(pool_stats().0, 1);
                assert_eq!(s1.ref_count(), 2);
            }

            thread::sleep(Duration::from_millis(5));
            let (count, _) = pool_stats();
            assert_eq!(count, 0);
        });
    }

    #[test]
    fn test_from_implementations() {
        run_isolated_test(|| {
            use alloc::borrow::Cow;

            let s1 = ArcStr::from("from_str");
            let s2 = ArcStr::from(String::from("from_string"));
            let s3 = ArcStr::from(Cow::Borrowed("from_cow"));
            let s4 = ArcStr::from(Cow::Owned::<str>(String::from("from_cow_owned")));

            assert_eq!(s1.as_str(), "from_str");
            assert_eq!(s2.as_str(), "from_string");
            assert_eq!(s3.as_str(), "from_cow");
            assert_eq!(s4.as_str(), "from_cow_owned");
            assert_eq!(pool_stats().0, 4);
        });
    }

    #[test]
    fn test_equality_operations() {
        run_isolated_test(|| {
            let arc_str = ArcStr::new("test");
            let arc_str2 = ArcStr::new("test");
            let arc_str3 = ArcStr::new("test3");

            assert_eq!(arc_str, arc_str2);
            assert_ne!(arc_str, arc_str3);
            assert_eq!(arc_str, "test");
            assert_eq!("test", arc_str);
            assert_eq!(arc_str, String::from("test"));
            assert_eq!(String::from("test"), arc_str);
        });
    }

    #[test]
    fn test_empty_string() {
        run_isolated_test(|| {
            let empty = ArcStr::new("");
            assert!(empty.is_empty());
            assert_eq!(empty.len(), 0);
            assert_eq!(empty.as_str(), "");
        });
    }

    #[test]
    fn test_thread_safety() {
        run_isolated_test(|| {
            use alloc::sync::Arc;

            let s = Arc::new(ArcStr::new("shared"));
            let handles: Vec<_> = (0..10)
                .map(|_| {
                    let s_clone = Arc::clone(&s);
                    thread::spawn(move || {
                        let local = ArcStr::new("shared");
                        assert_eq!(*s_clone, local);
                        assert_eq!(s_clone.ptr, local.ptr);
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }
        });
    }
}
