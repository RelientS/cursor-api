mod leak_ids;

use super::model::Model;
use crate::{
    app::{
        constant::UNKNOWN,
        model::{AppConfig, FetchMode, ModelIdSource},
    },
    common::model::{
        cached::JsonCached,
        raw_json::RawJson,
    },
    core::model::ModelsResponse,
};
use alloc::sync::Arc;
use arc_swap::ArcSwap;
use core::time::Duration;
use manually_init::ManuallyInit;
use std::time::Instant;

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;
type HashSet<K> = hashbrown::HashSet<K, ahash::RandomState>;

// AI 服务商
crate::define_typed_constants!(
    &'static str => {
        CURSOR = "cursor",
        ANTHROPIC = "anthropic",
        GOOGLE = "google",
        OPENAI = "openai",
        DEEPSEEK = "deepseek",
        XAI = "xai",
        FIREWORKS = "fireworks",
    }
);

macro_rules! def_const_models {
    ($($name:ident => $value:expr),+ $(,)?) => {
        // 定义常量
        $(
            const $name: &'static str = $value;
        )+

        // 生成 PHF map
        static MODEL_MAP: ::phf::Map<&'static str, &'static str> = ::phf::phf_map! {
            $(
                $value => $name,
            )+
        };
    };
}

// AI 模型
def_const_models!(
    // 默认模型
    DEFAULT => "default",

    // Anthropic 模型
    CLAUDE_4_5_OPUS_HIGH => "claude-4.5-opus-high",
    CLAUDE_4_5_OPUS_HIGH_THINKING => "claude-4.5-opus-high-thinking",
    CLAUDE_4_5_SONNET => "claude-4.5-sonnet",
    CLAUDE_4_5_SONNET_THINKING => "claude-4.5-sonnet-thinking",
    CLAUDE_4_5_HAIKU => "claude-4.5-haiku",
    CLAUDE_4_5_HAIKU_THINKING => "claude-4.5-haiku-thinking",
    CLAUDE_4_SONNET => "claude-4-sonnet",
    CLAUDE_4_SONNET_THINKING => "claude-4-sonnet-thinking",
    CLAUDE_4_SONNET_1M => "claude-4-sonnet-1m",
    CLAUDE_4_SONNET_1M_THINKING => "claude-4-sonnet-1m-thinking",
    CLAUDE_4_OPUS => "claude-4-opus",
    CLAUDE_4_1_OPUS => "claude-4.1-opus",
    CLAUDE_4_OPUS_THINKING => "claude-4-opus-thinking",
    CLAUDE_4_1_OPUS_THINKING => "claude-4.1-opus-thinking",
    CLAUDE_4_OPUS_LEGACY => "claude-4-opus-legacy",
    CLAUDE_4_OPUS_THINKING_LEGACY => "claude-4-opus-thinking-legacy",

    // Cursor 模型
    COMPOSER_1 => "composer-1",
    CURSOR_SMALL => "cursor-small",

    // Google 模型
    GEMINI_3_PRO => "gemini-3-pro",
    GEMINI_3_PRO_PREVIEW => "gemini-3-pro-preview",
    GEMINI_2_5_PRO_PREVIEW_05_06 => "gemini-2.5-pro-preview-05-06",
    GEMINI_2_5_PRO => "gemini-2.5-pro",
    GEMINI_2_5_FLASH_PREVIEW_05_20 => "gemini-2.5-flash-preview-05-20",
    GEMINI_2_5_FLASH => "gemini-2.5-flash",

    // OpenAI 模型
    GPT_5_1_CODEX_MAX => "gpt-5.1-codex-max",
    GPT_5_1_CODEX_MAX_HIGH => "gpt-5.1-codex-max-high",
    GPT_5_1_CODEX_MAX_LOW => "gpt-5.1-codex-max-low",
    GPT_5_1_CODEX_MAX_XHIGH => "gpt-5.1-codex-max-xhigh",
    GPT_5_1_CODEX_MAX_MEDIUM_FAST => "gpt-5.1-codex-max-medium-fast",
    GPT_5_1_CODEX_MAX_HIGH_FAST => "gpt-5.1-codex-max-high-fast",
    GPT_5_1_CODEX_MAX_LOW_FAST => "gpt-5.1-codex-max-low-fast",
    GPT_5_1_CODEX_MAX_XHIGH_FAST => "gpt-5.1-codex-max-xhigh-fast",
    GPT_5_1_CODEX => "gpt-5.1-codex",
    GPT_5_1_CODEX_HIGH => "gpt-5.1-codex-high",
    GPT_5_1_CODEX_FAST => "gpt-5.1-codex-fast",
    GPT_5_1_CODEX_HIGH_FAST => "gpt-5.1-codex-high-fast",
    GPT_5_1_CODEX_LOW => "gpt-5.1-codex-low",
    GPT_5_1_CODEX_LOW_FAST => "gpt-5.1-codex-low-fast",
    GPT_5_1 => "gpt-5.1",
    GPT_5_1_FAST => "gpt-5.1-fast",
    GPT_5_1_HIGH => "gpt-5.1-high",
    GPT_5_1_HIGH_FAST => "gpt-5.1-high-fast",
    GPT_5_1_LOW => "gpt-5.1-low",
    GPT_5_1_LOW_FAST => "gpt-5.1-low-fast",
    GPT_5_1_CODEX_MINI => "gpt-5.1-codex-mini",
    GPT_5_1_CODEX_MINI_HIGH => "gpt-5.1-codex-mini-high",
    GPT_5_1_CODEX_MINI_LOW => "gpt-5.1-codex-mini-low",
    O3 => "o3",
    GPT_4_1 => "gpt-4.1",
    GPT_5_MINI => "gpt-5-mini",
    GPT_5_NANO => "gpt-5-nano",
    O3_PRO => "o3-pro",
    GPT_5_PRO => "gpt-5-pro",

    // Deepseek 模型
    DEEPSEEK_R1_0528 => "deepseek-r1-0528",
    DEEPSEEK_V3_1 => "deepseek-v3.1",

    // XAI 模型
    GROK_3_MINI => "grok-3-mini",
    GROK_CODE_FAST_1 => "grok-code-fast-1",
    GROK_4 => "grok-4",
    GROK_4_0709 => "grok-4-0709",
    GROK_4_FAST_REASONING => "grok-4-fast-reasoning",
    GROK_4_FAST_NON_REASONING => "grok-4-fast-non-reasoning",

    // MoonshotAI 模型
    KIMI_K2_INSTRUCT => "kimi-k2-instruct",
    ACCOUNTS_FIREWORKS_MODELS_KIMI_K2_INSTRUCT => "accounts/fireworks/models/kimi-k2-instruct",

    // Cursor 模型 (legacy)
    CURSOR_FAST => "cursor-fast",

    // Deepseek 模型 (legacy)
    DEEPSEEK_V3 => "deepseek-v3",

    // OpenAI 模型 (legacy)
    GPT_4O_MINI => "gpt-4o-mini",
);

