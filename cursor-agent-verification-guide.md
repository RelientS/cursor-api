# Cursor Agent Mode 验证指南

## 📋 验证流程概览

```
┌─────────────────────────────────────────────────────┐
│  Phase 1: 本地验证（今天完成）                        │
├─────────────────────────────────────────────────────┤
│  1. 准备 cursor-api 环境                            │
│  2. 运行 Python 测试脚本                            │
│  3. 对比 Cursor 后台用量                            │
│  4. 确认 model_call_id 复用是否生效                 │
└─────────────────────────────────────────────────────┘
                    ↓
           验证结果判断
                    ↓
        ┌───────────┴───────────┐
        ↓                       ↓
    ✅ 生效                  ❌ 不生效
        │                       │
        ↓                       ↓
┌───────────────────┐   ┌────────────────────┐
│ Phase 2: 实现 PR  │   │ 调整方案或等官方   │
│ • 修改源码        │   │ • 尝试其他方法     │
│ • 添加测试        │   │ • 或等作者实现     │
│ • 提交 PR         │   └────────────────────┘
└───────────────────┘
```

---

## 🚀 Phase 1: 本地验证

### Step 1: 环境准备

#### 1.1 确保 cursor-api 运行

```bash
# 如果还没安装
cd /tmp
git clone https://github.com/wisdgod/cursor-api.git
cd cursor-api

# 编译运行
cargo build --release
cargo run --release

# 默认监听 http://localhost:3000
```

#### 1.2 准备测试脚本

```bash
cd /home/ubuntu/clawd

# 脚本已生成
chmod +x cursor-api-agent-test.py

# 安装依赖（如果需要）
pip3 install requests
```

---

### Step 2: 记录初始用量

**在运行测试前，先记录 Cursor 当前用量！**

1. 打开 Cursor 用量页面：https://www.cursor.com/settings
2. 记录当前 Request 用量（例如：450/500）
3. 截图保存

---

### Step 3: 运行测试脚本

```bash
cd /home/ubuntu/clawd
python3 cursor-api-agent-test.py
```

**交互式输入：**
```
请输入 cursor-api 地址: http://localhost:3000
请输入 API Token: sk-xxxxx（你的 cursor-api token）
测试迭代次数: 3
```

**测试流程：**
```
1️⃣ 记录初始用量
2️⃣ 测试 1: 传统模式（3次独立调用）
   - 暂停 30 秒
3️⃣ 测试 2: Agent 模式（3次调用，共享 model_call_id）
   - 暂停 30 秒
4️⃣ 生成对比报告
```

---

### Step 4: 验证结果

#### 4.1 检查测试输出

测试完成后会显示：

```
📊 测试结果总结
==================

传统模式:
  调用次数: 3
  预期消耗: 3 requests

Agent 模式:
  调用次数: 3
  预期消耗: 1 request (如果生效)
```

#### 4.2 检查 Cursor 后台

**关键步骤：**

1. 刷新 https://www.cursor.com/settings
2. 查看 Request 用量变化

**判断标准：**

| 场景 | 用量变化 | 结论 |
|------|---------|------|
| **成功** | +4 requests (3+1) | ✅ model_call_id 复用生效 |
| **失败** | +6 requests (3+3) | ❌ 没有复用，还是独立计费 |

---

### Step 5: 分析测试结果

#### 场景 A：✅ 验证成功（+4 requests）

**说明**：
- 传统模式：3 次调用 = 3 requests ✅
- Agent 模式：3 次调用 = 1 request ✅
- **model_call_id 复用生效！**

**下一步**：
→ 进入 Phase 2，实现 PR

---

#### 场景 B：❌ 验证失败（+6 requests）

**说明**：
- 传统模式：3 次调用 = 3 requests
- Agent 模式：3 次调用 = 3 requests
- **model_call_id 复用无效**

**可能原因：**

1. **Headers 不起作用**
   - Cursor 后端不识别自定义 headers

2. **需要特殊的请求格式**
   - 必须使用 Protobuf 编码
   - 不能直接通过 REST API

3. **需要修改源码**
   - 外部 wrapper 无法实现
   - 必须在 cursor-api 内部处理

**调整方案：**

##### 方案 1：尝试 Protobuf 直接调用

```python
# 使用 Protobuf 而不是 REST API
# 需要实现 gRPC 客户端
```

##### 方案 2：直接修改 cursor-api 源码

→ 跳过外部验证，直接进入 Phase 2

##### 方案 3：等待官方实现

→ 联系作者，询问进度

---

## 🔧 Phase 2: 实现 PR（如果验证成功）

### 前置条件

- ✅ Phase 1 验证成功
- ✅ 熟悉 Rust 基础
- ✅ 理解 cursor-api 代码结构

### 实施步骤

#### Step 1: Fork 项目

```bash
# 1. 在 GitHub fork wisdgod/cursor-api
# 2. Clone 你的 fork
git clone https://github.com/YOUR_USERNAME/cursor-api.git
cd cursor-api
git checkout -b feature/agent-mode
```

---

#### Step 2: 实现核心代码

**参考文档：**
- `/home/ubuntu/clawd/cursor-api-agent-analysis.md`
- `/home/ubuntu/clawd/cursor-api-agent-implementation-example.rs`
- `/home/ubuntu/clawd/cursor-api-agent-quickstart.md`

