/// Stripe订阅状态枚举
///
/// 基于Stripe API定义的订阅生命周期状态
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, ::rkyv::Archive, ::rkyv::Serialize, ::rkyv::Deserialize,
)]
#[repr(u8)]
pub enum SubscriptionStatus {
    /// 试用期 - 客户可安全使用产品，首次付款后自动转为active
    Trialing,

    /// 活跃状态 - 订阅状态良好，可正常提供服务
    Active,

    /// 未完成 - 客户必须在23小时内成功付款以激活订阅
    /// 或付款需要额外操作（如客户认证）
    Incomplete,

    /// 未完成已过期 - 初始付款失败且23小时内未成功付款
    /// 这些订阅不会向客户计费，用于跟踪激活失败的客户
    IncompleteExpired,

    /// 逾期未付 - 最新发票付款失败或未尝试付款
    /// 订阅继续生成发票，根据设置可转为canceled/unpaid或保持past_due
    PastDue,

    /// 已取消 - 订阅已取消，终态，无法更新
    /// 取消期间所有未付发票的自动收款被禁用
    Canceled,

    /// 未支付 - 最新发票未付但订阅仍存在
    /// 发票保持开放状态并继续生成，但不尝试付款
    /// 应撤销产品访问权限，因为在past_due期间已尝试并重试过付款
    Unpaid,

    /// 已暂停 - 试用期结束但无默认支付方式且设置为暂停
    /// 不再为订阅创建发票，添加支付方式后可恢复
    Paused,
}

impl SubscriptionStatus {
    const TRIALING: &'static str = "trialing";
    const ACTIVE: &'static str = "active";
    const INCOMPLETE: &'static str = "incomplete";
    const INCOMPLETE_EXPIRED: &'static str = "incomplete_expired";
    const PAST_DUE: &'static str = "past_due";
    const CANCELED: &'static str = "canceled";
    const UNPAID: &'static str = "unpaid";
    const PAUSED: &'static str = "paused";

    #[inline]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            Self::TRIALING => Some(Self::Trialing),
            Self::ACTIVE => Some(Self::Active),
            Self::INCOMPLETE => Some(Self::Incomplete),
            Self::INCOMPLETE_EXPIRED => Some(Self::IncompleteExpired),
            Self::PAST_DUE => Some(Self::PastDue),
            Self::CANCELED => Some(Self::Canceled),
            Self::UNPAID => Some(Self::Unpaid),
            Self::PAUSED => Some(Self::Paused),
            _ => None,
        }
    }

    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trialing => Self::TRIALING,
            Self::Active => Self::ACTIVE,
            Self::Incomplete => Self::INCOMPLETE,
            Self::IncompleteExpired => Self::INCOMPLETE_EXPIRED,
            Self::PastDue => Self::PAST_DUE,
            Self::Canceled => Self::CANCELED,
            Self::Unpaid => Self::UNPAID,
            Self::Paused => Self::PAUSED,
        }
    }
}

impl ::serde::Serialize for SubscriptionStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: ::serde::Serializer {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> ::serde::Deserialize<'de> for SubscriptionStatus {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: ::serde::Deserializer<'de> {
        let s = <String as ::serde::Deserialize>::deserialize(deserializer)?;
        Self::from_str(&s).ok_or_else(|| {
            ::serde::de::Error::custom(format_args!("unknown subscription status: {s}"))
        })
    }
}
