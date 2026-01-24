use super::raw_json::{RawJson, to_raw_json};

pub struct Cached<T, U> {
    value: T,
    cache: U,
}

impl<T, U> Cached<T, U> {
    #[inline]
    fn new<E, F>(value: T, f: F) -> Result<Cached<T, U>, E>
    where F: FnOnce(&T) -> Result<U, E> {
        Ok(Cached { cache: f(&value)?, value })
    }
}

impl<T, U> core::ops::Deref for Cached<T, U> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target { &self.value }
}

impl<T, U> core::ops::DerefMut for Cached<T, U> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.value }
}

pub struct JsonCached<T: serde::Serialize> {
    inner: Cached<T, RawJson>,
}

impl<T: serde::Serialize> JsonCached<T> {
    #[inline]
    pub fn new(value: T) -> serde_json::Result<Self> {
        Ok(JsonCached { inner: Cached::new(value, to_raw_json)? })
    }
    #[inline]
    pub fn cache(&self) -> RawJson { self.inner.cache.clone() }
}

impl<T: serde::Serialize> core::ops::Deref for JsonCached<T> {
    type Target = Cached<T, RawJson>;
    #[inline(always)]
    fn deref(&self) -> &Self::Target { &self.inner }
}

impl<T: serde::Serialize> core::ops::DerefMut for JsonCached<T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.inner }
}

// pub struct StringCached<T: core::fmt::Display> {
//     inner: Cached<T, Box<str>>,
// }

// impl<T: core::fmt::Display> StringCached<T> {
//     #[inline]
//     pub fn new(value: T) -> Self {
//         StringCached {
//             inner: match Cached::new(value, |v| Ok::<_, !>(v.to_string().into_boxed_str())) {
//                 Ok(val) => val,
//             },
//         }
//     }
//     #[inline]
//     pub fn cache(&self) -> &str { &*self.inner.cache }
// }
