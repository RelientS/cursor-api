use crate::common::model::is_default;

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyMode {
    #[default]
    #[serde(alias = "PRIVACY_MODE_UNSPECIFIED")]
    Unspecified = 0,
    #[serde(alias = "PRIVACY_MODE_NO_STORAGE")]
    NoStorage = 1,
    #[serde(alias = "PRIVACY_MODE_NO_TRAINING")]
    NoTraining = 2,
    #[serde(alias = "PRIVACY_MODE_USAGE_DATA_TRAINING_ALLOWED")]
    UsageDataTrainingAllowed = 3,
    #[serde(alias = "PRIVACY_MODE_USAGE_CODEBASE_TRAINING_ALLOWED")]
    UsageCodebaseTrainingAllowed = 4,
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct PrivacyModeInfo {
    #[serde(alias = "privacyMode", default)]
    pub privacy_mode: PrivacyMode,
    #[serde(alias = "isEnforcedByTeam", default, skip_serializing_if = "is_default")]
    pub is_enforced_by_team: bool,
}
