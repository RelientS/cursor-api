use alloc::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::app::model::{FetchMode, UsageCheck, VisionAbility};

#[derive(Serialize)]
pub struct ConfigData {
    pub vision_ability: VisionAbility,
    pub enable_slow_pool: bool,
    pub enable_long_context: bool,
    pub usage_check_models: UsageCheck,
    pub enable_dynamic_key: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub share_token: String,
    pub include_web_references: bool,
    pub fetch_raw_models: FetchMode,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct ConfigUpdateRequest {
    pub action: String, // "get", "update", "reset"
    pub vision_ability: Option<VisionAbility>,
    pub enable_slow_pool: Option<bool>,
    pub enable_long_context: Option<bool>,
    pub usage_check_models: Option<UsageCheck>,
    pub enable_dynamic_key: Option<bool>,
    pub share_token: Option<String>,
    pub include_web_references: Option<bool>,
    pub fetch_raw_models: Option<FetchMode>,
}

#[derive(Serialize)]
pub struct ConfigResponse {
    pub status: super::ApiStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ConfigData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Cow<'static, str>>,
}
