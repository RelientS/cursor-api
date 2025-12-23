#![allow(dead_code)]

//! High-performance string builders for efficient string concatenation.
//!
//! This crate provides a flexible `StringBuilder` that optimizes storage:
//! - Stores only `&'a str` if only borrowed strings are added.
//! - Automatically transitions to store `Cow<'a, str>` if any owned string (`String`, `Cow::Owned`) is added.
//! - Uses a sealed trait pattern for type safety.

use core::{fmt::Debug, mem::MaybeUninit};
use alloc::borrow::Cow;

mod private {
    use alloc::borrow::Cow;
    pub trait Sealed: Sized {}

    impl Sealed for &str {}
    impl Sealed for String {}
    impl Sealed for &String {}
    impl<'a> Sealed for Cow<'a, str> {}
}

/// A trait representing types that can be appended to a `StringBuilder`.
/// This is a sealed trait and cannot be implemented for types outside this crate.
pub trait StringPart<'a>: private::Sealed + Into<Cow<'a, str>> {}

impl<'a, T> StringPart<'a> for T where T: private::Sealed + Into<Cow<'a, str>> {}

/// Internal storage state for StringBuilder
///
/// # Safety Invariants for MaybeUninit Usage
///
/// The `MaybeUninit<Vec<&'a str>>` in the `Borrowed` variant maintains the following invariants:
/// - It is ALWAYS initialized when accessed from outside the `push_part` method
/// - The ONLY time it becomes uninitialized is during the critical conversion in `push_part`
/// - After `assume_init_read()`, the storage is immediately replaced with `Mixed` variant
/// - The `&mut self` exclusive access guarantee ensures no concurrent access during conversion
#[derive(Debug)]
enum Storage<'a> {
    Borrowed(MaybeUninit<Vec<&'a str>>),
    Mixed(Vec<Cow<'a, str>>),
}

impl<'a> Storage<'a> {
    #[inline]
    const fn new_borrowed() -> Self { Self::Borrowed(MaybeUninit::new(Vec::new())) }

    #[inline]
    fn new_borrowed_with_capacity(cap: usize) -> Self {
        Self::Borrowed(MaybeUninit::new(Vec::with_capacity(cap)))
    }
}

impl<'a> Clone for Storage<'a> {
    fn clone(&self) -> Self {
        match self {
            Storage::Borrowed(maybe_vec) => {
                // 安全：外部访问时一定是已初始化的
                Storage::Borrowed(MaybeUninit::new(
                    unsafe { maybe_vec.assume_init_ref() }.clone(),
                ))
            }
            Storage::Mixed(vec) => Storage::Mixed(vec.clone()),
        }
    }
}

impl<'a> const Default for Storage<'a> {
    #[inline]
    fn default() -> Self { Self::new_borrowed() }
}

/// A builder for efficiently concatenating strings with adaptive storage.
#[derive(Debug, Clone, Default)]
pub struct StringBuilder<'a> {
    storage: Storage<'a>,
    total_len: usize,
}

impl<'a> StringBuilder<'a> {
    /// Creates a new, empty `StringBuilder` in `Borrowed` state.
    #[inline(always)]
    pub const fn new() -> Self {
        StringBuilder {
            storage: Storage::new_borrowed(),
            total_len: 0,
        }
    }

