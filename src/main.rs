use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use chrono::Local;
use cursor_api::app::*;
use cursor_api::message::*;
use futures::StreamExt;
use reqwest::Client;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{convert::Infallible, sync::Arc};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

// 支持的模型列表
mod models;
use models::AVAILABLE_MODELS;

// 自定义错误类型
enum ChatError {
    ModelNotSupported(String),
    EmptyMessages,
    StreamNotSupported(String),
    NoTokens,
    RequestFailed(String),
    Unauthorized,
}

impl ChatError {
    fn to_json(&self) -> serde_json::Value {
        let (code, message) = match self {
            ChatError::ModelNotSupported(model) => (
                "model_not_supported",
                format!("Model '{}' is not supported", model),
            ),
            ChatError::EmptyMessages => (
                "empty_messages",
                "Message array cannot be empty".to_string(),
            ),
            ChatError::StreamNotSupported(model) => (
                "stream_not_supported",
                format!("Streaming is not supported for model '{}'", model),
            ),
            ChatError::NoTokens => ("no_tokens", "No available tokens".to_string()),
            ChatError::RequestFailed(err) => ("request_failed", format!("Request failed: {}", err)),
            ChatError::Unauthorized => ("unauthorized", "Invalid authorization token".to_string()),
        };

        serde_json::json!({
            "error": {
                "code": code,
                "message": message
            }
        })
    }
}

