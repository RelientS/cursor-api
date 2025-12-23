mod command;
mod limit;
pub mod manager;
mod storage;

use crate::{app::model::ExtTokenHelper, core::constant::get_static_id};
pub use command::{GetLogsParams, LogUpdate};
use interned::Str;
pub use manager::{LogManager, create_task};

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;

#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
enum ErrorInfoHelper {
    Empty,
    Simple(String),
    Detailed { error: String, details: String },
}
impl From<ErrorInfoHelper> for super::ErrorInfo {
    #[inline]
    fn from(helper: ErrorInfoHelper) -> Self {
        match helper {
            ErrorInfoHelper::Empty => Self::Empty,
            ErrorInfoHelper::Simple(e) => Self::new(Str::new(e), None),
            ErrorInfoHelper::Detailed { error, details } => {
                Self::new(Str::new(error), Some(Str::new(details)))
            }
        }
    }
}
impl From<&super::ErrorInfo> for ErrorInfoHelper {
    #[inline]
    fn from(ori: &super::ErrorInfo) -> Self {
        match ori {
            super::ErrorInfo::Empty => Self::Empty,
            super::ErrorInfo::Simple(e) => Self::Simple(e.to_string()),
            super::ErrorInfo::Detailed { error, details } => {
                Self::Detailed { error: error.to_string(), details: details.to_string() }
            }
        }
    }
}
#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
pub(super) struct RequestLogHelper {
    id: u64,
    timestamp: chrono::NaiveDateTime,
    model: String,
    token_info: super::LogTokenInfo,
    chain: super::Chain,
    timing: super::TimingInfo,
    stream: bool,
    status: super::LogStatus,
    error: ErrorInfoHelper,
}
impl From<RequestLogHelper> for super::RequestLog {
    #[inline]
    fn from(log: RequestLogHelper) -> Self {
        Self {
            id: log.id,
            timestamp: log.timestamp.into(),
            model: get_static_id(log.model),
            token_info: log.token_info,
            chain: log.chain,
            timing: log.timing,
            stream: log.stream,
            status: log.status,
            error: log.error.into(),
        }
    }
}
impl From<&super::RequestLog> for RequestLogHelper {
    #[inline]
    fn from(log: &super::RequestLog) -> Self {
        Self {
            id: log.id,
            timestamp: log.timestamp.into(),
            model: log.model.to_string(),
            token_info: log.token_info.clone(),
            chain: log.chain.clone(),
            timing: log.timing,
            stream: log.stream,
            status: log.status,
            error: (&log.error).into(),
        }
    }
}
// #[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
// pub struct PromptMessageHelper {
//     role: Role,
//     content: String,
// }
// impl From<PromptMessageHelper> for super::PromptMessage {
//     #[inline]
//     fn from(helper: PromptMessageHelper) -> Self {
//         super::PromptMessage {
//             role: helper.role,
//             content: super::PromptContent(crate::leak::intern_arc(helper.content)),
//         }
//     }
// }
// impl From<super::PromptMessage> for PromptMessageHelper {
//     #[inline]
//     fn from(ori: super::PromptMessage) -> Self {
//         Self {
//             role: ori.role,
//             content: ori.content.into_owned(),
//         }
//     }
// }
// #[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
// pub enum PromptHelper {
//     None,
//     Origin(String),
//     Parsed(Vec<PromptMessageHelper>),
// }
// impl From<PromptHelper> for super::Prompt {
//     #[inline]
//     fn from(helper: PromptHelper) -> Self {
//         match helper {
//             PromptHelper::None => Self::None,
//             PromptHelper::Origin(s) => Self::Origin(s),
//             PromptHelper::Parsed(v) =>
//                 Self::Parsed(v.into_iter().map(Into::into).collect::<Vec<_>>()),
//         }
//     }
// }
// impl From<super::Prompt> for PromptHelper {
//     #[inline]
//     fn from(ori: super::Prompt) -> Self {
//         match ori {
//             super::Prompt::None => Self::None,
//             super::Prompt::Origin(s) => Self::Origin(s),
//             super::Prompt::Parsed(v) =>
//                 Self::Parsed(v.into_iter().map(Into::into).collect::<Vec<_>>()),
//         }
//     }
// }
// #[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
// pub struct ChainHelper {
//     // pub prompt: PromptHelper,
//     pub delays: Option<(String, Vec<(u32, f32)>)>,
//     pub usage: Option<super::ChainUsage>,
//     pub think: Option<String>,
// }
// impl From<ChainHelper> for super::Chain {
//     #[inline]
//     fn from(helper: ChainHelper) -> Self {
//         Self {
//             // prompt: helper.prompt.into(),
//             delays: helper.delays,
//             usage: helper.usage,
//             think: helper.think,
//         }
//     }
// }
// impl From<super::Chain> for ChainHelper {
//     #[inline]
//     fn from(ori: super::Chain) -> Self {
//         Self {
//             // prompt: ori.prompt.into(),
//             delays: ori.delays,
//             usage: ori.usage,
//             think: ori.think,
//         }
//     }
// }
#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
pub(super) struct AssociatedTokenHelper {
    token: ExtTokenHelper,
    ref_count: usize,
}
impl From<AssociatedTokenHelper> for storage::AssociatedToken {
    #[inline]
    fn from(a: AssociatedTokenHelper) -> storage::AssociatedToken {
        storage::AssociatedToken { token: a.token.extract(), ref_count: a.ref_count }
    }
}
impl From<&storage::AssociatedToken> for AssociatedTokenHelper {
    #[inline]
    fn from(a: &storage::AssociatedToken) -> Self {
        Self { token: ExtTokenHelper::new(&a.token), ref_count: a.ref_count }
    }
}
#[derive(rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
pub(super) struct LogManagerHelper {
    logs: Vec<RequestLogHelper>,
    tokens: HashMap<super::TokenKey, AssociatedTokenHelper>,
}
impl From<LogManagerHelper> for manager::LogManager {
    #[inline]
    fn from(helper: LogManagerHelper) -> manager::LogManager {
        manager::LogManager {
            logs: helper.logs.into_iter().map(Into::into).collect(),
            tokens: helper.tokens.into_iter().map(|(k, v)| (k, v.into())).collect(),
        }
    }
}
impl From<&manager::LogManager> for LogManagerHelper {
    #[inline]
    fn from(mgr: &manager::LogManager) -> Self {
        Self {
            logs: mgr.logs.iter().map(Into::into).collect(),
            tokens: mgr.tokens.iter().map(|(k, v)| (*k, v.into())).collect(),
        }
    }
}
