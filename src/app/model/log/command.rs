use crate::{
    app::model::{
        Chain, ChainUsage, DateTime, ErrorInfo, ExtToken, LogStatus, RequestLog, TokenKey, UserId,
    },
    common::model::userinfo::{MembershipType, StripeProfile, UsageProfile, UserProfile},
};
use tokio::sync::oneshot;

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;

pub struct GetLogsParams {
    pub token_key: Option<TokenKey>,
    pub log_status: Option<LogStatus>,
    pub membership_type: Option<MembershipType>,
    pub user_id: Option<UserId>,
    pub from_date: Option<DateTime>,
    pub to_date: Option<DateTime>,
    pub email: Option<String>,
    pub model: Option<String>,
    pub include_models: Option<Vec<String>>,
    pub exclude_models: Option<Vec<String>>,
    pub stream: Option<bool>,
    pub has_chain: Option<bool>,
    pub has_error: Option<bool>,
    pub error: Option<String>,
    pub min_total_time: Option<f64>,
    pub max_total_time: Option<f64>,
    pub min_tokens: Option<i32>,
    pub max_tokens: Option<i32>,
    pub reverse: bool,
    pub offset: usize,
    pub limit: usize,
}

// 定义发送给日志Actor的命令
pub enum LogCommand {
    // 获取日志
    GetLogs {
        params: GetLogsParams,
        tx: oneshot::Sender<(u64, Vec<RequestLog>)>,
    },
    // 添加一条日志
    AddLog {
        log: RequestLog,
        token: ExtToken,
    },
    // 获取下一个日志ID
    GetNextLogId {
        tx: oneshot::Sender<u64>,
    },
    GetToken {
        key: TokenKey,
        tx: oneshot::Sender<Option<ExtToken>>,
    },
    GetTokens {
        keys: Vec<(String, TokenKey)>,
        tx: oneshot::Sender<HashMap<String, Option<ExtToken>>>,
    },
    CloneToSave {
        tx: oneshot::Sender<super::LogManagerHelper>,
    },
    // 更新指定ID的日志
    UpdateLog {
        id: u64,
        ops: LogUpdate,
    },
}

pub enum LogUpdate {
    TokenProfile(Option<UserProfile>, Option<UsageProfile>, Option<StripeProfile>),
    Failure(ErrorInfo),
    Success,
    Timing(f64),
    Failure2(ErrorInfo, f64),
    Delays(Option<(String, Vec<(u32, f32)>)>, Option<String>),
    Usage(ChainUsage),
    TimingChain(f64, Chain),
}