/// 通过 PHF 快速查找模型 ID
#[inline]
fn get_model_const(id: &str) -> Option<&'static str> { MODEL_MAP.get(id).copied() }

/// 获取静态字符串引用，如果不存在则 intern
#[inline]
pub fn get_static_id(id: &str) -> &'static str {
    match get_model_const(id) {
        Some(id) => id,
        None => leak_ids::intern(id),
    }
}

static INSTANCE: ManuallyInit<ArcSwap<Models>> = ManuallyInit::new();

pub(super) static MODEL_ID_SOURCE: ManuallyInit<ModelIdSource> = ManuallyInit::new();

macro_rules! create_models {
    ($($owner:ident => [$($model:expr,)+]),* $(,)?) => {
        pub fn create_models() {
            // ModelIds 只在这个作用域内有效
            #[derive(Debug, Clone, Copy)]
            struct ModelIds {
                id: &'static str,
                client_id: &'static str,
                server_id: &'static str,
            }

            #[allow(unused)]
            impl ModelIds {
                const fn new(id: &'static str) -> Self {
                    Self {
                        id,
                        client_id: id,
                        server_id: id,
                    }
                }

                const fn with_client_id(mut self, client_id: &'static str) -> Self {
                    self.client_id = client_id;
                    self
                }

                const fn with_server_id(mut self, server_id: &'static str) -> Self {
                    self.server_id = server_id;
                    self
                }

                const fn with_same_id(mut self, same_id: &'static str) -> Self {
                    self.client_id = same_id;
                    self.server_id = same_id;
                    self
                }
            }

            MODEL_ID_SOURCE.init(ModelIdSource::from_env());

            let models = vec![
                $($(
                    {
                        #[allow(non_upper_case_globals)]
                        const model_ids: ModelIds = $model;
                        Model {
                            id: model_ids.id,
                            server_id: model_ids.server_id,
                            client_id: model_ids.client_id,
                            owned_by: $owner,
                            is_thinking: SUPPORTED_THINKING_MODELS.contains(&model_ids.id),
                            is_image: SUPPORTED_IMAGE_MODELS.contains(&model_ids.id),
                            is_max: SUPPORTED_MAX_MODELS.contains(&model_ids.id)
                                || MAX_MODELS.contains(&model_ids.id),
                            is_non_max: !MAX_MODELS.contains(&model_ids.id),
                        }
                    },
                )+)*
            ];

            leak_ids::init();

            let mut ids = Vec::with_capacity(models.len() * 4);
            for model in &models {
                let id = model.id();

                push_ids(&mut ids, id);

                if model.is_max && model.is_non_max {
                    push_ids(&mut ids, &format!("{id}-max"));
                }
            }
            let find_ids = HashMap::from_iter(models.iter().enumerate().map(|(i, m)| (m.id(), i)));

            INSTANCE.init(ArcSwap::from_pointee(Models {
                models: __unwrap!(JsonCached::new(ModelsResponse(models))),
                raw_models: None,
                ids: __unwrap!(JsonCached::new(ids)),
                find_ids,
                last_update: Instant::now(),
            }))
        }
    };
}

