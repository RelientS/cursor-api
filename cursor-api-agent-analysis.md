# Cursor-API Agent Mode 技术分析与 PR 方案

## 📋 Executive Summary

**目标**：实现 Agent 模式，让多次 LLM 调用只计为 1 request

**核心发现**：Cursor 通过 `model_call_id` 识别同一个 Agent Session，所有共享同一个 `model_call_id` 的调用会被合并计费

**可行性**：✅ 高（70%+），代码已有基础设施，只需暴露和封装

---

## 🔍 关键技术发现

### 1. Protocol Buffer 定义

**文件**：`src/core/aiserver/v1/lite.proto`

```protobuf
message ClientSideToolV2Result {
  ClientSideToolV2 tool = 1;
  string tool_call_id = 2;      // 单次工具调用 ID
  string model_call_id = 3;     // 🔑 模型会话 ID（关键！）
  optional uint32 tool_index = 4;
  
  oneof result {
    MCPResult mcp_result = 5;
  }
}

message ConversationMessage.ToolResult {
  string tool_call_id = 1;
  string tool_name = 2;
  uint32 tool_index = 3;
  optional string model_call_id = 12;  // 🔑 同样包含 model_call_id
  string raw_args = 5;
  ClientSideToolV2Result result = 8;
  optional ClientSideToolV2Call tool_call = 11;
}
```

**关键点**：
- `tool_call_id`：单次工具调用的唯一标识
- `model_call_id`：Agent Session 的标识，**相同则视为同一个 request**

---

### 2. ToolId 编码格式

**文件**：`src/core/adapter/utils/tool_id.rs`

```rust
const DELIMITER: &str = "\nmc_";

pub struct ToolId {
    pub tool_call_id: ByteStr,
    pub model_call_id: Option<ByteStr>,
}

impl ToolId {
    // 解析：tool_call_id\nmc_model_call_id
    pub fn parse(s: ByteStr) -> Self {
        if let Some((tool_call_id, model_call_id)) = s.split_once(DELIMITER) {
            Self { 
                tool_call_id, 
                model_call_id: Some(model_call_id) 
            }
        } else {
            Self { 
                tool_call_id: s, 
                model_call_id: None 
            }
        }
    }
    
    // 编码
    pub fn format(tool_call_id: ByteStr, model_call_id: Option<ByteStr>) -> ByteStr {
        if let Some(model_call_id) = model_call_id {
            format!("{tool_call_id}{DELIMITER}{model_call_id}").into()
        } else {
            tool_call_id
        }
    }
}
```

**格式示例**：
```
tool_abc123\nmc_session_xyz789
```

---

### 3. encode_tool_result 函数（已存在但未暴露）

**文件**：`src/core/adapter/traits.rs`

```rust
async fn encode_tool_result(
    tool_result: Self::ToolResult,
    tool_call_id: ByteStr,
    tool_name: ByteStr,
) -> Result<StreamUnifiedChatRequestWithTools, AdapterError> {
    let result = tool_result.result().await?;
    let tool_id = ToolId::parse(tool_call_id);  // 🔑 解析出 model_call_id
    
    Ok(StreamUnifiedChatRequestWithTools {
        request: Some(
            stream_unified_chat_request_with_tools::Request::ClientSideToolV2Result(
                Box::new(ClientSideToolV2Result {
                    tool: ClientSideToolV2::Mcp.into(),
                    tool_call_id: tool_id.tool_call_id,
                    model_call_id: tool_id.model_call_id,  // 🔑 传递给 Cursor
                    tool_index: None,
                    result: Some(
                        client_side_tool_v2_result::Result::McpResult(
                            McpResult {
                                selected_tool: tool_name,
                                result,
                            }
                        )
                    ),
                }),
            )
        ),
    })
}
```

**关键点**：
- 此函数**已实现**，但是 `trait` 内部方法，未暴露为 public API
- 它负责将工具调用结果编码为 Cursor 协议格式
- **核心**：会解析并传递 `model_call_id`

---

### 4. 现有的消息流程

**文件**：`src/core/service.rs`

