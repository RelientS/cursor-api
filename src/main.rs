use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use chrono::{DateTime, Local, Utc};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    LazyLock,
};
use std::{convert::Infallible, sync::Arc};
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

struct AppConfig {
    auth_token: String,
    token_file: String,
    token_list_file: String,
    route_prefix: String,
    version: String,
    start_time: DateTime<Local>,
}

static APP_CONFIG: LazyLock<AppConfig> = LazyLock::new(|| {
    // 加载环境变量
    if let Err(e) = dotenvy::dotenv() {
        eprintln!("警告: 无法加载 .env 文件: {}", e);
    }

    let auth_token = std::env::var("AUTH_TOKEN").unwrap_or_else(|_| "".to_string());
    if auth_token.is_empty() {
        eprintln!("错误: AUTH_TOKEN 未设置");
        std::process::exit(1);
    }

    AppConfig {
        auth_token,
        token_file: std::env::var("TOKEN_FILE").unwrap_or_else(|_| ".token".to_string()),
        token_list_file: std::env::var("TOKEN_LIST_FILE")
            .unwrap_or_else(|_| ".token-list".to_string()),
        route_prefix: std::env::var("ROUTE_PREFIX").unwrap_or_default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        start_time: Local::now(),
    }
});

struct AppState {
    total_requests: u64,
    active_requests: u64,
    request_logs: Vec<RequestLog>,
    token_infos: Vec<TokenInfo>,
}

// 模型定义
#[derive(Serialize, Deserialize, Clone)]
struct Model {
    id: String,
    created: i64,
    object: String,
    owned_by: String,
}

// 请求日志
#[derive(Serialize, Clone)]
struct RequestLog {
    timestamp: DateTime<Local>,
    model: String,
    checksum: String,
    auth_token: String,
    stream: bool,
    status: String,
    error: Option<String>,
}

// 聊天请求
#[derive(Deserialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(default)]
    stream: bool,
}

// 添加用于请求的消息结构体
#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

// 支持的模型列表
mod models;
use models::AVAILABLE_MODELS;

// 用于存储 token 信息
struct TokenInfo {
    token: String,
    checksum: String,
}

// TokenUpdateRequest 结构体
#[derive(Deserialize)]
struct TokenUpdateRequest {
    tokens: String,
    #[serde(default)]
    token_list: Option<String>,
}

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
    // 加载 tokens
    let token_infos = load_tokens();

    // 初始化需要互斥访问的状态
    let state = Arc::new(Mutex::new(AppState {
        total_requests: 0,
        active_requests: 0,
        request_logs: Vec::new(),
        token_infos,
    }));

    // 设置路由
    let app = Router::new()
        .route("/", get(handle_root))
        .route("/tokeninfo", get(handle_tokeninfo_page))
        .route(
            &format!("{}/v1/models", APP_CONFIG.route_prefix),
            get(handle_models),
        )
        .route("/checksum", get(handle_checksum))
        .route("/update-tokeninfo", get(handle_update_tokeninfo))
        .route("/get-tokeninfo", post(handle_get_tokeninfo))
        .route("/update-tokeninfo", post(handle_update_tokeninfo_post))
        .route(
            &format!("{}/v1/chat/completions", APP_CONFIG.route_prefix),
            post(handle_chat),
        )
        .route("/logs", get(handle_logs))
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
    // 读取 .token 文件并解析
    let tokens = match std::fs::read_to_string(&APP_CONFIG.token_file) {
        Ok(content) => {
            let normalized = content.replace("\r\n", "\n");
            // 如果内容被规范化，则更新文件
            if normalized != content {
                if let Err(e) = std::fs::write(&APP_CONFIG.token_file, &normalized) {
                    eprintln!("警告: 无法更新规范化的token文件: {}", e);
                }
            }

            normalized
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    if line.is_empty() {
                        return None;
                    }

                    // 处理 alias::token 格式
                    match line.split("::").collect::<Vec<_>>() {
                        parts if parts.len() == 1 => Some(line.to_string()),
                        parts if parts.len() == 2 => Some(parts[1].to_string()),
                        _ => {
                            eprintln!("警告: 忽略无效的token行: {}", line);
                            None
                        }
                    }
                })
                .collect::<Vec<_>>()
        }
        Err(e) => {
            eprintln!("警告: 无法读取token文件 '{}': {}", APP_CONFIG.token_file, e);
            Vec::new()
        }
    };

    // 读取现有的 token-list
    let mut token_map: std::collections::HashMap<String, String> =
        match std::fs::read_to_string(&APP_CONFIG.token_list_file) {
            Ok(content) => content
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    if line.is_empty() {
                        return None;
                    }

                    let parts: Vec<&str> = line.split(',').collect();
                    match parts[..] {
                        [token, checksum] => Some((token.to_string(), checksum.to_string())),
                        _ => {
                            eprintln!("警告: 忽略无效的token-list行: {}", line);
                            None
                        }
                    }
                })
                .collect(),
            Err(e) => {
                eprintln!("警告: 无法读取token-list文件: {}", e);
                std::collections::HashMap::new()
            }
        };

    // 为新 token 生成 checksum
    for token in tokens {
        if !token_map.contains_key(&token) {
            let checksum = cursor_api::generate_checksum(
                &cursor_api::generate_hash(),
                Some(&cursor_api::generate_hash()),
            );
            token_map.insert(token, checksum);
        }
    }

    // 更新 token-list 文件
    let token_list_content = token_map
        .iter()
        .map(|(token, checksum)| format!("{},{}", token, checksum))
        .collect::<Vec<_>>()
        .join("\n");

    if let Err(e) = std::fs::write(&APP_CONFIG.token_list_file, token_list_content) {
        eprintln!("警告: 无法更新token-list文件: {}", e);
    }

    // 转换为 TokenInfo vector
    token_map
        .into_iter()
        .map(|(token, checksum)| TokenInfo { token, checksum })
        .collect()
}

