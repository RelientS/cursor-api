pub mod log;
mod path;

use super::{
    constant::{
        CURSOR_API2_HOST, CURSOR_API4_HOST, CURSOR_GCPP_ASIA_HOST, CURSOR_GCPP_EU_HOST,
        CURSOR_GCPP_US_HOST, CURSOR_HOST, EMPTY_STRING, HTTPS_PREFIX,
    },
    model::{DateTime, GcppHost},
};
use crate::common::utils::parse_from_env;
use alloc::borrow::Cow;
use manually_init::ManuallyInit;
pub use path::{DATA_DIR, LOGS_FILE_PATH, PROXIES_FILE_PATH, TOKENS_FILE_PATH, init as init_paths};
use std::sync::LazyLock;

macro_rules! def_pub_static {
    ($name:ident,env: $env_key:expr,default: $default:expr) => {
        pub static $name: LazyLock<Cow<'static, str>> =
            LazyLock::new(|| parse_from_env($env_key, $default));
    };
}

pub static AUTH_TOKEN: ManuallyInit<Cow<'static, str>> = ManuallyInit::new();

pub static START_TIME: ManuallyInit<DateTime> = ManuallyInit::new();

#[inline]
pub fn init_start_time() { START_TIME.init(DateTime::now()); }

pub static GENERAL_TIMEZONE: LazyLock<chrono_tz::Tz> = LazyLock::new(|| {
    use std::str::FromStr as _;
    let tz = parse_from_env("GENERAL_TIMEZONE", EMPTY_STRING);
    if tz.is_empty() {
        __eprintln!(
            "未配置时区，请在环境变量GENERAL_TIMEZONE中设置，格式如'Asia/Shanghai'\n将使用默认时区: Asia/Shanghai"
        );
        return chrono_tz::Tz::Asia__Shanghai;
    }
    match chrono_tz::Tz::from_str(&tz) {
        Ok(tz) => tz,
        Err(e) => {
            eprintln!("无法解析时区 '{tz}': {e}\n将使用默认时区: Asia/Shanghai");
            chrono_tz::Tz::Asia__Shanghai
        }
    }
});

pub static GENERAL_GCPP_HOST: LazyLock<GcppHost> = LazyLock::new(|| {
    let gcpp_host = parse_from_env("GENERAL_GCPP_HOST", EMPTY_STRING);
    let gcpp_host = gcpp_host.trim();
    if gcpp_host.is_empty() {
        __eprintln!(
            "未配置默认代码补全区域，请在环境变量GENERAL_GCPP_HOST中设置，格式如'Asia'\n将使用默认区域: Asia"
        );
        return GcppHost::Asia;
    }
    match GcppHost::from_str(gcpp_host) {
        Some(gcpp_host) => gcpp_host,
        None => {
            eprintln!("无法解析区域 '{gcpp_host}'\n将使用默认区域: Asia");
            GcppHost::Asia
        }
    }
});

def_pub_static!(PRI_REVERSE_PROXY_HOST, env: "PRI_REVERSE_PROXY_HOST", default: EMPTY_STRING);

def_pub_static!(PUB_REVERSE_PROXY_HOST, env: "PUB_REVERSE_PROXY_HOST", default: EMPTY_STRING);

const DEFAULT_KEY_PREFIX: &str = "sk-";

def_pub_static!(KEY_PREFIX, env: "KEY_PREFIX", default: DEFAULT_KEY_PREFIX);

// pub static TOKEN_DELIMITER: LazyLock<char> = LazyLock::new(|| {
//     let delimiter = parse_ascii_char_from_env("TOKEN_DELIMITER", COMMA);
//     if delimiter.is_ascii_alphabetic()
//         || delimiter.is_ascii_digit()
//         || delimiter == '/'
//         || delimiter == '-'
//         || delimiter == '_'
//     {
//         COMMA
//     } else {
//         delimiter
//     }
// });

// pub static USE_COMMA_DELIMITER: LazyLock<bool> = LazyLock::new(|| {
//     let enable = parse_bool_from_env("USE_COMMA_DELIMITER", true);
//     if enable && *TOKEN_DELIMITER == COMMA {
//         false
//     } else {
//         enable
//     }
// });

