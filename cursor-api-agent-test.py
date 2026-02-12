#!/usr/bin/env python3
"""
Cursor API Agent Mode 验证脚本

测试 model_call_id 复用是否能减少 request 计费
"""

import requests
import uuid
import json
import time
from typing import List, Dict, Optional

class CursorAgentTester:
    """Cursor Agent 模式测试器"""
    
    def __init__(self, api_url: str, auth_token: str):
        self.api_url = api_url.rstrip('/')
        self.auth_token = auth_token
        self.session = requests.Session()
        self.session.headers.update({
            "Authorization": f"Bearer {auth_token}",
            "Content-Type": "application/json"
        })
    
    def test_traditional_mode(self, iterations: int = 3) -> Dict:
        """测试传统模式（每次调用独立）"""
        print(f"\n{'='*60}")
        print("🔴 测试 1: 传统模式（每次独立 request）")
        print(f"{'='*60}\n")
        
        results = []
        
        for i in range(iterations):
            print(f"  📤 第 {i+1}/{iterations} 次调用...")
            
            response = self.session.post(
                f"{self.api_url}/v1/chat/completions",
                json={
                    "model": "claude-3.5-sonnet",
                    "messages": [
                        {"role": "user", "content": f"Test iteration {i+1}"}
                    ],
                    "stream": False
                }
            )
            
            if response.status_code == 200:
                results.append(response.json())
                print(f"  ✅ 成功")
            else:
                print(f"  ❌ 失败: {response.status_code}")
                print(f"     {response.text}")
            
            time.sleep(1)  # 避免过快
        
        print(f"\n📊 传统模式总结:")
        print(f"   调用次数: {iterations}")
        print(f"   预期 requests: {iterations} ❌")
        print(f"   （每次调用都计为 1 request）\n")
        
        return {
            "mode": "traditional",
            "iterations": iterations,
            "expected_requests": iterations,
            "results": results
        }
    
    def test_agent_mode_with_model_call_id(self, iterations: int = 3) -> Dict:
        """测试 Agent 模式（复用 model_call_id）"""
        print(f"\n{'='*60}")
        print("🟢 测试 2: Agent 模式（复用 model_call_id）")
        print(f"{'='*60}\n")
        
        # 生成固定的 model_call_id
        model_call_id = str(uuid.uuid4())
        print(f"  🔑 model_call_id: {model_call_id}\n")
        
        results = []
        
        for i in range(iterations):
            # 构造包含 model_call_id 的 tool_call_id
            tool_call_id_base = f"call_{uuid.uuid4()}"
            tool_call_id = f"{tool_call_id_base}\nmc_{model_call_id}"
            
            print(f"  📤 第 {i+1}/{iterations} 次调用...")
            print(f"     tool_call_id: {tool_call_id[:40]}...")
            
            # 尝试在 headers 中传递 model_call_id
            headers = {
                "X-Model-Call-ID": model_call_id,
                "X-Tool-Call-ID": tool_call_id,
            }
            
            response = self.session.post(
                f"{self.api_url}/v1/chat/completions",
                json={
                    "model": "claude-3.5-sonnet",
                    "messages": [
                        {
                            "role": "user", 
                            "content": f"Agent iteration {i+1}"
                        }
                    ],
                    # 尝试在 metadata 中传递
                    "metadata": {
                        "model_call_id": model_call_id,
                        "tool_call_id": tool_call_id,
                        "is_agent_mode": True,
                        "iteration": i
                    },
                    "stream": False
                },
                headers=headers
            )
            
            if response.status_code == 200:
                results.append(response.json())
                print(f"  ✅ 成功")
            else:
                print(f"  ❌ 失败: {response.status_code}")
                print(f"     {response.text}")
            
            time.sleep(1)
        
        print(f"\n📊 Agent 模式总结:")
        print(f"   调用次数: {iterations}")
        print(f"   预期 requests: 1 ✅ (如果生效)")
        print(f"   （所有调用共享 model_call_id）\n")
        
        return {
            "mode": "agent",
            "model_call_id": model_call_id,
            "iterations": iterations,
            "expected_requests": 1,
            "results": results
        }
    
    def check_usage(self) -> Optional[Dict]:
        """检查当前 token 用量"""
        print("\n🔍 检查账户用量...\n")
        
        try:
            # 尝试获取用量信息（如果 cursor-api 暴露了这个接口）
            response = self.session.get(f"{self.api_url}/tokens/get")
            if response.status_code == 200:
                data = response.json()
                return data
            else:
                print("⚠️  无法自动获取用量，请手动检查 Cursor 后台")
                return None
        except Exception as e:
            print(f"⚠️  获取用量失败: {e}")
            print("   请手动检查 Cursor 后台用量")
            return None
    
    def run_comparison_test(self, iterations: int = 3):
        """运行对比测试"""
        print("\n" + "="*60)
        print("🧪 Cursor Agent Mode 验证测试")
        print("="*60)
        
        # 检查初始用量
        print("\n📌 步骤 1: 记录初始用量")
        initial_usage = self.check_usage()
        if initial_usage:
            print(f"   初始用量: {json.dumps(initial_usage, indent=2)}")
        
        input("\n按 Enter 继续测试 1（传统模式）...")
        
        # 测试 1: 传统模式
        traditional_result = self.test_traditional_mode(iterations)
        
        print("\n⏸️  暂停 30 秒，让 Cursor 后台更新用量...")
        time.sleep(30)
        
        after_traditional_usage = self.check_usage()
        
        input("\n按 Enter 继续测试 2（Agent 模式）...")
        
        # 测试 2: Agent 模式
        agent_result = self.test_agent_mode_with_model_call_id(iterations)
        
        print("\n⏸️  暂停 30 秒，让 Cursor 后台更新用量...")
        time.sleep(30)
        
        final_usage = self.check_usage()
        
        # 总结
        print("\n" + "="*60)
        print("📊 测试结果总结")
        print("="*60)
        
        print(f"\n传统模式:")
        print(f"  调用次数: {iterations}")
        print(f"  预期消耗: {iterations} requests")
        
        print(f"\nAgent 模式:")
        print(f"  调用次数: {iterations}")
        print(f"  预期消耗: 1 request (如果生效)")
        
        print(f"\n💡 验证方法:")
        print(f"  1. 检查 Cursor 后台用量")
        print(f"  2. 如果传统模式 +{iterations}，Agent 模式 +1")
        print(f"  3. 说明 model_call_id 复用生效！✅")
        
        print(f"\n🔗 Cursor 用量查看:")
        print(f"  https://www.cursor.com/settings")
        
        return {
            "traditional": traditional_result,
            "agent": agent_result,
            "initial_usage": initial_usage,
            "after_traditional_usage": after_traditional_usage,
            "final_usage": final_usage
        }