**修改文件清单：**

```
src/
├── core/
│   ├── adapter/
│   │   ├── openai.rs          ← 暴露 encode_tool_result
│   │   └── anthropic.rs       ← 暴露 encode_tool_result
│   ├── service/
│   │   ├── agent_session.rs   ← 新建：Session Manager
│   │   └── agent.rs           ← 新建：Agent Handler
│   └── route.rs               ← 添加路由
└── app/
    └── state.rs               ← 集成 Session Manager
```

---

#### Step 3: 添加测试

```bash
# 创建测试文件
mkdir -p tests/integration
touch tests/integration/agent_mode_test.rs
```

**测试内容：**
```rust
#[tokio::test]
async fn test_agent_mode_request_counting() {
    // 1. 创建 agent session
    // 2. 执行多次调用
    // 3. 验证 model_call_id 复用
    // 4. 模拟验证 request 计数
}
```

---

#### Step 4: 运行本地测试

```bash
# 编译
cargo build --release

# 运行测试
cargo test --release

# 运行 API 服务
cargo run --release
```

**验证：**
```bash
# 使用之前的 Python 脚本测试新的 /v1/agent/chat endpoint
curl -X POST http://localhost:3000/v1/agent/chat \
  -H "Authorization: Bearer your-token" \
  -d '{
    "model": "claude-3.5-sonnet",
    "messages": [...],
    "max_iterations": 5
  }'
```

---

#### Step 5: 编写文档

**创建：** `docs/AGENT_MODE.md`

```markdown
# Agent Mode 使用指南

## 简介
Agent Mode 允许多次 LLM 调用只计为 1 request。

## 使用方法
...

## API 参考
...

## 示例
...
```

---

#### Step 6: 提交 PR

```bash
# 提交代码
git add .
git commit -m "feat: Add Agent Mode with model_call_id reuse"
git push origin feature/agent-mode

# 在 GitHub 创建 PR
# 标题：feat: Add Agent Mode to reduce request consumption
# 描述：参考 issue #37，实现基于 model_call_id 的请求复用
```

**PR 描述模板：**

```markdown
## Summary
Implements Agent Mode to allow multiple LLM calls to count as 1 request by reusing `model_call_id`.

## Motivation
Addresses #37 - Users want to reduce request consumption when using cursor-api with tools/agents.

## Implementation
- Exposed `encode_tool_result` as public API
- Added `AgentSessionManager` for session management
- Created new `/v1/agent/chat` endpoint
- All iterations within an agent session share the same `model_call_id`

## Testing
- [x] Unit tests for `AgentSessionManager`
- [x] Integration tests for agent chat flow
- [x] Manual verification of request counting

## Documentation
- Added `docs/AGENT_MODE.md`
- Updated README.md with agent mode example

## Breaking Changes
None - New feature, backward compatible.

## Verification Results
Tested with Python script:
- Traditional mode: 5 calls = 5 requests
- Agent mode: 5 calls = 1 request ✅

See attached test results.
```

---

## 📊 验证检查清单

### Phase 1: 本地验证
- [ ] cursor-api 正常运行
- [ ] 记录初始 Cursor 用量
- [ ] 运行测试脚本
- [ ] 记录测试后用量
- [ ] 对比用量变化
- [ ] 确认 model_call_id 复用效果

### Phase 2: 实现 PR（如果验证成功）
- [ ] Fork 项目
- [ ] 创建功能分支
- [ ] 实现核心代码
- [ ] 添加单元测试
- [ ] 添加集成测试
- [ ] 运行所有测试
- [ ] 编写文档
- [ ] 提交 PR

---

## 💡 关键调试技巧

### 查看实际发送的请求

在测试脚本中添加：

```python
import logging
logging.basicConfig(level=logging.DEBUG)
```

### 验证 tool_call_id 格式

```python
tool_call_id = f"call_{uuid.uuid4()}\nmc_{model_call_id}"
print(f"tool_call_id length: {len(tool_call_id)}")
print(f"contains delimiter: {'\nmc_' in tool_call_id}")
```

### 检查 Cursor 响应

```python
response = self.session.post(...)
print(f"Response headers: {response.headers}")
print(f"Response body: {response.text[:500]}")
```

---

## ⚠️ 常见问题

### Q1: 测试脚本报 401 错误？

**A**: 检查 API Token 是否正确

```bash
# 测试 token
curl -H "Authorization: Bearer your-token" \
  http://localhost:3000/v1/models
```

### Q2: 用量没有变化？

**A**: 
1. 等待 1-2 分钟让 Cursor 后台更新
2. 清除浏览器缓存刷新页面
3. 检查是否使用了正确的账号

### Q3: 验证失败怎么办？

**A**: 
1. 查看 `/home/ubuntu/clawd/cursor-agent-verification-guide.md` 的"场景 B"部分
2. 尝试调整方案
3. 或者直接进入 Phase 2（修改源码）

---

## 📞 需要帮助？

- **测试遇到问题**：把错误日志发给我
- **不确定验证结果**：把 Cursor 用量截图发给我
- **准备开始 Phase 2**：我会提供详细的代码实现指导

---

**当前状态**：
✅ 测试脚本已生成
⏳ 等待运行验证

**下一步**：
```bash
cd /home/ubuntu/clawd
python3 cursor-api-agent-test.py
```

开始测试吧！🚀
