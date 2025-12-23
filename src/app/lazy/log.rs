use crate::common::utils::parse_from_env;
use alloc::borrow::Cow;
use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use manually_init::ManuallyInit;
use tokio::{
    fs::File,
    io::AsyncWriteExt as _,
    sync::{
        Mutex, OnceCell,
        mpsc::{self, UnboundedSender},
        watch,
    },
    task::JoinHandle,
};

// --- 全局配置 ---

/// 控制调试模式的开关，从环境变量 "DEBUG" 读取，默认为 true
pub static DEBUG: ManuallyInit<bool> = ManuallyInit::new();

/// 调试日志文件的路径，从环境变量 "DEBUG_LOG_FILE" 读取，默认为 "debug.log"
static DEBUG_LOG_FILE: ManuallyInit<Cow<'static, str>> = ManuallyInit::new();

/// 全局日志文件句柄
static LOG_FILE: ManuallyInit<Mutex<File>> = ManuallyInit::new();

/// 初始化日志系统配置
///
/// 必须在程序启动时调用一次（使用日志前）
#[forbid(unused)]
pub fn init() {
    DEBUG.init(parse_from_env("DEBUG", true));

    // 如果不启用调试，不初始化日志文件
    if !*DEBUG {
        return;
    }

    DEBUG_LOG_FILE.init(parse_from_env("DEBUG_LOG_FILE", "debug.log"));

    // 同步打开日志文件
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&**DEBUG_LOG_FILE)
        .expect("致命错误：日志系统初始化失败 - 无法打开日志文件");

    // 转换为 tokio 文件句柄
    LOG_FILE.init(Mutex::new(File::from_std(file)));
}

// --- 日志消息结构 ---

/// 带序列号的日志消息，确保有序处理
pub struct LogMessage {
    /// 全局递增的序列号，保证日志顺序
    pub seq: u64,
    /// 已格式化的日志内容（包含时间戳）
    pub content: String,
}

/// 全局日志序列号生成器（内部使用）
static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 获取下一个日志序列号
#[inline]
pub fn next_log_seq() -> u64 { LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed) }

// --- 核心组件 ---

/// 全局单例的日志系统状态，使用 OnceCell 确保只初始化一次
static LOGGER_STATE: OnceCell<LoggerState> = OnceCell::const_new();

/// 日志系统的状态结构，包含发送通道、关闭信号和后台任务句柄
pub struct LoggerState {
    /// 用于发送日志消息的无界通道发送端
    pub sender: UnboundedSender<LogMessage>,
    /// 用于发送关闭信号的 watch 通道发送端
    shutdown_tx: watch::Sender<bool>,
    /// 后台写入任务的句柄
    writer_handle: Mutex<Option<JoinHandle<()>>>,
}

/// 确保日志系统已初始化并返回其状态
///
/// 如果日志系统尚未初始化，会创建所需的通道和后台任务
///
/// 返回日志系统状态的引用
pub fn ensure_logger_initialized() -> impl Future<Output = &'static LoggerState> {
    LOGGER_STATE.get_or_init(|| async {
        // 创建用于传递日志消息的无界通道
        let (sender, mut receiver) = mpsc::unbounded_channel::<LogMessage>();
        // 创建用于发送关闭信号的 watch 通道
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        // 启动后台写入任务
        let writer_handle = tokio::spawn(async move {
            // 配置常量
            const BUFFER_CAPACITY: usize = 65536; // 64KB
            const MAX_PENDING_MESSAGES: usize = 1000;
            const OUT_OF_ORDER_THRESHOLD: u64 = 100;

            let mut buffer = Vec::<u8>::with_capacity(BUFFER_CAPACITY);
            // 定时刷新间隔
            let flush_interval = Duration::from_secs(3);
            let mut interval = tokio::time::interval(flush_interval);
            interval.tick().await; // 消耗初始 tick

            // 用于缓存乱序到达的消息
            let mut pending_messages = alloc::collections::BTreeMap::new();
            let mut next_seq = 0u64;

            // 主循环：处理日志消息、定时刷新和关闭信号
            loop {
                tokio::select! {
                    biased; // 优先处理上面的分支

                    // 接收新的日志消息
                    Some(message) = receiver.recv() => {
                        // 将消息加入待处理队列
                        pending_messages.insert(message.seq, message.content);

                        // 检查待处理队列是否过大
                        if pending_messages.len() > MAX_PENDING_MESSAGES {
                            let oldest_seq = *__unwrap!(pending_messages.keys().next());
                            eprintln!(
                                "日志系统警告：待处理消息过多（>{MAX_PENDING_MESSAGES}），强制跳过序号 {next_seq}-{}",
                                oldest_seq - 1
                            );
                            next_seq = oldest_seq;
                        }

                        // 处理所有连续的消息
                        while let Some(content) = pending_messages.remove(&next_seq) {
                            buffer.extend_from_slice(content.as_bytes());
                            buffer.push(b'\n');
                            next_seq += 1;

                            // 缓冲区达到容量时刷新
                            if buffer.len() >= BUFFER_CAPACITY {
                                flush_byte_buffer(&mut buffer).await;
                                interval.reset();
                            }
                        }
                    }

                    // 定时刷新触发
                    _ = interval.tick() => {
                        // 定时刷新时，如果有积压的消息且等待时间过长，强制写入
                        if !pending_messages.is_empty() {
                            let oldest_seq = *__unwrap!(pending_messages.keys().next());
                            // 如果最旧的消息序号与期望序号相差太大，可能有消息丢失
                            if oldest_seq > next_seq + OUT_OF_ORDER_THRESHOLD {
                                eprintln!(
                                    "日志系统警告：检测到可能的消息丢失，跳过序号 {next_seq} 到 {}",
                                    oldest_seq - 1
                                );
                                next_seq = oldest_seq;
                            }
                        }
                        flush_byte_buffer(&mut buffer).await;
                    }

                    // 监听关闭信号
                    result = shutdown_rx.changed() => {
                        if result.is_err() || *shutdown_rx.borrow() {
                            // 接收剩余所有消息
                            while let Ok(message) = receiver.try_recv() {
                                pending_messages.insert(message.seq, message.content);
                            }

                            // 处理所有待处理的消息并记录缺失范围
                            let mut missing_ranges = Vec::new();
                            for (seq, content) in pending_messages {
                                if seq != next_seq {
                                    missing_ranges.push((next_seq, seq - 1));
                                }
                                buffer.extend_from_slice(content.as_bytes());
                                buffer.push(b'\n');
                                next_seq = seq + 1;
                            }

                            // 报告缺失的日志
                            if !missing_ranges.is_empty() {
                                eprintln!("日志系统警告：关闭时发现缺失的日志序号：");
                                for (start, end) in missing_ranges {
                                    if start == end {
                                        eprintln!("  序号 {start}");
                                    } else {
                                        eprintln!("  序号 {start}-{end}");
                                    }
                                }
                            }

                            // 最终刷新
                            flush_byte_buffer(&mut buffer).await;
                            break;
                        }
                    }

                    // 所有其他情况（如通道关闭）
                    else => {
                        // 处理剩余的待处理消息
                        for (_, content) in pending_messages {
                            buffer.extend_from_slice(content.as_bytes());
                            buffer.push(b'\n');
                        }
                        flush_byte_buffer(&mut buffer).await;
                        break;
                    }
                }
            }
        });

        LoggerState {
            sender,
            shutdown_tx,
            writer_handle: Mutex::new(Some(writer_handle)),
        }
    })
}