pub struct Models {
    models: JsonCached<ModelsResponse>,
    raw_models: Option<JsonCached<crate::core::aiserver::v1::AvailableModelsResponse>>,
    ids: JsonCached<Vec<&'static str>>,

    find_ids: HashMap<&'static str, usize>,
    last_update: Instant,
}

impl Models {
    #[inline(always)]
    pub fn get() -> arc_swap::Guard<Arc<Self>> { INSTANCE.load() }

    #[inline]
    pub fn get_models_cache() -> RawJson { Self::get().models.cache() }

    #[inline]
    pub fn get_raw_models_cache() -> Option<RawJson> {
        Self::get().raw_models.as_ref().map(JsonCached::cache)
    }

    #[inline]
    pub fn get_ids_cache() -> RawJson { Self::get().ids.cache() }

    #[inline]
    pub fn last_update_elapsed() -> Duration { Self::get().last_update.elapsed() }

    // 克隆所有模型
    // pub fn cloned() -> Vec<Model> {
    //     Self::get().models.as_ref().clone()
    // }

    // 检查模型是否存在
    // pub fn exists(model_id: &str) -> bool {
    //     Self::get().models.iter().any(|m| m.id == model_id)
    // }

    // 查找模型并返回其 ID
    pub fn find_id(id: &str) -> Option<Model> {
        let guard = Self::get();
        guard.find_ids.get(id).map(|&i| *unsafe { guard.models.get_unchecked(i) })
    }

    // 返回所有模型 ID 的列表
    // pub fn ids() -> Arc<Vec<&'static str>> { Self::get().cached_ids.clone() }

    // 写入方法
    pub fn update(
        available_models: crate::core::aiserver::v1::AvailableModelsResponse,
    ) -> Result<(), &'static str> {
        if available_models.models.is_empty() {
            return Err("Models list cannot be empty");
        }

        let guard = Self::get();
        if let Some(ref raw) = guard.raw_models
            && raw.is_subset_equal(&available_models)
        {
            return Ok(());
        }

