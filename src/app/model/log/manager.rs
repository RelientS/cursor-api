use super::{
    command::{LogCommand, LogUpdate},
    limit::LogsLimit,
    storage::{AssociatedStorage, AssociatedToken, MainStorage},
};
use crate::{
    app::{
        constant::ERR_LOG_TOKEN_NOT_FOUND,
        lazy::LOGS_FILE_PATH,
        model::{ExtToken, LogStatus, RequestLog, TokenKey},
    },
    common::utils::{format_time_ms, parse_from_env},
};
use manually_init::ManuallyInit;
use tokio::sync::{
    mpsc::{Sender, channel},
    oneshot,
};

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;

macro_rules! unwrap {
    ($result:expr) => {
        match $result {
            ::core::result::Result::Ok(t) => t,
            ::core::result::Result::Err(_e) => ::core::unreachable!(),
        }
    };
}

pub struct LogManager {
    pub(super) logs: MainStorage,
    pub(super) tokens: AssociatedStorage,
}

impl LogManager {
    fn new() -> Self {
        Self {
            logs: MainStorage::new(),
            tokens: AssociatedStorage::with_hasher(ahash::RandomState::new()),
        }
    }

    pub async fn load() -> Result<Self, Box<dyn core::error::Error + Send + Sync + 'static>> {
        REQUEST_LOGS_LIMIT
            .init(LogsLimit::from_usize(parse_from_env("REQUEST_LOGS_LIMIT", 100usize)));
        if !REQUEST_LOGS_LIMIT.should_log() {
            return Ok(Self::new());
        }

        let file = match tokio::fs::OpenOptions::new().read(true).open(&*LOGS_FILE_PATH).await {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::new());
            }
            Err(e) => return Err(e.into()),
        };

        if file.metadata().await?.len() > usize::MAX as u64 {
            return Err("日志文件过大".into());
        }

        let mmap = unsafe { memmap2::MmapOptions::new().map(&file)? };
        let manager = unsafe {
            ::rkyv::from_bytes_unchecked::<super::LogManagerHelper, rkyv::rancor::Error>(&mmap)
        }
        .map_err(|_| "加载日志失败")?;

        Ok(manager.into())
    }

    pub async fn save() -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
        if !REQUEST_LOGS_LIMIT.should_log() {
            return Ok(());
        }
        let helper = clone_to_save().await;

        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&helper)?;

        let file = tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&*LOGS_FILE_PATH)
            .await?;

        if bytes.len() > usize::MAX >> 1 {
            return Err("日志数据过大".into());
        }

        file.set_len(bytes.len() as u64).await?;
        let mut mmap = unsafe { memmap2::MmapMut::map_mut(&file)? };
        mmap.copy_from_slice(&bytes);
        mmap.flush()?;

        Ok(())
    }

    /// 获取错误日志数量
    #[inline]
    pub fn error_count(&self) -> u64 {
        self.logs.iter().filter(|log| log.status as u8 != 1).count() as u64
    }

    /// 获取日志总数
    #[inline]
    pub fn total_count(&self) -> u64 { self.logs.len() as u64 }
}

static LOG_COMMAND_SENDER: ManuallyInit<Sender<LogCommand>> = ManuallyInit::new();
static REQUEST_LOGS_LIMIT: ManuallyInit<LogsLimit> = ManuallyInit::new();

pub fn create_task(log_manager: LogManager) {
    let (tx, rx) = channel({
        const MAX: usize = usize::MAX >> 6;
        let log_peak_rps = parse_from_env("LOG_PEAK_RPS", 25usize);
        let log_buffer_seconds = parse_from_env("LOG_BUFFER_SECONDS", 2usize);
        let result = log_peak_rps * log_buffer_seconds;
        assert!(result <= MAX, "a buffer may not have more than MAX ({MAX})");
        result * 8
    });
    tokio::spawn(async move {
        let mut mgr = log_manager;
        let mut rx = rx;
        while let Some(cmd) = rx.recv().await {
            if handle_command(&mut mgr, cmd) {
                break;
            }
        }
    });
    LOG_COMMAND_SENDER.init(tx)
}