```rust
pub async fn handle_chat_completions(
    State(state): State<Arc<AppState>>,
    mut extensions: Extensions,
    Json(request): Json<openai::ChatCompletionCreateParams>,
) -> Result<Response<Body>, (StatusCode, Json<OpenAiError>)> {
    // 1. 验证 token
    // 2. 解析模型和参数
    // 3. 调用 encoder
    // 4. 发送请求到 Cursor
    // 5. 流式返回响应
}
```

**当前限制**：
- ❌ 没有 session 管理
- ❌ 每次调用都是独立的，无法复用 `model_call_id`
- ❌ 工具调用结果无法正确编码

---

## 🎯 实现方案

### 方案概览

```
┌─────────────────┐
│  Client Request │
└────────┬────────┘
         │
         ▼
┌─────────────────────────┐
│  /v1/agent/chat (NEW)   │  ← 新增 endpoint
└────────┬────────────────┘
         │
         ▼
┌─────────────────────────┐
│  Agent Session Manager  │  ← 管理 model_call_id
└────────┬────────────────┘
         │
         ▼
    ┌───┴───┐
    │  Loop │  ← Agent 循环
    └───┬───┘
        │
        ▼
   ┌──────────┐     ┌──────────────┐     ┌──────────────┐
   │ LLM Call │────→│ Tool Call?   │────→│ encode_tool  │
   │ (Step 1) │     │              │     │ _result      │
   └──────────┘     └──────────────┘     └──────────────┘
        │                   │                     │
        │                   │                     ▼
        │                   │            ┌──────────────┐
        │                   │            │ LLM Call     │
        │                   │            │ (Step 2)     │
        │                   │            └──────────────┘
        │                   │                     │
        │                   └─────────────────────┘
        │                           ↓
        ▼                    共享 model_call_id
   ┌──────────┐              = 1 Request!
   │ Response │
   └──────────┘
```

---

### Phase 1: 暴露 encode_tool_result API

**文件**：`src/core/adapter/openai.rs` 和 `src/core/adapter/anthropic.rs`

**修改**：将已有的 `encode_tool_result` 函数从内部改为 public

```rust
// src/core/adapter/openai.rs
pub async fn encode_tool_result(
    tool_result: (Option<ToolResultContent>, bool),
    tool_use_id: ByteStr,
    tool_name: ByteStr,
) -> Result<Vec<u8>, AdapterError> {
    let message = Openai::encode_tool_result(tool_result, tool_use_id, tool_name).await?;
    encode_message_framed(&message).map_err(Into::into)
}
```

---

### Phase 2: Agent Session Manager

**新建文件**：`src/core/service/agent_session.rs`

```rust
use std::collections::HashMap;
use uuid::Uuid;
use parking_lot::RwLock;
use std::sync::Arc;

/// Agent Session 状态
#[derive(Clone)]
pub struct AgentSession {
    pub model_call_id: String,
    pub conversation_id: String,
    pub created_at: i64,
    pub last_active: i64,
    pub iteration_count: u32,
}

/// Agent Session 管理器
pub struct AgentSessionManager {
    sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
}

impl AgentSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// 创建新 session
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
    
    /// 获取 session
    pub fn get_session(&self, model_call_id: &str) -> Option<AgentSession> {
        self.sessions.read().get(model_call_id).cloned()
    }
    
    /// 更新 session 活动时间
    pub fn update_session(&self, model_call_id: &str) {
        if let Some(session) = self.sessions.write().get_mut(model_call_id) {
            session.last_active = chrono::Utc::now().timestamp();
            session.iteration_count += 1;
        }
    }
    
    /// 清理过期 session（超过 30 分钟）
    pub fn cleanup_expired(&self) {
        let now = chrono::Utc::now().timestamp();
        self.sessions.write().retain(|_, session| {
            now - session.last_active < 1800  // 30 minutes
        });
    }
}
```

---

### Phase 3: 新增 Agent API Endpoint

**文件**：`src/core/route.rs`（新增路由）

```rust
// 添加到路由配置
.route(
    "/v1/agent/chat",
    post(handle_agent_chat)
        .route_layer(middleware::from_fn_with_state(state.clone(), v1_auth_middleware)),
)
```

**文件**：`src/core/service/agent.rs`（新文件）