        // 内联辅助函数：将服务器模型转换为内部模型表示
        #[inline]
        fn convert_model(
            model: &crate::core::aiserver::v1::available_models_response::AvailableModel,
        ) -> Model {
            let (id, client_id, server_id) = {
                let id = get_static_id(model.name.as_str());
                let client_id = if let Some(ref client_id) = model.client_display_name
                    && client_id != id
                {
                    get_static_id(client_id.as_str())
                } else {
                    id
                };
                let server_id = if let Some(ref server_id) = model.server_model_name
                    && server_id != id
                {
                    get_static_id(server_id.as_str())
                } else {
                    id
                };
                (id, client_id, server_id)
            };
            let owned_by = {
                (|server_id: &str| -> Option<&'static str> {
                    let bytes = server_id.as_bytes();
                    if !byte_str::is_valid_ascii(bytes) {
                        return None;
                    }

                    #[allow(clippy::get_first)]
                    match *bytes.get(0)? {
                        b'g' => match *bytes.get(1)? {
                            b'p' => Some(OPENAI), // g + p → "gp" (gpt)
                            b'e' => Some(GOOGLE), // g + e → "ge" (gemini)
                            b'r' => Some(XAI),    // g + r → "gr" (grok)
                            _ => None,
                        },
                        b'o' => match *bytes.get(1)? {
                            b'1' | b'3' | b'4' => Some(OPENAI), // o1/o3/o4 系列
                            _ => None,
                        },
                        b'c' => match *bytes.get(1)? {
                            b'l' => Some(ANTHROPIC), // c + l → "cl" (claude)
                            b'u' |                   // c + u → "cu" (cursor)
                            b'o' => Some(CURSOR),    // c + o → "co" (composer)
                            _ => None,
                        },
                        b'd' => match *bytes.get(1)? {
                            b'e' => match *bytes.get(2)? {
                                b'e' => Some(DEEPSEEK), // d + e + e → "dee" (deepseek)
                                _ => None,
                            },
                            _ => None,
                        },
                        b'a' => {
                            if bytes.len() > 26 && unsafe { *bytes.get_unchecked(9) } == b'f' {
                                Some(FIREWORKS)
                            } else {
                                None
                            }
                        }
                        // 其他情况
                        _ => None,
                    }
                })(server_id)
                .unwrap_or(UNKNOWN)
            };
            let is_thinking = model.supports_thinking.unwrap_or_default();
            let is_image =
                if server_id == DEFAULT { true } else { model.supports_images.unwrap_or_default() };
            let is_max = model.supports_max_mode.unwrap_or_default();
            let is_non_max = model.supports_non_max_mode.unwrap_or_default();

            Model { id, client_id, owned_by, server_id, is_thinking, is_image, is_max, is_non_max }
        }

        // 先获取当前模型列表的引用
        let current_models = &guard.models;

        // 根据不同的FetchMode来确定如何处理模型
        let new_models: Vec<_> = match AppConfig::raw_model_fetch_mode() {
            FetchMode::Truncate => {
                // 完全使用新获取的模型列表
                available_models.models.iter().map(convert_model).collect()
            }
            FetchMode::AppendTruncate => {
                // 先收集所有在available_models中的模型ID
                let new_model_ids: HashSet<_> = available_models
                    .models
                    .iter()
                    .map(|model| get_static_id(model.name.as_str()))
                    .collect();

                // 保留current_models中不在new_model_ids中的模型
                let mut result: Vec<_> = current_models
                    .iter()
                    .filter(|model| !new_model_ids.contains(&model.id))
                    .cloned()
                    .collect();

                // 添加所有新模型
                result.extend(available_models.models.iter().map(convert_model));

                result
            }
            FetchMode::Append => {
                // 只添加不存在的模型
                let existing_ids: HashSet<_> =
                    current_models.iter().map(|model| model.id).collect();

                // 复制现有模型
                let mut result = current_models.to_vec();

                // 仅添加ID不存在的新模型
                result.extend(
                    available_models
                        .models
                        .iter()
                        .filter(|model| !existing_ids.contains(&model.name.as_str()))
                        .map(convert_model),
                );

                result
            }
        };

        // 计算模型变化
        let old_ids: HashSet<_> = guard.models.iter().map(|m| m.id()).collect();
        let new_ids: HashSet<_> = new_models.iter().map(|m| m.id()).collect();

        // 获取需要添加和移除的模型
        let to_add: Vec<_> = new_models.iter().filter(|m| !old_ids.contains(&m.id())).collect();

        let to_remove: Vec<_> =
            guard.models.iter().filter(|m| !new_ids.contains(&m.id())).collect();

        // 从缓存中移除不再需要的ID
        let mut ids: Vec<_> = guard
            .ids
            .iter()
            .filter(|&&id| {
                !to_remove.iter().any(|m| {
                    let mid = m.id();

                    // 基本ID匹配
                    if id == mid {
                        return true;
                    }

                    // 处理带有"-online"后缀的情况
                    if let Some(base) = id.strip_suffix("-online") {
                        if base == mid {
                            return true;
                        }
                        // 处理同时有"-max"和"-online"后缀的情况（即"-max-online"）
                        if let Some(base_without_max) = base.strip_suffix("-max")
                            && base_without_max == mid
                        {
                            return true;
                        }
                        false
                    }
                    // 处理仅带有"-max"后缀的情况
                    else if let Some(base) = id.strip_suffix("-max") {
                        base == mid
                    } else {
                        false
                    }
                })
            })
            .copied()
            .collect();

        // 只为新增的模型创建ID组合
        for model in to_add {
            let id = model.id();

            push_ids(&mut ids, id);

            if model.is_max && model.is_non_max {
                push_ids(&mut ids, &format!("{id}-max"));
            }
        }

        // 更新数据和时间戳
        let find_ids = HashMap::from_iter(new_models.iter().enumerate().map(|(i, m)| (m.id(), i)));

        INSTANCE.store(Arc::new(Models {
            models: __unwrap!(JsonCached::new(ModelsResponse(new_models))),
            raw_models: Some(__unwrap!(JsonCached::new(available_models))),
            ids: __unwrap!(JsonCached::new(ids)),
            find_ids,
            last_update: Instant::now(),
        }));

        Ok(())
    }
}