fn handle_command(mgr: &mut LogManager, cmd: LogCommand) -> bool {
    match cmd {
        LogCommand::GetLogs { params, tx } => {
            let params = &params;
            let filtered_logs: Vec<_> = mgr
                .logs
                .iter()
                .filter(|log| {
                    if let Some(token_key) = params.token_key
                        && log.token_info.key != token_key
                    {
                        return false;
                    }

                    if let Some(from) = params.from_date
                        && log.timestamp < from
                    {
                        return false;
                    }

                    if let Some(to) = params.to_date
                        && log.timestamp > to
                    {
                        return false;
                    }

                    if let Some(user_id) = params.user_id
                        && mgr
                            .tokens
                            .get(&log.token_info.key)
                            .expect(ERR_LOG_TOKEN_NOT_FOUND)
                            .token
                            .primary_token
                            .raw()
                            .subject
                            .id
                            != user_id
                    {
                        return false;
                    }

                    if let Some(ref email) = params.email
                        && !log
                            .token_info
                            .user
                            .as_ref()
                            .and_then(|user| user.email.as_ref())
                            .map(|s| s.contains(email))
                            .unwrap_or(false)
                    {
                        return false;
                    }

                    if let Some(membership) = params.membership_type
                        && log
                            .token_info
                            .stripe
                            .as_ref()
                            .map(|p| p.membership_type != membership)
                            .unwrap_or(true)
                    {
                        return false;
                    }

                    if let Some(status) = params.log_status
                        && log.status != status
                    {
                        return false;
                    }

                    if let Some(ref model) = params.model
                        && !log.model.contains(model)
                    {
                        return false;
                    }

                    if let Some(ref includes) = params.include_models
                        && includes.iter().all(|m| log.model != *m)
                    {
                        return false;
                    }

                    if let Some(ref excludes) = params.exclude_models
                        && excludes.iter().any(|m| log.model == *m)
                    {
                        return false;
                    }

                    if let Some(stream) = params.stream
                        && log.stream != stream
                    {
                        return false;
                    }

                    if let Some(has_chain) = params.has_chain
                        && log.chain.has_some() != has_chain
                    {
                        return false;
                    }

                    if let Some(has_error) = params.has_error
                        && log.error.is_some() != has_error
                    {
                        return false;
                    }

                    if let Some(ref error_str) = params.error
                        && !log.error.contains(error_str)
                    {
                        return false;
                    }

                    if let Some(min_time) = params.min_total_time
                        && log.timing.total < min_time
                    {
                        return false;
                    }

                    if let Some(max_time) = params.max_total_time
                        && log.timing.total > max_time
                    {
                        return false;
                    }

                    if let Some(min) = params.min_tokens
                        && log.chain.usage.as_ref().map(|u| u.total() < min).unwrap_or(true)
                    {
                        return false;
                    }

                    if let Some(max) = params.max_tokens
                        && log.chain.usage.as_ref().map(|u| u.total() > max).unwrap_or(true)
                    {
                        return false;
                    }

                    true
                })
                .collect();

            unwrap!(tx.send((
                filtered_logs.len() as u64,
                if params.reverse {
                    filtered_logs
                        .into_iter()
                        .rev()
                        .skip(params.offset)
                        .take(params.limit)
                        .cloned()
                        .collect()
                } else {
                    filtered_logs
                        .into_iter()
                        .skip(params.offset)
                        .take(params.limit)
                        .cloned()
                        .collect()
                },
            )))
        }
        LogCommand::AddLog { log, token } => {
            use hashbrown::hash_map::Entry;
            let key = log.token_key();
            while mgr.logs.len() >= REQUEST_LOGS_LIMIT.get_limit() {
                if let Some(log) = mgr.logs.pop_front() {
                    let key = log.token_key();
                    match mgr.tokens.entry(key) {
                        Entry::Occupied(mut e) => {
                            let a = e.get_mut();
                            a.ref_count -= 1;
                            if a.ref_count == 0 {
                                e.remove();
                            }
                        }
                        Entry::Vacant(e) => {
                            crate::debug!("[LOG] 数据不一致: {:?}", e.into_key())
                        }
                    };
                }
            }
            mgr.logs.push_back(*log);
            match mgr.tokens.entry(key) {
                Entry::Occupied(e) => {
                    let a = e.into_mut();
                    a.token = token;
                    a.ref_count += 1;
                }
                Entry::Vacant(e) => {
                    e.insert(AssociatedToken { token, ref_count: 1 });
                }
            }
        }
        LogCommand::GetNextLogId { tx } => {
            unwrap!(tx.send(mgr.logs.back().map_or(1, |log| log.id + 1)))
        }
        LogCommand::GetToken { key, tx } => {
            unwrap!(tx.send(mgr.tokens.get(&key).map(|a| a.token.clone())))
        }
        LogCommand::GetTokens { keys, tx } => {
            let mut map =
                HashMap::with_capacity_and_hasher(keys.len(), mgr.tokens.hasher().clone());
            for (s, key) in keys {
                let value = mgr.tokens.get(&key).map(|a| a.token.clone());
                map.insert(s, value);
            }
            unwrap!(tx.send(map))
        }
        LogCommand::CloneToSave { tx } => {
            unwrap!(tx.send((mgr as &LogManager).into()))
        }
        LogCommand::UpdateLog { id, ops } => {
            if let Some(log) = mgr.logs.iter_mut().rev().find(|log| log.id == id) {
                match ops {
                    LogUpdate::TokenProfile(user, usage, stripe) => {
                        log.token_info.user = user;
                        log.token_info.usage = usage;
                        log.token_info.stripe = stripe;
                    }
                    LogUpdate::Failure(error) => {
                        log.status = LogStatus::Failure;
                        log.error = error;
                    }
                    LogUpdate::Success => log.status = LogStatus::Success,
                    LogUpdate::Timing(t) => log.timing.total = format_time_ms(t),
                    LogUpdate::Failure2(error, t) => {
                        log.status = LogStatus::Failure;
                        log.error = error;
                        log.timing.total = format_time_ms(t);
                    }
                    LogUpdate::Delays(delays, think) => {
                        log.chain.delays = delays;
                        log.chain.think = think;
                    }
                    LogUpdate::Usage(usage) => log.chain.usage = Some(usage),
                    LogUpdate::TimingChain(t, chain) => {
                        log.timing.total = format_time_ms(t);
                        log.chain = chain;
                    }
                }
            }
        }
    }
    false
}

