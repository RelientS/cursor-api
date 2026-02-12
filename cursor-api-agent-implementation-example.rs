// ==========================================
// Cursor-API Agent Mode 实现示例代码
// ==========================================

// ===== 1. Agent Session Manager =====
// 文件：src/core/service/agent_session.rs

use std::collections::HashMap;
use uuid::Uuid;
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct AgentSession {
    pub model_call_id: String,
    pub conversation_id: String,
    pub created_at: i64,
    pub last_active: i64,
    pub iteration_count: u32,
}

pub struct AgentSessionManager {
    sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
}

impl AgentSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn create_session(&self) -> AgentSession {
        let model_call_id = Uuid::new_v4().to_string();
        let conversation_id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        
        let session = AgentSession {
            model_call_id: model_call_id.clone(),
            conversation_id,
            created_at: now,
            last_active: now,
            iteration_count: 0,
        };
        
        self.sessions.write().insert(model_call_id.clone(), session.clone());
        session
    }
    
    pub fn get_session(&self, model_call_id: &str) -> Option<AgentSession> {
        self.sessions.read().get(model_call_id).cloned()
    }
    
    pub fn update_session(&self, model_call_id: &str) {
        if let Some(session) = self.sessions.write().get_mut(model_call_id) {
            session.last_active = chrono::Utc::now().timestamp();
            session.iteration_count += 1;
        }
    }
    
    pub fn cleanup_expired(&self) {
        let now = chrono::Utc::now().timestamp();
        self.sessions.write().retain(|_, session| {
            now - session.last_active < 1800  // 30 minutes
        });
    }
}

// ===== 2. 暴露 encode_tool_result =====
// 文件：src/core/adapter/openai.rs（在现有基础上修改）

use super::{AdapterError, ToolResultContent};
use crate::common::utils::proto_encode::encode_message_framed;
use byte_str::ByteStr;

/// 🔑 将此函数从 trait 内部改为 public
pub async fn encode_tool_result(
    tool_result: (Option<ToolResultContent>, bool),
    tool_call_id: ByteStr,  // 格式：tool_id\nmc_model_call_id
    tool_name: ByteStr,
) -> Result<Vec<u8>, AdapterError> {
    let message = Openai::encode_tool_result(tool_result, tool_call_id, tool_name).await?;
    encode_message_framed(&message).map_err(Into::into)
}

// ===== 3. Agent Chat Handler =====
// 文件：src/core/service/agent.rs（新建）

use axum::{
    extract::{State, Json},
    response::Response,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::sync::Arc;
use super::agent_session::AgentSessionManager;

#[derive(Deserialize)]
pub struct AgentChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<Tool>>,
    
    // Agent 特定参数
    pub session_id: Option<String>,
    pub max_iterations: Option<u32>,
    pub auto_execute_tools: Option<bool>,
}

#[derive(Serialize)]
pub struct AgentChatResponse {
    pub session_id: String,
    pub model_call_id: String,
    pub iterations: Vec<AgentIteration>,
    pub final_response: String,
    pub total_tool_calls: u32,
}

#[derive(Serialize, Clone)]
pub struct AgentIteration {
    pub step: u32,
    pub tool_calls: Vec<ToolCall>,
    pub response: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize, Clone)]
pub struct Tool {
    pub r#type: String,
    pub function: ToolFunction,
}

#[derive(Deserialize, Clone)]
pub struct ToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Serialize, Clone)]
pub struct ToolCall {
    pub id: String,
    pub function: ToolCallFunction,
}

#[derive(Serialize, Clone)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

