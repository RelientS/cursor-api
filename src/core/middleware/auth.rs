use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::http::header::AUTHORIZATION;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::app::constant::{API_KEY, AUTHORIZATION_BEARER_PREFIX, STAINLESS_ARCH, STAINLESS_OS};
use crate::app::lazy::AUTH_TOKEN;
use crate::app::model::{AppConfig, AppState, DateTime, TokenKey};
use crate::common::model::error::ChatError;
use crate::common::utils::tokeninfo_to_token;
use crate::core::aiserver::v1::EnvironmentInfo;
use crate::core::config::{KeyConfig, parse_dynamic_token};

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
fn get_environment_info(headers: &http::HeaderMap, request_time: DateTime) -> EnvironmentInfo {
    use crate::common::model::HeaderValue;
    #[inline(always)]
    pub const fn from(v: Option<&http::HeaderValue>) -> Option<&HeaderValue> {
        unsafe { core::mem::transmute(v) }
    }
    fn inner(v: &HeaderValue) -> Option<prost::ByteStr> {
        const fn is_visible_ascii(b: u8) -> bool {
            b >= 32 && b < 127 || b == b'\t'
        }
        let bytes = v.inner.as_ref();
        for &b in bytes {
            if !is_visible_ascii(b) {
                return None;
            }
        }
        // crate::debug!("{}", unsafe { str::from_utf8_unchecked(bytes) });
        Some(unsafe { prost::ByteStr::from_utf8_unchecked(v.inner.clone()) })
    }
    EnvironmentInfo {
        exthost_platform: from(headers.get(STAINLESS_OS)).and_then(inner).map(|b| match &*b {
            "MacOS" => prost::ByteStr::from_static("darwin"),
            "Windows" => prost::ByteStr::from_static("win32"),
            "Linux" => prost::ByteStr::from_static("linux"),
            s => {
                crate::debug!("hit platform: {s}");
                b
            }
        }),
        exthost_arch: from(headers.get(STAINLESS_ARCH)).and_then(inner),
        local_timestamp: request_time.to_utc().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        cursor_version: crate::app::constant::cursor_version(),
    }
}

// 管理员认证中间件函数
pub async fn admin_auth_middleware(request: Request<Body>, next: Next) -> Response {
    if let Some(token) = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix(AUTHORIZATION_BEARER_PREFIX))
        && token == *AUTH_TOKEN
    {
        return next.run(request).await;
    };

    (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic())).into_response()
}

pub async fn v1_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let auth_token = match auth(request.headers()) {
        Some(v) => v,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                .into_response();
        }
    };

    let mut current_config = KeyConfig::new_with_global();

    // 获取token信息
    let v = {
        // 管理员Token
        if let Some(part) = auth_token.strip_prefix(&**AUTH_TOKEN) {
            let token_manager = state.token_manager.read().await;

            let token_info = if part.is_empty() {
                let token_infos: Vec<_> =
                    token_manager.tokens().iter().flatten().filter(|t| t.is_enabled()).collect();

                if token_infos.is_empty() {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(ChatError::NoTokens.to_generic()),
                    )
                        .into_response();
                }

                static CURRENT_KEY_INDEX: AtomicUsize = AtomicUsize::new(0);

                let index = CURRENT_KEY_INDEX.fetch_add(1, Ordering::AcqRel) % token_infos.len();
                token_infos[index]
            } else if let Some(alias) = part.strip_prefix('-') {
                if !token_manager.alias_map().contains_key(alias) {
                    return StatusCode::NOT_FOUND.into_response();
                }
                if let Some(token_info) = token_manager.get_by_alias(alias) {
                    token_info
                } else {
                    return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                        .into_response();
                }
            } else {
                return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                    .into_response();
            };
            (token_info.bundle.clone_without_user(), true)
        }
        // 共享Token
        else if AppConfig::is_share() && AppConfig::share_token_eq(auth_token) {
            let token_manager = state.token_manager.read().await;
            let token_infos: Vec<_> =
                token_manager.tokens().iter().flatten().filter(|t| t.is_enabled()).collect();

            if token_infos.is_empty() {
                return (StatusCode::SERVICE_UNAVAILABLE, Json(ChatError::NoTokens.to_generic()))
                    .into_response();
            }

            static CURRENT_KEY_INDEX: AtomicUsize = AtomicUsize::new(0);

            let index = CURRENT_KEY_INDEX.fetch_add(1, Ordering::AcqRel) % token_infos.len();
            let token_info = token_infos[index];
            (token_info.bundle.clone_without_user(), true)
        }
        // 普通用户Token
        else if let Some(key) = TokenKey::from_string(auth_token) {
            let log_manager = state.log_manager_lock().await;
            if let Some(bundle) = log_manager.tokens().get(&key) {
                (bundle.clone_without_user(), false)
            } else {
                return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                    .into_response();
            }
        }
        // 动态密钥
        else if AppConfig::get_dynamic_key() {
            if let Some(ext_token) = parse_dynamic_token(auth_token)
                .and_then(|key_config| {
                    key_config.copy_without_auth_token(&mut current_config);
                    key_config.token_info
                })
                .and_then(tokeninfo_to_token)
            {
                (ext_token, false)
            } else {
                return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                    .into_response();
            }
        } else {
            return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                .into_response();
        }
    };
    let request_time = DateTime::now();
    let environment_info = get_environment_info(request.headers(), request_time);

    request.extensions_mut().insert(v);
    request.extensions_mut().insert(current_config);
    request.extensions_mut().insert(request_time);
    request.extensions_mut().insert(environment_info);

    // let (parts, body) = request.into_parts();

    // let body = Body::from_stream(body.into_data_stream().map(move |c| {
    //     if let Ok(ref b) = c {
    //         crate::debug!("{:?}", unsafe { str::from_utf8_unchecked(b) });
    //     }
    //     c
    // }));

    // let request = Request::from_parts(parts, body);

    next.run(request).await
}