pub static USE_PRI_REVERSE_PROXY: LazyLock<bool> =
    LazyLock::new(|| !PRI_REVERSE_PROXY_HOST.is_empty());

pub static USE_PUB_REVERSE_PROXY: LazyLock<bool> =
    LazyLock::new(|| !PUB_REVERSE_PROXY_HOST.is_empty());

macro_rules! def_cursor_api_url {
    (
        init_fn: $init_fn:ident,
        $(
            $group_name:ident => {
                host: $api_host:ident,
                apis: [
                    $( $name:ident => $path:expr ),+ $(,)?
                ]
            }
        ),+ $(,)?
    ) => {
        // 为每个API生成静态变量和getter函数
        $(
            $(
                paste::paste! {
                    static [<URL_PRI_ $name:upper>]: ManuallyInit<String> = ManuallyInit::new();
                    static [<URL_PUB_ $name:upper>]: ManuallyInit<String> = ManuallyInit::new();

                    #[inline(always)]
                    #[doc = $path]
                    pub fn $name(use_pri: bool) -> &'static str {
                        if use_pri {
                            [<URL_PRI_ $name:upper>].get()
                        } else {
                            [<URL_PUB_ $name:upper>].get()
                        }
                    }
                }
            )+
        )+

        // 生成统一的初始化函数
        pub fn $init_fn() {
            $(
                $(
                    paste::paste! {
                        // 初始化私有URL
                        {
                            let host = if *USE_PRI_REVERSE_PROXY {
                                &PRI_REVERSE_PROXY_HOST
                            } else {
                                $api_host
                            };
                            let mut url = String::with_capacity(HTTPS_PREFIX.len() + host.len() + $path.len());
                            url.push_str(HTTPS_PREFIX);
                            url.push_str(host);
                            url.push_str($path);
                            [<URL_PRI_ $name:upper>].init(url);
                        }

                        // 初始化公共URL
                        {
                            let host = if *USE_PUB_REVERSE_PROXY {
                                &PUB_REVERSE_PROXY_HOST
                            } else {
                                $api_host
                            };
                            let mut url = String::with_capacity(HTTPS_PREFIX.len() + host.len() + $path.len());
                            url.push_str(HTTPS_PREFIX);
                            url.push_str(host);
                            url.push_str($path);
                            [<URL_PUB_ $name:upper>].init(url);
                        }
                    }
                )+
            )+
        }
    };
}