def main():
    """主函数"""
    print("""
╔═══════════════════════════════════════════════════════════════╗
║                                                               ║
║   🧪 Cursor API Agent Mode 验证脚本                           ║
║                                                               ║
║   测试目标：验证 model_call_id 复用是否能减少 request 计费    ║
║                                                               ║
╚═══════════════════════════════════════════════════════════════╝
""")
    
    # 配置
    API_URL = input("请输入 cursor-api 地址 (默认: http://localhost:3000): ").strip()
    if not API_URL:
        API_URL = "http://localhost:3000"
    
    AUTH_TOKEN = input("请输入 API Token: ").strip()
    if not AUTH_TOKEN:
        print("❌ 必须提供 Token")
        return
    
    iterations = input("测试迭代次数 (默认: 3): ").strip()
    iterations = int(iterations) if iterations else 3
    
    # 创建测试器
    tester = CursorAgentTester(API_URL, AUTH_TOKEN)
    
    # 运行测试
    try:
        results = tester.run_comparison_test(iterations)
        
        # 保存结果
        output_file = "cursor-agent-test-results.json"
        with open(output_file, 'w') as f:
            json.dump(results, f, indent=2)
        
        print(f"\n💾 完整结果已保存到: {output_file}")
        
    except KeyboardInterrupt:
        print("\n\n⚠️  测试中断")
    except Exception as e:
        print(f"\n❌ 测试失败: {e}")
        import traceback
        traceback.print_exc()


if __name__ == "__main__":
    main()