pub async fn cpp_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let auth_token = match auth(request.headers()) {
        Some(v) => v,
        None => {
            return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                .into_response();
        }
    };

    // 获取token信息
    let v = {
        // 管理员Token
        if let Some(part) = auth_token.strip_prefix(&**AUTH_TOKEN) {
            let token_manager = state.token_manager.read().await;

            let token_info = if part.is_empty() {
                let token_infos: Vec<_> =
                    token_manager.tokens().iter().flatten().filter(|t| t.is_enabled()).collect();

                if token_infos.is_empty() {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(ChatError::NoTokens.to_generic()),
                    )
                        .into_response();
                }

                static CURRENT_KEY_INDEX: AtomicUsize = AtomicUsize::new(0);

                let index = CURRENT_KEY_INDEX.fetch_add(1, Ordering::AcqRel) % token_infos.len();
                token_infos[index]
            } else if let Some(alias) = part.strip_prefix('-') {
                if !token_manager.alias_map().contains_key(alias) {
                    return StatusCode::NOT_FOUND.into_response();
                }
                if let Some(token_info) = token_manager.get_by_alias(alias) {
                    token_info
                } else {
                    return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                        .into_response();
                }
            } else {
                return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                    .into_response();
            };
            (token_info.bundle.clone_without_user(), true)
        }
        // 共享Token
        else if AppConfig::is_share() && AppConfig::share_token_eq(auth_token) {
            let token_manager = state.token_manager.read().await;
            let token_infos: Vec<_> =
                token_manager.tokens().iter().flatten().filter(|t| t.is_enabled()).collect();

            if token_infos.is_empty() {
                return (StatusCode::SERVICE_UNAVAILABLE, Json(ChatError::NoTokens.to_generic()))
                    .into_response();
            }

            static CURRENT_KEY_INDEX: AtomicUsize = AtomicUsize::new(0);

            let index = CURRENT_KEY_INDEX.fetch_add(1, Ordering::AcqRel) % token_infos.len();
            let token_info = token_infos[index];
            (token_info.bundle.clone_without_user(), true)
        }
        // 普通用户Token
        else if let Some(key) = TokenKey::from_string(auth_token) {
            let log_manager = state.log_manager_lock().await;
            if let Some(bundle) = log_manager.tokens().get(&key) {
                (bundle.clone_without_user(), false)
            } else {
                return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                    .into_response();
            }
        }
        // 动态密钥
        else if AppConfig::get_dynamic_key() {
            if let Some(ext_token) = parse_dynamic_token(auth_token)
                .and_then(|key_config| key_config.token_info)
                .and_then(tokeninfo_to_token)
            {
                (ext_token, false)
            } else {
                return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                    .into_response();
            }
        } else {
            return (StatusCode::UNAUTHORIZED, Json(ChatError::Unauthorized.to_generic()))
                .into_response();
        }
    };

    request.extensions_mut().insert(v);

    next.run(request).await
}