#[inline]
fn push_ids(ids: &mut Vec<&'static str>, id: &str) {
    let id = leak_ids::add(id);
    ids.push(id.0);
    ids.push(id.1);
}

create_models! {
    CURSOR => [
        ModelIds::new(DEFAULT)
            .with_client_id("Auto"),
        ModelIds::new(COMPOSER_1)
            .with_client_id("Composer 1"),
        ModelIds::new(CURSOR_SMALL)
            .with_client_id("Cursor Small"),
        ModelIds::new(CURSOR_FAST)
            .with_client_id("Cursor Fast"),
    ],

    ANTHROPIC => [
        ModelIds::new(CLAUDE_4_5_OPUS_HIGH)
            .with_client_id("Opus 4.5"),
        ModelIds::new(CLAUDE_4_5_OPUS_HIGH_THINKING)
            .with_client_id("Opus 4.5"),
        ModelIds::new(CLAUDE_4_5_SONNET)
            .with_client_id("Sonnet 4.5"),
        ModelIds::new(CLAUDE_4_5_SONNET_THINKING)
            .with_client_id("Sonnet 4.5"),
        ModelIds::new(CLAUDE_4_5_HAIKU)
            .with_client_id("Haiku 4.5"),
        ModelIds::new(CLAUDE_4_5_HAIKU_THINKING)
            .with_client_id("Haiku 4.5"),
        ModelIds::new(CLAUDE_4_OPUS)
            .with_client_id("Opus 4.1")
            .with_server_id(CLAUDE_4_1_OPUS),
        ModelIds::new(CLAUDE_4_OPUS_THINKING)
            .with_client_id("Opus 4.1")
            .with_server_id(CLAUDE_4_1_OPUS_THINKING),
        ModelIds::new(CLAUDE_4_OPUS_LEGACY)
            .with_client_id("Opus 4")
            .with_server_id(CLAUDE_4_OPUS),
        ModelIds::new(CLAUDE_4_OPUS_THINKING_LEGACY)
            .with_client_id("Opus 4")
            .with_server_id(CLAUDE_4_OPUS_THINKING),
        ModelIds::new(CLAUDE_4_SONNET)
            .with_client_id("Sonnet 4"),
        ModelIds::new(CLAUDE_4_SONNET_THINKING)
            .with_client_id("Sonnet 4"),
        ModelIds::new(CLAUDE_4_SONNET_1M)
            .with_client_id("Sonnet 4 1M"),
        ModelIds::new(CLAUDE_4_SONNET_1M_THINKING)
            .with_client_id("Sonnet 4 1M"),
    ],

    GOOGLE => [
        ModelIds::new(GEMINI_3_PRO)
            .with_client_id("Gemini 3 Pro")
            .with_server_id(GEMINI_3_PRO_PREVIEW),
        ModelIds::new(GEMINI_2_5_PRO_PREVIEW_05_06)
            .with_client_id("Gemini 2.5 Pro")
            .with_server_id(GEMINI_2_5_PRO),
        ModelIds::new(GEMINI_2_5_FLASH_PREVIEW_05_20)
            .with_client_id("Gemini 2.5 Flash")
            .with_server_id(GEMINI_2_5_FLASH),
    ],

    OPENAI => [
        ModelIds::new(GPT_5_1_CODEX_MAX)
            .with_client_id("GPT-5.1 Codex Max"),
        ModelIds::new(GPT_5_1_CODEX_MAX_HIGH)
            .with_client_id("GPT-5.1 Codex Max High"),
        ModelIds::new(GPT_5_1_CODEX_MAX_LOW)
            .with_client_id("GPT-5.1 Codex Max Low"),
        ModelIds::new(GPT_5_1_CODEX_MAX_XHIGH)
            .with_client_id("GPT-5.1 Codex Max Extra High"),
        ModelIds::new(GPT_5_1_CODEX_MAX_MEDIUM_FAST)
            .with_client_id("GPT-5.1 Codex Max Medium Fast"),
        ModelIds::new(GPT_5_1_CODEX_MAX_HIGH_FAST)
            .with_client_id("GPT-5.1 Codex Max High Fast"),
        ModelIds::new(GPT_5_1_CODEX_MAX_LOW_FAST)
            .with_client_id("GPT-5.1 Codex Max Low Fast"),
        ModelIds::new(GPT_5_1_CODEX_MAX_XHIGH_FAST)
            .with_client_id("GPT-5.1 Codex Max Extra High Fast"),
        ModelIds::new(GPT_5_1_CODEX)
            .with_client_id("GPT-5.1 Codex"),
        ModelIds::new(GPT_5_1_CODEX_HIGH)
            .with_client_id("GPT-5.1 Codex High"),
        ModelIds::new(GPT_5_1_CODEX_FAST)
            .with_client_id("GPT-5.1 Codex Fast"),
        ModelIds::new(GPT_5_1_CODEX_HIGH_FAST)
            .with_client_id("GPT-5.1 Codex High Fast"),
        ModelIds::new(GPT_5_1_CODEX_LOW)
            .with_client_id("GPT-5.1 Codex Low"),
        ModelIds::new(GPT_5_1_CODEX_LOW_FAST)
            .with_client_id("GPT-5.1 Codex Low Fast"),
        ModelIds::new(GPT_5_1)
            .with_client_id("GPT-5.1"),
        ModelIds::new(GPT_5_1_FAST)
            .with_client_id("GPT-5.1 Fast"),
        ModelIds::new(GPT_5_1_HIGH)
            .with_client_id("GPT-5.1 High"),
        ModelIds::new(GPT_5_1_HIGH_FAST)
            .with_client_id("GPT-5.1 High Fast"),
        ModelIds::new(GPT_5_1_LOW)
            .with_client_id("GPT-5.1 Low"),
        ModelIds::new(GPT_5_1_LOW_FAST)
            .with_client_id("GPT-5.1 Low Fast"),
        ModelIds::new(GPT_5_1_CODEX_MINI)
            .with_client_id("GPT-5.1 Codex Mini"),
        ModelIds::new(GPT_5_1_CODEX_MINI_HIGH)
            .with_client_id("GPT-5.1 Codex Mini High"),
        ModelIds::new(GPT_5_1_CODEX_MINI_LOW)
            .with_client_id("GPT-5.1 Codex Mini Low"),
        ModelIds::new(O3),
        ModelIds::new(GPT_4_1)
            .with_client_id("GPT-4.1"),
        ModelIds::new(GPT_5_MINI)
            .with_client_id("GPT-5 Mini"),
        ModelIds::new(GPT_5_NANO)
            .with_client_id("GPT-5 Nano"),
        ModelIds::new(O3_PRO)
            .with_client_id("o3 Pro"),
        ModelIds::new(GPT_5_PRO)
            .with_client_id("GPT-5 Pro"),
    ],

    DEEPSEEK => [
        ModelIds::new(DEEPSEEK_R1_0528)
            .with_client_id("DeepSeek R1"),
        ModelIds::new(DEEPSEEK_V3_1)
            .with_client_id("DeepSeek V3.1"),
    ],

    XAI => [
        ModelIds::new(GROK_CODE_FAST_1)
            .with_client_id("Grok Code"),
        ModelIds::new(GROK_4)
            .with_client_id("Grok 4")
            .with_server_id(GROK_4_0709),
        ModelIds::new(GROK_4_FAST_REASONING)
            .with_client_id("Grok 4 Fast"),
        ModelIds::new(GROK_4_FAST_NON_REASONING)
            .with_client_id("Grok 4 Fast"),
    ],

    FIREWORKS => [
        ModelIds::new(KIMI_K2_INSTRUCT)
            .with_client_id("Kimi K2")
            .with_server_id(ACCOUNTS_FIREWORKS_MODELS_KIMI_K2_INSTRUCT),
    ],
}

