/// Cursor订阅计划类型
///
/// 各计划包含的usage额度：
/// - Free: 有限的免费额度
/// - FreeTrial: 试用期额度
/// - Pro: $20 API usage + bonus (~225 Sonnet 4.5 requests)
/// - ProPlus: $70 API usage + bonus (~675 Sonnet 4.5 requests)  
/// - Ultra: $400 API usage + bonus (~4,500 Sonnet 4.5 requests)
/// - Enterprise: 自定义配额
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
#[repr(u8)]
pub enum MembershipType {
    #[default]
    Free,
    FreeTrial,
    Pro,
    ProPlus,
    Ultra,
    Enterprise,
}

impl MembershipType {
    // 定义常量字符串
    const FREE: &'static str = "free";
    const PRO: &'static str = "pro";
    const PRO_PLUS: &'static str = "pro_plus";
    const ENTERPRISE: &'static str = "enterprise";
    const FREE_TRIAL: &'static str = "free_trial";
    const ULTRA: &'static str = "ultra";

    #[inline]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            Self::FREE => Some(MembershipType::Free),
            Self::FREE_TRIAL => Some(MembershipType::FreeTrial),
            Self::PRO => Some(MembershipType::Pro),
            Self::PRO_PLUS => Some(MembershipType::ProPlus),
            Self::ULTRA => Some(MembershipType::Ultra),
            Self::ENTERPRISE => Some(MembershipType::Enterprise),
            _ => None,
        }
    }

    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            MembershipType::Free => Self::FREE,
            MembershipType::FreeTrial => Self::FREE_TRIAL,
            MembershipType::Pro => Self::PRO,
            MembershipType::ProPlus => Self::PRO_PLUS,
            MembershipType::Ultra => Self::ULTRA,
            MembershipType::Enterprise => Self::ENTERPRISE,
        }
    }
}

impl serde::Serialize for MembershipType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: ::serde::Serializer {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for MembershipType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: ::serde::Deserializer<'de> {
        let s = <String as ::serde::Deserialize>::deserialize(deserializer)?;
        Self::from_str(&s)
            .ok_or_else(|| ::serde::de::Error::custom(format_args!("unknown membership type: {s}")))
    }
}
