use serde::Serialize;

#[derive(Clone, Copy, PartialEq)]
pub enum TriState<T> {
    Null(bool),
    Value(T),
}

impl<T> const Default for TriState<T> {
    fn default() -> Self {
        Self::Null(false)
    }
}

impl<T> TriState<T> {
    #[inline(always)]
    pub const fn is_undefined(&self) -> bool {
        matches!(*self, TriState::Null(false))
    }

    // #[inline(always)]
    // pub const fn is_null(&self) -> bool {
    //     matches!(*self, TriState::Null)
    // }

    // #[inline(always)]
    // pub const fn is_value(&self) -> bool {
    //     matches!(*self, TriState::Value(_))
    // }

    // pub const fn as_value(&self) -> Option<&T> {
    //     match self {
    //         TriState::Value(v) => Some(v),
    //         _ => None,
    //     }
    // }
}

impl<T> Serialize for TriState<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            TriState::Null(false) => __unreachable!(),
            TriState::Null(true) => serializer.serialize_unit(),
            TriState::Value(value) => value.serialize(serializer),
        }
    }
}
