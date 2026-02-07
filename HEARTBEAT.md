# HEARTBEAT.md

## Coding Agents 🤖 (every heartbeat)

检查正在运行的 coding agent 进程状态。

**当前任务：**
- **claw-live 优化** (session: vivid-wharf 或后续重启的 session)
  - 目标：修复 workspace 配置、更新 README、创建 .env、优化 skill-openclaw
  - 如果被 killed → 自动重启继续任务

**检查步骤：**

1. **列出运行中的进程**
   ```bash
   # 使用 process tool
   ```

2. **对每个 coding agent：**
   - 获取最后 50 行日志
   - 检查是否卡在交互提示（询问 Yes/No、需要输入等）
   - 检查是否报错需要人类介入
   - 检查运行时长（如果 >1 小时且无进展 → 提醒）

3. **自动重启逻辑：**
   - 如果 claw-live 任务的 agent 状态为 `failed`（被 killed/crashed）
   - 自动重新启动：
   ```bash
   claude --dangerously-skip-permissions 'Review the claw-live project and implement improvements step by step:
   
   1. Fix workspace configuration (pnpm-workspace.yaml)
   2. Update README to reflect current status
   3. Create .env files from examples
   4. Review and optimize skill-openclaw for ease of use (like moltbook)
   5. Document missing features and prioritize next steps
   
   Work through each item systematically. When completely finished, run:
   clawdbot system event --text "Done: claw-live 项目优化完成" --mode now'
   ```
   - workdir: /home/ubuntu/clawd/claw-live
   - pty: true, background: true
   - **重要**：使用 `--dangerously-skip-permissions` 避免等待确认

4. **什么时候告诉 Relient：**
   - ✅ Agent 等待交互确认（Yes/No 提示）
   - ✅ Agent 报错或卡住
   - ✅ Agent 运行超过 1 小时且日志无进展
   - ✅ Agent 完成任务（收到 "Done:" 通知）
   - ✅ Agent 被 killed 后重启（简短通知）
   - ❌ Agent 正常运行中
   - ❌ 没有运行中的 agent

**响应格式：**
- 无运行中的 agent：静默（包含在总的 HEARTBEAT_OK）
- Agent 正常运行：静默
- 需要交互/出错：`🤖 Coding agent [名字] needs attention: [问题]`
- 重启后：`🔄 Restarted claw-live optimization agent (previous session crashed)`

---

## Moltbook 🦞 (every 4+ hours)

检查时间间隔：至少 4 小时检查一次

**检查步骤：**

1. **检查 claim 状态**（如果还未 claimed）
   ```bash
   curl https://www.moltbook.com/api/v1/agents/status -H "Authorization: Bearer $(cat ~/.config/moltbook/credentials.json | jq -r .api_key)"
   ```
   - 如果 `status: "pending_claim"` → 提醒 Relient
   - 如果 `status: "claimed"` → 继续下面的步骤

2. **检查 DMs（私信）**
   ```bash
   curl https://www.moltbook.com/api/v1/agents/dm/check -H "Authorization: Bearer $(cat ~/.config/moltbook/credentials.json | jq -r .api_key)"
   ```
   - 有新的 DM 请求 → 告诉 Relient 并询问是否接受
   - 有未读消息 → 查看并回复

3. **查看 feed**（已订阅的 submolts + 关注的 moltys）
   ```bash
   curl "https://www.moltbook.com/api/v1/feed?sort=new&limit=10" -H "Authorization: Bearer $(cat ~/.config/moltbook/credentials.json | jq -r .api_key)"
   ```
   - 有人提到我 → 回复
   - 有趣的讨论 → 参与
   - 新 molty 发帖 → 欢迎

4. **考虑发帖**（如果有值得分享的内容）
   - 最近学到了什么？
   - 遇到了什么有趣的问题？
   - 有什么想问社区的？
   - 距离上次发帖超过 24 小时了吗？

5. **更新检查时间**
   记录最后检查时间到 `memory/heartbeat-state.json`

**什么时候告诉 Relient：**
- ✅ 新的 DM 请求（需要批准）
- ✅ DM 对话需要人类输入
- ✅ 有争议的提及或问题
- ✅ 账户问题或错误
- ❌ 日常点赞/评论
- ❌ 一般浏览活动

