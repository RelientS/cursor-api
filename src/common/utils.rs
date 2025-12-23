#![allow(clippy::too_many_arguments)]

// mod checksum;
// mod token;
pub mod base62;
mod base64;
pub mod duration_fmt;
pub mod hex;
pub mod proto_encode;
pub mod string_builder;

use super::model::userinfo::{Session, StripeProfile, UsageProfile, UserProfile};
use crate::common::model::userinfo::{
    // GetAggregatedUsageEventsRequest, GetAggregatedUsageEventsResponse,
    GetFilteredUsageEventsRequest,
    GetFilteredUsageEventsResponse,
    PrivacyModeInfo,
};
use crate::{
    app::{
        constant::EMPTY_STRING,
        lazy::{
            chat_models_url, filtered_usage_events_url, get_privacy_mode_url,
            is_on_new_pricing_url, server_config_url, user_api_url,
        },
        model::{
            ChainUsage, Checksum, DateTime, ExtToken, GcppHost, Hash, RawToken, Token, TokenWriter,
            UnextTokenRef,
        },
    },
    core::{
        aiserver::v1::{AvailableModelsRequest, AvailableModelsResponse, GetServerConfigResponse},
        config::configured_key,
    },
};
use ::base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use alloc::borrow::Cow;
pub use base64::{from_base64, to_base64};
pub use hex::hex_to_byte;
use interned::ArcStr;
use prost::Message as _;
pub use proto_encode::{encode_message, encode_message_framed};
use reqwest::Client;
pub use string_builder::StringBuilder;

mod sealed {
    pub trait Sealed: Sized + 'static {}

    impl Sealed for bool {}
    impl Sealed for &'static str {}
}

pub trait ParseFromEnv: sealed::Sealed {
    type Result: Sized + 'static = Self;
    fn parse_from_env(key: &str, default: Self) -> Self::Result;
}

impl ParseFromEnv for bool {
    #[inline]
    fn parse_from_env(key: &str, default: bool) -> bool {
        ::std::env::var(key)
            .ok()
            .map(|mut val| {
                let res = {
                    val.make_ascii_lowercase();
                    val.trim()
                };
                match res {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => default,
                }
            })
            .unwrap_or(default)
    }
}

impl ParseFromEnv for &'static str {
    type Result = Cow<'static, str>;
    #[inline]
    fn parse_from_env(key: &str, default: &'static str) -> Cow<'static, str> {
        match ::std::env::var(key) {
            Ok(mut value) => {
                let trimmed = value.trim();

                if trimmed.is_empty() {
                    // 如果 trim 后为空，使用默认值（不分配）
                    Cow::Borrowed(default)
                } else if trimmed.len() == value.len() {
                    // 不需要 trim，直接使用
                    Cow::Owned(value)
                } else {
                    // 需要 trim - 就地修改
                    let trimmed_len = trimmed.len();
                    let start_offset = trimmed.as_ptr() as usize - value.as_ptr() as usize;

                    unsafe {
                        let vec = value.as_mut_vec();
                        if start_offset > 0 {
                            vec.copy_within(start_offset..start_offset + trimmed_len, 0);
                        }
                        vec.set_len(trimmed_len);
                    }

                    Cow::Owned(value)
                }
            }
            Err(_) => Cow::Borrowed(default),
        }
    }
}

macro_rules! impl_parse_num_from_env {
    ($($ty:ty)*) => {
        $(
            impl sealed::Sealed for $ty {}
            impl ParseFromEnv for $ty {
                #[inline]
                fn parse_from_env(key: &str, default: $ty) -> $ty {
                    ::std::env::var(key).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
                }
            }
        )*
    };
}

impl_parse_num_from_env!(i8 u8 i16 u16 i32 u32 i64 u64 i128 u128 isize usize);

impl sealed::Sealed for duration_fmt::DurationFormat {}
impl sealed::Sealed for duration_fmt::Language {}