// 一次性定义所有API
def_cursor_api_url! {
    init_fn: init_all_cursor_urls,

    // API2 HOST 相关API
    api2_group => {
        host: CURSOR_API2_HOST,
        apis: [
            chat_url => "/aiserver.v1.ChatService/StreamUnifiedChatWithTools",
            chat_models_url => "/aiserver.v1.AiService/AvailableModels",
            stripe_url => "/auth/full_stripe_profile",
            token_poll_url => "/auth/poll",
            token_refresh_url => "/oauth/token",
            server_config_url => "/aiserver.v1.ServerConfigService/GetServerConfig",
            dry_chat_url => "/aiserver.v1.ChatService/GetPromptDryRun",
        ]
    },

    // CURSOR HOST 相关API
    cursor_group => {
        host: CURSOR_HOST,
        apis: [
            usage_api_url => "/api/usage-summary",
            user_api_url => "/api/dashboard/get-me",
            token_upgrade_url => "/api/auth/loginDeepCallbackControl",
            // teams_url => "/api/dashboard/teams",
            // aggregated_usage_events_url => "/api/dashboard/get-aggregated-usage-events",
            filtered_usage_events_url => "/api/dashboard/get-filtered-usage-events",
            sessions_url => "/api/auth/sessions",
            is_on_new_pricing_url => "/api/dashboard/is-on-new-pricing",
            get_privacy_mode_url => "/api/dashboard/get-user-privacy-mode",
        ]
    },

    // API4 HOST 相关API
    api4_group => {
        host: CURSOR_API4_HOST,
        apis: [
            cpp_config_url => "/aiserver.v1.AiService/CppConfig",
        ]
    },

    // API2 HOST CPP相关API
    api2_cpp_group => {
        host: CURSOR_API2_HOST,
        apis: [
            cpp_models_url => "/aiserver.v1.CppService/AvailableModels",
        ]
    },

    // GCPP ASIA HOST 相关API
    gcpp_asia_group => {
        host: CURSOR_GCPP_ASIA_HOST,
        apis: [
            asia_upload_file_url => "/aiserver.v1.FileSyncService/FSUploadFile",
            asia_sync_file_url => "/aiserver.v1.FileSyncService/FSSyncFile",
            asia_stream_cpp_url => "/aiserver.v1.AiService/StreamCpp",
            // asia_next_cursor_prediction_url => "/aiserver.v1.AiService/StreamNextCursorPrediction",
        ]
    },

    // GCPP EU HOST 相关API
    gcpp_eu_group => {
        host: CURSOR_GCPP_EU_HOST,
        apis: [
            eu_upload_file_url => "/aiserver.v1.FileSyncService/FSUploadFile",
            eu_sync_file_url => "/aiserver.v1.FileSyncService/FSSyncFile",
            eu_stream_cpp_url => "/aiserver.v1.AiService/StreamCpp",
            // eu_next_cursor_prediction_url => "/aiserver.v1.AiService/StreamNextCursorPrediction",
        ]
    },

    // GCPP US HOST 相关API
    gcpp_us_group => {
        host: CURSOR_GCPP_US_HOST,
        apis: [
            us_upload_file_url => "/aiserver.v1.FileSyncService/FSUploadFile",
            us_sync_file_url => "/aiserver.v1.FileSyncService/FSSyncFile",
            us_stream_cpp_url => "/aiserver.v1.AiService/StreamCpp",
            // us_next_cursor_prediction_url => "/aiserver.v1.AiService/StreamNextCursorPrediction",
        ]
    }
}

// TCP 和超时相关常量
const DEFAULT_TCP_KEEPALIVE: usize = 90;
const MAX_TCP_KEEPALIVE: u64 = 600;

pub static TCP_KEEPALIVE: LazyLock<u64> = LazyLock::new(|| {
    let keepalive = parse_from_env("TCP_KEEPALIVE", DEFAULT_TCP_KEEPALIVE);
    u64::try_from(keepalive)
        .map(|t| t.min(MAX_TCP_KEEPALIVE))
        .unwrap_or(DEFAULT_TCP_KEEPALIVE as u64)
});

const DEFAULT_SERVICE_TIMEOUT: usize = 30;
const MAX_SERVICE_TIMEOUT: u64 = 600;

pub static SERVICE_TIMEOUT: LazyLock<u64> = LazyLock::new(|| {
    let timeout = parse_from_env("SERVICE_TIMEOUT", DEFAULT_SERVICE_TIMEOUT);
    u64::try_from(timeout)
        .map(|t| t.min(MAX_SERVICE_TIMEOUT))
        .unwrap_or(DEFAULT_SERVICE_TIMEOUT as u64)
});

pub static REAL_USAGE: LazyLock<bool> = LazyLock::new(|| parse_from_env("REAL_USAGE", true));

// pub static TOKEN_VALIDITY_RANGE: LazyLock<TokenValidityRange> = LazyLock::new(|| {
//     let short = if let Ok(Ok(validity)) = std::env::var("TOKEN_SHORT_VALIDITY")
//         .as_deref()
//         .map(ValidityRange::from_str)
//     {
//         validity
//     } else {
//         ValidityRange::new(5400, 5400)
//     };
//     let long = if let Ok(Ok(validity)) = std::env::var("TOKEN_LONG_VALIDITY")
//         .as_deref()
//         .map(ValidityRange::from_str)
//     {
//         validity
//     } else {
//         ValidityRange::new(5184000, 5184000)
//     };
//     TokenValidityRange::new(short, long)
// });
