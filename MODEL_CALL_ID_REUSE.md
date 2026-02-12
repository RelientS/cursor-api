# Model Call ID 复用实现说明

## 修改目标
实现 Agent 模式下多次 tool use 共享同一个 model_call_id，以减少 API 计费。

## 修改内容

### 1. 文件：`src/core/stream/decoder.rs`

#### 修改点1：在 Context 结构体中添加 model_call_id 存储字段
**位置：** 第120-127行

```rust
#[derive(Default)]
struct Context {
    raw_args_len: usize,
    processed: u32,
    // 保存第一次收到的 model_call_id，用于后续复用
    saved_model_call_id: Option<ByteStr>,
    // 调试使用
    // counter: u32,
}
```

#### 修改点2：初始化 saved_model_call_id 字段
**位置：** 第153-157行

```rust
context: Context {
    raw_args_len: 0,
    processed: 0,
    saved_model_call_id: None,
    // counter: COUNTER.fetch_add(1, Ordering::SeqCst),
},
```

#### 修改点3：实现 model_call_id 复用逻辑
**位置：** 第355-377行（handle_text_message 函数中）

```rust
Response::ClientSideToolV2Call(mut response) => {
    let mut result = None;
    let mut finish = false;

    // model_call_id 复用逻辑：保存第一个 model_call_id 并在后续调用中复用
    if let Some(ref model_call_id) = response.model_call_id {
        if ctx.saved_model_call_id.is_none() {
            // 第一次收到 model_call_id，保存它
            ctx.saved_model_call_id = Some(model_call_id.clone());
            crate::debug!("保存 model_call_id 用于复用: {}", model_call_id);
        } else {
            // 后续调用：使用保存的 model_call_id 替换当前的
            let saved_id = ctx.saved_model_call_id.as_ref().unwrap();
            crate::debug!(
                "复用 model_call_id: {} -> {}",
                model_call_id,
                saved_id
            );
            response.model_call_id = Some(saved_id.clone());
        }
    }

    // ... 后续处理逻辑
```

## 工作原理

1. **首次 Tool Call**：当收到第一个 `ClientSideToolV2Call` 响应时，从中提取 `model_call_id` 并保存到 `Context.saved_model_call_id` 中。

2. **后续 Tool Calls**：当收到后续的 `ClientSideToolV2Call` 响应时，检测到 `saved_model_call_id` 已存在，则用保存的值替换响应中的 `model_call_id`。

3. **生命周期**：每个 `StreamDecoder` 实例对应一个请求-响应流。在该流的整个生命周期内，所有 tool calls 共享同一个 `model_call_id`。新请求会创建新的 `StreamDecoder`，从而获得新的 `model_call_id`。

## 预期效果

- **修改前**：每次 tool call 都有独立的 model_call_id，导致每次都单独计费
  - 示例：6次调用 = 6次计费

- **修改后**：同一个请求流中的多次 tool calls 共享同一个 model_call_id
  - 示例：3次调用共享1个 model_call_id = 只计费1次

## 调试日志

修改后会在日志中输出以下信息（需要启用 debug 日志）：
- `保存 model_call_id 用于复用: {id}` - 首次保存 model_call_id
- `复用 model_call_id: {old_id} -> {new_id}` - 后续复用已保存的 model_call_id

## 部署

编译后的可执行文件位于：
- 源码目录：`/tmp/cursor-api/target/release/cursor-api`
- 备份位置：`/tmp/cursor-api-official/cursor-api.new`

替换现有版本：
```bash
cd /tmp/cursor-api-official
mv cursor-api cursor-api.old
mv cursor-api.new cursor-api
# 重启服务
```

## 测试建议

1. 启用 debug 日志以观察 model_call_id 的复用情况
2. 使用 Agent 模式发送包含多个 tool use 的请求
3. 检查响应中的 tool_call_id 是否包含相同的 model_call_id 部分
4. 验证 Cursor 计费是否减少

## 技术细节

- **数据结构**：tool_call_id 格式为 `{tool_call_id}\nmc_{model_call_id}`
- **分隔符**：使用 `\nmc_` 作为 tool_call_id 和 model_call_id 的分隔符
- **解析工具**：`ToolId::parse()` 和 `ToolId::format()` 用于 ID 的解析和格式化

---

修改时间：2026-02-10
修改版本：v0.4.0-pre.22