pub const FREE_MODELS: [&str; 6] =
    [GPT_4O_MINI, CURSOR_FAST, CURSOR_SMALL, DEEPSEEK_V3, DEEPSEEK_V3_1, GROK_3_MINI];

// pub(super) const LONG_CONTEXT_MODELS: [&str; 4] =
//     [GPT_4O_128K, GEMINI_1_5_FLASH_500K, CLAUDE_3_HAIKU_200K, CLAUDE_3_5_SONNET_200K];

// 支持思考的模型
const SUPPORTED_THINKING_MODELS: [&str; 42] = [
    CLAUDE_4_5_OPUS_HIGH_THINKING,
    CLAUDE_4_5_SONNET_THINKING,
    GPT_5_1_CODEX_MAX,
    GPT_5_1_CODEX_MAX_HIGH,
    GPT_5_1_CODEX_MAX_LOW,
    GPT_5_1_CODEX_MAX_XHIGH,
    GPT_5_1_CODEX_MAX_MEDIUM_FAST,
    GPT_5_1_CODEX_MAX_HIGH_FAST,
    GPT_5_1_CODEX_MAX_LOW_FAST,
    GPT_5_1_CODEX_MAX_XHIGH_FAST,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_HIGH,
    GPT_5_1_CODEX_FAST,
    GPT_5_1_CODEX_HIGH_FAST,
    GPT_5_1_CODEX_LOW,
    GPT_5_1_CODEX_LOW_FAST,
    GPT_5_1,
    GPT_5_1_FAST,
    GPT_5_1_HIGH,
    GPT_5_1_HIGH_FAST,
    GPT_5_1_LOW,
    GPT_5_1_LOW_FAST,
    GEMINI_3_PRO,
    GPT_5_1_CODEX_MINI,
    GPT_5_1_CODEX_MINI_HIGH,
    GPT_5_1_CODEX_MINI_LOW,
    CLAUDE_4_5_HAIKU_THINKING,
    GROK_CODE_FAST_1,
    CLAUDE_4_OPUS_THINKING,
    CLAUDE_4_OPUS_THINKING_LEGACY,
    CLAUDE_4_SONNET_THINKING,
    CLAUDE_4_SONNET_1M_THINKING,
    O3,
    GPT_5_MINI,
    GPT_5_NANO,
    O3_PRO,
    GPT_5_PRO,
    GEMINI_2_5_PRO_PREVIEW_05_06,
    GEMINI_2_5_FLASH_PREVIEW_05_20,
    DEEPSEEK_R1_0528,
    GROK_4,
    GROK_4_FAST_REASONING,
];

