# cursor-api

## 说明

* 当前版本已稳定，若发现响应出现缺字漏字，与本程序无关。
* 若发现首字慢，与本程序无关。
* 若发现响应出现乱码，也与本程序无关。
* 属于官方的问题，请不要像作者反馈。
* 本程序拥有堪比客户端原本的速度，甚至可能更快。
* 本程序的性能是非常厉害的。
* 根据本项目开源协议，Fork的项目不能以作者的名义进行任何形式的宣传、推广或声明。原则上希望低调使用。
* 更新的时间跨度达近10月了，求赞助，项目不收费，不定制。
* 推荐自部署，[官方网站](https://cc.wisdgod.com) 仅用于作者测试，不保证稳定性。

## 获取key

1. 访问 [www.cursor.com](https://www.cursor.com) 并完成注册登录
2. 在浏览器中打开开发者工具（F12）
3. 在 Application-Cookies 中查找名为 `WorkosCursorSessionToken` 的条目，并复制其第三个字段。请注意，%3A%3A 是 :: 的 URL 编码形式，cookie 的值使用冒号 (:) 进行分隔。

## 配置说明

### 环境变量

* `PORT`: 服务器端口号（默认：3000）
* `AUTH_TOKEN`: 认证令牌（必须，用于API认证）
* `ROUTE_PREFIX`: 路由前缀（可选）

更多请查看 `/env-example`

### Token文件格式（已弃用）

`.tokens` 文件：每行为token和checksum的对应关系：

```
# 这里的#表示这行在下次读取要删除
token1,checksum1
token2,checksum2
```

该文件可以被自动管理，但用户仅可在确认自己拥有修改能力时修改，一般仅有以下情况需要手动修改：

* 需要删除某个 token
* 需要使用已有 checksum 来对应某一个 token

### 模型列表

写死了，后续也不会会支持自定义模型列表，因为本身就支持动态更新，详见[更新模型列表说明](#更新模型列表说明)

打开程序自己看，以实际为准，这里不再赘述。

## 接口说明

### 基础对话

* 接口地址: `/v1/chat/completions`
* 请求方法: POST
* 认证方式: Bearer Token
  1. 使用环境变量 `AUTH_TOKEN` 进行认证
  2. 使用 `/build-key` 构建的动态密钥认证
  3. 使用 `/config` 设置的共享Token进行认证 (关联：环境变量`SHARED_TOKEN`)
  4. 日志中的缓存 token key 的两种表示方式认证 (`/build-key` 同时也会给出这两种格式作为动态密钥的别名，该数字key本质为一个192位的整数)

#### 请求格式

```json
{
  "model": string,
  "messages": [
    {
      "role": "system" | "user" | "assistant", // "system" 也可以是 "developer"
      "content": string | [
        {
          "type": "text" | "image_url",
          "text": string,
          "image_url": {
            "url": string
          }
        }
      ]
    }
  ],
  "stream": bool,
  "stream_options": {
    "include_usage": bool
  }
}
```

#### 响应格式

如果 `stream` 为 `false`:

```json
{
  "id": string,
  "object": "chat.completion",
  "created": number,
  "model": string,
  "choices": [
    {
      "index": number,
      "message": {
        "role": "assistant",
        "content": string
      },
      "finish_reason": "stop" | "length"
    }
  ],
  "usage": {
    "prompt_tokens": 0,
    "completion_tokens": 0,
    "total_tokens": 0
  }
}
```

如果 `stream` 为 `true`:

```
data: {"id":string,"object":"chat.completion.chunk","created":number,"model":string,"choices":[{"index":number,"delta":{"role":"assistant","content":string},"finish_reason":null}]}

data: {"id":string,"object":"chat.completion.chunk","created":number,"model":string,"choices":[{"index":number,"delta":{"content":string},"finish_reason":null}]}

data: {"id":string,"object":"chat.completion.chunk","created":number,"model":string,"choices":[{"index":number,"delta":{},"finish_reason":"stop"}]}

data: [DONE]
```

### 获取模型列表

* 接口地址: `/v1/models`
* 请求方法: GET
* 认证方式: Bearer Token

#### 查询参数

可选的 JSON 请求体用于作为请求模型列表的参数：

```json
{
  "is_nightly": bool,                    // 是否包含 nightly 版本模型，默认 false
  "include_long_context_models": bool,   // 是否包含长上下文模型，默认 false  
  "exclude_max_named_models": bool,      // 是否排除 max 命名的模型，默认 true
  "additional_model_names": [string]        // 额外包含的模型名称列表，默认空数组
}
```

**注意**: 认证可选，查询参数可选且认证时生效，未提供时使用默认值。

#### 响应格式

```typescript
{
  object: "list",
  data: [
    {
      id: string,
      display_name: string,
      created: number,
      created_at: string,
      object: "model",
      type: "model", 
      owned_by: string,
      supports_thinking: bool,
      supports_images: bool,
      supports_max_mode: bool,
      supports_non_max_mode: bool
    }
  ]
}
```

#### 更新模型列表说明

每次携带Token时都会拉取最新的模型列表，与上次更新需距离至少30分钟。`additional_model_names` 可以用添加额外模型。

### Token管理接口

#### 获取Token信息

* 接口地址: `/tokens/get`
* 请求方法: POST
* 认证方式: Bearer Token
* 响应格式:

```typescript
{
  status: "success",
  tokens: [
    [
      number,
      string,
      {
        primary_token: string,
        secondary_token?: string,
        checksum: {
          first: string,
          second: string,
        },
        client_key?: string,
        config_version?: string,
        session_id?: string,
        proxy?: string,
        timezone?: string,
        gcpp_host?: "Asia" | "EU" | "US",
        user?: {
          user_id: int32,
          email?: string,
          first_name?: string,
          last_name?: string,
          workos_id?: string,
          team_id?: number,
          created_at?: string,
          is_enterprise_user: bool,
          is_on_new_pricing: bool,
          privacy_mode_info: {
            privacy_mode: "unspecified" | "no_storage" | "no_training" | "usage_data_training_allowed" | "usage_codebase_training_allowed",
            is_enforced_by_team?: bool
          }
        },
        status: {
          enabled: bool
        },
        usage?: {
          billing_cycle_start: string,
          billing_cycle_end: string,
          membership_type: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
          limit_type: "user" | "team",
          is_unlimited: bool,
          individual_usage: {
            plan?: {
              enabled: bool,
              used: int32,
              limit: int32,
              remaining: int32,
              breakdown: {
                included: int32,
                bonus: int32,
                total: int32
              }
            },
            on_demand?: {
              enabled: bool,
              used: int32,
              limit?: int32,
              remaining?: int32
            }
          },
          team_usage: {
            plan?: {
              enabled: bool,
              used: int32,
              limit: int32,
              remaining: int32,
              breakdown: {
                included: int32,
                bonus: int32,
                total: int32
              }
            },
            on_demand?: {
              enabled: bool,
              used: int32,
              limit?: int32,
              remaining?: int32
            }
          },
        },
        stripe?: {
          membership_type: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
          payment_id?: string,
          days_remaining_on_trial: int32,
          subscription_status?: "trialing" | "active" | "incomplete" | "incomplete_expired" | "past_due" | "canceled" | "unpaid" | "paused",
          verified_student: bool,
          trial_eligible: bool,
          trial_length_days: int32,
          is_on_student_plan: bool,
          is_on_billable_auto: bool,
          customer_balance?: double,
          trial_was_cancelled: bool,
          is_team_member: bool,
          team_membership_type?: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
          individual_membership_type?: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise"
        },
        sessions?: [
          {
            session_id: string,
            type: "unspecified" | "web" | "client" | "bugbot" | "background_agent",
            created_at: string,
            expires_at: string
          }
        ]
      }
    ]
  ],
  tokens_count: uint64
}
```

#### 设置Token信息

* 接口地址: `/tokens/set`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```typescript
[
  [
    string,
    {
      primary_token: string,
      secondary_token?: string,
      checksum: {
        first: string,
        second: string,
      },
      client_key?: string,
      config_version?: string,
      session_id?: string,
      proxy?: string,
      timezone?: string,
      gcpp_host?: "Asia" | "EU" | "US",
      user?: {
        user_id: int32,
        email?: string,
        first_name?: string,
        last_name?: string,
        workos_id?: string,
        team_id?: number,
        created_at?: string,
        is_enterprise_user: bool,
        is_on_new_pricing: bool,
        privacy_mode_info: {
          privacy_mode: "unspecified" | "no_storage" | "no_training" | "usage_data_training_allowed" | "usage_codebase_training_allowed",
          is_enforced_by_team?: bool
        }
      },
      status: {
        enabled: bool
      },
      usage?: {
        billing_cycle_start: string,
        billing_cycle_end: string,
        membership_type: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
        limit_type: "user" | "team",
        is_unlimited: bool,
        individual_usage: {
          plan?: {
            enabled: bool,
            used: int32,
            limit: int32,
            remaining: int32,
            breakdown: {
              included: int32,
              bonus: int32,
              total: int32
            }
          },
          on_demand?: {
            enabled: bool,
            used: int32,
            limit?: int32,
            remaining?: int32
          }
        },
        team_usage: {
          plan?: {
            enabled: bool,
            used: int32,
            limit: int32,
            remaining: int32,
            breakdown: {
              included: int32,
              bonus: int32,
              total: int32
            }
          },
          on_demand?: {
            enabled: bool,
            used: int32,
            limit?: int32,
            remaining?: int32
          }
        },
      },
      stripe?: {
        membership_type: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
        payment_id?: string,
        days_remaining_on_trial: int32,
        subscription_status?: "trialing" | "active" | "incomplete" | "incomplete_expired" | "past_due" | "canceled" | "unpaid" | "paused",
        verified_student: bool,
        trial_eligible: bool,
        trial_length_days: int32,
        is_on_student_plan: bool,
        is_on_billable_auto: bool,
        customer_balance?: double,
        trial_was_cancelled: bool,
        is_team_member: bool,
        team_membership_type?: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
        individual_membership_type?: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise"
      }
    }
  ]
]
```

* 响应格式:

```typescript
{
  status: "success",
  tokens_count: uint64,
  message: "Token files have been updated and reloaded"
}
```

#### 添加Token

* 接口地址: `/tokens/add`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```typescript
{
  tokens: [
    {
      alias?: string, // 可选，无则自动生成
      token: string,
      checksum?: string, // 可选，无则自动生成
      client_key?: string, // 可选，无则自动生成
      session_id?: string, // 可选
      config_version?: string, // 可选
      proxy?: string, // 可选
      timezone?: string, // 可选
      gcpp_host?: string // 可选
    }
  ],
  enabled: bool
}
```

* 响应格式:

```typescript
{
  status: "success",
  tokens_count: uint64,
  message: string  // "New tokens have been added and reloaded" 或 "No new tokens were added"
}
```

#### 删除Token

* 接口地址: `/tokens/del`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
{
  "aliases": [string], // 要删除的token列表
  "include_failed_tokens": bool // 默认为false
}
```

* 响应格式:

```json
{
  "status": "success",
  "failed_tokens": [string] // 可选，根据include_failed_tokens返回，表示未找到的token列表
}
```

* expectation说明:
  - simple: 只返回基本状态
  - updated_tokens: 返回更新后的token列表
  - failed_tokens: 返回未找到的token列表
  - detailed: 返回完整信息（包括updated_tokens和failed_tokens）

#### 设置Tokens标签（已弃用）

* 接口地址: `/tokens/tags/set`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
{
  "tokens": [string],
  "tags": {
    string: null | string // 键可以为 timezone: 时区标识符 或 proxy: 代理名称
  }
}
```

* 响应格式:

```json
{
  "status": "success",
  "message": string  // "标签更新成功"
}
```

#### 更新令牌Profile

* 接口地址: `/tokens/profile/update`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
[
  string // aliases
]
```

* 响应格式:

```json
{
  "status": "success",
  "message": "已更新{}个令牌配置, {}个令牌更新失败"
}
```

#### 更新令牌Config Version

* 接口地址: `/tokens/config-version/update`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
[
  string // aliases
]
```

* 响应格式:

```json
{
  "status": "success",
  "message": "已更新{}个令牌配置版本, {}个令牌更新失败"
}
```

#### 刷新令牌

* 接口地址: `/tokens/refresh`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
[
  string // aliases
]
```

* 响应格式:

```json
{
  "status": "success",
  "message": "已刷新{}个令牌, {}个令牌刷新失败"
}
```

#### 设置令牌状态

* 接口地址: `/tokens/status/set`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```typescript
{
  "aliases": [string],
  "enabled": bool
}
```

* 响应格式:

```json
{
  "status": "success",
  "message": "已设置{}个令牌状态, {}个令牌设置失败"
}
```

#### 设置令牌别名

* 接口地址: `/tokens/alias/set`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
{
  "{old_alias}": "{new_alias}"
}
```

* 响应格式:

```json
{
  "status": "success",
  "message": "已设置{}个令牌别名, {}个令牌设置失败"
}
```

#### 设置Tokens代理

* 接口地址: `/tokens/proxy/set`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
{
  "aliases": [string],
  "proxy": string  // 可选，代理地址，null表示清除代理
}
```

* 响应格式:

```json
{
  "status": "success",
  "message": "已设置{}个令牌代理, {}个令牌设置失败"
}
```

#### 设置Tokens时区

* 接口地址: `/tokens/timezone/set`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
{
  "aliases": [string],
  "timezone": string  // 可选，时区标识符（如"Asia/Shanghai"），null表示清除时区
}
```

* 响应格式:

```json
{
  "status": "success",
  "message": "已设置{}个令牌时区, {}个令牌设置失败"
}
```

#### 合并Tokens附带数据

* 接口地址: `/tokens/merge`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
{
  "{alias}": { // 以下至少其一存在，否则会失败
    "primary_token": string, // 可选
    "secondary_token": string, // 可选
    "checksum": { // 可选
      "first": string,
      "second": string,
    },
    "client_key": string, // 可选
    "config_version": string, // 可选
    "session_id": string, // 可选
    "proxy": string, // 可选
    "timezone": string, // 可选
    "gcpp_host": object, // 可选
  }
}
```

* 响应格式:

```json
{
  "status": "success",
  "message": "已合并{}个令牌, {}个令牌合并失败"
}
```

#### 构建API Key

* 接口地址: `/build-key`
* 请求方法: POST
* 认证方式: Bearer Token (当SHARE_AUTH_TOKEN启用时需要)
* 请求格式:

```json
{
  "token": string,               // 格式: JWT
  "checksum": {
    "first": string,             // 格式: 长度为64的Hex编码字符串
    "second": string,            // 格式: 长度为64的Hex编码字符串
  },
  "client_key": string,          // 格式: 长度为64的Hex编码字符串
  "config_version": string,      // 格式: UUID
  "session_id": string,          // 格式: UUID
  "secret": string,              // 可选，没什么用
  "proxy_name": string,          // 可选，指定代理
  "timezone": string,            // 可选，指定时区
  "gcpp_host": string,           // 可选，代码补全区域
  "disable_vision": bool,        // 可选，禁用图片处理能力
  "enable_slow_pool": bool,      // 可选，启用慢速池
  "include_web_references": bool,
  "usage_check_models": {          // 可选，使用量检查模型配置
    "type": "default" | "disabled" | "all" | "custom",
    "model_ids": string  // 当type为custom时生效，以逗号分隔的模型ID列表
  }
}
```

* 响应格式:

```json
{
  "keys": [string]    // 成功时返回生成的key
}
```

或出错时:

```json
{
  "error": string  // 错误信息
}
```

说明：

1. 此接口用于生成携带动态配置的API Key，是对直接传token与checksum模式的升级版本，在0.3起，直接传token与checksum的方式已经不再适用

2. 生成的key格式为: `sk-{encoded_config}`，其中sk-为默认前缀(可配置)

3. usage_check_models配置说明:
   - default: 使用默认模型列表(同下 `usage_check_models` 字段的默认值)
   - disabled: 禁用使用量检查
   - all: 检查所有可用模型
   - custom: 使用自定义模型列表(需在model_ids中指定)

4. 在当前版本，keys数组长度总为3，后2个基于缓存，仅第1个使用过才行：
   1. 完整key，旧版本也存在
   2. 数字key的base64编码版本
   3. 数字key的明文版本

5. 数字key是一个128位无符号整数与一个64位无符号整数组成的，比通常使用的uuid更难破解。

### 代理管理接口

#### 获取代理配置信息

* 接口地址: `/proxies/get`
* 请求方法: POST
* 响应格式:

```json
{
  "status": "success",
  "proxies": {
    "proxies": {
      "proxy_name": "non" | "sys" | "http://proxy-url",
    },
    "general": string // 当前使用的通用代理名称
  },
  "proxies_count": number,
  "general_proxy": string,
  "message": string // 可选
}
```

#### 设置代理配置

* 接口地址: `/proxies/set`
* 请求方法: POST
* 请求格式:

```json
{
  "proxies": {
    "{proxy_name}": "non" | "sys" | "http://proxy-url"
  },
  "general": string  // 要设置的通用代理名称
}
```

* 响应格式:

```json
{
  "status": "success",
  "proxies_count": number,
  "message": "代理配置已更新"
}
```

#### 添加代理

* 接口地址: `/proxies/add`
* 请求方法: POST
* 请求格式:

```json
{
  "proxies": {
    "{proxy_name}": "non" | "sys" | "http://proxy-url"
  }
}
```

* 响应格式:

```json
{
  "status": "success",
  "proxies_count": number,
  "message": string  // "已添加 X 个新代理" 或 "没有添加新代理"
}
```

#### 删除代理

* 接口地址: `/proxies/del`
* 请求方法: POST
* 请求格式:

```json
{
  "names": [string],  // 要删除的代理名称列表
  "expectation": "simple" | "updated_proxies" | "failed_names" | "detailed"  // 默认为simple
}
```

* 响应格式:

```json
{
  "status": "success",
  "updated_proxies": {  // 可选，根据expectation返回
    "proxies": {
      "proxy_name": "non" | "sys" | "http://proxy-url"
    },
    "general": string
  },
  "failed_names": [string]  // 可选，根据expectation返回，表示未找到的代理名称列表
}
```

#### 设置通用代理

* 接口地址: `/proxies/set-general`
* 请求方法: POST
* 请求格式:

```json
{
  "name": string  // 要设置为通用代理的代理名称
}
```

* 响应格式:

```json
{
  "status": "success",
  "message": "通用代理已设置"
}
```

#### 代理类型说明

* `non`: 表示不使用代理
* `sys`: 表示使用系统代理
* 其他: 表示具体的代理URL地址（如 `http://proxy-url`）

#### 注意事项

1. 代理名称必须是唯一的，添加重复名称的代理会被忽略
2. 设置通用代理时，指定的代理名称必须存在于当前的代理配置中
3. 删除代理时的 expectation 参数说明：
   - simple: 只返回基本状态
   - updated_proxies: 返回更新后的代理配置
   - failed_names: 返回未找到的代理名称列表
   - detailed: 返回完整信息（包括updated_proxies和failed_names）

### 错误格式

所有接口在发生错误时会返回统一的错误格式：

```json
{
  "status": "error",
  "code": number,   // 可选
  "error": string,  // 可选，错误详细信息
  "message": string // 错误提示信息
}
```

### 配置管理接口

#### 更新配置

* 接口地址: `/config`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```json
{
  "action": "get" | "update" | "reset",
  "path": string,
  "content": {
    "type": "default" | "not_found" | "redirect" | "plain_text" | "html" | "css" | "js",
    "value": string  // type=redirect时为URL, type=plain_text/html/css/js时为对应内容
  },
  "vision_ability": "none" | "base64" | "all", // "disabled" | "base64-only" | "base64-http"
  "enable_slow_pool": bool,
  "enable_long_context": bool,
  "usage_check_models": {
    "type": "none" | "default" | "all" | "list",
    "content": string
  },
  "enable_dynamic_key": bool,
  "share_token": string,
  "calibrate_token": string,
  "include_web_references": bool
}
```

* 响应格式:

```json
{
  "status": "success",
  "message": string,
  "data": {
    "content": {
      "type": "default" | "not_found" | "redirect" | "plain_text" | "html" | "css" | "js",
      "value": string
    },
    "vision_ability": "none" | "base64" | "all",
    "enable_slow_pool": bool,
    "enable_long_context": bool,
    "usage_check_models": {
      "type": "none" | "default" | "all" | "list",
      "content": string
    },
    "enable_dynamic_key": bool,
    "share_token": string,
    "calibrate_token": string,
    "include_web_references": bool
  }
}
```

注意：`usage_check_models` 字段的默认值为非"cursor-small"、"deepseek-v3.1"、"grok-3-mini"的所有模型。

这些模型将默认进行使用量检查。您可以通过配置接口修改此设置。

### 日志管理接口

#### 获取日志接口

* 接口地址: `/logs`
* 请求方法: GET
* 响应格式: 根据配置返回不同的内容类型(默认、文本或HTML)

#### 获取日志数据

* 接口地址: `/logs/get`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```typescript
{
  "query": {
    // 分页与排序控制
    "limit": number,            // 可选，返回记录数量限制
    "offset": number,           // 可选，起始位置偏移量
    "reverse": bool,            // 可选，反向排序，默认false（从旧到新），true时从新到旧

    // 时间范围过滤
    "from_date": string,        // 可选，开始日期时间，RFC3339格式
    "to_date": string,          // 可选，结束日期时间，RFC3339格式

    // 用户标识过滤
    "user_id": string,          // 可选，按用户ID精确匹配
    "email": string,            // 可选，按用户邮箱过滤（支持部分匹配）
    "membership_type": string,  // 可选，按会员类型过滤 ("free"/"free_trial"/"pro"/"pro_plus"/"ultra"/"enterprise")

    // 核心业务过滤
    "status": string,           // 可选，按状态过滤 ("pending"/"success"/"failure")
    "model": string,            // 可选，按模型名称过滤（支持部分匹配）
    "include_models": [string], // 可选，包含特定模型
    "exclude_models": [string], // 可选，排除特定模型

    // 请求特征过滤
    "stream": bool,             // 可选，是否为流式请求
    "has_chain": bool,          // 可选，是否包含对话链
    "has_usage": bool,          // 可选，是否有usage信息

    // 错误相关过滤
    "has_error": bool,          // 可选，是否包含错误
    "error": string,            // 可选，按错误过滤（支持部分匹配）

    // 性能指标过滤
    "min_total_time": number,   // 可选，最小总耗时（秒）
    "max_total_time": number,   // 可选，最大总耗时（秒）
    "min_tokens": number,       // 可选，最小token数（input + output）
    "max_tokens": number        // 可选，最大token数
  }
}
```

* 响应格式:

```typescript
{
  status: "success",
  total: uint64,
  active?: uint64,
  error?: uint64,
  logs: [
    {
      id: uint64,
      timestamp: string,
      model: string,
      token_info: {
        key: string,
        usage?: {
          billing_cycle_start: string,
          billing_cycle_end: string,
          membership_type: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
          limit_type: "user" | "team",
          is_unlimited: bool,
          individual_usage: {
            plan?: {
              enabled: bool,
              used: int32,
              limit: int32,
              remaining: int32,
              breakdown: {
                included: int32,
                bonus: int32,
                total: int32
              }
            },
            on_demand?: {
              enabled: bool,
              used: int32,
              limit?: int32,
              remaining?: int32
            }
          },
          team_usage: {
            plan?: {
              enabled: bool,
              used: int32,
              limit: int32,
              remaining: int32,
              breakdown: {
                included: int32,
                bonus: int32,
                total: int32
              }
            },
            on_demand?: {
              enabled: bool,
              used: int32,
              limit?: int32,
              remaining?: int32
            }
          },
        },
        stripe?: {
          membership_type: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
          payment_id?: string,
          days_remaining_on_trial: int32,
          subscription_status?: "trialing" | "active" | "incomplete" | "incomplete_expired" | "past_due" | "canceled" | "unpaid" | "paused",
          verified_student: bool,
          trial_eligible: bool,
          trial_length_days: int32,
          is_on_student_plan: bool,
          is_on_billable_auto: bool,
          customer_balance?: double,
          trial_was_cancelled: bool,
          is_team_member: bool,
          team_membership_type?: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
          individual_membership_type?: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise"
        }
      },
      chain: {
        delays?: [
          string,
          [
            number, // chars count
            number // time
          ]
        ],
        usage?: {
          input: int32,
          output: int32,
          cache_write: int32,
          cache_read: int32,
          cents: float
        }
      },
      timing: {
        total: double
      },
      stream: bool,
      status: "pending" | "success" | "failure",
      error?: string | {
        error:string,
        details:string
      }
    }
  ],
  timestamp: string
}
```

* 说明：
  - 所有查询参数都是可选的
  - 管理员可以查看所有日志，普通用户只能查看与其token相关的日志
  - 如果提供了无效的状态或会员类型，将返回空结果
  - 日期时间格式需遵循 RFC3339 标准，如："2024-03-20T15:30:00+08:00"
  - 邮箱和模型名称支持部分匹配

#### 获取日志令牌

* 接口地址: `/logs/tokens/get`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```typescript
[
  string
]
```

* 响应格式:

```typescript
{
  status: "success",
  tokens: {
    {key}: {
      primary_token: string,
      secondary_token?: string,
      checksum: {
        first: string,
        second: string,
      },
      client_key?: string,
      config_version?: string,
      session_id?: string,
      proxy?: string,
      timezone?: string,
      gcpp_host?: "Asia" | "EU" | "US",
      user?: {
        user_id: int32,
        email?: string,
        first_name?: string,
        last_name?: string,
        workos_id?: string,
        team_id?: number,
        created_at?: string,
        is_enterprise_user: bool,
        is_on_new_pricing: bool,
        privacy_mode_info: {
          privacy_mode: "unspecified" | "no_storage" | "no_training" | "usage_data_training_allowed" | "usage_codebase_training_allowed",
          is_enforced_by_team?: bool
        }
      }
    }
  },
  total: uint64,
  timestamp: string
}
```

### 静态资源接口

#### 环境变量示例

* 接口地址: `/env-example`
* 请求方法: GET
* 响应格式: 文本

#### 文档

* 接口地址: `/readme`
* 请求方法: GET
* 响应格式: HTML

#### 许可

* 接口地址: `/license`
* 请求方法: GET
* 响应格式: HTML

### 健康检查接口

* **接口地址**: `/health`
* **请求方法**: GET
* **认证方式**: 无需
* **响应格式**: 根据配置返回不同的内容类型(默认JSON、文本或HTML)

#### 响应结构

```json
{
  "status": "success",
  "service": {
    "name": "cursor-api",
    "version": "1.0.0",
    "is_debug": false,
    "build": {
      "version": 1,
      "timestamp": "2024-01-15T10:30:00Z",
      "is_debug": false,
      "is_prerelease": false
    }
  },
  "runtime": {
    "started_at": "2024-01-15T10:00:00+08:00",
    "uptime_seconds": 1800,
    "requests": {
      "total": 1250,
      "active": 3,
      "errors": 12
    }
  },
  "system": {
    "memory": {
      "used_bytes": 134217728,
      "used_percentage": 12.5,
      "available_bytes": 1073741824
    },
    "cpu": {
      "usage_percentage": 15.2,
      "load_average": [0.8, 1.2, 1.5]
    }
  },
  "capabilities": {
    "models": ["gpt-4", "claude-3"],
    "endpoints": ["/v1/chat/completions", "/v1/messages"],
    "features": [".."]
  }
}
```

#### 字段说明

| 字段 | 类型 | 说明 |
|------|------|------|
| `status` | string | 服务状态: "success", "warning", "error" |
| `service.name` | string | 服务名称 |
| `service.version` | string | 服务版本 |
| `service.is_debug` | bool | 是否为调试模式 |
| `service.build.version` | number | 构建版本号(仅preview功能启用时) |
| `service.build.timestamp` | string | 构建时间戳 |
| `service.build.is_prerelease` | bool | 是否为预发布版本 |
| `runtime.started_at` | string | 服务启动时间 |
| `runtime.uptime_seconds` | number | 运行时长(秒) |
| `runtime.requests.total` | number | 总请求数 |
| `runtime.requests.active` | number | 当前活跃请求数 |
| `runtime.requests.errors` | number | 错误请求数 |
| `system.memory.used_bytes` | number | 已使用内存(字节) |
| `system.memory.used_percentage` | number | 内存使用率(%) |
| `system.memory.available_bytes` | number | 可用内存(字节,可选) |
| `system.cpu.usage_percentage` | number | CPU使用率(%) |
| `system.cpu.load_average` | array | 系统负载[1分钟,5分钟,15分钟] |
| `capabilities.models` | array | 支持的模型列表 |
| `capabilities.endpoints` | array | 可用的API端点 |
| `capabilities.features` | array | 支持的功能特性 |

### 其他接口

#### 随机生成一个uuid

* 接口地址: `/gen-uuid`
* 请求方法: GET
* 响应格式:

```plaintext
string
```

#### 随机生成一个hash

* 接口地址: `/gen-hash`
* 请求方法: GET
* 响应格式:

```plaintext
string
```

#### 随机生成一个checksum

* 接口地址: `/gen-checksum`
* 请求方法: GET
* 响应格式:

```plaintext
string
```

#### 随机生成一个token（已弃用）

* 接口地址: `/gen-token`
* 请求方法: GET
* 响应格式:

```plaintext
string
```

#### 获取当前的checksum header

* 接口地址: `/get-checksum-header`
* 请求方法: GET
* 响应格式:

```plaintext
string
```

#### 获取账号信息

* 接口地址: `/token-profile/get`
* 请求方法: POST
* 认证方式: Bearer Token
* 请求格式:

```typescript
{
  session_token: string,
  web_token: string,
  proxy_name?: string,
  include_sessions: bool
}
```

* 响应格式:

```typescript
{
  token_profile: [
    null | {
      billing_cycle_start: string,
      billing_cycle_end: string,
      membership_type: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
      limit_type: "user" | "team",
      is_unlimited: bool,
      individual_usage: {
        plan?: {
          enabled: bool,
          used: int32,
          limit: int32,
          remaining: int32,
          breakdown: {
            included: int32,
            bonus: int32,
            total: int32
          }
        },
        on_demand?: {
          enabled: bool,
          used: int32,
          limit?: int32,
          remaining?: int32
        }
      },
      team_usage: {
        plan?: {
          enabled: bool,
          used: int32,
          limit: int32,
          remaining: int32,
          breakdown: {
            included: int32,
            bonus: int32,
            total: int32
          }
        },
        on_demand?: {
          enabled: bool,
          used: int32,
          limit?: int32,
          remaining?: int32
        }
      },
    },
    null | {
      membership_type: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
      payment_id?: string,
      days_remaining_on_trial: int32,
      subscription_status?: "trialing" | "active" | "incomplete" | "incomplete_expired" | "past_due" | "canceled" | "unpaid" | "paused",
      verified_student: bool,
      trial_eligible: bool,
      trial_length_days: int32,
      is_on_student_plan: bool,
      is_on_billable_auto: bool,
      customer_balance?: double,
      trial_was_cancelled: bool,
      is_team_member: bool,
      team_membership_type?: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise",
      individual_membership_type?: "free" | "free_trial" | "pro" | "pro_plus" | "ultra" | "enterprise"
    },
    null | {
      user_id: int32,
      email?: string,
      first_name?: string,
      last_name?: string,
      workos_id?: string,
      team_id?: number,
      created_at?: string,
      is_enterprise_user: bool,
      is_on_new_pricing: bool,
      privacy_mode_info: {
        privacy_mode: "unspecified" | "no_storage" | "no_training" | "usage_data_training_allowed" | "usage_codebase_training_allowed",
        is_enforced_by_team?: bool
      }
    },
    null | [
      {
        session_id: string,
        type: "unspecified" | "web" | "client" | "bugbot" | "background_agent",
        created_at: string,
        expires_at: string
      }
    ]
  ]
}
```

如果发生错误，响应格式为:

```json
{
  "error": string
}
```

#### 获取Config Version

* 接口地址: `/config-version/get`
* 请求方法: POST
* 认证方式: Bearer Token (当SHARE_AUTH_TOKEN启用时需要)
* 请求格式:

```json
{
  "token": string,               // 格式: JWT
  "checksum": {
    "first": string,             // 格式: 长度为64的Hex编码字符串
    "second": string,            // 格式: 长度为64的Hex编码字符串
  },
  "client_key": string,          // 格式: 长度为64的Hex编码字符串
  "session_id": string,          // 格式: UUID
  "proxy_name": string,          // 可选，指定代理
  "timezone": string,            // 可选，指定时区
  "gcpp_host": string            // 可选，代码补全区域
}
```

* 响应格式:

```json
{
  "config_version": string    // 成功时返回生成的UUID
}
```

或出错时:

```json
{
  "error": string  // 错误信息
}
```

#### 获取更新令牌（已弃用）

* 接口地址: `/token-upgrade`
* 请求方法: POST
* 认证方式: 请求体中包含token
* 请求格式:

```json
{
  "token": string
}
```

* 响应格式:

```json
{
  "status": "success" | "failure" | "error",
  "message": string,
  "result": string // optional
}
```

## Copilot++ 接口文档

1. 相关接口都需要 `x-client-key`, 格式请见 `/gen-hash`（32字节）。
2. Cookie `FilesyncCookie` 是16字节，工作区不变即不变。
3. 关于形如 `AWSALBAPP-0` 的 Cookie 具有7天有效期，可能变化，详情请查阅 Amazon 相关文档。
4. `FilesyncCookie` 和 `AWSALBAPP` 总是被 `/file/upload` 或 `/file/sync`。
5. 以下所有接口都使用 POST 方法，且都需要认证。

### 获取代码补全服务的配置信息

* 接口地址: `/cpp/config`

#### 请求格式

```json
{
  "is_nightly": bool,  // 可选，是否使用nightly版本
  "model": string,        // 模型名称
  "supports_cpt": bool // 可选，是否支持CPT
}
```

### 响应格式

```json
{
  "above_radius": number,                                        // 可选，上方扫描半径
  "below_radius": number,                                        // 可选，下方扫描半径
  "merge_behavior": {                                            // 可选，合并行为
    "type": string,
    "limit": number,                                             // 可选，限制
    "radius": number                                             // 可选，半径
  },
  "is_on": bool,                                              // 可选，是否开启
  "is_ghost_text": bool,                                      // 可选，是否使用幽灵文本
  "should_let_user_enable_cpp_even_if_not_pro": bool,         // 可选，非专业用户是否可以启用
  "heuristics": [                                                // 启用的启发式规则列表
    "lots_of_added_text",
    "duplicating_line_after_suggestion",
    "duplicating_multiple_lines_after_suggestion",
    "reverting_user_change",
    "output_extends_beyond_range_and_is_repeated",
    "suggesting_recently_rejected_edit"
  ],
  "exclude_recently_viewed_files_patterns": [string],            // 最近查看文件排除模式
  "enable_rvf_tracking": bool,                                // 是否启用RVF跟踪
  "global_debounce_duration_millis": number,                     // 全局去抖动时间(毫秒)
  "client_debounce_duration_millis": number,                     // 客户端去抖动时间(毫秒)
  "cpp_url": string,                                             // CPP服务URL
  "use_whitespace_diff_history": bool,                        // 是否使用空白差异历史
  "import_prediction_config": {                                  // 导入预测配置
    "is_disabled_by_backend": bool,                           // 是否被后端禁用
    "should_turn_on_automatically": bool,                     // 是否自动开启
    "python_enabled": bool                                    // Python是否启用
  },
  "enable_filesync_debounce_skipping": bool,                  // 是否启用文件同步去抖动跳过
  "check_filesync_hash_percent": number,                         // 文件同步哈希检查百分比
  "geo_cpp_backend_url": string,                                 // 地理位置CPP后端URL
  "recently_rejected_edit_thresholds": {                         // 可选，最近拒绝编辑阈值
    "hard_reject_threshold": number,                             // 硬拒绝阈值
    "soft_reject_threshold": number                              // 软拒绝阈值
  },
  "is_fused_cursor_prediction_model": bool,                   // 是否使用融合光标预测模型
  "include_unchanged_lines": bool,                            // 是否包含未更改行
  "should_fetch_rvf_text": bool,                              // 是否获取RVF文本
  "max_number_of_cleared_suggestions_since_last_accept": number, // 可选，上次接受后清除建议的最大数量
  "suggestion_hint_config": {                                    // 可选，建议提示配置
    "important_lsp_extensions": [string],                        // 重要的LSP扩展
    "enabled_for_path_extensions": [string]                      // 启用的路径扩展
  }
}
```

### 获取可用的代码补全模型列表

* 接口地址: `/cpp/models`

#### 请求格式

无

### 响应格式

```json
{
  "models": [string],     // 可用模型列表
  "default_model": string // 可选，默认模型
}
```

### 上传文件

* 接口地址: `/file/upload`

#### 请求格式

```json
{
  "uuid": string,                    // 文件标识符
  "relative_workspace_path": string, // 文件相对于工作区的路径
  "contents": string,                // 文件内容
  "model_version": number,           // 模型版本
  "sha256_hash": string              // 可选，文件的SHA256哈希值
}
```

### 响应格式

```json
{
  "error": string // 错误类型：unspecified, non_existant, hash_mismatch
}
```

### 同步文件变更

* 接口地址: `/file/sync`

#### 请求格式

```json
{
  "uuid": string,                                // 文件标识符
  "relative_workspace_path": string,             // 文件相对于工作区的路径
  "model_version": number,                       // 模型版本
  "filesync_updates": [                          // 文件同步更新数组
    {
      "model_version": number,                   // 模型版本
      "relative_workspace_path": string,         // 文件相对于工作区的路径
      "updates": [                               // 单个更新请求数组
        {
          "start_position": number,              // 更新开始位置
          "end_position": number,                // 更新结束位置
          "change_length": number,               // 变更长度
          "replaced_string": string,             // 替换的字符串
          "range": {                             // 简单范围
            "start_line_number": number,         // 开始行号
            "start_column": number,              // 开始列
            "end_line_number_inclusive": number, // 结束行号（包含）
            "end_column": number                 // 结束列
          }
        }
      ],
      "expected_file_length": number             // 预期文件长度
    }
  ],
  "sha256_hash": string                          // 文件的SHA256哈希值
}
```

### 响应格式

```json
{
  "error": string // 错误类型：unspecified, non_existant, hash_mismatch
}
```

### 流式代码补全

* 接口地址: `/cpp/stream`

#### 请求格式

```typescript
{
  current_file: {                                                 // 当前文件信息
    relative_workspace_path: string,                              // 文件相对于工作区的路径
    contents: string,                                             // 文件内容
    rely_on_filesync: bool,                                       // 是否依赖文件同步
    sha_256_hash?: string,                                        // 可选，文件内容SHA256哈希值
    top_chunks: [                                                 // BM25检索的顶级代码块
      {
        content: string,                                          // 代码块内容
        range: {                                                  // SimplestRange 最简单范围
          start_line: int32,                                      // 开始行号
          end_line_inclusive: int32                               // 结束行号（包含）
        },
        score: int32,                                             // BM25分数
        relative_path: string                                     // 代码块所在文件相对路径
      }
    ],
    contents_start_at_line: int32,                                // 内容开始行号（一般为0）
    cursor_position: {                                            // CursorPosition 光标位置
      line: int32,                                                // 行号（0-based）
      column: int32                                               // 列号（0-based）
    },
    dataframes: [                                                 // DataframeInfo 数据框信息（用于数据分析场景）
      {
        name: string,                                             // 数据框变量名
        shape: string,                                            // 形状描述，如"(100, 5)"
        data_dimensionality: int32,                               // 数据维度
        columns: [                                                // 列定义
          {
            key: string,                                          // 列名
            type: string                                          // 列数据类型
          }
        ],
        row_count: int32,                                         // 行数
        index_column: string                                      // 索引列名称
      }
    ],
    total_number_of_lines: int32,                                 // 文件总行数
    language_id: string,                                          // 语言标识符（如"python", "rust"）
    selection?: {                                                 // 可选，CursorRange 当前选中范围
      start_position: {                                           // CursorPosition 开始位置
        line: int32,                                              // 行号
        column: int32                                             // 列号
      },
      end_position: {                                             // CursorPosition 结束位置
        line: int32,                                              // 行号
        column: int32                                             // 列号
      }
    },
    alternative_version_id?: int32,                               // 可选，备选版本ID
    diagnostics: [                                                // Diagnostic 诊断信息数组
      {
        message: string,                                          // 诊断消息内容
        range: {                                                  // CursorRange 诊断范围
          start_position: {                                       // CursorPosition 开始位置
            line: int32,                                          // 行号
            column: int32                                         // 列号
          },
          end_position: {                                         // CursorPosition 结束位置
            line: int32,                                          // 行号
            column: int32                                         // 列号
          }
        },
        severity: "error" | "warning" | "information" | "hint",   // DiagnosticSeverity 严重程度
        related_information: [                                    // RelatedInformation 相关信息
          {
            message: string,                                      // 相关信息消息
            range: {                                              // CursorRange 相关信息范围
              start_position: {                                   // CursorPosition 开始位置
                line: int32,                                      // 行号
                column: int32                                     // 列号
              },
              end_position: {                                     // CursorPosition 结束位置
                line: int32,                                      // 行号
                column: int32                                     // 列号
              }
            }
          }
        ]
      }
    ],
    file_version?: int32,                                         // 可选，文件版本号（用于增量更新）
    workspace_root_path: string,                                  // 工作区根路径（绝对路径）
    line_ending?: string,                                         // 可选，行结束符（"\n" 或 "\r\n"）
    file_git_context: {                                           // FileGit Git上下文信息
      commits: [                                                  // GitCommit 相关提交数组
        {
          commit: string,                                         // 提交哈希
          author: string,                                         // 作者
          date: string,                                           // 提交日期
          message: string                                         // 提交消息
        }
      ]
    }
  },
  diff_history: [string],                                         // 差异历史（已弃用，使用file_diff_histories代替）
  model_name?: string,                                            // 可选，指定使用的模型名称
  linter_errors?: {                                               // 可选，LinterErrors Linter错误信息
    relative_workspace_path: string,                              // 错误所在文件相对路径
    errors: [                                                     // LinterError 错误数组
      {
        message: string,                                          // 错误消息
        range: {                                                  // CursorRange 错误范围
          start_position: {                                       // CursorPosition 开始位置
            line: int32,                                          // 行号
            column: int32                                         // 列号
          },
          end_position: {                                         // CursorPosition 结束位置
            line: int32,                                          // 行号
            column: int32                                         // 列号
          }
        },
        source?: string,                                          // 可选，错误来源（如"eslint", "pyright"）
        related_information: [                                    // Diagnostic.RelatedInformation 相关信息
          {
            message: string,                                      // 相关信息消息
            range: {                                              // CursorRange 相关信息范围
              start_position: {                                   // CursorPosition 开始位置
                line: int32,                                      // 行号
                column: int32                                     // 列号
              },
              end_position: {                                     // CursorPosition 结束位置
                line: int32,                                      // 行号
                column: int32                                     // 列号
              }
            }
          }
        ],
        severity?: "error" | "warning" | "information" | "hint"   // 可选，DiagnosticSeverity 严重程度
      }
    ],
    file_contents: string                                         // 文件内容（用于错误上下文）
  },
  context_items: [                                                // CppContextItem 上下文项数组
    {
      contents: string,                                           // 上下文内容
      symbol?: string,                                            // 可选，符号名称
      relative_workspace_path: string,                            // 上下文所在文件相对路径
      score: float                                                // 相关性分数
    }
  ],
  diff_history_keys: [string],                                    // 差异历史键（已弃用）
  give_debug_output?: bool,                                       // 可选，是否输出调试信息
  file_diff_histories: [                                          // CppFileDiffHistory 文件差异历史数组
    {
      file_name: string,                                          // 文件名
      diff_history: [string],                                     // 差异历史数组，格式："行号-|旧内容\n行号+|新内容\n"
      diff_history_timestamps: [double]                           // 差异时间戳数组（Unix毫秒时间戳）
    }
  ],
  merged_diff_histories: [                                        // CppFileDiffHistory 合并后的差异历史
    {
      file_name: string,                                          // 文件名
      diff_history: [string],                                     // 合并后的差异历史
      diff_history_timestamps: [double]                           // 时间戳数组
    }
  ],
  block_diff_patches: [                                           // BlockDiffPatch 块级差异补丁
    {
      start_model_window: {                                       // ModelWindow 模型窗口起始状态
        lines: [string],                                          // 窗口内的代码行
        start_line_number: int32,                                 // 窗口起始行号
        end_line_number: int32                                    // 窗口结束行号
      },
      changes: [                                                  // Change 变更数组
        {
          text: string,                                           // 变更后的文本
          range: {                                                // IRange 变更范围
            start_line_number: int32,                             // 起始行号
            start_column: int32,                                  // 起始列号
            end_line_number: int32,                               // 结束行号
            end_column: int32                                     // 结束列号
          }
        }
      ],
      relative_path: string,                                      // 文件相对路径
      model_uuid: string,                                         // 模型UUID（用于追踪补全来源）
      start_from_change_index: int32                              // 从第几个change开始应用
    }
  ],
  is_nightly?: bool,                                              // 可选，是否为nightly构建版本
  is_debug?: bool,                                                // 可选，是否为调试模式
  immediately_ack?: bool,                                         // 可选，是否立即确认请求
  enable_more_context?: bool,                                     // 可选，是否启用更多上下文检索
  parameter_hints: [                                              // CppParameterHint 参数提示数组
    {
      label: string,                                              // 参数标签（如"x: int"）
      documentation?: string                                      // 可选，参数文档说明
    }
  ],
  lsp_contexts: [                                                 // LspSubgraphFullContext LSP子图上下文
    {
      uri?: string,                                               // 可选，文件URI
      symbol_name: string,                                        // 符号名称
      positions: [                                                // LspSubgraphPosition 位置数组
        {
          line: int32,                                            // 行号
          character: int32                                        // 字符位置
        }
      ],
      context_items: [                                            // LspSubgraphContextItem 上下文项
        {
          uri?: string,                                           // 可选，URI
          type: string,                                           // 类型（如"definition", "reference"）
          content: string,                                        // 内容
          range?: {                                               // 可选，LspSubgraphRange 范围
            start_line: int32,                                    // 起始行
            start_character: int32,                               // 起始字符
            end_line: int32,                                      // 结束行
            end_character: int32                                  // 结束字符
          }
        }
      ],
      score: float                                                // 相关性分数
    }
  ],
  cpp_intent_info?: {                                             // 可选，CppIntentInfo 代码补全意图信息
    source: "line_change" | "typing" | "option_hold" |            // 触发来源
            "linter_errors" | "parameter_hints" | 
            "cursor_prediction" | "manual_trigger" | 
            "editor_change" | "lsp_suggestions"
  },
  workspace_id?: string,                                          // 可选，工作区唯一标识符
  additional_files: [                                             // AdditionalFile 附加文件数组
    {
      relative_workspace_path: string,                            // 文件相对路径
      is_open: bool,                                              // 是否在编辑器中打开
      visible_range_content: [string],                            // 可见范围的内容（按行）
      last_viewed_at?: double,                                    // 可选，最后查看时间（Unix毫秒时间戳）
      start_line_number_one_indexed: [int32],                     // 可见范围起始行号（1-based索引）
      visible_ranges: [                                           // LineRange 可见范围数组
        {
          start_line_number: int32,                               // 起始行号
          end_line_number_inclusive: int32                        // 结束行号（包含）
        }
      ]
    }
  ],
  control_token?: "quiet" | "loud" | "op",                        // 可选，ControlToken 控制标记
  client_time?: double,                                           // 可选，客户端时间（Unix毫秒时间戳）
  filesync_updates: [                                             // FilesyncUpdateWithModelVersion 文件同步增量更新
    {
      model_version: int32,                                       // 模型版本号
      relative_workspace_path: string,                            // 文件相对路径
      updates: [                                                  // SingleUpdateRequest 更新操作数组
        {
          start_position: int32,                                  // 起始位置（字符偏移量，0-based）
          end_position: int32,                                    // 结束位置（字符偏移量，0-based）
          change_length: int32,                                   // 变更后的长度
          replaced_string: string,                                // 替换的字符串内容
          range: {                                                // SimpleRange 变更范围
            start_line_number: int32,                             // 起始行号
            start_column: int32,                                  // 起始列号
            end_line_number_inclusive: int32,                     // 结束行号（包含）
            end_column: int32                                     // 结束列号
          }
        }
      ],
      expected_file_length: int32                                 // 应用更新后预期的文件长度
    }
  ],
  time_since_request_start: double,                               // 从请求开始到当前的时间（毫秒）
  time_at_request_send: double,                                   // 请求发送时的时间戳（Unix毫秒时间戳）
  client_timezone_offset?: double,                                // 可选，客户端时区偏移（分钟，如-480表示UTC+8）
  lsp_suggested_items?: {                                         // 可选，LspSuggestedItems LSP建议项
    suggestions: [                                                // LspSuggestion 建议数组
      {
        label: string                                             // 建议标签
      }
    ]
  },
  supports_cpt?: bool,                                            // 可选，是否支持CPT（Code Patch Token）格式
  supports_crlf_cpt?: bool,                                       // 可选，是否支持CRLF换行的CPT格式
  code_results: [                                                 // CodeResult 代码检索结果
    {
      code_block: {                                               // CodeBlock 代码块
        relative_workspace_path: string,                          // 文件相对路径
        file_contents?: string,                                   // 可选，完整文件内容
        file_contents_length?: int32,                             // 可选，文件内容长度
        range: {                                                  // CursorRange 代码块范围
          start_position: {                                       // CursorPosition 开始位置
            line: int32,                                          // 行号
            column: int32                                         // 列号
          },
          end_position: {                                         // CursorPosition 结束位置
            line: int32,                                          // 行号
            column: int32                                         // 列号
          }
        },
        contents: string,                                         // 代码块内容
        signatures: {                                             // Signatures 签名信息
          ranges: [                                               // CursorRange 签名范围数组
            {
              start_position: {                                   // CursorPosition 开始位置
                line: int32,                                      // 行号
                column: int32                                     // 列号
              },
              end_position: {                                     // CursorPosition 结束位置
                line: int32,                                      // 行号
                column: int32                                     // 列号
              }
            }
          ]
        },
        override_contents?: string,                               // 可选，覆盖内容
        original_contents?: string,                               // 可选，原始内容
        detailed_lines: [                                         // DetailedLine 详细行信息
          {
            text: string,                                         // 行文本
            line_number: float,                                   // 行号（浮点数用于支持虚拟行）
            is_signature: bool                                    // 是否为签名行
          }
        ],
        file_git_context: {                                       // FileGit Git上下文
          commits: [                                              // GitCommit 提交数组
            {
              commit: string,                                     // 提交哈希
              author: string,                                     // 作者
              date: string,                                       // 提交日期
              message: string                                     // 提交消息
            }
          ]
        }
      },
      score: float                                                // 检索相关性分数
    }
  ]
}
```

### 响应格式 (SSE 流)

服务器通过 Server-Sent Events (SSE) 返回流式响应。每个事件包含 `type` 字段区分消息类型。

---

#### 事件类型

**1. model_info** - 模型信息
```typescript
{
  type: "model_info",
  is_fused_cursor_prediction_model: bool,
  is_multidiff_model: bool
}
```

---

**2. range_replace** - 范围替换
```typescript
{
  type: "range_replace",
  start_line_number: int32,                  // 起始行（1-based）
  end_line_number_inclusive: int32,          // 结束行（1-based，包含）
  binding_id?: string,
  should_remove_leading_eol?: bool
}
```
> **注意**：替换的文本内容通过后续的 `text` 事件发送

---

**3. text** - 文本内容
```typescript
{
  type: "text",
  text: string
}
```
> **说明**：流式输出的主要内容，客户端应累积

---

**4. cursor_prediction** - 光标预测
```typescript
{
  type: "cursor_prediction",
  relative_path: string,
  line_number_one_indexed: int32,
  expected_content: string,
  should_retrigger_cpp: bool,
  binding_id?: string
}
```

---

**5. done_edit** - 编辑完成
```typescript
{
  type: "done_edit"
}
```

---

**6. begin_edit** - 编辑开始
```typescript
{
  type: "begin_edit"
}
```

---

**7. done_stream** - 内容阶段结束
```typescript
{
  type: "done_stream"
}
```
> **说明**：之后可能会有 `debug` 消息

---

**8. debug** - 调试信息
```typescript
{
  type: "debug",
  model_input?: string,
  model_output?: string,
  stream_time?: string,
  total_time?: string,
  ttft_time?: string,
  server_timing?: string
}
```
> **说明**：可能出现多次，前端可累积用于统计

---

**9. error** - 错误
```typescript
{
  type: "error",
  error: {
    code: uint16,                            // 非零错误码
    type: string,                            // 错误类型
    details?: {                              // 可选的详细信息
      title: string,
      detail: string,
      additional_info?: Record<string, string>
    }
  }
}
```

---

**10. stream_end** - 流结束
```typescript
{
  type: "stream_end"
}
```

---

#### 典型消息序列

**基础场景：**
```
model_info
range_replace        // 指定范围
text (×N)           // 流式文本
done_edit
done_stream
debug (×N)          // 可选的多个调试消息
stream_end
```

**多次编辑：**
```
model_info
range_replace
text (×N)
done_edit
begin_edit          // 下一次编辑
range_replace
text (×N)
cursor_prediction   // 可选
done_edit
done_stream
stream_end
```

---

#### 客户端处理要点

1. **累积文本**
   - `range_replace` 指定范围
   - 累积后续所有 `text` 内容
   - `done_edit` 时应用变更

2. **换行符处理**
   - `should_remove_leading_eol=true` 时移除首个换行符

3. **多编辑会话**
   - `begin_edit` 标记新会话开始
   - `binding_id` 用于关联同一补全的多个编辑

4. **错误处理**
   - 流中出现 `error` 时，客户端应中止当前操作

5. **调试信息**
   - `done_stream` 后可能有多个 `debug` 消息
   - 前端可累积用于性能分析

## 鸣谢

感谢以下项目和贡献者:

- [cursor-api](https://github.com/wisdgod/cursor-api) - 本项目本身
- [zhx47/cursor-api](https://github.com/zhx47/cursor-api) - 提供了本项目起步阶段的主要参考
- [luolazyandlazy/cursorToApi](https://github.com/luolazyandlazy/cursorToApi) - zhx47/cursor-api基于此项目优化

## 关于赞助

非常感谢我自己持续8个多月的更新和大家的支持！你想赞助的话，清直接联系我，我一般不会拒绝。

有人说少个二维码来着，还是算了。如果觉得好用，给点支持。没啥大不了的，有空尽量做一点，只是心力确实消耗很大。

~~要不给我邮箱发口令红包？~~

**赞助一定要是你真心想给，也不强求。**

就算你给我赞助，我可能也不会区别对待你。我不想说你赞助多少就有什么，不想赞助失去本来的意味。

纯粹！