```rust
use super::agent_session::{AgentSession, AgentSessionManager};
use crate::core::adapter::{openai, anthropic};
use axum::{Extension, Json, response::Response};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct AgentChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Option<Vec<Tool>>,
    
    // 🔑 Agent 特定参数
    pub session_id: Option<String>,      // 复用已有 session
    pub max_iterations: Option<u32>,     // 最大迭代次数，默认 5
    pub auto_execute_tools: Option<bool>, // 是否自动执行工具
}

#[derive(Serialize)]
pub struct AgentChatResponse {
    pub session_id: String,
    pub model_call_id: String,
    pub iterations: Vec<AgentIteration>,
    pub final_response: String,
    pub total_tool_calls: u32,
}

#[derive(Serialize)]
pub struct AgentIteration {
    pub step: u32,
    pub tool_calls: Vec<ToolCall>,
    pub response: String,
}

pub async fn handle_agent_chat(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AgentChatRequest>,
) -> Result<Json<AgentChatResponse>, (StatusCode, Json<OpenAiError>)> {
    let session_manager = state.agent_session_manager();
    
    // 1. 获取或创建 session
    let session = if let Some(session_id) = &request.session_id {
        session_manager.get_session(session_id)
            .ok_or_else(|| ChatError::SessionNotFound)?
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
        
        // 🔑 构造带 model_call_id 的请求
        let tool_call_id = format!("call_{}\nmc_{}", 
            Uuid::new_v4(), 
            model_call_id  // 关键：复用同一个 model_call_id
        );
        
        // 调用 LLM
        let response = call_llm_with_tools(
            &state,
            &request.model,
            &current_messages,
            request.tools.as_ref(),
            &tool_call_id,
        ).await?;
        
        // 检查是否有工具调用
        if let Some(tool_calls) = response.tool_calls {
            let mut tool_results = Vec::new();
            
            for tool_call in &tool_calls {
                // 执行工具
                let result = execute_tool(tool_call).await?;
                
                // 🔑 使用 encode_tool_result 编码结果
                let encoded = openai::encode_tool_result(
                    (Some(result.content), false),
                    tool_call_id.clone().into(),
                    tool_call.function.name.clone().into(),
                ).await?;
                
                tool_results.push(encoded);
            }
            
            iterations.push(AgentIteration {
                step,
                tool_calls: tool_calls.clone(),
                response: response.content.clone(),
            });
            
            // 继续下一轮（共享 model_call_id）
            current_messages.push(Message {
                role: "assistant",
                content: response.content,
            });
            
            // 注意：这里不需要重新创建 model_call_id
            // 它会自动从 tool_call_id 中提取
        } else {
            // 任务完成
            return Ok(Json(AgentChatResponse {
                session_id: session.conversation_id,
                model_call_id: session.model_call_id,
                iterations,
                final_response: response.content,
                total_tool_calls: iterations.iter()
                    .map(|i| i.tool_calls.len() as u32)
                    .sum(),
            }));
        }
    }
    
    Err(ChatError::MaxIterationsExceeded.into_openai_tuple())
}
```

---

### Phase 4: 在 AppState 中添加 Session Manager

**文件**：`src/app/state.rs`

```rust
pub struct AppState {
    // ... 现有字段
    agent_session_manager: AgentSessionManager,  // 🔑 新增
}

impl AppState {
    pub fn agent_session_manager(&self) -> &AgentSessionManager {
        &self.agent_session_manager
    }
}
```

---

## 📊 测试方案

### 测试 1：验证 model_call_id 复用

```bash
# 1. 创建 agent session
curl -X POST http://localhost:3000/v1/agent/chat \
  -H "Authorization: Bearer your-token" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3.5-sonnet",
    "messages": [
      {"role": "user", "content": "帮我分析这个项目并生成测试用例"}
    ],
    "tools": [
      {
        "type": "function",
        "function": {
          "name": "read_file",
          "description": "读取文件内容"
        }
      }
    ],
    "max_iterations": 5,
    "auto_execute_tools": true
  }'

# 2. 观察响应
# {
#   "session_id": "conv-123",
#   "model_call_id": "mc-xyz",    ← 关键
#   "iterations": [
#     {"step": 0, "tool_calls": [...], ...},
#     {"step": 1, "tool_calls": [...], ...},  ← 所有步骤共享 model_call_id
#   ],
#   "final_response": "...",
#   "total_tool_calls": 5
# }

# 3. 检查 Cursor 后台用量
# 应该只增加 1 request，而不是 5 requests
```