impl ParseFromEnv for duration_fmt::DurationFormat {
    fn parse_from_env(key: &str, default: Self) -> Self::Result {
        let s = <&'static str as ParseFromEnv>::parse_from_env(key, EMPTY_STRING);
        match &*s {
            "auto" => Self::Auto,
            "compact" => Self::Compact,
            "standard" => Self::Standard,
            "detailed" => Self::Detailed,
            "iso8601" => Self::ISO8601,
            "fuzzy" => Self::Fuzzy,
            "numeric" => Self::Numeric,
            "verbose" => Self::Verbose,
            "random" => Self::Random,
            _ => default,
        }
    }
}
impl ParseFromEnv for duration_fmt::Language {
    fn parse_from_env(key: &str, default: Self) -> Self::Result {
        let s = <&'static str as ParseFromEnv>::parse_from_env(key, EMPTY_STRING);
        match &*s {
            "english" => Self::English,
            "chinese" => Self::Chinese,
            "japanese" => Self::Japanese,
            "spanish" => Self::Spanish,
            "german" => Self::German,
            "random" => Self::Random,
            _ => default,
        }
    }
}

#[inline]
pub fn parse_from_env<T: ParseFromEnv>(key: &str, default: T) -> T::Result {
    ParseFromEnv::parse_from_env(key, default)
}

pub fn now() -> core::time::Duration {
    now_with_epoch(std::time::UNIX_EPOCH, "system time before Unix epoch")
}

#[inline]
pub fn now_with_epoch(earlier: std::time::SystemTime, expect: &'static str) -> std::time::Duration {
    let now = std::time::SystemTime::now().duration_since(earlier).expect(expect);
    let delta = super::model::ntp::DELTA.load(core::sync::atomic::Ordering::Relaxed);
    if delta.is_negative() {
        now.checked_sub(core::time::Duration::from_nanos(delta.wrapping_neg() as u64))
            .expect("NTP delta underflow: adjustment exceeds current time")
    } else {
        now.checked_add(core::time::Duration::from_nanos(delta as u64))
            .expect("NTP delta overflow: time adjustment too large")
    }
}

#[inline]
pub fn now_secs() -> u64 { now().as_secs() }

const LEN: usize = 2;

pub trait TrimNewlines: Sized {
    fn trim_leading_newlines(self) -> Self;
}

impl TrimNewlines for &str {
    #[inline(always)]
    fn trim_leading_newlines(self) -> Self {
        let bytes = self.as_bytes();
        if bytes.len() >= LEN && bytes[0] == b'\n' && bytes[1] == b'\n' {
            return unsafe { self.get_unchecked(LEN..) };
        }
        self
    }
}

impl TrimNewlines for String {
    #[inline(always)]
    fn trim_leading_newlines(mut self) -> Self {
        let bytes = self.as_bytes();
        if bytes.len() >= LEN && bytes[0] == b'\n' && bytes[1] == b'\n' {
            unsafe {
                let vec = self.as_mut_vec();
                vec.drain(..LEN);
            }
        }
        self
    }
}

// #[inline(never)]
pub async fn get_token_profile(
    client: Client,
    unext: UnextTokenRef<'_>,
    use_pri: bool,
    include_sessions: bool,
) -> (Option<UsageProfile>, Option<StripeProfile>, Option<UserProfile>, Option<Vec<Session>>) {
    let cookie = unext.format_workos_cursor_session_token();
    let bearer_token = unext.format_bearer_token();

    if include_sessions {
        let (usage, stripe, mut user, is_on_new_pricing, privacy_mode, sessions) = tokio::join!(
            get_usage_profile(&client, cookie.clone(), use_pri),
            get_stripe_profile(&client, bearer_token, use_pri),
            get_user_profile(&client, cookie.clone(), use_pri),
            get_is_on_new_pricing(&client, cookie.clone(), use_pri),
            get_user_privacy_mode(&client, cookie.clone(), use_pri),
            get_sessions(&client, cookie, use_pri),
        );

        if let Some(user) = user.as_mut() {
            user.is_on_new_pricing = is_on_new_pricing.unwrap_or(true);
            user.privacy_mode_info = privacy_mode.unwrap_or_default();
        }

        (usage, stripe, user, sessions)
    } else {
        let (usage, stripe, mut user, is_on_new_pricing, privacy_mode) = tokio::join!(
            get_usage_profile(&client, cookie.clone(), use_pri),
            get_stripe_profile(&client, bearer_token, use_pri),
            get_user_profile(&client, cookie.clone(), use_pri),
            get_is_on_new_pricing(&client, cookie.clone(), use_pri),
            get_user_privacy_mode(&client, cookie, use_pri),
        );

        if let Some(user) = user.as_mut() {
            user.is_on_new_pricing = is_on_new_pricing.unwrap_or(true);
            user.privacy_mode_info = privacy_mode.unwrap_or_default();
        }

        (usage, stripe, user, None)
    }
}