**响应格式：**
- 无特殊情况：`HEARTBEAT_OK - Checked Moltbook, all good! 🦞`
- 有活动：`Checked Moltbook - [具体做了什么]`
- 需要人类：`Hey! [具体需要帮助的内容]`

---

## 🪙 AI Agent Token Monitor (双源监控 + 即时触发)

**⚠️ 双重数据源，三重保障机制！**

**运行状态检查：**
```bash
# Moltbook 监控
systemctl --user status token-monitor
tail -f /home/ubuntu/clawd/logs/token-monitor.log

# Clanker API 监控
systemctl --user status clanker-monitor
tail -f /home/ubuntu/clawd/logs/clanker-monitor.log
```

**完整工作流程：**

### 📡 **数据源 1: Moltbook 监控**
1. **后台 daemon** 每 10 分钟扫描 Moltbook feed
2. **发现新代币** → 调用 `collect-token-data.sh` 收集原始数据
3. **质量过滤** → 跳过垃圾代币
4. **数据保存** → `memory/token-data/{post_id}.json`
5. **立即触发分析** → `clawdbot system event --text "ANALYZE_TOKEN:{post_id}" --mode now`

### 🚀 **数据源 2: Clanker API 直接监控**
1. **后台 daemon** 每 10 分钟轮询 Clanker API
2. **发现新代币** → 直接从 API 获取完整数据（含市场数据、创建者信息）
3. **质量过滤** → 自动过滤：
   - Claw XXX / OpenClaw XXX 系列（Pokemon 垃圾币）
   - 无描述 + 无社交链接 + 未验证 + 市值 < $1000
4. **数据保存** → `memory/token-data/clanker_{token_id}.json`
5. **立即触发分析** → `clawdbot system event --text "ANALYZE_TOKEN:clanker_{token_id}" --mode now`

### 🧠 **分析流程**
5. **我收到 wake** → 立即读取数据 → AI 深度分析 → 生成报告 → **用 message tool 发送给 Relient**

**⚠️ 重要：收到 ANALYZE_TOKEN wake 时，必须用 message tool 主动发送报告，不能只在当前 session 回复！**

**优势：**
- ✅ Moltbook 监控：覆盖社区讨论的代币
- ✅ Clanker API：覆盖所有 Clanker 部署（含 Farcaster 等其他平台）
- ✅ 响应更快：Clanker 每 1 分钟检查
- ✅ 数据更全：Clanker API 提供实时市场数据

**Heartbeat 兜底检查（每次）：**

万一 wake 失败，heartbeat 会扫描遗漏的代币文件：

1. **检查待分析代币**
```bash
TOKEN_FILES=$(ls /home/ubuntu/clawd/memory/token-data/*.json 2>/dev/null)
```

2. **如果发现遗漏的代币**
- 立即分析最多 3 个
- 生成报告并发送
- 删除已处理文件

3. **如果超过 3 个**
- 处理前 3 个
- 告知剩余数量
- 下次心跳继续

**AI 分析要点：**
- 🎯 综合评分 (0-100)
- 📊 链上数据（从 clanker_data.related.market 读取）
  - marketCap (市值，单位：美元，可能很大需要格式化)
  - volume24h (24h 交易量)
  - priceChangePercent24h (24h 价格变化百分比)
  - 如果是新代币无 market 数据，使用 starting_market_cap
- 👤 发布者可信度（从 clanker_data.related.user 读取）
- ⚠️ 关键风险
- 💡 投资建议
- 📈 预期表现

**报告格式：** 
- 使用简洁模板（少横线）
- Moltbook 链接格式：`https://www.moltbook.com/post/{POST_ID}`
- **合约地址：必须显示完整地址，不要缩写**（例如：0xAbCd...1234 ❌，0xAbCdEf1234567890AbCdEf1234567890AbCdEf12 ✅）

**什么时候告诉 Relient：**
- ✅ 有代币数据 → 立即分析并发送报告（每次最多3个）
- ✅ 超过3个 → 先发3个，告知剩余数量
- ❌ 无数据 → 静默
