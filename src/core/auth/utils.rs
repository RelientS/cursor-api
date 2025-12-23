use super::{error::AuthError, model::TokenBundleResult};
use crate::{
    app::{
        constant::{
            AUTHORIZATION_BEARER_PREFIX,
            header::{API_KEY, STAINLESS_ARCH, STAINLESS_OS},
        },
        lazy::AUTH_TOKEN,
        model::{AppConfig, AppState, DateTime, QueueType, TokenKey, log_manager},
    },
    common::utils::tokeninfo_to_token,
    core::{
        aiserver::v1::EnvironmentInfo,
        config::{KeyConfig, parse_dynamic_token},
    },
};
use http::header::AUTHORIZATION;

#[inline]
pub fn auth(headers: &http::HeaderMap) -> Option<&str> {
    if let Some(val) = headers.get(API_KEY)
        && let Ok(s) = val.to_str()
    {
        return Some(s);
    }
    if let Some(val) = headers.get(AUTHORIZATION)
        && let Ok(s) = val.to_str()
    {
        return s.strip_prefix(AUTHORIZATION_BEARER_PREFIX);
    }
    None
}

#[inline]
pub(super) fn get_environment_info(
    headers: &http::HeaderMap,
    request_time: DateTime,
) -> EnvironmentInfo {
    fn get(headers: &http::HeaderMap, key: http::HeaderName) -> Option<prost::ByteStr> {
        headers.get(key).and_then(|v| {
            let v: &crate::common::model::HeaderValue = unsafe { core::mem::transmute(v) };
            let bytes = v.inner.as_ref();
            for &b in bytes {
                if !(b >= 32 && b < 127 || b == b'\t') {
                    return None;
                }
            }
            // crate::debug!("{}", unsafe { str::from_utf8_unchecked(bytes) });
            Some(unsafe { prost::ByteStr::from_utf8_unchecked(v.inner.clone()) })
        })
    }
    EnvironmentInfo {
        exthost_platform: get(headers, STAINLESS_OS).map(|b| match &*b {
            "MacOS" => prost::ByteStr::from_static("darwin"),
            "Windows" => prost::ByteStr::from_static("win32"),
            "Linux" => prost::ByteStr::from_static("linux"),
            s => {
                crate::debug!("hit platform: {s}");
                b
            }
        }),
        exthost_arch: get(headers, STAINLESS_ARCH),
        local_timestamp: request_time.to_utc().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        cursor_version: crate::app::constant::header::cursor_version(),
    }
}

/// 统一的 token 获取函数
///
/// 从 HTTP headers 中提取并验证认证 token，返回对应的 ExtToken
pub(super) async fn get_token_bundle(
    state: &AppState,
    auth_token: &str,
    privileged_queue: QueueType,
    normal_queue: QueueType,
    key_config: Option<&mut KeyConfig>,
) -> TokenBundleResult {
    // 管理员 Token
    if let Some(part) = auth_token.strip_prefix(&**AUTH_TOKEN) {
        let token_manager = state.token_manager.read().await;

        let bundle = if part.is_empty() {
            token_manager.select(privileged_queue).ok_or(AuthError::NoAvailableTokens)?
        } else if let Some(alias) = part.strip_prefix('-') {
            if !token_manager.alias_map().contains_key(alias) {
                return Err(AuthError::AliasNotFound);
            }
            token_manager
                .get_by_alias(alias)
                .map(|token_info| token_info.bundle.clone())
                .ok_or(AuthError::Unauthorized)?
        } else {
            return Err(AuthError::Unauthorized);
        };

        return Ok((bundle, true));
    } else
    // 共享 Token
    if AppConfig::is_share() && AppConfig::share_token_eq(auth_token) {
        let token_manager = state.token_manager.read().await;
        let bundle = token_manager.select(normal_queue).ok_or(AuthError::NoAvailableTokens)?;
        return Ok((bundle, true));
    } else
    // 普通用户 Token
    if let Some(key) = TokenKey::from_string(auth_token) {
        if let Some(bundle) = log_manager::get_token(key).await {
            return Ok((bundle, false));
        }
    } else
    // 动态密钥
    if AppConfig::get_dynamic_key() {
        if let Some(mut parsed_config) = parse_dynamic_token(auth_token) {
            if let Some(config) = key_config {
                parsed_config.move_without_auth_token(config);
                config.with_global();
            }

            if let Some(ext_token) = parsed_config.token_info.and_then(tokeninfo_to_token) {
                return Ok((ext_token, false));
            }
        }
    }

    Err(AuthError::Unauthorized)
}
