#!/usr/bin/env python3
"""
直接测试 Cursor 官方 API
验证 model_call_id 复用的可能性
"""
import requests
import uuid
import time
import json

CURSOR_API = "https://api2.cursor.sh"
TOKEN = "user_01KE45NR1288CH2B6DAE11NPVB::eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJnb29nbGUtb2F1dGgyfHVzZXJfMDFLRTQ1TlIxMjg4Q0gyQjZEQUUxMU5QVkIiLCJ0aW1lIjoiMTc2ODE5MjE1MiIsInJhbmRvbW5lc3MiOiIyNmY2ZGNkNi04YzY5LTQzZWQiLCJleHAiOjE3NzMzNzYxNTIsImlzcyI6Imh0dHBzOi8vYXV0aGVudGljYXRpb24uY3Vyc29yLnNoIiwic2NvcGUiOiJvcGVuaWQgcHJvZmlsZSBlbWFpbCBvZmZsaW5lX2FjY2VzcyIsImF1ZCI6Imh0dHBzOi8vY3Vyc29yLmNvbSIsInR5cGUiOiJ3ZWIifQ.J5atYNdwQFT2sLQ6hhGgcZ6K1n2bwixmZZWskoCyYv4"

print("🧪 测试 Cursor 官方 API\n")
print("=" * 60)

# 记录初始用量
print("⚠️  请先记录当前 Cursor 用量：https://www.cursor.com/settings\n")
input("记录完成后按 Enter 继续...")

print("\n" + "=" * 60)
print("测试 1: 传统模式（3次独立调用）")
print("=" * 60 + "\n")

for i in range(3):
    print(f"📤 第 {i+1}/3 次调用...")
    
    response = requests.post(
        f"{CURSOR_API}/aiserver.v1.ChatService/StreamUnifiedChatWithTools",
        headers={
            "Authorization": f"Bearer {TOKEN}",
            "Content-Type": "application/json"
        },
        json={
            "model": "claude-3.5-sonnet",
            "messages": [{"role": "user", "content": f"Test {i+1}"}]
        }
    )
    
    print(f"  Status: {response.status_code}")
    if response.status_code != 200:
        print(f"  Response: {response.text[:200]}")
    else:
        print(f"  ✅ 成功")
    
    time.sleep(2)

print("\n等待 30 秒...")
time.sleep(30)

print("\n" + "=" * 60)
print("测试 2: Agent 模式尝试（复用 model_call_id）")
print("=" * 60 + "\n")

model_call_id = str(uuid.uuid4())
print(f"🔑 model_call_id: {model_call_id}\n")

for i in range(3):
    tool_call_id = f"call_{uuid.uuid4()}\nmc_{model_call_id}"
    
    print(f"📤 第 {i+1}/3 次调用（共享 model_call_id）...")
    
    # 尝试在 headers 中传递
    response = requests.post(
        f"{CURSOR_API}/aiserver.v1.ChatService/StreamUnifiedChatWithTools",
        headers={
            "Authorization": f"Bearer {TOKEN}",
            "Content-Type": "application/json",
            "X-Model-Call-ID": model_call_id,
            "X-Tool-Call-ID": tool_call_id
        },
        json={
            "model": "claude-3.5-sonnet",
            "messages": [{"role": "user", "content": f"Agent test {i+1}"}]
        }
    )
    
    print(f"  Status: {response.status_code}")
    if response.status_code != 200:
        print(f"  Response: {response.text[:200]}")
    else:
        print(f"  ✅ 成功")
    
    time.sleep(2)

print("\n" + "=" * 60)
print("📊 测试完成")
print("=" * 60)

print("""
现在请检查 Cursor 用量：https://www.cursor.com/settings

预期结果：
- 测试 1: +3 requests
- 测试 2: +3 requests (如果没有复用) 或 +1 request (如果复用成功)

总计：
- 失败场景（没有复用）: +6 requests
- 成功场景（复用生效）: +4 requests
""")