// 支持图像的模型（DEFAULT 始终支持）
const SUPPORTED_IMAGE_MODELS: [&str; 51] = [
    DEFAULT,
    COMPOSER_1,
    CLAUDE_4_5_OPUS_HIGH,
    CLAUDE_4_5_OPUS_HIGH_THINKING,
    CLAUDE_4_5_SONNET,
    CLAUDE_4_5_SONNET_THINKING,
    GPT_5_1_CODEX_MAX,
    GPT_5_1_CODEX_MAX_HIGH,
    GPT_5_1_CODEX_MAX_LOW,
    GPT_5_1_CODEX_MAX_XHIGH,
    GPT_5_1_CODEX_MAX_MEDIUM_FAST,
    GPT_5_1_CODEX_MAX_HIGH_FAST,
    GPT_5_1_CODEX_MAX_LOW_FAST,
    GPT_5_1_CODEX_MAX_XHIGH_FAST,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_HIGH,
    GPT_5_1_CODEX_FAST,
    GPT_5_1_CODEX_HIGH_FAST,
    GPT_5_1_CODEX_LOW,
    GPT_5_1_CODEX_LOW_FAST,
    GPT_5_1,
    GPT_5_1_FAST,
    GPT_5_1_HIGH,
    GPT_5_1_HIGH_FAST,
    GPT_5_1_LOW,
    GPT_5_1_LOW_FAST,
    GEMINI_3_PRO,
    GPT_5_1_CODEX_MINI,
    GPT_5_1_CODEX_MINI_HIGH,
    GPT_5_1_CODEX_MINI_LOW,
    CLAUDE_4_5_HAIKU,
    CLAUDE_4_5_HAIKU_THINKING,
    CLAUDE_4_OPUS,
    CLAUDE_4_OPUS_THINKING,
    CLAUDE_4_OPUS_LEGACY,
    CLAUDE_4_OPUS_THINKING_LEGACY,
    CLAUDE_4_SONNET,
    CLAUDE_4_SONNET_THINKING,
    CLAUDE_4_SONNET_1M,
    CLAUDE_4_SONNET_1M_THINKING,
    O3,
    GPT_4_1,
    GPT_5_MINI,
    GPT_5_NANO,
    O3_PRO,
    GPT_5_PRO,
    GEMINI_2_5_PRO_PREVIEW_05_06,
    GEMINI_2_5_FLASH_PREVIEW_05_20,
    GROK_4,
    GROK_4_FAST_REASONING,
    GROK_4_FAST_NON_REASONING,
];

