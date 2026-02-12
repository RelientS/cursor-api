#!/usr/bin/env python3
"""简化版 Cursor Agent 测试"""
import requests
import uuid
import time

API_URL = "http://localhost:3001"
# 直接使用原始 token
TOKEN = "user_01KCQMK1CCZCCRKC29ABD22RMA::eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJhdXRoMHx1c2VyXzAxS0NRTUsxQ0NaQ0NSS0MyOUFCRDIyUk1BIiwidGltZSI6IjE3NzAwMDExNTYiLCJyYW5kb21uZXNzIjoiMTgwZTFkYWMtOTFkMy00MjljIiwiZXhwIjoxNzc1MTg1MTU2LCJpc3MiOiJodHRwczovL2F1dGhlbnRpY2F0aW9uLmN1cnNvci5zaCIsInNjb3BlIjoib3BlbmlkIHByb2ZpbGUgZW1haWwgb2ZmbGluZV9hY2Nlc3MiLCJhdWQiOiJodHRwczovL2N1cnNvci5jb20iLCJ0eXBlIjoid2ViIn0.1kWf7xZnZyYi5hA2FFfYUOlBRmuM1lfdoHJbUuxrRsw"

print("🧪 Cursor Agent Mode 简化测试\n")

# 测试 1: 单次调用
print("=" * 60)
print("测试 1: 单次正常调用")
print("=" * 60)

response = requests.post(
    f"{API_URL}/v1/chat/completions",
    headers={
        "Authorization": f"Bearer {TOKEN}",
        "Content-Type": "application/json"
    },
    json={
        "model": "claude-3.5-sonnet",
        "messages": [{"role": "user", "content": "Say hello"}],
        "stream": False
    }
)

print(f"Status: {response.status_code}")
if response.status_code == 200:
    print("✅ 基础调用成功")
    result = response.json()
    print(f"Response: {result.get('choices', [{}])[0].get('message', {}).get('content', '')[:100]}")
else:
    print(f"❌ 失败: {response.text}")
    print("\n⚠️  无法继续测试，cursor-api 配置有问题")
    exit(1)

print("\n" + "=" * 60)
print("测试 2: Agent 模式（复用 model_call_id）")
print("=" * 60)

# 生成固定的 model_call_id
model_call_id = str(uuid.uuid4())
print(f"\n🔑 model_call_id: {model_call_id}\n")

iterations = 3
for i in range(iterations):
    tool_call_id = f"call_{uuid.uuid4()}\nmc_{model_call_id}"
    
    print(f"📤 第 {i+1}/{iterations} 次调用 (共享 model_call_id)...")
    
    response = requests.post(
        f"{API_URL}/v1/chat/completions",
        headers={
            "Authorization": f"Bearer {TOKEN}",
            "Content-Type": "application/json",
            "X-Model-Call-ID": model_call_id,  # 尝试通过 header 传递
            "X-Tool-Call-ID": tool_call_id
        },
        json={
            "model": "claude-3.5-sonnet",
            "messages": [{"role": "user", "content": f"Agent test {i+1}"}],
            "stream": False,
            # 尝试在 metadata 中传递
            "metadata": {
                "model_call_id": model_call_id,
                "tool_call_id": tool_call_id,
                "is_agent_mode": True
            }
        }
    )
    
    if response.status_code == 200:
        print(f"  ✅ 成功")
    else:
        print(f"  ❌ 失败: {response.status_code}")
    
    time.sleep(1)

print("\n" + "=" * 60)
print("📊 测试完成")
print("=" * 60)
print("""
请手动检查 Cursor 后台用量：
https://www.cursor.com/settings

预期结果：
- 测试 1: +1 request
- 测试 2: +3 requests (如果没有复用) 或 +1 request (如果复用成功)

总计应该是 +4 (失败) 或 +2 (成功)
""")