/// 将缓冲区内容刷新到日志文件
///
/// # 参数
/// * `buffer` - 要写入的字节缓冲区，函数调用后会清空此缓冲区
async fn flush_byte_buffer(buffer: &mut Vec<u8>) {
    if buffer.is_empty() {
        return;
    }

    // 获取日志文件的互斥锁
    let mut file_guard = LOG_FILE.lock().await;

    // 写入数据
    if let Err(err) = file_guard.write_all(buffer).await {
        eprintln!("日志系统错误：写入日志数据失败。错误：{err}");
        buffer.clear();
        return;
    }
    buffer.clear();

    // 确保数据刷新到磁盘
    if let Err(err) = file_guard.flush().await {
        eprintln!("日志系统错误：刷新日志文件缓冲区到磁盘失败。错误：{err}");
    }
}

// --- 公开接口 ---

/// 提交调试日志到异步处理队列
///
/// # 参数
/// * `seq` - 日志序列号
/// * `content` - 已格式化的日志内容
#[inline]
pub fn submit_debug_log(seq: u64, content: String) {
    tokio::spawn(async move {
        let state = ensure_logger_initialized().await;
        let log_msg = LogMessage { seq, content };
        if let Err(e) = state.sender.send(log_msg) {
            eprintln!("日志系统错误：发送日志消息至后台任务失败。错误：{e}");
        }
    });
}

/// 记录调试日志的宏
///
/// 仅当 DEBUG 开启时记录日志，异步发送到日志处理任务
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if *$crate::app::lazy::log::DEBUG {
            // 立即获取序列号和时间戳，确保顺序性
            let seq = $crate::app::lazy::log::next_log_seq();
            let msg = format!(
                "{} | {}",
                $crate::app::model::DateTime::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                format_args!($($arg)*)
            );
            $crate::app::lazy::log::submit_debug_log(seq, msg);
        }
    };
}

/// 程序结束前调用，确保所有缓冲日志写入文件
///
/// 发送关闭信号，等待后台写入任务完成
pub async fn flush_all_debug_logs() {
    if let Some(state) = LOGGER_STATE.get() {
        if *DEBUG {
            __println!("日志系统：开始关闭流程...");
        }

        // 发送关闭信号
        if let Err(err) = state.shutdown_tx.send(true)
            && *DEBUG
        {
            println!("日志系统调试：发送关闭信号失败（可能写入任务已提前结束）：{err}");
        }

        // 提取后台任务句柄
        let handle = {
            let mut guard = state.writer_handle.lock().await;
            guard.take()
        };

        // 等待后台任务完成，设置5秒超时
        if let Some(handle) = handle {
            match tokio::time::timeout(Duration::from_secs(5), handle).await {
                Ok(Ok(_)) => {
                    if *DEBUG {
                        __println!("日志系统：关闭完成");
                    }
                }
                Ok(Err(join_err)) => {
                    eprintln!(
                        "日志系统错误：后台写入任务异常终止。部分日志可能丢失。错误详情：{join_err}"
                    );
                }
                Err(_) => {
                    __eprintln!(
                        "日志系统错误：等待后台写入任务超时（5秒）。部分日志可能未能写入。"
                    );
                }
            }
        } else if *DEBUG {
            __println!("日志系统调试：未找到活动写入任务句柄，可能已关闭。");
        }
    } else if *DEBUG {
        __println!("日志系统调试：日志系统未初始化，无需关闭。");
    }
}