// 根路由处理
async fn handle_root(State(state): State<Arc<Mutex<AppState>>>) -> Json<serde_json::Value> {
    let state = state.lock().await;
    let uptime = (Local::now() - APP_CONFIG.start_time).num_seconds();

    Json(serde_json::json!({
        "status": "healthy",
        "version": APP_CONFIG.version,
        "uptime": uptime,
        "stats": {
            "started": APP_CONFIG.start_time,
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
            &format!("{}/v1/chat/completions", APP_CONFIG.route_prefix),
            &format!("{}/v1/models", APP_CONFIG.route_prefix),
            "/checksum",
            "/tokeninfo",
            "/update-tokeninfo",
            "/get-tokeninfo"
        ]
    }))
}

async fn handle_tokeninfo_page() -> impl IntoResponse {
    Response::builder()
        .header("Content-Type", "text/html")
        .body(include_str!("../static/tokeninfo.min.html").to_string())
        .unwrap()
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
    // 验证 AUTH_TOKEN
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if auth_header != APP_CONFIG.auth_token {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 读取文件内容
    let tokens = std::fs::read_to_string(&APP_CONFIG.token_file).unwrap_or_else(|_| String::new());
    let token_list =
        std::fs::read_to_string(&APP_CONFIG.token_list_file).unwrap_or_else(|_| String::new());

    Ok(Json(serde_json::json!({
        "status": "success",
        "token_file": APP_CONFIG.token_file,
        "token_list_file": APP_CONFIG.token_list_file,
        "tokens": tokens,
        "token_list": token_list
    })))
}

async fn handle_update_tokeninfo_post(
    State(state): State<Arc<Mutex<AppState>>>,
    headers: HeaderMap,
    Json(request): Json<TokenUpdateRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // 验证 AUTH_TOKEN
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if auth_header != APP_CONFIG.auth_token {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // 写入 .token 文件
    std::fs::write(&APP_CONFIG.token_file, &request.tokens)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 如果提供了 token_list，则写入
    if let Some(token_list) = request.token_list {
        std::fs::write(&APP_CONFIG.token_list_file, token_list)
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
        "token_file": APP_CONFIG.token_file,
        "token_list_file": APP_CONFIG.token_list_file,
        "token_count": token_infos_len
    })))
}

// 日志处理
async fn handle_logs(State(state): State<Arc<Mutex<AppState>>>) -> Json<serde_json::Value> {
    let state = state.lock().await;
    Json(serde_json::json!({
        "total": state.request_logs.len(),
        "logs": state.request_logs,
        "timestamp": Utc::now(),
        "status": "success"
    }))
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
            Json(ChatError::ModelNotSupported(request.model.clone()).to_json()),
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
            Json(ChatError::StreamNotSupported(request.model.clone()).to_json()),
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

    // 验证 AuthToken
    if auth_token != APP_CONFIG.auth_token {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ChatError::Unauthorized.to_json()),
        ));
    }

    // 完整的令牌处理逻辑和对应的 checksum
    let (auth_token, checksum) = {
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
        (token_info.token.clone(), token_info.checksum.clone())
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
            stream: request.stream,
            status: "pending".to_string(),
            error: None,
        });

        if state.request_logs.len() > 100 {
            state.request_logs.remove(0);
        }
    }

    // 消息转换
    let chat_inputs: Vec<cursor_api::ChatInput> = request
        .messages
        .into_iter()
        .map(|m| cursor_api::ChatInput {
            role: m.role,
            content: m.content,
        })
        .collect();

    // 将消息转换为hex格式
    let hex_data = cursor_api::encode_chat_message(chat_inputs, &request.model)
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
        .header("user-agent", "connect-es/1.4.0")
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
                Json(ChatError::RequestFailed(format!("Request failed: {}", e)).to_json()),
            ));
        }
    };

    // 释放活动请求计数
    {
        let mut state = state.lock().await;
        state.active_requests -= 1;
    }

    if request.stream {
        let response_id = format!("chatcmpl-{}", Uuid::new_v4());

        let stream = response.bytes_stream().then(move |chunk| {
            let response_id = response_id.clone();
            let model = request.model.clone();

            async move {
                let chunk = chunk.unwrap_or_default();
                let text = match cursor_api::decode_response(&chunk).await {
                    Ok(text) if text.is_empty() => return Ok(Bytes::from("data: [DONE]\n\n")),
                    Ok(text) => text,
                    Err(_) => return Ok(Bytes::new()),
                };

                let data = serde_json::json!({
                    "id": &response_id,
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": {
                            "content": text
                        }
                    }]
                });

                Ok::<_, Infallible>(Bytes::from(format!("data: {}\n\n", data.to_string())))
            }
        });

        Ok(Response::builder()
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(Body::from_stream(stream))
            .unwrap())
    } else {
        // 非流式响应
        let mut full_text = String::new();
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
            full_text.push_str(
                &cursor_api::decode_response(&chunk)
                    .await
                    .unwrap_or_default(),
            );
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

        let response_data = serde_json::json!({
            "id": format!("chatcmpl-{}", Uuid::new_v4()),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": request.model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": full_text
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 0,
                "completion_tokens": 0,
                "total_tokens": 0
            }
        });

        Ok(Response::new(Body::from(response_data.to_string())))
    }
}
