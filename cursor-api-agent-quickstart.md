# Cursor-API Agent Mode 快速开始指南

## 🚀 5 分钟快速实现

### Step 1: 克隆并理解项目结构

```bash
git clone https://github.com/wisdgod/cursor-api.git
cd cursor-api

# 关键文件位置
tree -L 3 src/core/
# src/core/
# ├── adapter/
# │   ├── traits.rs       ← encode_tool_result 在这里
# │   ├── openai.rs       ← 需要暴露的函数
# │   ├── anthropic.rs    ← 需要暴露的函数
# │   └── utils/
# │       └── tool_id.rs  ← ToolId 解析逻辑
# ├── service.rs          ← 现有的 chat handler
# └── aiserver/
#     └── v1/
#         └── lite.proto  ← Protobuf 定义
```

---

### Step 2: 暴露 encode_tool_result（最小改动）

**文件**：`src/core/adapter/openai.rs`

找到这段代码：
```rust
pub async fn encode_tool_result(
    tool_result: (Option<ToolResultContent>, bool),
    tool_use_id: ByteStr,
    tool_name: ByteStr,
) -> Result<Vec<u8>, AdapterError> {
    // ... 现有实现
}
```

**改动**：确认函数已经是 `pub`（当前版本可能已经是）

如果不是，添加 `pub` 关键字。

---

### Step 3: 创建最简 Agent Session Manager

**新建文件**：`src/core/service/agent_session.rs`

```rust
use std::collections::HashMap;
use uuid::Uuid;
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone)]
pub struct AgentSession {
    pub model_call_id: String,
    pub created_at: i64,
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
        let session = AgentSession {
            model_call_id: model_call_id.clone(),
            created_at: chrono::Utc::now().timestamp(),
        };
        self.sessions.write().insert(model_call_id.clone(), session.clone());
        session
    }
    
    pub fn get_session(&self, id: &str) -> Option<AgentSession> {
        self.sessions.read().get(id).cloned()
    }
}
```

---

### Step 4: 最简化的 Agent Handler

**新建文件**：`src/core/service/agent.rs`

```rust
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct AgentRequest {
    pub model: String,
    pub prompt: String,
    pub max_iterations: Option<u32>,
}

#[derive(Serialize)]
pub struct AgentResponse {
    pub model_call_id: String,
    pub iterations: u32,
    pub result: String,
}

pub async fn handle_agent_chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<AgentRequest>,
) -> Json<AgentResponse> {
    // 1. 创建 session
    let session = state.agent_session_manager().create_session();
    let model_call_id = session.model_call_id.clone();
    
    // 2. 构造 tool_call_id（包含 model_call_id）
    let base_call_id = Uuid::new_v4().to_string();
    let tool_call_id = format!("{}\nmc_{}", base_call_id, model_call_id);
    
    // 3. 循环调用 LLM（共享 model_call_id）
    let max = req.max_iterations.unwrap_or(3);
    for i in 0..max {
        // TODO: 调用 LLM
        // 关键：使用相同的 tool_call_id（包含 model_call_id）
        
        // TODO: 如果有工具调用，使用 encode_tool_result
        
        // TODO: 检查是否完成
    }
    
    Json(AgentResponse {
        model_call_id,
        iterations: max,
        result: "Done".to_string(),
    })
}
```

---

### Step 5: 集成到路由

**文件**：`src/core/route.rs`

```rust
// 添加 mod 声明
mod service {
    pub mod agent;
    pub mod agent_session;
}

// 在路由中添加
.route(
    "/v1/agent/chat",
    post(service::agent::handle_agent_chat)
        .route_layer(middleware::from_fn_with_state(state.clone(), v1_auth_middleware)),
)
```

---

### Step 6: 在 AppState 中添加 Session Manager

**文件**：`src/app/state.rs`

```rust
use crate::core::service::agent_session::AgentSessionManager;

pub struct AppState {
    // ... 现有字段
    agent_session_manager: AgentSessionManager,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            // ... 现有初始化
            agent_session_manager: AgentSessionManager::new(),
        }
    }
    
    pub fn agent_session_manager(&self) -> &AgentSessionManager {
        &self.agent_session_manager
    }
}
```

---

## 🧪 验证测试

### 编译

```bash
cargo build --release
```

### 运行

```bash
cargo run --release
```

### 测试 API

```bash
# 基础测试
curl -X POST http://localhost:3000/v1/agent/chat \
  -H "Authorization: Bearer your-token" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "claude-3.5-sonnet",
    "prompt": "Hello, test agent mode",
    "max_iterations": 3
  }'

# 应该返回：
{
  "model_call_id": "uuid-here",
  "iterations": 3,
  "result": "Done"
}
```

### 验证 Request 计费

1. 记录调用前的 Cursor 用量
2. 执行上面的测试
3. 检查 Cursor 用量是否只增加了 1 request（而不是 3）

---

## 🔍 调试技巧

### 查看 tool_call_id 格式

在代码中添加日志：

```rust
eprintln!("tool_call_id: {}", tool_call_id);
// 应该输出类似：call_abc123\nmc_xyz789
```

### 验证 model_call_id 复用

```rust
eprintln!("Iteration {}: using model_call_id = {}", i, model_call_id);
// 所有迭代应该输出相同的 model_call_id
```

---

## 📝 下一步

### 完整实现需要：

1. ✅ 暴露 `encode_tool_result` ← **你已经完成**
2. ⬜ 实现工具调用执行
3. ⬜ 实现 LLM 响应解析
4. ⬜ 错误处理
5. ⬜ 完整的 session 管理
6. ⬜ 测试和文档

### 参考完整实现

查看 `/home/ubuntu/clawd/cursor-api-agent-implementation-example.rs`

---

## 💡 核心要点

**关键 1**：`tool_call_id` 格式
```
call_abc123\nmc_xyz789
^           ^   ^
|           |   |
单次调用ID  分隔符 模型会话ID（关键！）
```

**关键 2**：复用 `model_call_id`
```rust
// ❌ 错误：每次创建新的
for i in 0..5 {
    let model_call_id = Uuid::new_v4();  // 错误！
}

// ✅ 正确：复用同一个
let model_call_id = Uuid::new_v4();
for i in 0..5 {
    let tool_call_id = format!("call_{}\nmc_{}", Uuid::new_v4(), model_call_id);
    // 正确！model_call_id 保持不变
}
```

**关键 3**：使用 `encode_tool_result`
```rust
use crate::core::adapter::openai::encode_tool_result;

let encoded = encode_tool_result(
    (Some(result), false),
    tool_call_id.into(),  // 包含 model_call_id
    tool_name.into(),
).await?;
```

---

## 🤝 需要帮助？

- **GitHub Issue**: https://github.com/wisdgod/cursor-api/issues/37
- **完整文档**: `/home/ubuntu/clawd/cursor-api-agent-analysis.md`
- **示例代码**: `/home/ubuntu/clawd/cursor-api-agent-implementation-example.rs`

---

**最后检查清单**：
- [ ] `encode_tool_result` 是否 public？
- [ ] `tool_call_id` 格式是否正确（包含 `\nmc_`）？
- [ ] 所有迭代是否复用同一个 `model_call_id`？
- [ ] 测试时 Cursor 用量是否只增加 1 request？

如果以上全部 ✅，恭喜你成功实现了 Agent 模式！🎉