// #[inline(never)]
// pub async fn get_token_profile_o(
//     client: Client,
//     unext: UnextTokenRef<'_>,
//     use_pri: bool,
// ) -> (Option<UsageProfile>, Option<StripeProfile>) {
//     tokio::join!(
//         get_usage_profile(&client, unext.format_workos_cursor_session_token(), use_pri),
//         get_stripe_profile(&client, unext.format_bearer_token(), use_pri)
//     )
// }

/// 获取用户使用情况配置文件
pub async fn get_usage_profile(
    client: &Client,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> Option<UsageProfile> {
    let request = super::client::build_usage_request(client, cookie, use_pri);
    let response = request.send().await.ok()?;
    crate::debug!("<get_usage_profile> {}", response.status());
    response.json().await.ok()
}

/// 获取Stripe付费配置文件
pub async fn get_stripe_profile(
    client: &Client,
    bearer_token: http::HeaderValue,
    use_pri: bool,
) -> Option<StripeProfile> {
    let request = super::client::build_stripe_request(client, bearer_token, use_pri);

    // let response = request.send().await.ok()?;
    // crate::debug!("<get_stripe_profile> {response:?}");
    // let bytes = response.bytes().await.ok()?;
    // crate::debug!("<get_stripe_profile> {:?}", unsafe { std::str::from_utf8_unchecked(&bytes[..]) });
    // serde_json::from_slice::<StripeProfile>(&bytes).ok()
    let response = request.send().await.ok()?;
    crate::debug!("<get_stripe_profile> {}", response.status());
    response.json().await.ok()
}

/// 获取用户基础配置文件
pub async fn get_user_profile(
    client: &Client,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> Option<UserProfile> {
    let request = super::client::build_proto_web_request(
        client,
        cookie,
        user_api_url(use_pri),
        use_pri,
        EMPTY_JSON,
    );

    // let response = request.send().await.ok()?;
    // crate::debug!("<get_user_profile> {response:?}");
    // let bytes = response.bytes().await.ok()?;
    // crate::debug!("<get_user_profile> {:?}", unsafe { std::str::from_utf8_unchecked(&bytes[..]) });
    // serde_json::from_slice::<UserProfile>(&bytes).ok()
    let response = request.send().await.ok()?;
    crate::debug!("<get_user_profile> {}", response.status());
    response.json::<UserProfile>().await.ok()
}

pub async fn get_available_models(
    ext_token: ExtToken,
    use_pri: bool,
    request: AvailableModelsRequest,
) -> Option<AvailableModelsResponse> {
    let response = {
        let (data, compressed) = encode_message(&request).ok()?;
        let client = super::client::build_client_request(super::client::AiServiceRequest {
            ext_token: &ext_token,
            fs_client_key: None,
            url: chat_models_url(use_pri),
            stream: false,
            compressed,
            trace_id: new_uuid_v4(),
            use_pri,
            cookie: None,
        });
        client.body(data).send().await.ok()?.bytes().await.ok()?
    };
    AvailableModelsResponse::decode(response.as_ref()).ok()
}

pub async fn get_token_usage(
    ext_token: ExtToken,
    use_pri: bool,
    time: DateTime,
    model_id: &'static str,
) -> Option<ChainUsage> {
    const POLL_MAX_ATTEMPTS: usize = 5;
    const POLL_INTERVAL: core::time::Duration = core::time::Duration::from_secs(1);

    let mut token_usage = None;

    // crate::debug!("{}",time.timestamp_millis());
    // crate::debug!("{}",DateTime::now().timestamp_millis());

    let body = bytes::Bytes::from(__unwrap!(serde_json::to_vec(&{
        let req: GetFilteredUsageEventsRequest = FilteredUsageArgs {
            start: Some(time),
            end: None,
            model_id: Some(model_id),
            size: Some(10),
        }
        .into();
        req
    })));

    for request in rep_move::RepMove::new(
        super::client::build_proto_web_request(
            &ext_token.get_client(),
            ext_token.as_unext().format_workos_cursor_session_token(),
            filtered_usage_events_url(use_pri),
            use_pri,
            body,
        ),
        RequestBuilderClone,
        POLL_MAX_ATTEMPTS,
    ) {
        tokio::time::sleep(POLL_INTERVAL).await;
        let res = get_filtered_usage_events(request).await?;

        if let Some(first) = res.usage_events_display.first()
            && let Some(usage) = first.token_usage
        {
            token_usage = Some(usage);
            break;
        };
    }

    token_usage.map(Into::into)
}

// pub fn validate_token_and_checksum(auth_token: &str) -> Option<(String, Checksum)> {
//     // 尝试使用自定义分隔符查找
//     let mut delimiter_pos = auth_token.rfind(*TOKEN_DELIMITER);

//     // 如果自定义分隔符未找到，并且 USE_COMMA_DELIMITER 为 true，则尝试使用逗号
//     if delimiter_pos.is_none() && *USE_COMMA_DELIMITER {
//         delimiter_pos = auth_token.rfind(COMMA);
//     }

//     // 如果最终都没有找到分隔符，则返回 None
//     let comma_pos = delimiter_pos?;

//     // 使用找到的分隔符位置分割字符串
//     let (token_part, checksum) = auth_token.split_at(comma_pos);
//     let checksum = &checksum[1..]; // 跳过逗号

//     // 解析 token - 为了向前兼容,忽略最后一个:或%3A前的内容
//     let colon_pos = token_part.rfind(':');
//     let encoded_colon_pos = token_part.rfind("%3A");

//     let token = match (colon_pos, encoded_colon_pos) {
//         (None, None) => token_part, // 最简单的构成: token,checksum
//         (Some(pos1), None) => &token_part[(pos1 + 1)..],
//         (None, Some(pos2)) => &token_part[(pos2 + 3)..],
//         (Some(pos1), Some(pos2)) => {
//             let pos = pos1.max(pos2);
//             let start = if pos == pos2 { pos + 3 } else { pos + 1 };
//             &token_part[start..]
//         }
//     };

//     // 验证 token 和 checksum 有效性
//     if let Ok(chekcsum) = Checksum::from_str(checksum) {
//         if validate_token(token) {
//             Some((token.to_string(), chekcsum))
//         } else {
//             None
//         }
//     } else {
//         None
//     }
// }

// pub fn extract_token(auth_token: &str) -> Option<&str> {
//     // 尝试使用自定义分隔符查找
//     let mut delimiter_pos = auth_token.rfind(*TOKEN_DELIMITER);

//     // 如果自定义分隔符未找到，并且 USE_COMMA_DELIMITER 为 true，则尝试使用逗号
//     if delimiter_pos.is_none() && *USE_COMMA_DELIMITER {
//         delimiter_pos = auth_token.rfind(COMMA);
//     }

//     // 根据是否找到分隔符来确定 token_part
//     let token_part = match delimiter_pos {
//         Some(pos) => &auth_token[..pos],
//         None => auth_token,
//     };

//     // 向前兼容
//     let colon_pos = token_part.rfind(':');
//     let encoded_colon_pos = token_part.rfind("%3A");

//     let token = match (colon_pos, encoded_colon_pos) {
//         (None, None) => token_part,
//         (Some(pos1), None) => &token_part[(pos1 + 1)..],
//         (None, Some(pos2)) => &token_part[(pos2 + 3)..],
//         (Some(pos1), Some(pos2)) => {
//             let pos = pos1.max(pos2);
//             let start = if pos == pos2 { pos + 3 } else { pos + 1 };
//             &token_part[start..]
//         }
//     };

//     // 验证 token 有效性
//     if validate_token(token) {
//         Some(token)
//     } else {
//         None
//     }
// }

#[inline(always)]
pub fn format_time_ms(seconds: f64) -> f64 { (seconds * 1000.0).round() / 1000.0 }

/// 将 JWT token 转换为 TokenInfo
#[inline]
pub fn token_to_tokeninfo(
    token: RawToken,
    checksum: Checksum,
    client_key: Hash,
    config_version: Option<uuid::Uuid>,
    session_id: uuid::Uuid,
    proxy_name: Option<String>,
    timezone: Option<chrono_tz::Tz>,
    gcpp_host: Option<GcppHost>,
) -> configured_key::TokenInfo {
    configured_key::TokenInfo {
        token: configured_key::token_info::Token::from_raw(token),
        checksum: checksum.into_bytes(),
        client_key: client_key.into_bytes(),
        config_version: config_version.map(|v| v.into_bytes()),
        session_id: session_id.into_bytes(),
        proxy_name,
        timezone: timezone.map(|tz| tz.name().to_string()),
        gcpp_host: gcpp_host.map(|gh| gh as u8),
    }
}

/// 将 TokenInfo 转换为 JWT token
#[inline]
pub fn tokeninfo_to_token(info: configured_key::TokenInfo) -> Option<ExtToken> {
    let checksum = Checksum::from_bytes(info.checksum);
    let client_key = Hash::from_bytes(info.client_key);
    let config_version = info.config_version.and_then(|v| uuid::Uuid::from_slice(&v).ok());
    let session_id = uuid::Uuid::from_slice(&info.session_id).ok()?;
    let timezone = info.timezone.and_then(|s| core::str::FromStr::from_str(&s).ok());
    let gcpp_host = info.gcpp_host.and_then(GcppHost::from_u8);
    Some(ExtToken {
        primary_token: Token::new(info.token.into_raw()?, None),
        secondary_token: None,
        checksum,
        client_key,
        config_version,
        session_id,
        proxy: info.proxy_name.map(ArcStr::new),
        timezone,
        gcpp_host,
    })
}

/// 生成 PKCE code_verifier 和对应的 code_challenge (S256 method)
#[inline]
fn generate_pkce_pair() -> ([u8; 43], [u8; 43]) {
    use core::mem::MaybeUninit;
    use rand::TryRngCore as _;
    use sha2::Digest as _;

    // 生成 32 字节随机数作为 verifier
    let mut verifier_bytes = MaybeUninit::<[u8; 32]>::uninit();

    // 直接填充到未初始化内存
    unsafe {
        let bytes_ptr = verifier_bytes.as_mut_ptr().cast();
        let bytes_slice = core::slice::from_raw_parts_mut(bytes_ptr, 32);

        __unwrap!(rand::rngs::OsRng.try_fill_bytes(bytes_slice));

        // 此时 verifier_bytes 已完全初始化
        let verifier_bytes = verifier_bytes.assume_init();

        // Base64 编码为 code_verifier
        let mut code_verifier = MaybeUninit::<[u8; 43]>::uninit();
        let verifier_ptr = code_verifier.as_mut_ptr().cast();
        let verifier_slice = core::slice::from_raw_parts_mut(verifier_ptr, 43);
        __unwrap!(URL_SAFE_NO_PAD.encode_slice(verifier_bytes, verifier_slice));
        let code_verifier = code_verifier.assume_init();

        // SHA-256 哈希后 Base64 编码为 code_challenge
        let hash_result = sha2::Sha256::digest(code_verifier);
        let mut code_challenge = MaybeUninit::<[u8; 43]>::uninit();
        let challenge_ptr = code_challenge.as_mut_ptr().cast();
        let challenge_slice = core::slice::from_raw_parts_mut(challenge_ptr, 43);
        __unwrap!(URL_SAFE_NO_PAD.encode_slice(hash_result, challenge_slice));
        let code_challenge = code_challenge.assume_init();

        (code_verifier, code_challenge)
    }
}

pub async fn get_new_token(mut writer: TokenWriter<'_>, use_pri: bool) -> bool {
    let ext_token = &mut **writer;
    let is_session = ext_token.primary_token.is_session();

    match if is_session {
        refresh_token(ext_token, use_pri).await
    } else {
        upgrade_token(ext_token, use_pri).await
    } {
        Some(new_token) => {
            if !is_session && ext_token.secondary_token.is_none() {
                let old_token = core::mem::replace(&mut ext_token.primary_token, new_token);
                ext_token.secondary_token = Some(old_token);
            } else {
                ext_token.primary_token = new_token;
            }
            true
        }
        None => {
            if is_session
                && ext_token.secondary_token.is_some()
                && let Some(new_token) = upgrade_token(ext_token, use_pri).await
            {
                ext_token.primary_token = new_token;
                true
            } else {
                false
            }
        }
    }
}

async fn upgrade_token(ext_token: &ExtToken, use_pri: bool) -> Option<Token> {
    const POLL_MAX_ATTEMPTS: usize = 5;
    const POLL_INTERVAL: core::time::Duration = core::time::Duration::from_secs(1);

    #[derive(::serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PollResponse {
        pub access_token: Token,
        // pub refresh_token: String,
        // pub challenge: String,
        // pub auth_id: String,
        // pub uuid: String,
    }

    let (verifier, challenge) = generate_pkce_pair();
    let verifier = unsafe { core::str::from_utf8_unchecked(&verifier) };
    let challenge = unsafe { core::str::from_utf8_unchecked(&challenge) };
    let mut buf = [0; 36];
    let uuid = uuid::Uuid::new_v4().hyphenated().encode_lower(&mut buf) as &str;

    // 发起刷新请求
    let upgrade_response = super::client::build_token_upgrade_request(
        &ext_token.get_client(),
        uuid,
        challenge,
        ext_token.as_unext().format_workos_cursor_session_token(),
        use_pri,
    )
    .send()
    .await
    .ok()?;

    let status = upgrade_response.status();
    crate::debug!("<upgrade_token1> {}", status);
    if !status.is_success() {
        return None;
    }

    // 轮询获取token
    for request in rep_move::RepMove::new(
        super::client::build_token_poll_request(&ext_token.get_client(), uuid, verifier, use_pri),
        RequestBuilderClone,
        POLL_MAX_ATTEMPTS,
    ) {
        let poll_response = request.send().await.ok()?;

        let status = poll_response.status();
        crate::debug!("<upgrade_token2> {}", status);
        match status {
            http::StatusCode::OK => {
                let token = poll_response.json::<PollResponse>().await.ok()?.access_token;
                return Some(token);
            }
            http::StatusCode::NOT_FOUND => {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            _ => return None,
        }
    }

    None
}

async fn refresh_token(ext_token: &ExtToken, use_pri: bool) -> Option<Token> {
    const CLIENT_ID: &str = "KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB";

    struct RefreshTokenRequest<'a> {
        refresh_token: &'a str,
    }

    impl ::serde::Serialize for RefreshTokenRequest<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: ::serde::Serializer {
            use ::serde::ser::SerializeStruct as _;
            let mut state = serializer.serialize_struct("RefreshTokenRequest", 3)?;
            state.serialize_field("grant_type", "refresh_token")?;
            state.serialize_field("client_id", CLIENT_ID)?;
            state.serialize_field("refresh_token", self.refresh_token)?;
            state.end()
        }
    }

    #[derive(::serde::Deserialize)]
    struct RefreshTokenResponse {
        access_token: Token,
        // id_token: String,
        // #[serde(rename = "shouldLogout")]
        // should_logout: bool,
    }

    let refresh_request = RefreshTokenRequest { refresh_token: ext_token.primary_token.as_str() };

    let body = serde_json::to_vec(&refresh_request).ok()?;

    let response =
        super::client::build_token_refresh_request(&ext_token.get_client(), use_pri, body)
            .send()
            .await
            .ok()?;

    crate::debug!("<refresh_token> {}", response.status());

    let token = response.json::<RefreshTokenResponse>().await.ok()?.access_token;

    Some(token)
}

