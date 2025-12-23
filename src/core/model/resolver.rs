use super::Models;
use crate::app::model::{AppConfig, UsageCheck};
use crate::core::constant::{FREE_MODELS, get_static_id};

static mut BYPASS_MODEL_VALIDATION: bool = false;

pub fn init_resolver() {
    unsafe {
        BYPASS_MODEL_VALIDATION =
            crate::common::utils::parse_from_env("BYPASS_MODEL_VALIDATION", false)
    }
}

#[derive(Clone, Copy)]
pub struct ExtModel {
    pub id: &'static str,
    pub is_image: bool,
    pub is_thinking: bool,
    pub web: bool,
    pub max: bool,
}

impl ExtModel {
    /// 从字符串解析 ExtModel
    /// 支持 "-online" 和 "-max" 后缀
    /// 当 BYPASS_MODEL_VALIDATION 为 true 时，在正常验证失败后仍返回结果
    #[inline]
    pub fn from_str(s: &str) -> Option<Self> {
        // 处理 online 后缀
        let (base_str, web) = s.strip_suffix("-online").map_or((s, false), |base| (base, true));

        // 先尝试直接匹配（可能带有 -max 后缀）
        if let Some(raw) = Models::find_id(base_str) {
            return Some(Self {
                id: raw.server_id,
                is_image: raw.is_image,
                is_thinking: raw.is_thinking,
                web,
                max: !raw.is_non_max,
            });
        }

        // 处理 max 后缀
        let (model_str, max) =
            base_str.strip_suffix("-max").map_or((base_str, false), |base| (base, true));

        // 如果有 -max 后缀，尝试匹配不带后缀的模型名
        if max
            && let Some(raw) = Models::find_id(model_str)
            && raw.is_max
        {
            return Some(Self {
                id: raw.server_id,
                is_image: raw.is_image,
                is_thinking: raw.is_thinking,
                web,
                max,
            });
        }

        // 正常验证都失败后，检查是否绕过验证
        if unsafe { BYPASS_MODEL_VALIDATION } && !model_str.is_empty() {
            let id = get_static_id(model_str);
            return Some(Self {
                id,
                is_image: true,
                // 这里的检测是孱弱的，尽管如此，影响可能不大
                is_thinking: id.contains("-thinking")
                    || {
                        let bytes = id.as_bytes();
                        bytes.len() >= 2 && bytes[0] == b'o' && bytes[1].is_ascii_digit()
                    }
                    || id.starts_with("gemini-2.5-")
                    || id.starts_with("deepseek-r1")
                    || id.starts_with("grok-4"),
                web,
                max,
            });
        }

        None
    }

    pub fn is_usage_check(&self, usage_check: Option<UsageCheck>) -> bool {
        match usage_check.unwrap_or(AppConfig::get_usage_check()) {
            UsageCheck::None => false,
            UsageCheck::Default => !FREE_MODELS.contains(&self.id),
            UsageCheck::All => true,
            UsageCheck::Custom(models) => models.contains(&self.id),
        }
    }

    #[inline(always)]
    pub const fn id(&self) -> ::prost::ByteStr {
        ::prost::ByteStr::from_static(self.id)
    }
}
