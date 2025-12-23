use parking_lot::Mutex;
use manually_init::ManuallyInit;

use crate::app::constant::EMPTY_STRING;

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;

static DISPLAY_NAME_CACHE: ManuallyInit<Mutex<HashMap<&'static str, &'static str>>> =
    ManuallyInit::new();

pub fn init_display_name_cache() {
    DISPLAY_NAME_CACHE.init(Mutex::new(HashMap::default()))
}

/// 计算 AI 模型标识符的显示名称。
///
/// # 转换规则
///
/// 1. **版本号合并**：单数字-单数字 → 小数点版本号（如 `3-5` → `3.5`）
/// 2. **日期保留**：日期格式在括号中显示
///    - `YYYY-MM-DD` 格式：`2024-04-09` → `(2024-04-09)`
///    - `MM-DD` 格式：`03-25` → `(03-25)`  
///    - `MMDD` 格式：`0528` → `(05-28)`
/// 3. **时间标记**：`latest` 和 `legacy` 在括号中显示
/// 4. **特殊前缀**：
///    - `gpt` → `GPT`
///    - `o1`/`o3`/`o4` → `O1`/`O3`/`O4`
/// 5. **版本标记**：`v`/`r`/`k` 开头的版本号首字母大写（如 `v3.1` → `V3.1`）
/// 6. **分隔符转换**：其他 `-` 转为空格，各部分首字母大写
///
/// # Arguments
///
/// * `identifier` - AI 模型的原始标识符字符串
///
/// # Returns
///
/// * `&'static str` - 格式化后的显示名称（缓存）
///
/// # Examples
///
/// ```
/// // 基础转换
/// assert_eq!(calculate_display_name("claude-3-5-sonnet"), "Claude 3.5 Sonnet");
/// assert_eq!(calculate_display_name("deepseek-v3"), "Deepseek V3");
///
/// // GPT 特殊处理
/// assert_eq!(calculate_display_name("gpt-4o"), "GPT 4o");
/// assert_eq!(calculate_display_name("gpt-3.5-turbo"), "GPT 3.5 Turbo");
///
/// // 日期处理（放入括号）
/// assert_eq!(calculate_display_name("gpt-4-turbo-2024-04-09"), "GPT 4 Turbo (2024-04-09)");
/// assert_eq!(calculate_display_name("gemini-2.5-pro-exp-03-25"), "Gemini 2.5 Pro Exp (03-25)");
/// assert_eq!(calculate_display_name("deepseek-r1-0528"), "Deepseek R1 (05-28)");
///
/// // 时间标记（放入括号）
/// assert_eq!(calculate_display_name("gemini-2.5-pro-latest"), "Gemini 2.5 Pro (latest)");
/// assert_eq!(calculate_display_name("claude-4-opus-legacy"), "Claude 4 Opus (legacy)");
///
/// // O 系列
/// assert_eq!(calculate_display_name("o3-mini"), "O3 Mini");
///
/// // 边界情况
/// assert_eq!(calculate_display_name("version-10-beta"), "Version 10 Beta"); // 10 不是单数字
/// assert_eq!(calculate_display_name("model-1-test-9-case"), "Model 1 Test 9 Case"); // 单数字不相邻
/// ```
pub fn calculate_display_name(identifier: &'static str) -> &'static str {
    if let Some(cached) = DISPLAY_NAME_CACHE.lock().get(identifier) {
        return cached;
    }

    let result = if identifier.is_empty() {
        EMPTY_STRING
    } else {
        crate::leak::intern_static(calculate_display_name_internal(identifier))
    };

    DISPLAY_NAME_CACHE.lock().insert(identifier, result);

    result
}

mod formatter;
mod parser;
mod tokenizer;

use formatter::format_output;
use parser::parse_patterns;
use tokenizer::tokenize;

#[inline(always)]
fn calculate_display_name_internal(identifier: &'static str) -> String {
    let tokens = tokenize(identifier);
    let patterns = parse_patterns(tokens);
    format_output(patterns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_output() {
        let test_cases = vec![
            // 基础测试
            "",
            "default",
            "sonic",
            // GPT 系列
            "gpt",
            "gpt-4",
            "gpt-4o",
            "gpt-3.5-turbo",
            "gpt-4-turbo-2024-04-09",
            "gpt-5-high-fast",
            "gpt-5-mini",
            // O 系列
            "o1",
            "o3",
            "o3-mini",
            "o3-pro",
            "o4-mini",
            "o1-preview",
            // Claude 系列
            "claude-3-opus",
            "claude-3.5-sonnet",
            "claude-3-5-sonnet",
            "claude-4-opus-thinking",
            "claude-4.1-opus-thinking",
            "claude-3.7-sonnet-thinking",
            "claude-4-opus-legacy",
            "claude-4-opus-thinking-legacy",
            "claude-3-haiku-200k",
            "claude-3.5-sonnet-200k",
            // Gemini 系列
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemini-2.5-pro-latest",
            "gemini-2.5-pro-exp-03-25",
            "gemini-2.5-pro-preview-05-06",
            "gemini-2.0-flash-thinking-exp",
            "gemini-1.5-flash-500k",
            "gemini-2.5-pro-max",
            // Deepseek 系列
            "deepseek-v3",
            "deepseek-v3.1",
            "deepseek-r1",
            "deepseek-r1-0528",
            // Grok 系列
            "grok-2",
            "grok-3",
            "grok-3-beta",
            "grok-3-mini",
            "grok-4",
            "grok-4-0709",
            // 其他模型
            "cursor-small",
            "cursor-fast",
            "kimi-k2-instruct",
            "accounts/fireworks/models/kimi-k2-instruct",
            // 版本号测试
            "model-3-5",
            "model-3.5",
            "test-1-0",
            "version-10-beta",
            "model-10-5",
            "app-2.5-release",
            // 日期测试
            "release-2024-04-09",
            "update-03-25",
            "version-0528",
            "model-123",
            "test-12345",
            // 边界情况
            "model-1-2-3",
            "model-1-test-9-case",
            "model-fast-experimental-latest",
            "-start",
            "end-",
            "-",
            "a--b",
            "3-5",
            "2024",
            // 复杂组合
            "gpt-4.5-preview",
            "claude-3.5-sonnet-200k",
            "gemini-1-5-flash-500k",
        ];

        println!("\n=== Parser Test Results ===\n");

        for identifier in test_cases {
            println!("Input: {:?}", identifier);

            let tokens = tokenize(identifier);
            println!("  Tokens: {:?}", tokens);

            let patterns = parse_patterns(tokens);
            println!("  Main parts: {:?}", patterns.main_parts);
            println!("  Date parts: {:?}", patterns.date_parts);

            let output = format_output(patterns);
            println!("  Output: {:?}", output);
            println!("  ---");
        }
    }

    #[test]
    fn test_tokenizer_details() {
        println!("\n=== Tokenizer Details ===\n");

        let test_cases = vec![
            "gpt-4-turbo-2024-04-09",
            "claude-3.5-sonnet-thinking",
            "deepseek-r1-0528",
            "gemini-2.5-pro-exp-03-25",
        ];

        for identifier in test_cases {
            println!("Input: {:?}", identifier);
            let tokens = tokenize(identifier);

            for (i, token) in tokens.iter().enumerate() {
                println!("  Token[{}]: {:?}", i, token);
                println!("    content: {:?}", token.content);
                println!("    meta: {{");
                println!("      is_digit_only: {}", token.meta.is_digit_only);
                println!("      digit_count: {}", token.meta.digit_count);
                println!("      has_dot: {}", token.meta.has_dot);
                println!(
                    "      first_char: {:?} ({})",
                    token.meta.first_char as char, token.meta.first_char
                );
                println!("      len: {}", token.meta.len);
                println!("    }}");
            }
            println!();
        }
    }

    #[test]
    fn test_pattern_recognition() {
        println!("\n=== Pattern Recognition ===\n");

        let special_cases = vec![
            ("Single digit merge", "model-3-5-test"),
            ("Version with dot", "v3.1-release"),
            ("Date YYYY-MM-DD", "version-2024-04-09"),
            ("Date MM-DD", "update-03-25"),
            ("Date MMDD", "release-0528"),
            ("Latest marker", "model-latest"),
            ("Legacy marker", "model-legacy"),
            ("Mixed", "gpt-4.5-turbo-latest-2024-04-09"),
        ];

        for (description, identifier) in special_cases {
            println!("{}: {:?}", description, identifier);

            let tokens = tokenize(identifier);
            let patterns = parse_patterns(tokens);

            println!("  Patterns breakdown:");
            for (i, pattern) in patterns.main_parts.iter().enumerate() {
                println!("    Main[{}]: {:?}", i, pattern);
            }
            for (i, pattern) in patterns.date_parts.iter().enumerate() {
                println!("    Date[{}]: {:?}", i, pattern);
            }

            let output = format_output(patterns);
            println!("  Final: {:?}", output);
            println!();
        }
    }

    #[test]
    fn test_edge_cases() {
        println!("\n=== Edge Cases ===\n");

        let edge_cases = vec![
            ("Empty", ""),
            ("Single hyphen", "-"),
            ("Double hyphen", "--"),
            ("Start hyphen", "-model"),
            ("End hyphen", "model-"),
            ("Multiple hyphens", "a---b"),
            ("Just numbers", "123"),
            ("Just dot", "."),
            ("Dot at start", ".model"),
            ("Dot at end", "model."),
            ("Multiple dots", "model...test"),
            ("Mixed separators", "model-1.5-test"),
        ];

        for (description, identifier) in edge_cases {
            println!("{}: {:?}", description, identifier);

            let tokens = tokenize(identifier);
            println!("  Tokens: {:?}", tokens);

            let patterns = parse_patterns(tokens);
            let output = format_output(patterns);
            println!("  Output: {:?}", output);
            println!();
        }
    }
}
