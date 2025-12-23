mod payment_id;
mod subscription_status;
mod membership_type;
mod limit_type;
mod usage_event;
mod usage_info;
mod privacy_mode;

use chrono::{DateTime, Utc};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
pub use payment_id::PaymentId;
pub use subscription_status::SubscriptionStatus;
pub use membership_type::MembershipType;
pub use limit_type::LimitType;
pub use usage_event::{GetFilteredUsageEventsRequest, GetFilteredUsageEventsResponse, TokenUsage};
pub use usage_info::{IndividualUsage, TeamUsage};
pub use privacy_mode::PrivacyModeInfo;

// #[derive(Serialize)]
// #[serde(untagged)]
// pub enum GetUserInfo {
//     Usage(Box<(UsageProfile, UserProfile, StripeProfile)>),
//     Error { error: String },
// }

// #[derive(Deserialize, Serialize, Clone, Archive, RkyvDeserialize, RkyvSerialize)]
// pub struct TokenProfile {
//     pub usage: UsageProfile,
//     pub user: UserProfile,
//     pub stripe: StripeProfile,
// }

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct StripeProfile {
    #[serde(alias = "membershipType")]
    pub membership_type: MembershipType,
    #[serde(alias = "paymentId", default, skip_serializing_if = "Option::is_none")]
    pub payment_id: Option<PaymentId>,
    #[serde(alias = "daysRemainingOnTrial", skip_serializing_if = "Option::is_none")]
    pub days_remaining_on_trial: Option<i32>,
    #[serde(alias = "subscriptionStatus", skip_serializing_if = "Option::is_none")]
    pub subscription_status: Option<SubscriptionStatus>,
    #[serde(alias = "verifiedStudent", default)]
    pub verified_student: bool,
    #[serde(alias = "trialEligible", default)]
    pub trial_eligible: bool,
    #[serde(alias = "trialLengthDays", default)]
    pub trial_length_days: i32,
    #[serde(alias = "isOnStudentPlan", default)]
    pub is_on_student_plan: bool,
    #[serde(alias = "isOnBillableAuto", default)]
    pub is_on_billable_auto: bool,
    #[serde(alias = "customerBalance")]
    pub customer_balance: Option<f64>,
    #[serde(alias = "trialWasCancelled", default)]
    pub trial_was_cancelled: bool,
    #[serde(alias = "isTeamMember", default)]
    pub is_team_member: bool,
    #[serde(alias = "teamMembershipType")]
    pub team_membership_type: Option<MembershipType>,
    #[serde(alias = "individualMembershipType")]
    pub individual_membership_type: Option<MembershipType>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct UsageProfile {
    #[serde(alias = "billingCycleStart")]
    pub billing_cycle_start: DateTime<Utc>,
    #[serde(alias = "billingCycleEnd")]
    pub billing_cycle_end: DateTime<Utc>,
    #[serde(alias = "membershipType")]
    pub membership_type: MembershipType,
    #[serde(alias = "limitType")]
    pub limit_type: LimitType,
    #[serde(alias = "isUnlimited")]
    pub is_unlimited: bool,
    #[serde(alias = "individualUsage")]
    pub individual_usage: IndividualUsage,
    #[serde(alias = "teamUsage")]
    pub team_usage: TeamUsage,
}

// #[derive(Deserialize, Serialize, Clone, Archive, RkyvDeserialize, RkyvSerialize)]
// pub struct UserProfile {
//     pub email: String,
//     // pub email_verified: bool,
//     pub name: String,
//     // #[serde(alias = "sub")]
//     // pub id: UserId,
//     pub updated_at: DateTime<Utc>,
//     // Image link, rendered in /logs? and /tokens?
//     pub picture: Option<String>,
//     #[serde(skip_deserializing)]
//     pub is_on_new_pricing: bool,
// }

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct UserProfile {
    // #[serde(alias = "authId")]
    // pub auth_id: Subject,
    #[serde(alias = "userId")]
    pub user_id: i32,
    pub email: Option<String>,
    #[serde(alias = "firstName")]
    pub first_name: Option<String>,
    #[serde(alias = "lastName")]
    pub last_name: Option<String>,
    #[serde(alias = "workosId")]
    pub workos_id: Option<crate::app::model::UserId>,
    #[serde(alias = "teamId", skip_serializing_if = "Option::is_none")]
    pub team_id: Option<i32>,
    #[serde(alias = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(alias = "isEnterpriseUser", default)]
    pub is_enterprise_user: bool,
    #[serde(skip_deserializing)]
    pub is_on_new_pricing: bool,
    #[serde(skip_deserializing)]
    pub privacy_mode_info: PrivacyModeInfo,
}

impl UserProfile {
    #[inline]
    pub fn alias(&self) -> Option<&String> {
        if let Some(ref email) = self.email {
            Some(email)
        } else if let Some(ref first_name) = self.first_name {
            Some(first_name)
        } else if let Some(ref last_name) = self.last_name {
            Some(last_name)
        } else {
            None
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Copy, Archive, RkyvDeserialize, RkyvSerialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum SessionType {
    #[serde(alias = "SESSION_TYPE_UNSPECIFIED")]
    Unspecified,
    #[serde(alias = "SESSION_TYPE_WEB")]
    Web,
    #[serde(alias = "SESSION_TYPE_CLIENT")]
    Client,
    #[serde(alias = "SESSION_TYPE_BUGBOT")]
    Bugbot,
    #[serde(alias = "SESSION_TYPE_BACKGROUND_AGENT")]
    BackgroundAgent,
}

#[derive(Deserialize, Serialize, Clone, Copy, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct Session {
    #[serde(alias = "sessionId")]
    pub session_id: crate::app::model::Hash,
    pub r#type: SessionType,
    #[serde(alias = "createdAt")]
    pub created_at: DateTime<Utc>,
    #[serde(alias = "expiresAt")]
    pub expires_at: DateTime<Utc>,
}

// /// aiserver.v1.Team
// #[derive(::serde::Deserialize)]
// #[serde(rename_all(deserialize = "camelCase"))]
// pub struct Team {
//     pub name: String,
//     pub id: i32,
//     pub role: TeamRole,
//     pub seats: i32,
//     #[serde(default)]
//     pub has_billing: bool,
//     #[serde(default)]
//     pub request_quota_per_seat: i32,
//     #[serde(default)]
//     pub privacy_mode_forced: bool,
//     #[serde(default)]
//     pub allow_sso: bool,
//     #[serde(default)]
//     pub admin_only_usage_pricing: bool,
//     pub subscription_status: Option<SubscriptionStatus>,
//     #[serde(default)]
//     pub bedrock_iam_role: String,
//     #[serde(default)]
//     pub verified: bool,
//     #[serde(default)]
//     pub is_enterprise: bool,
// }
// #[derive(::serde::Deserialize)]
// pub struct GetTeamsResponse {
//     #[serde(default)]
//     pub teams: Vec<Team>,
// }
// #[derive(::serde::Serialize, Clone, Copy, Archive, RkyvDeserialize, RkyvSerialize)]
// #[serde(rename_all(serialize = "snake_case"))]
// #[repr(u8)]
// pub enum TeamRole {
//     Unspecified = 0,
//     Owner = 1,
//     Member = 2,
//     FreeOwner = 3,
// }
// impl TeamRole {
//     const STR_UNSPECIFIED: &'static str = "TEAM_ROLE_UNSPECIFIED";
//     const STR_OWNER: &'static str = "TEAM_ROLE_OWNER";
//     const STR_MEMBER: &'static str = "TEAM_ROLE_MEMBER";
//     const STR_FREE_OWNER: &'static str = "TEAM_ROLE_FREE_OWNER";
//     // pub fn as_str_name(&self) -> &'static str {
//     //     match self {
//     //         Self::Unspecified => Self::STR_UNSPECIFIED,
//     //         Self::Owner => Self::STR_OWNER,
//     //         Self::Member => Self::STR_MEMBER,
//     //         Self::FreeOwner => Self::STR_FREE_OWNER,
//     //     }
//     // }
//     pub fn from_str_name(value: &str) -> ::core::option::Option<Self> {
//         match value {
//             Self::STR_UNSPECIFIED => Some(Self::Unspecified),
//             Self::STR_OWNER => Some(Self::Owner),
//             Self::STR_MEMBER => Some(Self::Member),
//             Self::STR_FREE_OWNER => Some(Self::FreeOwner),
//             _ => None,
//         }
//     }
// }
// impl<'de> ::serde::Deserialize<'de> for TeamRole {
//     fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
//     where
//         D: ::serde::Deserializer<'de>,
//     {
//         struct TeamRoleVisitor;

//         impl<'de> ::serde::de::Visitor<'de> for TeamRoleVisitor {
//             type Value = TeamRole;

//             fn expecting(&self, formatter: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
//                 formatter.write_str("a valid TeamRole string")
//             }

//             fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
//             where
//                 E: ::serde::de::Error,
//             {
//                 TeamRole::from_str_name(value)
//                     .ok_or_else(|| E::custom(format_args!("unknown team role value: {value}")))
//             }
//         }

//         deserializer.deserialize_str(TeamRoleVisitor)
//     }
// }