// 支持Max与非Max的模型
const SUPPORTED_MAX_MODELS: [&str; 47] = [
    DEFAULT,
    COMPOSER_1,
    CLAUDE_4_5_OPUS_HIGH,
    CLAUDE_4_5_OPUS_HIGH_THINKING,
    CLAUDE_4_5_SONNET,
    CLAUDE_4_5_SONNET_THINKING,
    GPT_5_1_CODEX_MAX,
    GPT_5_1_CODEX_MAX_HIGH,
    GPT_5_1_CODEX_MAX_LOW,
    GPT_5_1_CODEX_MAX_XHIGH,
    GPT_5_1_CODEX_MAX_MEDIUM_FAST,
    GPT_5_1_CODEX_MAX_HIGH_FAST,
    GPT_5_1_CODEX_MAX_LOW_FAST,
    GPT_5_1_CODEX_MAX_XHIGH_FAST,
    GPT_5_1_CODEX,
    GPT_5_1_CODEX_HIGH,
    GPT_5_1_CODEX_FAST,
    GPT_5_1_CODEX_HIGH_FAST,
    GPT_5_1_CODEX_LOW,
    GPT_5_1_CODEX_LOW_FAST,
    GPT_5_1,
    GPT_5_1_FAST,
    GPT_5_1_HIGH,
    GPT_5_1_HIGH_FAST,
    GPT_5_1_LOW,
    GPT_5_1_LOW_FAST,
    GEMINI_3_PRO,
    GPT_5_1_CODEX_MINI,
    GPT_5_1_CODEX_MINI_HIGH,
    GPT_5_1_CODEX_MINI_LOW,
    CLAUDE_4_5_HAIKU,
    CLAUDE_4_5_HAIKU_THINKING,
    GROK_CODE_FAST_1,
    CLAUDE_4_SONNET,
    CLAUDE_4_SONNET_THINKING,
    O3,
    GPT_4_1,
    GPT_5_MINI,
    GPT_5_NANO,
    GEMINI_2_5_PRO_PREVIEW_05_06,
    GEMINI_2_5_FLASH_PREVIEW_05_20,
    DEEPSEEK_R1_0528,
    DEEPSEEK_V3_1,
    GROK_4,
    GROK_4_FAST_REASONING,
    GROK_4_FAST_NON_REASONING,
    KIMI_K2_INSTRUCT,
];

// 只支持Max的模型
const MAX_MODELS: [&str; 8] = [
    CLAUDE_4_OPUS,
    CLAUDE_4_OPUS_THINKING,
    CLAUDE_4_OPUS_LEGACY,
    CLAUDE_4_OPUS_THINKING_LEGACY,
    CLAUDE_4_SONNET_1M,
    CLAUDE_4_SONNET_1M_THINKING,
    O3_PRO,
    GPT_5_PRO,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn check_duplicates() {
        let targets: [(&str, &[&str]); 4] = [
            ("THINKING", &SUPPORTED_THINKING_MODELS),
            ("IMAGE", &SUPPORTED_IMAGE_MODELS),
            ("MAX", &SUPPORTED_MAX_MODELS),
            ("MAX_ONLY", &MAX_MODELS),
        ];

        let mut failed = false;

        for (name, list) in targets {
            let mut seen = HashSet::new();
            let mut dups = Vec::new();

            for &model in list {
                if !seen.insert(model) {
                    dups.push(model);
                }
            }

            if !dups.is_empty() {
                println!("Duplicate in {name}: {dups:?}");
                failed = true;
            }
        }

        if failed {
            panic!("Found duplicates in model lists");
        }
    }
}
