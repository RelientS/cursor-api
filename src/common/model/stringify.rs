#![allow(private_bounds)]

//! 数字字符串化模块
//!
//! 用于在序列化/反序列化时将数字转换为字符串，
//! 特别适用于与 JavaScript 交互时避免精度损失的场景。

use core::fmt;
use core::marker::PhantomData;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// 密封特征，限制可以被字符串化的类型
mod private {
    use super::*;

    pub trait Sealed: Num + ::core::fmt::Display + ::core::str::FromStr {}

    impl Sealed for i64 {}
    impl Sealed for u64 {}

    pub trait Num: Sized + Copy {
        fn deserialize_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>;
    }

    impl Num for i64 {
        #[inline(always)]
        fn deserialize_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_any(NumVisitor::<Self>(PhantomData))
        }
    }

    impl Num for u64 {
        #[inline(always)]
        fn deserialize_from<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_any(NumVisitor::<Self>(PhantomData))
        }
    }

    struct NumVisitor<T: Num>(PhantomData<T>);

    impl<'de> de::Visitor<'de> for NumVisitor<i64> {
        type Value = i64;

        #[inline]
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an integer or a string containing an integer")
        }

        #[inline]
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(v)
        }

        #[inline]
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            i64::try_from(v).map_err(|_| E::custom(format_args!("integer {v} is out of range")))
        }

        #[inline]
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse().map_err(|_| E::custom(format_args!("invalid integer: {v}")))
        }
    }

    impl<'de> de::Visitor<'de> for NumVisitor<u64> {
        type Value = u64;

        #[inline]
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an unsigned integer or a string containing an unsigned integer")
        }

        #[inline]
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(v)
        }

        #[inline]
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse().map_err(|_| E::custom(format_args!("invalid unsigned integer: {v}")))
        }
    }
}

/// 可字符串化的项特征（内部使用）
trait Item: Sized + Copy {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>;
}

// 为所有 Sealed 类型实现 Item
impl<T> Item for T
where
    T: private::Sealed,
{
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }

    #[inline]
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize_from(deserializer)
    }
}

struct OptVisitor<T>(PhantomData<T>);

impl<'de, T> de::Visitor<'de> for OptVisitor<T>
where
    T: private::Sealed,
{
    type Value = Option<T>;

    #[inline]
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a string, a integer or null")
    }

    #[inline]
    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(None)
    }

    #[inline]
    fn visit_some<DE: Deserializer<'de>>(self, deserializer: DE) -> Result<Self::Value, DE::Error> {
        T::deserialize_from(deserializer).map(Some)
    }
}

// 为 Option<T> 实现 Item
impl<T> Item for Option<T>
where
    T: private::Sealed,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Some(value) => value.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_option(OptVisitor(PhantomData))
    }
}

/// 字符串化包装器
///
/// 用于将数字类型在序列化时转换为字符串
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Stringify<T>(pub T);

// impl<T> Stringify<T> {
//     /// 创建新的字符串化包装器
//     #[inline(always)]
//     #[must_use]
//     pub const fn new(value: T) -> Self {
//         Self(value)
//     }

//     /// 获取内部值的引用
//     #[inline(always)]
//     #[must_use]
//     pub const fn inner(&self) -> &T {
//         &self.0
//     }

//     /// 获取内部值
//     #[inline(always)]
//     #[must_use]
//     pub fn into_inner(self) -> T {
//         self.0
//     }
// }

impl<T: Item> Serialize for Stringify<T> {
    #[inline]
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de, T: Item> Deserialize<'de> for Stringify<T> {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self)
    }
}

/// 用于 `#[serde(with = "stringify")]` 的序列化函数
#[inline]
pub fn serialize<T: Item, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    value.serialize(serializer)
}

/// 用于 `#[serde(with = "stringify")]` 的反序列化函数
#[inline]
pub fn deserialize<'de, T: Item, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error> {
    T::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestStruct {
        #[serde(with = "super")]
        large_number: u64,

        #[serde(with = "super")]
        optional_number: Option<i64>,
    }

    #[test]
    fn test_stringify_wrapper() {
        let value = Stringify(9007199254740993u64);
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, r#""9007199254740993""#);

        let deserialized: Stringify<u64> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.0, 9007199254740993u64);
    }

    #[test]
    fn test_with_attribute() {
        let test = TestStruct {
            large_number: 9007199254740993u64,
            optional_number: Some(-9007199254740993i64),
        };

        let json = serde_json::to_string(&test).unwrap();
        assert!(json.contains(r#""large_number":"9007199254740993""#));
        assert!(json.contains(r#""optional_number":"-9007199254740993""#));

        let deserialized: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, test);
    }

    #[test]
    fn test_optional_none() {
        let test = TestStruct { large_number: 123, optional_number: None };

        let json = serde_json::to_string(&test).unwrap();
        assert!(json.contains(r#""optional_number":null"#));

        let deserialized: TestStruct = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.optional_number, None);
    }
}
