/// 配额限制类型
///
/// 决定配额是个人级别还是团队共享
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
#[repr(u8)]
pub enum LimitType {
    /// 用户级别限制（个人配额）
    User,
    /// 团队级别限制（共享池）
    Team,
}

impl LimitType {
    const USER: &'static str = "user";
    const TEAM: &'static str = "team";

    #[inline]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            Self::USER => Some(LimitType::User),
            Self::TEAM => Some(LimitType::Team),
            _ => None,
        }
    }

    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            LimitType::User => Self::USER,
            LimitType::Team => Self::TEAM,
        }
    }
}

impl serde::Serialize for LimitType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: ::serde::Serializer {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for LimitType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: ::serde::Deserializer<'de> {
        let s = <String as ::serde::Deserialize>::deserialize(deserializer)?;
        Self::from_str(&s)
            .ok_or_else(|| ::serde::de::Error::custom(format_args!("unknown limit type: {s}")))
    }
}