#[tokio::main]
async fn main() {
    // 设置自定义 panic hook
    std::panic::set_hook(Box::new(|info| {
        // std::env::set_var("RUST_BACKTRACE", "1");
        if let Some(msg) = info.payload().downcast_ref::<String>() {
            eprintln!("{}", msg);
        } else if let Some(msg) = info.payload().downcast_ref::<&str>() {
            eprintln!("{}", msg);
        }
    }));

    // 加载环境变量
    dotenvy::dotenv().ok();

    // 初始化全局配置
    let auth_token = std::env::var("AUTH_TOKEN").expect("AUTH_TOKEN must be set");

    AppConfig::init(
        std::env::var("VISION_ABILITY").unwrap_or_else(|_| "base64".to_string()),
        std::env::var("ENABLE_SLOW_POOL")
            .ok()
            .filter(|v| v == "true")
            .map(|_| true),
        auth_token,
        std::env::var("TOKEN_FILE").unwrap_or_else(|_| ".token".to_string()),
        std::env::var("TOKEN_LIST_FILE").unwrap_or_else(|_| ".token-list".to_string()),
        std::env::var("ROUTE_PREFIX").unwrap_or_default(),
    );

    // 加载 tokens
    let token_infos = load_tokens();

    // 初始化应用状态
    let state = Arc::new(Mutex::new(AppState::new(token_infos)));

    let route_prefix = {
        let config = APP_CONFIG.read().unwrap();
        config.route_prefix.clone()
    };

    // 设置路由
    let app = Router::new()
        .route("/", get(handle_root))
        .route("/health", get(handle_health))
        .route("/tokeninfo", get(handle_tokeninfo_page))
        .route(&format!("{}/v1/models", route_prefix), get(handle_models))
        .route("/checksum", get(handle_checksum))
        .route("/update-tokeninfo", get(handle_update_tokeninfo))
        .route("/get-tokeninfo", post(handle_get_tokeninfo))
        .route("/update-tokeninfo", post(handle_update_tokeninfo_post))
        .route(
            &format!("{}/v1/chat/completions", route_prefix),
            post(handle_chat),
        )
        .route("/logs", get(handle_logs))
        .route("/logs", post(handle_logs_post))
        .route("/env-example", get(handle_env_example))
        .route("/config", get(handle_config_page))
        .route("/config", post(handle_config_update))
        .route("/static/:path", get(handle_static))
        .layer(CorsLayer::permissive())
        .with_state(state);

    // 启动服务器
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("0.0.0.0:{}", port);
    println!("服务器运行在端口 {}", port);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// Token 加载函数
fn load_tokens() -> Vec<TokenInfo> {
    let (token_file, token_list_file) = {
        let config = APP_CONFIG.read().unwrap();
        (config.token_file.clone(), config.token_list_file.clone())
    };

    // 确保文件存在
    for file in [&token_file, &token_list_file] {
        if !std::path::Path::new(file).exists() {
            if let Err(e) = std::fs::write(file, "") {
                eprintln!("警告: 无法创建文件 '{}': {}", file, e);
            }
        }
    }

    // 读取和规范化 token 文件
    let token_entries = match std::fs::read_to_string(&token_file) {
        Ok(content) => {
            let normalized = content.replace("\r\n", "\n");
            if normalized != content {
                if let Err(e) = std::fs::write(&token_file, &normalized) {
                    eprintln!("警告: 无法更新规范化的token文件: {}", e);
                }
            }

            normalized
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        return None;
                    }

                    match line.split("::").collect::<Vec<_>>() {
                        parts if parts.len() == 1 => Some((parts[0].to_string(), None)),
                        parts if parts.len() == 2 => {
                            Some((parts[1].to_string(), Some(parts[0].to_string())))
                        }
                        _ => {
                            eprintln!("警告: 忽略无效的token行: {}", line);
                            None
                        }
                    }
                })
                .collect::<Vec<_>>()
        }
        Err(e) => {
            eprintln!("警告: 无法读取token文件 '{}': {}", token_file, e);
            Vec::new()
        }
    };

    // 读取和规范化 token-list 文件
    let mut token_map: std::collections::HashMap<String, (String, Option<String>)> =
        match std::fs::read_to_string(&token_list_file) {
            Ok(content) => {
                let normalized = content.replace("\r\n", "\n");
                if normalized != content {
                    if let Err(e) = std::fs::write(&token_list_file, &normalized) {
                        eprintln!("警告: 无法更新规范化的token-list文件: {}", e);
                    }
                }

                normalized
                    .lines()
                    .filter_map(|line| {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') {
                            return None;
                        }

                        let parts: Vec<&str> = line.split(',').collect();
                        match parts[..] {
                            [token_part, checksum] => {
                                let (token, alias) =
                                    match token_part.split("::").collect::<Vec<_>>() {
                                        parts if parts.len() == 1 => (parts[0].to_string(), None),
                                        parts if parts.len() == 2 => {
                                            (parts[1].to_string(), Some(parts[0].to_string()))
                                        }
                                        _ => {
                                            eprintln!("警告: 忽略无效的token-list行: {}", line);
                                            return None;
                                        }
                                    };
                                Some((token, (checksum.to_string(), alias)))
                            }
                            _ => {
                                eprintln!("警告: 忽略无效的token-list行: {}", line);
                                None
                            }
                        }
                    })
                    .collect()
            }
            Err(e) => {
                eprintln!("警告: 无法读取token-list文件: {}", e);
                std::collections::HashMap::new()
            }
        };

    // 更新或添加新token
    for (token, alias) in token_entries {
        if let Some((_, existing_alias)) = token_map.get(&token) {
            // 只在alias不同时更新已存在的token
            if alias != *existing_alias {
                if let Some((checksum, _)) = token_map.get(&token) {
                    token_map.insert(token.clone(), (checksum.clone(), alias));
                }
            }
        } else {
            // 为新token生成checksum
            let checksum = cursor_api::generate_checksum(
                &cursor_api::generate_hash(),
                Some(&cursor_api::generate_hash()),
            );
            token_map.insert(token, (checksum, alias));
        }
    }

    // 更新 token-list 文件
    let token_list_content = token_map
        .iter()
        .map(|(token, (checksum, alias))| {
            if let Some(alias) = alias {
                format!("{}::{},{}", alias, token, checksum)
            } else {
                format!("{},{}", token, checksum)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if let Err(e) = std::fs::write(&token_list_file, token_list_content) {
        eprintln!("警告: 无法更新token-list文件: {}", e);
    }

    // 转换为 TokenInfo vector
    token_map
        .into_iter()
        .map(|(token, (checksum, alias))| TokenInfo {
            token,
            checksum,
            alias,
        })
        .collect()
}

// 根路由处理
async fn handle_root() -> impl IntoResponse {
    let config = APP_CONFIG.read().unwrap();
    match &config.root_content {
        PageContent::Default => Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header("Location", "/health")
            .body(Body::empty())
            .unwrap(),
        PageContent::Text(content) => Response::builder()
            .header("Content-Type", "text/plain;charset=utf-8")
            .body(Body::from(content.clone()))
            .unwrap(),
        PageContent::Html(content) => Response::builder()
            .header("Content-Type", "text/html;charset=utf-8")
            .body(Body::from(content.clone()))
            .unwrap(),
    }
}

async fn handle_health(State(state): State<Arc<Mutex<AppState>>>) -> Json<serde_json::Value> {
    let (start_time, version, route_prefix) = {
        let config = APP_CONFIG.read().unwrap();
        (
            config.start_time,
            config.version.clone(),
            config.route_prefix.clone(),
        )
    };

    let state = state.lock().await;
    let uptime = (Local::now() - start_time).num_seconds();

    Json(serde_json::json!({
        "status": "healthy",
        "version": version,
        "uptime": uptime,
        "stats": {
            "started": start_time,
            "totalRequests": state.total_requests,
            "activeRequests": state.active_requests,
            "memory": {
                "heapTotal": 0,
                "heapUsed": 0,
                "rss": 0
            }
        },
        "models": AVAILABLE_MODELS.iter().map(|m| &m.id).collect::<Vec<_>>(),
        "endpoints": [
            &format!("{}/v1/chat/completions", route_prefix),
            &format!("{}/v1/models", route_prefix),
            "/checksum",
            "/tokeninfo",
            "/update-tokeninfo",
            "/get-tokeninfo",
            "/logs",
            "/env-example",
            "/config",
            "/static"
        ]
    }))
}

async fn handle_tokeninfo_page() -> impl IntoResponse {
    let config = APP_CONFIG.read().unwrap();
    match &config.tokeninfo_content {
        PageContent::Default => Response::builder()
            .header("Content-Type", "text/html;charset=utf-8")
            .body(include_str!("../static/tokeninfo.min.html").to_string())
            .unwrap(),
        PageContent::Text(content) => Response::builder()
            .header("Content-Type", "text/plain;charset=utf-8")
            .body(content.clone())
            .unwrap(),
        PageContent::Html(content) => Response::builder()
            .header("Content-Type", "text/html;charset=utf-8")
            .body(content.clone())
            .unwrap(),
    }
}

// 模型列表处理
async fn handle_models() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "object": "list",
        "data": AVAILABLE_MODELS.to_vec()
    }))
}