pub async fn get_server_config(ext_token: ExtToken, use_pri: bool) -> Option<uuid::Uuid> {
    let response = {
        let client = super::client::build_client_request(super::client::AiServiceRequest {
            ext_token: &ext_token,
            fs_client_key: None,
            url: server_config_url(use_pri),
            stream: false,
            compressed: false,
            trace_id: new_uuid_v4(),
            use_pri,
            cookie: None,
        });
        client.send().await.ok()?.bytes().await.ok()?
    };
    let server_config = GetServerConfigResponse::decode(response.as_ref()).ok()?;
    uuid::Uuid::try_parse(&server_config.config_version).ok()
}

// pub async fn get_geo_cpp_backend_url(
//     client: Client,
//     auth_token: &str,
//     checksum: Checksum,
//     client_key: Hash,
//     timezone: &'static str,
//     session_id: Option<uuid::Uuid>,
//     use_pri: bool,
// ) -> Option<String> {
//     let response = {
//         let client = super::client::build_client_request(super::client::AiServiceRequest {
//             client,
//             auth_token,
//             checksum,
//             client_key,
//             fs_client_key: None,
//             url: crate::app::lazy::cpp_config_url(use_pri),
//             is_stream: false,
//             config_version: None,
//             timezone,
//             trace_id: Some(new_uuid_v4()),
//             session_id,
//             use_pri,
//         });
//         let request = crate::core::aiserver::v1::CppConfigRequest::default();
//         client
//             .body(__unwrap!(encode_message(&request, false)))
//             .send()
//             .await
//             .ok()?
//             .bytes()
//             .await
//             .ok()?
//     };
//     crate::core::aiserver::v1::CppConfigResponse::decode(response.as_ref())
//         .ok()
//         .map(|res| res.geo_cpp_backend_url)
// }