trait Expect: Sized {
    type T;
    fn expect(self) -> <Self as Expect>::T;
}
impl<V> Expect for Result<(), tokio::sync::mpsc::error::SendError<V>> {
    type T = ();
    fn expect(self) {
        core::result::Result::expect(self, "Log actor is dead - this should never happen")
    }
}
impl<T> Expect for Result<T, tokio::sync::oneshot::error::RecvError> {
    type T = T;
    fn expect(self) -> T {
        core::result::Result::expect(self, "Log actor crashed - this should never happen")
    }
}

fn expect<R: Expect>(r: R) -> R::T { r.expect() }

pub async fn get_logs(params: super::GetLogsParams) -> (u64, Vec<RequestLog>) {
    let (tx, rx) = oneshot::channel();
    expect(LOG_COMMAND_SENDER.send(LogCommand::GetLogs { params, tx }).await);
    expect(rx.await)
}

pub async fn add_log(log: RequestLog, token: ExtToken) {
    expect(LOG_COMMAND_SENDER.send(LogCommand::AddLog { log: Box::new(log), token }).await)
}

pub async fn get_next_log_id() -> u64 {
    let (tx, rx) = oneshot::channel();
    expect(LOG_COMMAND_SENDER.send(LogCommand::GetNextLogId { tx }).await);
    expect(rx.await)
}

pub async fn get_token(key: TokenKey) -> Option<ExtToken> {
    let (tx, rx) = oneshot::channel();
    expect(LOG_COMMAND_SENDER.send(LogCommand::GetToken { key, tx }).await);
    expect(rx.await)
}

pub async fn get_tokens(keys: Vec<(String, TokenKey)>) -> HashMap<String, Option<ExtToken>> {
    let (tx, rx) = oneshot::channel();
    expect(LOG_COMMAND_SENDER.send(LogCommand::GetTokens { keys, tx }).await);
    expect(rx.await)
}

async fn clone_to_save() -> super::LogManagerHelper {
    let (tx, rx) = oneshot::channel();
    expect(LOG_COMMAND_SENDER.send(LogCommand::CloneToSave { tx }).await);
    expect(rx.await)
}

pub async fn update_log(id: u64, ops: LogUpdate) {
    expect(LOG_COMMAND_SENDER.send(LogCommand::UpdateLog { id, ops }).await)
}

pub fn is_enabled() -> bool { REQUEST_LOGS_LIMIT.should_log() }
