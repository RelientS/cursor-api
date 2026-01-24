use core::{fmt, marker::PhantomData};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Enum<T>(pub i32, PhantomData<T>);

impl<T> Enum<T> {
    #[inline(always)]
    pub const fn new(value: i32) -> Self { Self(value, PhantomData) }
    #[inline(always)]
    pub const fn get(self) -> i32 { self.0 }
}

impl<T> Clone for Enum<T> {
    fn clone(&self) -> Self { Self(self.0, PhantomData) }
}

impl<T> Copy for Enum<T> {}

impl<T: TryFrom<i32> + fmt::Debug> fmt::Debug for Enum<T> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match T::try_from(self.get()) {
            Ok(v) => v.fmt(f),
            Err(_) => f.debug_tuple("EnumValue").field(&self.get()).finish(),
        }
    }
}

impl<T: [const] Into<i32> + [const] Default> const Default for Enum<T> {
    #[inline]
    fn default() -> Self { Self::new(T::default().into()) }
}

impl<T: [const] Into<i32>> const From<T> for Enum<T> {
    #[inline]
    fn from(value: T) -> Self { Self::new(value.into()) }
}

#[cfg(feature = "serde")]
mod serde_impls {
    use super::Enum;
    use core::{fmt, marker::PhantomData};
    use serde_core::{
        Deserialize, Deserializer, Serialize, Serializer,
        de::{self, Unexpected, Visitor, value::StrDeserializer},
    };

    impl<T: TryFrom<i32> + Serialize> Serialize for Enum<T> {
        #[inline]
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: Serializer {
            match T::try_from(self.get()) {
                Ok(v) => v.serialize(serializer),
                Err(_) => serializer.serialize_i32(self.get()),
            }
        }
    }

    impl<'de, T: Into<i32> + Deserialize<'de>> Deserialize<'de> for Enum<T> {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where D: Deserializer<'de> {
            struct EnumVisitor<T>(PhantomData<T>);

            impl<'de, T: Into<i32> + Deserialize<'de>> Visitor<'de> for EnumVisitor<T> {
                type Value = Enum<T>;

                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("an integer or a string representing the enum variant")
                }

                #[inline]
                fn visit_i32<E>(self, v: i32) -> Result<Self::Value, E>
                where E: de::Error {
                    Ok(Enum::new(v))
                }
                fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
                where E: de::Error {
                    match v.try_into() {
                        Ok(v) => self.visit_i32(v),
                        Err(_) => Err(de::Error::invalid_value(Unexpected::Signed(v), &self)),
                    }
                }
                fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
                where E: de::Error {
                    match v.try_into() {
                        Ok(v) => self.visit_i32(v),
                        Err(_) => Err(de::Error::invalid_value(Unexpected::Unsigned(v), &self)),
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where E: de::Error {
                    let t = T::deserialize(StrDeserializer::new(v))?;
                    Ok(Enum::new(t.into()))
                }
            }

            deserializer.deserialize_any(EnumVisitor(PhantomData))
        }
    }
}