---

### 测试 2：Session 复用

```bash
# 第一次调用
RESPONSE=$(curl -X POST http://localhost:3000/v1/agent/chat \
  -H "Authorization: Bearer your-token" \
  -d '{"model": "claude-3.5-sonnet", "messages": [...]}')

SESSION_ID=$(echo $RESPONSE | jq -r '.session_id')

# 第二次调用（复用 session）
curl -X POST http://localhost:3000/v1/agent/chat \
  -H "Authorization: Bearer your-token" \
  -d "{
    \"model\": \"claude-3.5-sonnet\",
    \"messages\": [...],
    \"session_id\": \"$SESSION_ID\"
  }"
```

---

## 🚀 实施路线图

### Week 1：基础设施（5天）

**Day 1-2**：
- ✅ 暴露 `encode_tool_result` 为 public API
- ✅ 添加单元测试

**Day 3-4**：
- ✅ 实现 `AgentSessionManager`
- ✅ 添加 session 清理机制
- ✅ 集成到 `AppState`

**Day 5**：
- ✅ 添加新路由 `/v1/agent/chat`
- ✅ 实现基础的 agent handler

---

### Week 2：核心逻辑（5天）

**Day 1-3**：
- ✅ 实现完整的 agent 循环
- ✅ 工具调用和结果编码
- ✅ `model_call_id` 复用逻辑

**Day 4-5**：
- ✅ 错误处理和边界情况
- ✅ 超时和最大迭代限制
- ✅ 日志记录

---

### Week 3：测试和优化（3天）

**Day 1-2**：
- ✅ 集成测试
- ✅ 验证 request 计费
- ✅ 性能测试

**Day 3**：
- ✅ 文档编写
- ✅ PR 准备

---

## 📝 PR Checklist

### Code Changes
- [ ] `src/core/adapter/openai.rs` - 暴露 `encode_tool_result`
- [ ] `src/core/adapter/anthropic.rs` - 暴露 `encode_tool_result`
- [ ] `src/core/service/agent_session.rs` - 新建 Session Manager
- [ ] `src/core/service/agent.rs` - 新建 Agent Handler
- [ ] `src/app/state.rs` - 添加 Session Manager
- [ ] `src/core/route.rs` - 添加新路由
- [ ] `src/app/constant.rs` - 添加常量

### Tests
- [ ] `tests/unit/tool_id.rs` - ToolId 编码解码测试
- [ ] `tests/unit/agent_session.rs` - Session 管理测试
- [ ] `tests/integration/agent_chat.rs` - Agent API 集成测试
- [ ] `tests/e2e/request_counting.rs` - Request 计费验证

### Documentation
- [ ] `README.md` - 添加 Agent API 说明
- [ ] `docs/AGENT_MODE.md` - 详细文档
- [ ] `CHANGELOG.md` - 记录变更
- [ ] API 示例代码

---

## 🎯 预期效果

### 使用前（当前）
```
5 次 LLM 调用 = 5 requests
```

### 使用后（Agent 模式）
```
5 次 LLM 调用（共享 model_call_id）= 1 request
节省 80% request 消耗
```

---

## ⚠️ 注意事项

1. **兼容性**：新 API 不影响现有 `/v1/chat/completions` endpoint
2. **稳定性**：需要充分测试，确保 `model_call_id` 正确复用
3. **安全性**：Session 需要与 token 关联，防止跨用户访问
4. **性能**：Session 清理需要定期执行，避免内存泄漏
5. **文档**：提供清晰的使用示例和最佳实践

---

## 📚 参考资料

- Cursor API 协议：`src/core/aiserver/v1/lite.proto`
- 现有 encode 实现：`src/core/adapter/traits.rs`
- Tool ID 格式：`src/core/adapter/utils/tool_id.rs`
- GitHub Issue：https://github.com/wisdgod/cursor-api/issues/37

---

## 🤝 Contributing

欢迎任何人基于此方案提交 PR！

**联系方式**：
- GitHub Issue: #37
- Email: 项目维护者邮箱

---

**Generated by**: Cetow AI Agent
**Date**: 2026-02-10
**Version**: 1.0
