use crate::common::model::stringify;

// #[derive(serde::Serialize, Clone, Copy)]
// #[serde(rename_all = "camelCase")]
// pub struct GetAggregatedUsageEventsRequest {
//     pub team_id: i32,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub start_date: Option<i64>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub end_date: Option<i64>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub user_id: Option<i32>,
// }

// #[derive(
//     serde::Serialize,
//     serde::Deserialize,
//     rkyv::Archive,
//     rkyv::Serialize,
//     rkyv::Deserialize,
//     Clone
// )]
// pub struct AggregatedUsageEvents {
//     #[serde(default)]
//     pub aggregations: Vec<get_aggregated_usage_events_response::ModelUsageAggregation>,
//     #[serde(alias = "totalInputTokens", deserialize_with = "stringify::deserialize", default)]
//     pub total_input_tokens: i64,
//     #[serde(alias = "totalOutputTokens", deserialize_with = "stringify::deserialize", default)]
//     pub total_output_tokens: i64,
//     #[serde(alias = "totalCacheWriteTokens", deserialize_with = "stringify::deserialize", default)]
//     pub total_cache_write_tokens: i64,
//     #[serde(alias = "totalCacheReadTokens", deserialize_with = "stringify::deserialize", default)]
//     pub total_cache_read_tokens: i64,
//     #[serde(alias = "totalCostCents", default)]
//     pub total_cost_cents: f64,
//     #[serde(alias = "percentOfBurstUsed", default)]
//     pub percent_of_burst_used: f64,
// }

// pub type GetAggregatedUsageEventsResponse = AggregatedUsageEvents;

// pub mod get_aggregated_usage_events_response {
//     use super::stringify;

//     #[derive(
//         serde::Serialize,
//         serde::Deserialize,
//         rkyv::Archive,
//         rkyv::Serialize,
//         rkyv::Deserialize,
//         Clone
//     )]
//     pub struct ModelUsageAggregation {
//         #[serde(alias = "modelIntent", default)]
//         pub model_intent: String,
//         #[serde(alias = "inputTokens", deserialize_with = "stringify::deserialize", default)]
//         pub input_tokens: i64,
//         #[serde(alias = "outputTokens", deserialize_with = "stringify::deserialize", default)]
//         pub output_tokens: i64,
//         #[serde(alias = "cacheWriteTokens", deserialize_with = "stringify::deserialize", default)]
//         pub cache_write_tokens: i64,
//         #[serde(alias = "cacheReadTokens", deserialize_with = "stringify::deserialize", default)]
//         pub cache_read_tokens: i64,
//         #[serde(alias = "totalCents", default)]
//         pub total_cents: f64,
//     }
// }

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct UsageEventDisplay {
    #[serde(deserialize_with = "stringify::deserialize", default)]
    pub timestamp: i64,
    pub model: String,
    pub kind: UsageEventKind,
    #[serde(alias = "customSubscriptionName", skip_serializing_if = "Option::is_none")]
    pub custom_subscription_name: Option<String>,
    #[serde(alias = "maxMode", default)]
    pub max_mode: bool,
    #[serde(alias = "requestsCosts", default)]
    pub requests_costs: f32,
    #[serde(alias = "usageBasedCosts", skip_serializing_if = "Option::is_none")]
    pub usage_based_costs: Option<String>,
    #[serde(alias = "isTokenBasedCall", skip_serializing_if = "Option::is_none")]
    pub is_token_based_call: Option<bool>,
    #[serde(alias = "tokenUsage", skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
    #[serde(alias = "owningUser", skip_serializing_if = "Option::is_none")]
    pub owning_user: Option<String>,
    #[serde(alias = "owningTeam", skip_serializing_if = "Option::is_none")]
    pub owning_team: Option<String>,
    #[serde(alias = "userEmail", skip_serializing_if = "Option::is_none")]
    pub user_email: Option<String>,
    #[serde(alias = "cursorTokenFee", skip_serializing_if = "Option::is_none")]
    pub cursor_token_fee: Option<f32>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Copy)]
pub struct TokenUsage {
    #[serde(alias = "inputTokens", default)]
    pub input_tokens: i32,
    #[serde(alias = "outputTokens", default)]
    pub output_tokens: i32,
    #[serde(alias = "cacheWriteTokens", default)]
    pub cache_write_tokens: i32,
    #[serde(alias = "cacheReadTokens", default)]
    pub cache_read_tokens: i32,
    #[serde(alias = "totalCents", default)]
    pub total_cents: f32,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetFilteredUsageEventsRequest {
    pub team_id: i32,
    #[serde(skip_serializing_if = "Option::is_none", with = "stringify")]
    pub start_date: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", with = "stringify")]
    pub end_date: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<&'static str>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    pub page: i32,
    // #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: i32,
}

#[derive(Debug, serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetFilteredUsageEventsResponse {
    // #[serde(default)]
    // pub total_usage_events_count: i32,
    #[serde(default)]
    pub usage_events_display: Vec<UsageEventDisplay>,
}

#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageEventKind {
    #[default]
    #[serde(alias = "USAGE_EVENT_KIND_UNSPECIFIED")]
    Unspecified = 0,
    #[serde(alias = "USAGE_EVENT_KIND_USAGE_BASED")]
    UsageBased = 1,
    #[serde(alias = "USAGE_EVENT_KIND_USER_API_KEY")]
    UserApiKey = 2,
    #[serde(alias = "USAGE_EVENT_KIND_INCLUDED_IN_PRO")]
    IncludedInPro = 3,
    #[serde(alias = "USAGE_EVENT_KIND_INCLUDED_IN_BUSINESS")]
    IncludedInBusiness = 4,
    #[serde(alias = "USAGE_EVENT_KIND_ERRORED_NOT_CHARGED")]
    ErroredNotCharged = 5,
    #[serde(alias = "USAGE_EVENT_KIND_ABORTED_NOT_CHARGED")]
    AbortedNotCharged = 6,
    #[serde(alias = "USAGE_EVENT_KIND_CUSTOM_SUBSCRIPTION")]
    CustomSubscription = 7,
    #[serde(alias = "USAGE_EVENT_KIND_INCLUDED_IN_PRO_PLUS")]
    IncludedInProPlus = 8,
    #[serde(alias = "USAGE_EVENT_KIND_INCLUDED_IN_ULTRA")]
    IncludedInUltra = 9,
}
