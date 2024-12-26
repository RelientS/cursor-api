use super::message::Message;
use chrono::{DateTime, Local};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;

// 页面内容类型枚举
#[derive(Clone, Serialize)]
#[serde(tag = "type", content = "content")]
pub enum PageContent {
    Default,      // 默认行为
    Text(String), // 纯文本
    Html(String), // HTML 内容
}

impl Default for PageContent {
    fn default() -> Self {
        Self::Default
    }
}

// 静态配置
#[derive(Clone)]
pub struct AppConfig {
    pub vision_ability: String,
    pub enable_slow_pool: Option<bool>,
    pub auth_token: String,
    pub token_file: String,
    pub token_list_file: String,
    pub route_prefix: String,
    pub version: String,
    pub start_time: chrono::DateTime<chrono::Local>,
    pub root_content: PageContent,
    pub logs_content: PageContent,
    pub config_content: PageContent,
    pub tokeninfo_content: PageContent,
    pub shared_styles_content: PageContent,
    pub shared_js_content: PageContent,
}

// 运行时状态
pub struct AppState {
    pub total_requests: u64,
    pub active_requests: u64,
    pub request_logs: Vec<RequestLog>,
    pub token_infos: Vec<TokenInfo>,
}

// 全局配置实例
lazy_static! {
    pub static ref APP_CONFIG: RwLock<AppConfig> = RwLock::new(AppConfig::default());
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            vision_ability: "base64".to_string(),
            enable_slow_pool: None,
            auth_token: String::new(),
            token_file: ".token".to_string(),
            token_list_file: ".token-list".to_string(),
            route_prefix: String::new(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            start_time: chrono::Local::now(),
            root_content: PageContent::Default,
            logs_content: PageContent::Default,
            config_content: PageContent::Default,
            tokeninfo_content: PageContent::Default,
            shared_styles_content: PageContent::Default,
            shared_js_content: PageContent::Default,
        }
    }
}

impl AppConfig {
    pub fn init(
        vision_ability: String,
        enable_slow_pool: Option<bool>,
        auth_token: String,
        token_file: String,
        token_list_file: String,
        route_prefix: String,
    ) {
        if let Ok(mut config) = APP_CONFIG.write() {
            config.vision_ability = vision_ability;
            config.enable_slow_pool = enable_slow_pool;
            config.auth_token = auth_token;
            config.token_file = token_file;
            config.token_list_file = token_list_file;
            config.route_prefix = route_prefix;
        }
    }

    pub fn update_vision_ability(&self, new_ability: String) -> Result<(), &'static str> {
        if let Ok(mut config) = APP_CONFIG.write() {
            config.vision_ability = new_ability;
            Ok(())
        } else {
            Err("无法更新配置")
        }
    }

    pub fn update_slow_pool(&self, enable: bool) -> Result<(), &'static str> {
        if let Ok(mut config) = APP_CONFIG.write() {
            config.enable_slow_pool = Some(enable);
            Ok(())
        } else {
            Err("无法更新配置")
        }
    }

    pub fn update_page_content(
        &self,
        path: &str,
        content: PageContent,
    ) -> Result<(), &'static str> {
        if let Ok(mut config) = APP_CONFIG.write() {
            match path {
                "/" => config.root_content = content,
                "/logs" => config.logs_content = content,
                "/config" => config.config_content = content,
                "/tokeninfo" => config.tokeninfo_content = content,
                "/static/shared-styles.css" => config.shared_styles_content = content,
                "/static/shared.js" => config.shared_js_content = content,
                _ => return Err("无效的路径"),
            }
            Ok(())
        } else {
            Err("无法更新配置")
        }
    }

    pub fn reset_page_content(&self, path: &str) -> Result<(), &'static str> {
        if let Ok(mut config) = APP_CONFIG.write() {
            match path {
                "/" => config.root_content = PageContent::Default,
                "/logs" => config.logs_content = PageContent::Default,
                "/config" => config.config_content = PageContent::Default,
                "/tokeninfo" => config.tokeninfo_content = PageContent::Default,
                "/static/shared-styles.css" => config.shared_styles_content = PageContent::Default,
                "/static/shared.js" => config.shared_js_content = PageContent::Default,
                _ => return Err("无效的路径"),
            }
            Ok(())
        } else {
            Err("无法重置配置")
        }
    }

    pub fn reset_vision_ability(&self) -> Result<(), &'static str> {
        if let Ok(mut config) = APP_CONFIG.write() {
            config.vision_ability = "base64".to_string();
            Ok(())
        } else {
            Err("无法重置配置")
        }
    }

    pub fn reset_slow_pool(&self) -> Result<(), &'static str> {
        if let Ok(mut config) = APP_CONFIG.write() {
            config.enable_slow_pool = None;
            Ok(())
        } else {
            Err("无法重置配置")
        }
    }
}

impl AppState {
    pub fn new(token_infos: Vec<TokenInfo>) -> Self {
        Self {
            total_requests: 0,
            active_requests: 0,
            request_logs: Vec::new(),
            token_infos,
        }
    }

    pub fn update_token_infos(&mut self, token_infos: Vec<TokenInfo>) {
        self.token_infos = token_infos;
    }
}

// 模型定义
#[derive(Serialize, Deserialize, Clone)]
pub struct Model {
    pub id: String,
    pub created: i64,
    pub object: String,
    pub owned_by: String,
}

// 请求日志
#[derive(Serialize, Clone)]
pub struct RequestLog {
    pub timestamp: DateTime<Local>,
    pub model: String,
    pub checksum: String,
    pub auth_token: String,
    pub alias: String,
    pub stream: bool,
    pub status: String,
    pub error: Option<String>,
}

// 聊天请求
#[derive(Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
}

// 用于存储 token 信息
pub struct TokenInfo {
    pub token: String,
    pub checksum: String,
    pub alias: Option<String>,
}

// TokenUpdateRequest 结构体
#[derive(Deserialize)]
pub struct TokenUpdateRequest {
    pub tokens: String,
    #[serde(default)]
    pub token_list: Option<String>,
}

// 添加用于接收更新请求的结构体
#[derive(Deserialize)]
pub struct ConfigUpdateRequest {
    pub action: String, // "get", "update", "reset"
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub content_type: Option<String>, // "default", "text", "html"
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub vision_ability: Option<String>,
    #[serde(default)]
    pub enable_slow_pool: Option<bool>,
}