pub async fn handle_agent_chat(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AgentChatRequest>,
) -> Result<Json<AgentChatResponse>, (StatusCode, Json<serde_json::Value>)> {
    let session_manager = state.agent_session_manager();
    
    // 1. 获取或创建 session
    let session = if let Some(session_id) = &request.session_id {
        session_manager
            .get_session(session_id)
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"error": "Session not found"})),
                )
            })?
    } else {
        session_manager.create_session()
    };
    
    let model_call_id = session.model_call_id.clone();
    let max_iterations = request.max_iterations.unwrap_or(5);
    
    let mut iterations = Vec::new();
    let mut current_messages = request.messages.clone();
    
    // 2. Agent 循环
    for step in 0..max_iterations {
        session_manager.update_session(&model_call_id);
        
        // 🔑 构造带 model_call_id 的 tool_call_id
        let tool_call_id_prefix = format!("call_{}", Uuid::new_v4());
        let tool_call_id = format!("{}\nmc_{}", tool_call_id_prefix, model_call_id);
        
        // 调用 LLM
        let response = call_llm_with_tools(
            &state,
            &request.model,
            &current_messages,
            request.tools.as_ref(),
            &tool_call_id,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
        })?;
        
        // 检查是否有工具调用
        if let Some(tool_calls) = response.tool_calls {
            let mut tool_results = Vec::new();
            
            for tool_call in &tool_calls {
                // 执行工具
                let result = execute_tool(tool_call)
                    .await
                    .map_err(|e| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({"error": e.to_string()})),
                        )
                    })?;
                
                // 🔑 使用 encode_tool_result 编码结果
                // 注意：这里的 tool_call_id 包含 model_call_id
                let encoded = crate::core::adapter::openai::encode_tool_result(
                    (Some(result.content.clone()), false),
                    tool_call_id.clone().into(),
                    tool_call.function.name.clone().into(),
                )
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": e.to_string()})),
                    )
                })?;
                
                tool_results.push(encoded);
            }
            
            iterations.push(AgentIteration {
                step,
                tool_calls: tool_calls.clone(),
                response: response.content.clone(),
            });
            
            // 添加 assistant 消息
            current_messages.push(Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
            });
            
            // 添加工具结果消息
            for (tool_call, result) in tool_calls.iter().zip(tool_results.iter()) {
                current_messages.push(Message {
                    role: "tool".to_string(),
                    content: format!("Tool result: {:?}", result),
                });
            }
            
            // 继续下一轮（自动复用 model_call_id）
        } else {
            // 任务完成
            return Ok(Json(AgentChatResponse {
                session_id: session.conversation_id,
                model_call_id: session.model_call_id,
                iterations,
                final_response: response.content,
                total_tool_calls: iterations
                    .iter()
                    .map(|i| i.tool_calls.len() as u32)
                    .sum(),
            }));
        }
    }
    
    Err((
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": "Max iterations exceeded",
            "iterations": iterations
        })),
    ))
}

// ===== 辅助函数 =====

struct LLMResponse {
    content: String,
    tool_calls: Option<Vec<ToolCall>>,
}

struct ToolResult {
    content: String,
}

async fn call_llm_with_tools(
    state: &Arc<AppState>,
    model: &str,
    messages: &[Message],
    tools: Option<&Vec<Tool>>,
    tool_call_id: &str,
) -> Result<LLMResponse, Box<dyn std::error::Error>> {
    // 🔑 关键：这里构造的请求会包含 tool_call_id
    // Cursor 后端会从中提取 model_call_id
    
    // TODO: 实际实现需要调用现有的 LLM 接口
    // 并确保 tool_call_id 被正确传递
    
    unimplemented!("调用现有的 LLM API")
}

async fn execute_tool(tool_call: &ToolCall) -> Result<ToolResult, Box<dyn std::error::Error>> {
    // TODO: 实际执行工具调用
    // 这里可以是读文件、执行命令等
    
    Ok(ToolResult {
        content: format!("Tool {} executed", tool_call.function.name),
    })
}

// ===== 4. 集成到 AppState =====
// 文件：src/app/state.rs（修改现有）

use super::service::agent_session::AgentSessionManager;

pub struct AppState {
    // ... 现有字段 ...
    
    // 🔑 新增 Agent Session Manager
    agent_session_manager: AgentSessionManager,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            // ... 现有初始化 ...
            agent_session_manager: AgentSessionManager::new(),
        }
    }
    
    pub fn agent_session_manager(&self) -> &AgentSessionManager {
        &self.agent_session_manager
    }
}

// ===== 5. 添加路由 =====
// 文件：src/core/route.rs（修改现有）

use axum::{routing::post, Router};
use super::service::agent::handle_agent_chat;

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // ... 现有路由 ...
        
        // 🔑 新增 Agent API
        .route(
            "/v1/agent/chat",
            post(handle_agent_chat)
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    v1_auth_middleware,
                )),
        )
        
        // ... 其他路由 ...
        .with_state(state)
}

// ==========================================
// 使用示例
// ==========================================

/*
# 创建 Agent Session 并执行任务
curl -X POST http://localhost:3000/v1/agent/chat \
  -H "Authorization: Bearer your-token" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3.5-sonnet",
    "messages": [
      {
        "role": "user",
        "content": "帮我分析项目并生成测试用例"
      }
    ],
    "tools": [
      {
        "type": "function",
        "function": {
          "name": "read_file",
          "description": "读取文件内容",
          "parameters": {
            "type": "object",
            "properties": {
              "path": {
                "type": "string",
                "description": "文件路径"
              }
            }
          }
        }
      }
    ],
    "max_iterations": 5,
    "auto_execute_tools": true
  }'

# 响应示例：
{
  "session_id": "conv-abc-123",
  "model_call_id": "mc-xyz-789",  // ← 关键：所有迭代共享此 ID
  "iterations": [
    {
      "step": 0,
      "tool_calls": [
        {
          "id": "call_1",
          "function": {
            "name": "read_file",
            "arguments": "{\"path\": \"src/main.rs\"}"
          }
        }
      ],
      "response": "我需要先读取主文件..."
    },
    {
      "step": 1,
      "tool_calls": [...],
      "response": "分析完成，开始生成测试..."
    }
  ],
  "final_response": "已为项目生成完整测试用例",
  "total_tool_calls": 5
}

# 🎯 关键：Cursor 后台只会计为 1 request！
*/
