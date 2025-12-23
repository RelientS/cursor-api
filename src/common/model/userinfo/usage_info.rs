use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct UsageSummary {
    /// 订阅计划的使用情况（包含的额度）
    pub plan: Option<PlanUsage>,
    /// 按需使用情况（超出包含额度后的付费使用）
    #[serde(alias = "onDemand", skip_serializing_if = "Option::is_none")]
    pub on_demand: Option<OnDemandUsage>,
}

/// 个人用户的使用情况
pub type IndividualUsage = UsageSummary;

/// 团队级别的使用情况
pub type TeamUsage = UsageSummary;

/// 订阅计划的使用情况
///
/// 包含计划自带的API usage额度，以API价格计费
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct PlanUsage {
    pub enabled: bool,
    /// 已使用量（可能是花费单位或请求计量单位）
    pub used: i32,
    /// 配额上限（当前计费周期内的总限额）
    pub limit: i32,
    /// 剩余可用量 (= limit - used)
    pub remaining: i32,
    /// 配额来源细分
    #[serde(default)]
    pub breakdown: UsageBreakdown,
}

/// 配额来源细分
///
/// - `included`: 计划包含的基础配额（如Pro的$20对应的量）
/// - `bonus`: 额外赠送的bonus capacity（动态发放）
/// - `total`: included + bonus（总承诺配额）
///
/// 注意：`total`可能小于或等于`PlanUsage.limit`，其中：
/// - `limit`是账户的总配额上限
/// - `breakdown`记录已发放/统计的配额细分
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct UsageBreakdown {
    /// 基础包含配额
    pub included: i32,
    /// 额外赠送配额（"work hard to grant additional bonus capacity"）
    pub bonus: i32,
    /// 总计 = included + bonus
    pub total: i32,
}

/// 按需使用情况
///
/// 当用户超出计划包含的配额后，可启用on-demand付费使用
/// 按相同的API价格计费，无质量或速度降级
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct OnDemandUsage {
    /// 是否启用按需计费
    pub enabled: bool,
    /// 已使用的按需配额
    pub used: i32,
    /// 按需配额上限（None表示无限制或未设置）
    pub limit: Option<i32>,
    /// 剩余按需配额
    pub remaining: Option<i32>,
}