const EMPTY_JSON: bytes::Bytes = bytes::Bytes::from_static(b"{}");

// pub async fn get_teams(
//     client: &Client,
//     user_id: &str,
//     auth_token: &str,
//     use_pri: bool,
// ) -> Option<Vec<Team>> {
//     let request = super::client::build_proto_web_request(
//         client, user_id, auth_token, teams_url, use_pri, EMPTY_JSON,
//     );

//     request.send().await.ok()?.json::<GetTeamsResponse>().await.ok().map(|r| r.teams)
// }

pub async fn get_is_on_new_pricing(
    client: &Client,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> Option<bool> {
    let request = super::client::build_proto_web_request(
        client,
        cookie,
        is_on_new_pricing_url(use_pri),
        use_pri,
        EMPTY_JSON,
    );

    #[derive(serde::Deserialize)]
    struct IsOnNewPricingResponse {
        #[serde(rename = "isOnNewPricing")]
        is_on_new_pricing: bool,
        // #[serde(rename = "isOptedOut")]
        // is_opted_out: bool,
    }

    let response = request.send().await.ok()?;
    crate::debug!("<get_is_on_new_pricing> {}", response.status());
    response.json::<IsOnNewPricingResponse>().await.ok().map(|r| r.is_on_new_pricing)
}

pub async fn get_user_privacy_mode(
    client: &Client,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> Option<PrivacyModeInfo> {
    let request = super::client::build_proto_web_request(
        client,
        cookie,
        get_privacy_mode_url(use_pri),
        use_pri,
        EMPTY_JSON,
    );

    let response = request.send().await.ok()?;
    crate::debug!("<get_user_privacy_mode> {}", response.status());
    response.json().await.ok()
}

pub async fn get_sessions(
    client: &Client,
    cookie: http::HeaderValue,
    use_pri: bool,
) -> Option<Vec<Session>> {
    let request = super::client::build_sessions_request(client, cookie, use_pri);

    #[derive(serde::Deserialize)]
    pub struct ListActiveSessionsResponse {
        pub sessions: Vec<Session>,
    }

    let response = request.send().await.ok()?;
    crate::debug!("<get_sessions> {}", response.status());
    // let bytes = response.bytes().await.ok()?;
    // crate::debug!("<get_sessions> {}", unsafe{core::str::from_utf8_unchecked(&bytes[..])});
    // serde_json::from_slice::<ListActiveSessionsResponse>(&bytes[..]).ok().map(|r| r.sessions)
    response.json::<ListActiveSessionsResponse>().await.ok().map(|r| r.sessions)
}

// pub async fn get_aggregated_usage_events(
//     client: &Client,
//     user_id: &str,
//     auth_token: &str,
//     use_pri: bool,
// ) -> Option<GetAggregatedUsageEventsResponse> {
//     let request = super::client::build_proto_web_request(
//         client,
//         user_id,
//         auth_token,
//         aggregated_usage_events_url,
//         use_pri,
//         bytes::Bytes::from(__unwrap!(serde_json::to_vec(&{
//             const DELTA: chrono::TimeDelta = __unwrap!(chrono::TimeDelta::new(2629743, 765840000));
//             let now = DateTime::utc_now();
//             let start_date = now - DELTA;
//             GetAggregatedUsageEventsRequest {
//                 team_id: -1,
//                 start_date: Some(start_date.timestamp_millis()),
//                 end_date: Some(now.timestamp_millis()),
//                 user_id: None,
//             }
//         }))),
//     );

//     let response = request.send().await.ok()?;
//     crate::debug!("<get_aggregated_usage_events> {}", response.status());
//     let bytes = response.bytes().await.ok()?;
//     // crate::debug!("<get_aggregated_usage_events> {}", unsafe{core::str::from_utf8_unchecked(&bytes[..])});
//     serde_json::from_slice(&bytes[..]).ok()
// }

pub struct FilteredUsageArgs {
    pub start: Option<DateTime>,
    pub end: Option<DateTime>,
    pub model_id: Option<&'static str>,
    pub size: Option<i32>,
}

impl From<FilteredUsageArgs> for GetFilteredUsageEventsRequest {
    #[inline]
    fn from(args: FilteredUsageArgs) -> Self {
        const TZ: chrono::FixedOffset = __unwrap!(chrono::FixedOffset::west_opt(16 * 3600));
        const TIME: chrono::NaiveTime = __unwrap!(chrono::NaiveTime::from_hms_opt(0, 0, 0));
        const START: chrono::TimeDelta = chrono::TimeDelta::days(-7);
        const END: chrono::TimeDelta = __unwrap!(chrono::TimeDelta::new(86399, 999000000));

        let (start_date, end_date) = if let (Some(a), Some(b)) = (args.start, args.end) {
            (a.timestamp_millis(), b.timestamp_millis())
        } else {
            let now = chrono::DateTime::<chrono::FixedOffset>::from_naive_utc_and_offset(
                DateTime::naive_now(),
                TZ,
            )
            .date_naive()
            .and_time(TIME);
            match (args.start, args.end) {
                (None, None) => (
                    (now + START).and_local_timezone(TZ).unwrap().timestamp_millis(),
                    (now + END).and_local_timezone(TZ).unwrap().timestamp_millis(),
                ),
                (None, Some(b)) => (
                    (now + START).and_local_timezone(TZ).unwrap().timestamp_millis(),
                    b.timestamp_millis(),
                ),
                (Some(a), None) => (
                    a.timestamp_millis(),
                    (now + END).and_local_timezone(TZ).unwrap().timestamp_millis(),
                ),
                (Some(_), Some(_)) => unsafe { core::hint::unreachable_unchecked() },
            }
        };
        Self {
            team_id: 0,
            start_date: Some(start_date),
            end_date: Some(end_date),
            user_id: None,
            model_id: args.model_id,
            page: 1,
            page_size: args.size.unwrap_or(100),
        }
    }
}

pub async fn get_filtered_usage_events(
    request: reqwest::RequestBuilder,
) -> Option<GetFilteredUsageEventsResponse> {
    let res = request.send().await.ok()?;
    crate::debug!("<get_filtered_usage_events> {}", res.status());
    let res = res.bytes().await.ok()?;
    // crate::debug!("<get_filtered_usage_events> {}", unsafe {core::str::from_utf8_unchecked(&res[..])});
    serde_json::from_slice(&res[..]).ok()
}

#[inline]
pub fn new_uuid_v4() -> [u8; 36] {
    let mut buf = [0; 36];
    uuid::Uuid::new_v4().hyphenated().encode_lower(&mut buf);
    buf
}

#[allow(non_upper_case_globals)]
pub static RequestBuilderClone: fn(&reqwest::RequestBuilder) -> reqwest::RequestBuilder =
    |v| __unwrap!(v.try_clone());

#[inline(always)]
pub const fn r#true() -> bool { true }

#[allow(non_snake_case)]
pub fn CollectBytes(
    req: reqwest::RequestBuilder,
) -> impl Future<Output = Result<bytes::Bytes, reqwest::Error>> {
    async { req.send().await?.bytes().await }
}

#[allow(non_snake_case)]
pub fn CollectBytesParts(
    req: reqwest::RequestBuilder,
) -> impl Future<Output = Result<(http::response::Parts, bytes::Bytes), reqwest::Error>> {
    async { req.send().await?.into_bytes_parts().await }
}