    /// Creates a new `StringBuilder` with capacity, in `Borrowed` state.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        StringBuilder {
            storage: Storage::new_borrowed_with_capacity(capacity),
            total_len: 0,
        }
    }

    #[inline]
    fn push_part(&mut self, part: Cow<'a, str>) {
        self.total_len += part.len();
        match &mut self.storage {
            Storage::Mixed(vec) => vec.push(part),
            Storage::Borrowed(maybe_vec) => match part {
                Cow::Borrowed(s) => {
                    // 安全：进入方法时一定是已初始化的
                    unsafe { maybe_vec.assume_init_mut() }.push(s);
                }
                Cow::Owned(s) => {
                    // 关键转换：偷取值并立即替换整个 Storage
                    let old_vec = unsafe { maybe_vec.assume_init_read() };
                    // 此时 maybe_vec 未初始化，但会被立即替换

                    let mut new_vec: Vec<Cow<'a, str>> = Vec::with_capacity(old_vec.len() + 1);
                    new_vec.extend(old_vec.into_iter().map(Cow::Borrowed));
                    new_vec.push(Cow::Owned(s));
                    self.storage = Storage::Mixed(new_vec);
                }
            },
        }
    }

    /// Appends a string part, potentially transitioning storage state.
    #[inline]
    pub fn append<S>(mut self, s: S) -> Self
    where
        S: StringPart<'a>,
    {
        self.push_part(s.into());
        self
    }

    /// Mutable version of append for use in loops.
    #[inline]
    pub fn append_mut<S>(&mut self, s: S) -> &mut Self
    where
        S: StringPart<'a>,
    {
        self.push_part(s.into());
        self
    }

    /// Builds the final string, consuming the builder.
    #[inline]
    pub fn build(self) -> String {
        if self.total_len == 0 {
            return String::new();
        }

        match self.storage {
            Storage::Borrowed(maybe_parts) => {
                // 安全：外部访问时一定是已初始化的
                let parts = unsafe { maybe_parts.assume_init() };
                if parts.len() == 1 {
                    return parts[0].to_string();
                }
                let mut result = String::with_capacity(self.total_len);
                for part in parts {
                    result.push_str(part);
                }
                result
            }
            Storage::Mixed(mut parts) => {
                if parts.len() == 1 {
                    return unsafe { parts.pop().unwrap_unchecked().into_owned() };
                }
                let mut result = String::with_capacity(self.total_len);
                for part in parts {
                    result.push_str(&part);
                }
                result
            }
        }
    }

    #[inline(always)]
    pub const fn len(&self) -> usize { self.total_len }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool { self.total_len == 0 }

    /// Clears the builder and resets state to `Borrowed`.
    #[inline]
    pub fn clear(&mut self) {
        self.storage = Storage::Borrowed(MaybeUninit::new(Vec::new()));
        self.total_len = 0;
    }

    /// Reserves capacity.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        match &mut self.storage {
            Storage::Borrowed(vec) => unsafe { vec.assume_init_mut().reserve(additional) },
            Storage::Mixed(vec) => vec.reserve(additional),
        }
    }

    fn parts_len(&self) -> usize {
        match &self.storage {
            Storage::Borrowed(vec) => unsafe { vec.assume_init_ref().len() },
            Storage::Mixed(vec) => vec.len(),
        }
    }

    /// Adds a string separator between each part when building.
    #[inline]
    pub fn join<S>(self, separator: S) -> JoinBuilder<'a>
    where
        S: StringPart<'a>,
    {
        let sep: Cow<'a, str> = separator.into();
        let sep_len = sep.len();
        let parts_count = self.parts_len();

        let total_sep_len = if parts_count > 0 {
            sep_len.saturating_mul(parts_count.saturating_sub(1))
        } else {
            0
        };

        JoinBuilder {
            builder: self,
            separator: sep,
            separator_total_len: total_sep_len,
        }
    }
}

/// A specialized builder that joins string parts with a separator.
#[derive(Debug, Clone)]
pub struct JoinBuilder<'a> {
    builder: StringBuilder<'a>,
    separator: Cow<'a, str>,
    separator_total_len: usize,
}

impl<'a> JoinBuilder<'a> {
    /// Builds the final string with separators, consuming the builder.
    #[inline]
    pub fn build(self) -> String {
        if self.builder.is_empty() {
            return String::new();
        }
        if self.builder.parts_len() == 1 {
            return self.builder.build();
        }

        let mut result = String::with_capacity(self.builder.total_len + self.separator_total_len);
        let separator_str: &str = &self.separator;

        match self.builder.storage {
            Storage::Borrowed(parts) => {
                let mut iter = unsafe { parts.assume_init_read().into_iter() };
                if let Some(first) = iter.next() {
                    result.push_str(first);
                }
                for part in iter {
                    result.push_str(separator_str);
                    result.push_str(part);
                }
            }
            Storage::Mixed(parts) => {
                let mut iter = parts.into_iter();
                if let Some(first) = iter.next() {
                    result.push_str(&first);
                }
                for part in iter {
                    result.push_str(separator_str);
                    result.push_str(&part);
                }
            }
        }
        result
    }
}

/// Joins an iterator of string parts with a separator.
#[inline]
pub fn join<'a, S, I, T>(separator: S, iter: I) -> String
where
    S: StringPart<'a> + Clone,
    I: IntoIterator<Item = T>,
    T: StringPart<'a>,
{
    let iterator = iter.into_iter();
    let mut builder = StringBuilder::new();
    let (lower, _) = iterator.size_hint();
    builder.reserve(lower);

    for item in iterator {
        builder.append_mut(item);
    }
    builder.join(separator).build()
}

