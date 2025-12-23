use alloc::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use http::Request;
use http::header::AUTHORIZATION;

use super::utils::{get_environment_info, get_token_bundle};
use super::{AuthError, auth};
use crate::app::constant::AUTHORIZATION_BEARER_PREFIX;
use crate::app::lazy::AUTH_TOKEN;
use crate::app::model::{AppState, DateTime, QueueType};
use crate::core::config::KeyConfig;

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

    AuthError::Unauthorized.into_response()
}

pub async fn v1_auth_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let Some(auth_token) = auth(request.headers()) else {
        return AuthError::Unauthorized.into_response();
    };

    let mut current_config = KeyConfig::new();

    match get_token_bundle(
        &state,
        auth_token,
        QueueType::PrivilegedPaid,
        QueueType::NormalPaid,
        Some(&mut current_config),
    )
    .await
    {
        v if v.is_ok() => {
            let request_time = DateTime::now();
            let environment_info = get_environment_info(request.headers(), request_time);

            request.extensions_mut().insert(v);
            request.extensions_mut().insert(current_config);
            request.extensions_mut().insert(request_time);
            request.extensions_mut().insert(environment_info);
        }
        e => {
            request.extensions_mut().insert(e);
        }
    };

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

pub async fn v1_auth2_middleware(
    State(state): State<Arc<AppState>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let Some(auth_token) = auth(request.headers()) else {
        return AuthError::Unauthorized.into_response();
    };

    let mut current_config = KeyConfig::new();

    match get_token_bundle(
        &state,
        auth_token,
        QueueType::PrivilegedFree,
        QueueType::NormalFree,
        Some(&mut current_config),
    )
    .await
    {
        v if v.is_ok() => {
            let request_time = DateTime::now();
            let environment_info = get_environment_info(request.headers(), request_time);

            request.extensions_mut().insert(current_config);
            request.extensions_mut().insert(environment_info);
            request.extensions_mut().insert(v);
        }
        e => {
            request.extensions_mut().insert(e);
        }
    };

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
    let Some(auth_token) = auth(request.headers()) else {
        return AuthError::Unauthorized.into_response();
    };

    let v = match get_token_bundle(
        &state,
        auth_token,
        QueueType::PrivilegedFree,
        QueueType::NormalFree,
        None,
    )
    .await
    {
        Ok(bundle) => bundle,
        Err(err) => return err.into_response(),
    };

    request.extensions_mut().insert(v);

    next.run(request).await
}