// Checksum 处理
async fn handle_checksum() -> Json<serde_json::Value> {
    let checksum = cursor_api::generate_checksum(
        &cursor_api::generate_hash(),
        Some(&cursor_api::generate_hash()),
    );
    Json(serde_json::json!({
        "checksum": checksum
    }))
}

// 更新 TokenInfo 处理
async fn handle_update_tokeninfo(
    State(state): State<Arc<Mutex<AppState>>>,
) -> Json<serde_json::Value> {
    // 重新加载 tokens
    let token_infos = load_tokens();

    // 更新应用状态
    {
        let mut state = state.lock().await;
        state.token_infos = token_infos;
    }

    Json(serde_json::json!({
        "status": "success",
        "message": "Token list has been reloaded"
    }))
}

// 获取 TokenInfo 处理
async fn handle_get_tokeninfo(
    State(_state): State<Arc<Mutex<AppState>>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (auth_token, token_file, token_list_file) = {
        let config = APP_CONFIG.read().unwrap();
        (
            config.auth_token.clone(),
            config.token_file.clone(),
            config.token_list_file.clone(),
        )
    };

    // 验证 AUTH_TOKEN
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if auth_header != auth_token {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 读取文件内容
    let tokens = std::fs::read_to_string(&token_file).unwrap_or_else(|_| String::new());
    let token_list = std::fs::read_to_string(&token_list_file).unwrap_or_else(|_| String::new());

    Ok(Json(serde_json::json!({
        "status": "success",
        "token_file": token_file,
        "token_list_file": token_list_file,
        "tokens": tokens,
        "token_list": token_list
    })))
}

async fn handle_update_tokeninfo_post(
    State(state): State<Arc<Mutex<AppState>>>,
    headers: HeaderMap,
    Json(request): Json<TokenUpdateRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let (auth_token, token_file, token_list_file) = {
        let config = APP_CONFIG.read().unwrap();
        (
            config.auth_token.clone(),
            config.token_file.clone(),
            config.token_list_file.clone(),
        )
    };

    // 验证 AUTH_TOKEN
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if auth_header != auth_token {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 写入 .token 文件
    std::fs::write(&token_file, &request.tokens).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 如果提供了 token_list，则写入
    if let Some(token_list) = request.token_list {
        std::fs::write(&token_list_file, token_list)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    // 重新加载 tokens
    let token_infos = load_tokens();
    let token_infos_len = token_infos.len();

    // 更新应用状态
    {
        let mut state = state.lock().await;
        state.token_infos = token_infos;
    }

    Ok(Json(serde_json::json!({
        "status": "success",
        "message": "Token files have been updated and reloaded",
        "token_file": token_file,
        "token_list_file": token_list_file,
        "token_count": token_infos_len
    })))
}

// 日志处理
async fn handle_logs() -> impl IntoResponse {
    let config = APP_CONFIG.read().unwrap();
    match &config.logs_content {
        PageContent::Default => Response::builder()
            .header("Content-Type", "text/html;charset=utf-8")
            .body(Body::from(
                include_str!("../static/logs.min.html").to_string(),
            ))
            .unwrap(),
        PageContent::Text(content) => Response::builder()
            .header("Content-Type", "text/plain;charset=utf-8")
            .body(Body::from(content.clone()))
            .unwrap(),
        PageContent::Html(content) => Response::builder()
            .header("Content-Type", "text/html;charset=utf-8")
            .body(Body::from(content.clone()))
            .unwrap(),
    }
}

async fn handle_logs_post(
    State(state): State<Arc<Mutex<AppState>>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let auth_token = {
        let config = APP_CONFIG.read().unwrap();
        config.auth_token.clone()
    };

    // 验证 AUTH_TOKEN
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if auth_header != auth_token {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let state = state.lock().await;
    Ok(Json(serde_json::json!({
        "total": state.request_logs.len(),
        "logs": state.request_logs,
        "timestamp": Local::now(),
        "status": "success"
    })))
}

async fn handle_env_example() -> impl IntoResponse {
    Response::builder()
        .header("Content-Type", "text/plain;charset=utf-8")
        .body(include_str!("../.env.example").to_string())
        .unwrap()
}

// 聊天处理函数的签名
async fn handle_chat(
    State(state): State<Arc<Mutex<AppState>>>,
    headers: HeaderMap,
    Json(request): Json<ChatRequest>,
) -> Result<Response<Body>, (StatusCode, Json<serde_json::Value>)> {
    // 验证模型是否支持
    if !AVAILABLE_MODELS.iter().any(|m| m.id == request.model) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ChatError::ModelNotSupported(request.model).to_json()),
        ));
    }

    let request_time = Local::now();

    // 验证请求
    if request.messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ChatError::EmptyMessages.to_json()),
        ));
    }

    // 验证 O1 模型不支持流式输出
    if request.model.starts_with("o1") && request.stream {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ChatError::StreamNotSupported(request.model).to_json()),
        ));
    }

    // 获取并处理认证令牌
    let auth_token = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or((
            StatusCode::UNAUTHORIZED,
            Json(ChatError::Unauthorized.to_json()),
        ))?;

    let auth_token_config = {
        let config = APP_CONFIG.read().unwrap();
        config.auth_token.clone()
    };

    // 验证 AuthToken
    if auth_token != auth_token_config {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ChatError::Unauthorized.to_json()),
        ));
    }

    // 完整的令牌处理逻辑和对应的 checksum
    let (auth_token, checksum, alias) = {
        static CURRENT_KEY_INDEX: AtomicUsize = AtomicUsize::new(0);
        let state_guard = state.lock().await;
        let token_infos = &state_guard.token_infos;

        if token_infos.is_empty() {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ChatError::NoTokens.to_json()),
            ));
        }

        let index = CURRENT_KEY_INDEX.fetch_add(1, Ordering::SeqCst) % token_infos.len();
        let token_info = &token_infos[index];
        (
            token_info.token.clone(),
            token_info.checksum.clone(),
            token_info.alias.clone(),
        )
    };

    // 更新请求日志
    {
        let mut state = state.lock().await;
        state.total_requests += 1;
        state.active_requests += 1;
        state.request_logs.push(RequestLog {
            timestamp: request_time,
            model: request.model.clone(),
            checksum: checksum.clone(),
            auth_token: auth_token.clone(),
            alias: alias.unwrap_or_default(),
            stream: request.stream,
            status: "pending".to_string(),
            error: None,
        });

        if state.request_logs.len() > 100 {
            state.request_logs.remove(0);
        }
    }

    // 将消息转换为hex格式
    let hex_data = cursor_api::encode_chat_message(request.messages, &request.model)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    ChatError::RequestFailed("Failed to encode chat message".to_string()).to_json(),
                ),
            )
        })?;

    // 构建请求客户端
    let client = Client::new();
    let request_id = Uuid::new_v4().to_string();
    let response = client
        .post("https://api2.cursor.sh/aiserver.v1.AiService/StreamChat")
        .header("Content-Type", "application/connect+proto")
        .header("Authorization", format!("Bearer {}", auth_token))
        .header("connect-accept-encoding", "gzip,br")
        .header("connect-protocol-version", "1")
        .header("user-agent", "connect-es/1.6.1")
        .header("x-amzn-trace-id", format!("Root={}", &request_id))
        .header("x-cursor-checksum", &checksum)
        .header("x-cursor-client-version", "0.42.5")
        .header("x-cursor-timezone", "Asia/Shanghai")
        .header("x-ghost-mode", "false")
        .header("x-request-id", &request_id)
        .header("Host", "api2.cursor.sh")
        .body(hex_data)
        .send()
        .await;

    // 处理请求结果
    let response = match response {
        Ok(resp) => {
            // 更新请求日志为成功
            {
                let mut state = state.lock().await;
                state.request_logs.last_mut().unwrap().status = "success".to_string();
            }
            resp
        }
        Err(e) => {
            // 更新请求日志为失败
            {
                let mut state = state.lock().await;
                if let Some(last_log) = state.request_logs.last_mut() {
                    last_log.status = "failed".to_string();
                    last_log.error = Some(e.to_string());
                }
            }
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ChatError::RequestFailed(e.to_string()).to_json()),
            ));
        }
    };

    // 释放活动请求计数
    {
        let mut state = state.lock().await;
        state.active_requests -= 1;
    }

    if request.stream {
        let response_id = format!("chatcmpl-{}", Uuid::new_v4().simple());
        let decode_error_count = Arc::new(AtomicUsize::new(0));
        let full_text = Arc::new(Mutex::new(String::with_capacity(1024)));

        // 克隆用于最终状态检查
        let final_state = state.clone();
        let stream = response.bytes_stream().then(move |chunk| {
            let response_id = response_id.clone();
            let model = request.model.clone();
            let full_text = full_text.clone();
            let decode_error_count = decode_error_count.clone();
            let final_state = final_state.clone();

            async move {
                let chunk = chunk.unwrap_or_default();
                let result = match cursor_api::decode_response(&chunk).await {
                    Ok(text) => {
                        let mut text_guard = full_text.lock().await;
                        text_guard.push_str(&text);

                        let response = ChatResponse {
                            id: response_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: chrono::Utc::now().timestamp(),
                            model: if text.is_empty() {
                                Some(model.clone())
                            } else {
                                None
                            },
                            choices: vec![Choice {
                                index: 0,
                                message: None,
                                delta: Some(Delta {
                                    content: Some(text),
                                }),
                                finish_reason: None,
                            }],
                            usage: None,
                        };

                        Ok::<_, Infallible>(Bytes::from(format!(
                            "data: {}\n\n",
                            serde_json::to_string(&response).unwrap()
                        )))
                    }
                    Err(_) => {
                        let count = decode_error_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if count == 2 && !full_text.lock().await.is_empty() {
                            Ok(Bytes::from("data: [DONE]\n\n"))
                        } else {
                            Ok(Bytes::new())
                        }
                    }
                };

                // 第二个 then 的逻辑
                let error_count = decode_error_count.load(Ordering::SeqCst);
                if error_count == 1 {
                    let text = full_text.lock().await.clone();

                    // 更新请求日志
                    if let Ok(mut state) = final_state.try_lock() {
                        if let Some(last_log) = state.request_logs.last_mut() {
                            last_log.status = "failed".to_string();
                            last_log.error = Some(if text.is_empty() {
                                "Empty response received".to_string()
                            } else {
                                "Incomplete response received".to_string()
                            });
                        }
                    }

                    Ok(Bytes::from("data: [DONE]\n\n"))
                } else {
                    result
                }
            }
        });

        Ok(Response::builder()
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .header("Content-Type", "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap())
    } else {
        // 非流式响应
        let mut full_text = String::with_capacity(1024); // 预分配合适的容量
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        ChatError::RequestFailed(format!("Failed to read response chunk: {}", e))
                            .to_json(),
                    ),
                )
            })?;
            if let Ok(text) = cursor_api::decode_response(&chunk).await {
                full_text.push_str(&text);
            }
        }

        // 处理文本
        full_text = full_text
            .replace(
                regex::Regex::new(r"^.*<\|END_USER\|>").unwrap().as_str(),
                "",
            )
            .replace(regex::Regex::new(r"^\n[a-zA-Z]?").unwrap().as_str(), "")
            .trim()
            .to_string();

        // 检查响应是否为空
        if full_text.is_empty() {
            // 更新请求日志为失败
            {
                let mut state = state.lock().await;
                if let Some(last_log) = state.request_logs.last_mut() {
                    last_log.status = "failed".to_string();
                    last_log.error = Some("Empty response received".to_string());
                }
            }
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ChatError::RequestFailed("Empty response received".to_string()).to_json()),
            ));
        }

        let response_data = ChatResponse {
            id: format!("chatcmpl-{}", Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: Some(request.model),
            choices: vec![Choice {
                index: 0,
                message: Some(Message {
                    role: "assistant".to_string(),
                    content: MessageContent::Text(full_text),
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            }),
        };

        Ok(Response::new(Body::from(
            serde_json::to_string(&response_data).unwrap(),
        )))
    }
}

// 配置页面处理函数
async fn handle_config_page() -> impl IntoResponse {
    let config = APP_CONFIG.read().unwrap();
    match &config.config_content {
        PageContent::Default => Response::builder()
            .header("Content-Type", "text/html;charset=utf-8")
            .body(include_str!("../static/config.min.html").to_string())
            .unwrap(),
        PageContent::Text(content) => Response::builder()
            .header("Content-Type", "text/plain;charset=utf-8")
            .body(content.clone())
            .unwrap(),
        PageContent::Html(content) => Response::builder()
            .header("Content-Type", "text/html;charset=utf-8")
            .body(content.clone())
            .unwrap(),
    }
}

// 配置更新处理函数
async fn handle_config_update(
    State(_state): State<Arc<Mutex<AppState>>>,
    headers: HeaderMap,
    Json(request): Json<ConfigUpdateRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 验证 AUTH_TOKEN
    let auth_token = {
        let config = APP_CONFIG.read().unwrap();
        config.auth_token.clone()
    };

    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "未提供认证令牌"
            })),
        ))?;

    if auth_header != auth_token {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "无效的认证令牌"
            })),
        ));
    }

    let config = APP_CONFIG.read().unwrap();

    match request.action.as_str() {
        "get" => Ok(Json(serde_json::json!({
            "status": "success",
            "data": {
                "root_content": config.root_content,
                "logs_content": config.logs_content,
                "config_content": config.config_content,
                "tokeninfo_content": config.tokeninfo_content,
                "shared_styles_content": config.shared_styles_content,
                "shared_js_content": config.shared_js_content,
                "vision_ability": config.vision_ability,
                "enable_slow_pool": config.enable_slow_pool
            }
        }))),

        "update" => {
            // 处理页面内容更新
            if !request.path.is_empty() && request.content_type.is_some() {
                let content = match request.content_type.as_deref() {
                    Some("default") => PageContent::Default,
                    Some("text") => PageContent::Text(request.content.clone()),
                    Some("html") => PageContent::Html(request.content.clone()),
                    _ => {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(serde_json::json!({
                                "error": "无效的内容类型"
                            })),
                        ))
                    }
                };

                if let Err(e) = config.update_page_content(&request.path, content) {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": format!("更新页面内容失败: {}", e)
                        })),
                    ));
                }
            }

            // 处理 vision_ability 更新
            if let Some(vision_ability) = request.vision_ability {
                if let Err(e) = config.update_vision_ability(vision_ability) {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": format!("更新 vision_ability 失败: {}", e)
                        })),
                    ));
                }
            }

            // 处理 enable_slow_pool 更新
            if let Some(enable_slow_pool) = request.enable_slow_pool {
                if let Err(e) = config.update_slow_pool(enable_slow_pool) {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": format!("更新 enable_slow_pool 失败: {}", e)
                        })),
                    ));
                }
            }

            Ok(Json(serde_json::json!({
                "status": "success",
                "message": "配置已更新"
            })))
        }

        "reset" => {
            // 重置页面内容
            if !request.path.is_empty() {
                if let Err(e) = config.reset_page_content(&request.path) {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": format!("重置页面内容失败: {}", e)
                        })),
                    ));
                }
            }

            // 重置 vision_ability
            if request.vision_ability.is_some() {
                if let Err(e) = config.reset_vision_ability() {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": format!("重置 vision_ability 失败: {}", e)
                        })),
                    ));
                }
            }

            // 重置 enable_slow_pool
            if request.enable_slow_pool.is_some() {
                if let Err(e) = config.reset_slow_pool() {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": format!("重置 enable_slow_pool 失败: {}", e)
                        })),
                    ));
                }
            }

            Ok(Json(serde_json::json!({
                "status": "success",
                "message": "配置已重置"
            })))
        }

        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "无效的操作类型"
            })),
        )),
    }
}

async fn handle_static(Path(path): Path<String>) -> impl IntoResponse {
    let config = APP_CONFIG.read().unwrap();
    match path.as_str() {
        "shared-styles.css" => match &config.shared_styles_content {
            PageContent::Default => Response::builder()
                .header("Content-Type", "text/css;charset=utf-8")
                .body(include_str!("../static/shared-styles.min.css").to_string())
                .unwrap(),
            PageContent::Text(content) | PageContent::Html(content) => Response::builder()
                .header("Content-Type", "text/css;charset=utf-8")
                .body(content.clone())
                .unwrap(),
        },
        "shared.js" => match &config.shared_js_content {
            PageContent::Default => Response::builder()
                .header("Content-Type", "text/javascript;charset=utf-8")
                .body(include_str!("../static/shared.min.js").to_string())
                .unwrap(),
            PageContent::Text(content) | PageContent::Html(content) => Response::builder()
                .header("Content-Type", "text/javascript;charset=utf-8")
                .body(content.clone())
                .unwrap(),
        },
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body("Not found".to_string())
            .unwrap(),
    }
}