/// Concatenates an iterator of string parts without a separator.
#[inline]
pub fn concat<'a, I, T>(iter: I) -> String
where
    I: IntoIterator<Item = T>,
    T: StringPart<'a>,
{
    let iterator = iter.into_iter();
    let mut builder = StringBuilder::new();
    let (lower, _) = iterator.size_hint();
    builder.reserve(lower);
    for item in iterator {
        builder.append_mut(item);
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use alloc::borrow::Cow;

    use super::*;

    impl<'a> StringBuilder<'a> {
        fn is_borrowed_state(&self) -> bool { matches!(self.storage, Storage::Borrowed(_)) }
        fn is_mixed_state(&self) -> bool { matches!(self.storage, Storage::Mixed(_)) }
    }

    #[test]
    fn test_state_transition() {
        let mut builder = StringBuilder::new();
        assert!(builder.is_borrowed_state());
        builder.append_mut("hello");
        assert!(builder.is_borrowed_state());
        builder.append_mut(String::from(" world"));
        assert!(builder.is_mixed_state());
        builder.append_mut("!");
        assert!(builder.is_mixed_state());
        assert_eq!(builder.build(), "hello world!");
    }

    #[test]
    fn test_clear_resets_state() {
        let mut builder = StringBuilder::new();
        builder.append_mut(String::from(" world"));
        assert!(builder.is_mixed_state());
        builder.clear();
        assert!(builder.is_borrowed_state());
        assert!(builder.is_empty());
        builder.append_mut("borrowed again");
        assert!(builder.is_borrowed_state());
        assert_eq!(builder.build(), "borrowed again");
    }

    #[test]
    fn test_basic_builder_borrowed_only() {
        let builder = StringBuilder::with_capacity(3)
            .append("Hello")
            .append(", ")
            .append("world!");
        assert!(builder.is_borrowed_state());
        assert_eq!(builder.build(), "Hello, world!");
    }

    #[test]
    fn test_builder_with_string_ref() {
        let owned = String::from("owned");
        let builder = StringBuilder::with_capacity(3)
            .append("prefix: ")
            .append(&owned)
            .append(" suffix");
        assert!(builder.is_borrowed_state());
        assert_eq!(builder.build(), "prefix: owned suffix");
    }

    #[test]
    fn test_builder_join_borrowed() {
        let parts = ["apple", "banana", "cherry"];
        let sep = ", ";
        let builder = StringBuilder::with_capacity(3)
            .append(parts[0])
            .append(parts[1])
            .append(parts[2]);
        assert!(builder.is_borrowed_state());
        let result = builder.join(sep).build();
        assert_eq!(result, "apple, banana, cherry");
    }

    #[test]
    fn test_builder_join_mixed() {
        let parts = ["apple", "banana", "cherry"];
        let sep = String::from(", ");
        let builder = StringBuilder::with_capacity(3)
            .append(parts[0])
            .append(String::from(parts[1]))
            .append(parts[2]);
        assert!(builder.is_mixed_state());
        let result = builder.join(&sep).build();
        assert_eq!(result, "apple, banana, cherry");

        let result_single = StringBuilder::new().append("apple").join(", ").build();
        assert_eq!(result_single, "apple");
        let result_single_owned = StringBuilder::new()
            .append(String::from("apple"))
            .join(", ")
            .build();
        assert_eq!(result_single_owned, "apple");

        let result_empty = StringBuilder::new().join(", ").build();
        assert_eq!(result_empty, "");
    }

    #[test]
    fn test_mixed_types_transition() {
        let literal = "world";
        let owned = String::from("!");
        let borrowed_owned = String::from(", ");
        let cow: Cow<'_, str> = Cow::Borrowed("Hello");
        let cow_owned: Cow<'_, str> = Cow::Owned(String::from(" start"));

        let builder = StringBuilder::with_capacity(5)
            .append(cow)
            .append(&borrowed_owned)
            .append(literal);
        assert!(builder.is_borrowed_state());

        let builder = builder.append(owned);
        assert!(builder.is_mixed_state());

        let builder = builder.append(cow_owned);
        assert!(builder.is_mixed_state());

        assert_eq!(builder.build(), "Hello, world! start");
    }

    #[test]
    fn test_single_optimizations() {
        let b1 = StringBuilder::new().append("borrowed");
        assert!(b1.is_borrowed_state());
        assert_eq!(b1.build(), "borrowed");

        let owned = String::from("Hello, world!");
        let b2 = StringBuilder::new().append(owned.clone());
        assert!(b2.is_mixed_state());
        assert_eq!(b2.build(), owned);

        let owned_cow: Cow<'static, str> = Cow::Owned(String::from("Hello, world!"));
        let b3 = StringBuilder::new().append(owned_cow.clone());
        assert!(b3.is_mixed_state());
        assert_eq!(b3.build(), "Hello, world!");

        let borrowed_cow: Cow<'static, str> = Cow::Borrowed("Hello, world!");
        let b4 = StringBuilder::new().append(borrowed_cow.clone());
        assert!(b4.is_borrowed_state());
        assert_eq!(b4.build(), "Hello, world!");
    }

    #[test]
    fn test_format() {
        let builder = StringBuilder::with_capacity(3)
            .append("The answer is ")
            .append(42.to_string())
            .append(".");
        assert!(builder.is_mixed_state());
        assert_eq!(builder.build(), "The answer is 42.");
    }

    #[test]
    fn test_mutable_append() {
        let mut builder = StringBuilder::with_capacity(4);
        builder.append_mut("Start: ");
        assert!(builder.is_borrowed_state());
        for i in 0..3 {
            builder.append_mut(format!("{i} "));
        }
        assert!(builder.is_mixed_state());
        builder.append_mut("done");
        assert!(builder.is_mixed_state());
        assert_eq!(builder.build(), "Start: 0 1 2 done");
    }

    #[test]
    fn test_clone_functionality() {
        // 测试 Clone 在 Borrowed 状态下的正确性
        let builder1 = StringBuilder::with_capacity(2)
            .append("Hello")
            .append(" World");
        assert!(builder1.is_borrowed_state());

        let builder2 = builder1.clone();
        assert!(builder2.is_borrowed_state());

        // 两个 builder 应该产生相同的结果
        assert_eq!(builder1.build(), "Hello World");
        assert_eq!(builder2.build(), "Hello World");
    }

    #[test]
    fn test_clone_mixed_state() {
        // 测试 Clone 在 Mixed 状态下的正确性
        let builder1 = StringBuilder::with_capacity(3)
            .append("Hello")
            .append(String::from(" World"))
            .append("!");
        assert!(builder1.is_mixed_state());

        let builder2 = builder1.clone();
        assert!(builder2.is_mixed_state());

        // 两个 builder 应该产生相同的结果
        assert_eq!(builder1.build(), "Hello World!");
        assert_eq!(builder2.build(), "Hello World!");
    }

    #[test]
    fn test_maybe_uninit_state_transition() {
        // 测试 MaybeUninit 在状态转换过程中的安全性
        let mut builder = StringBuilder::with_capacity(3);

        // 初始状态：Borrowed，MaybeUninit 已初始化
        assert!(builder.is_borrowed_state());

        // 添加借用字符串，保持 Borrowed 状态
        builder.append_mut("Hello");
        builder.append_mut(" ");
        assert!(builder.is_borrowed_state());

        // 添加拥有字符串，触发状态转换
        builder.append_mut(String::from("World"));
        assert!(builder.is_mixed_state());

        // 继续添加应该正常工作
        builder.append_mut("!");
        assert!(builder.is_mixed_state());

        assert_eq!(builder.build(), "Hello World!");
    }

    #[test]
    fn test_empty_builder_safety() {
        // 测试空 builder 的安全性
        let empty_builder = StringBuilder::with_capacity(0);
        assert!(empty_builder.is_borrowed_state());
        assert!(empty_builder.is_empty());

        // 空 builder 的 clone 也应该安全
        let cloned_empty = empty_builder.clone();
        assert!(cloned_empty.is_borrowed_state());
        assert!(cloned_empty.is_empty());

        // 测试 build 功能
        assert_eq!(empty_builder.build(), "");
        assert_eq!(cloned_empty.build(), "");
    }

    #[test]
    fn test_free_join() {
        let items = vec!["apple", "banana"];
        let s = String::from("cherry");
        let iter = items.iter().cloned().chain(std::iter::once(s.as_str()));
        assert_eq!(join(", ", iter), "apple, banana, cherry");

        let items_owned = vec![String::from("a"), String::from("b")];
        assert_eq!(join(String::from("-"), items_owned.into_iter()), "a-b");

        assert_eq!(join(",", Vec::<&str>::new()), "");
        assert_eq!(join(",", vec!["single"]), "single");
    }

    #[test]
    fn test_free_concat() {
        let items = vec!["Hello", ", "];
        let s = String::from("world!");
        let iter = items.iter().cloned().chain(std::iter::once(s.as_str()));
        assert_eq!(concat(iter), "Hello, world!");

        let items_mixed: Vec<Cow<str>> = vec!["Hello".into(), String::from(", world!").into()];
        assert_eq!(concat(items_mixed), "Hello, world!");

        assert_eq!(concat(Vec::<&str>::new()), "");
    }

    #[test]
    fn test_clear_and_len() {
        let mut builder = StringBuilder::new();
        assert!(builder.is_empty());
        assert_eq!(builder.len(), 0);
        builder.append_mut("hello");
        assert!(builder.is_borrowed_state());
        builder.append_mut(String::from(" world"));
        assert!(builder.is_mixed_state());
        assert!(!builder.is_empty());
        assert_eq!(builder.len(), 11);
        builder.clear();
        assert!(builder.is_borrowed_state());
        assert!(builder.is_empty());
        assert_eq!(builder.len(), 0);
        assert_eq!(builder.build(), "");
    }
}
