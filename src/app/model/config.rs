use super::{UsageCheck, VisionAbility};
use crate::{
    app::{constant::EMPTY_STRING, model::FetchMode},
    common::utils::parse_from_env,
};
use manually_init::ManuallyInit;
use parking_lot::RwLock;

// 静态配置
#[derive(Default, Clone)]
pub struct AppConfig {
    vision_ability: VisionAbility,
    slow_pool: bool,
    long_context: bool,
    // pages: Pages,
    usage_check: UsageCheck,
    dynamic_key: bool,
    share_token: String,
    web_refs: bool,
    fetch_models: FetchMode,
}

// 全局配置实例
static APP_CONFIG: ManuallyInit<RwLock<AppConfig>> = ManuallyInit::new();

macro_rules! config_methods {
    ($($field:ident: $type:ty, $default:expr;)*) => {
        $(
            paste::paste! {
                pub fn [<get_ $field>]() -> $type
                where
                    $type: Copy + PartialEq,
                {
                    APP_CONFIG.read().$field
                }

                pub fn [<update_ $field>](value: $type)
                where
                    $type: Copy + PartialEq,
                {
                    if Self::[<get_ $field>]() != value {
                        APP_CONFIG.write().$field = value;
                    }
                }

                pub fn [<reset_ $field>]()
                where
                    $type: Copy + PartialEq,
                {
                    if Self::[<get_ $field>]() != $default {
                        APP_CONFIG.write().$field = $default;
                    }
                }
            }
        )*
    };
}

macro_rules! config_methods_clone {
    ($($field:ident: $type:ty, $default:expr;)*) => {
        $(
            paste::paste! {
                #[inline]
                pub fn [<get_ $field>]() -> $type
                where
                    $type: Clone + PartialEq,
                {
                    APP_CONFIG.read().$field.clone()
                }

                #[inline]
                pub fn [<update_ $field>](value: $type)
                where
                    $type: Clone + PartialEq,
                {
                    if Self::[<get_ $field>]() != value {
                        APP_CONFIG.write().$field = value;
                    }
                }

                #[inline]
                pub fn [<reset_ $field>]()
                where
                    $type: Clone + PartialEq,
                {
                    let default_value = $default;
                    if Self::[<get_ $field>]() != default_value {
                        APP_CONFIG.write().$field = default_value;
                    }
                }
            }
        )*
    };
}

static IS_SHARE: ::core::sync::atomic::AtomicBool = ::core::sync::atomic::AtomicBool::new(false);

impl AppConfig {
    pub fn init() {
        // base
        {
            crate::leak::init_pool();
            super::token::__init()
        };
        // env
        {
            crate::common::model::ntp::Servers::init();
            super::tz::__init();
            super::super::lazy::init_all_cursor_urls();
            super::super::lazy::log::init();
            super::hash::init();
            super::super::constant::header::initialize_cursor_version();
            super::super::constant::init_thinking_tags();
            crate::core::model::init_resolver();
            super::token::parse_providers();
            super::context_fill_mode::init();
        }
        crate::core::constant::create_models();
        APP_CONFIG.init(RwLock::new(AppConfig::default()));

        let mut config = APP_CONFIG.write();
        config.vision_ability =
            VisionAbility::from_str(&parse_from_env("VISION_ABILITY", EMPTY_STRING));
        config.slow_pool = parse_from_env("ENABLE_SLOW_POOL", false);
        config.long_context = parse_from_env("ENABLE_LONG_CONTEXT", false);
        config.usage_check = UsageCheck::from_str(&parse_from_env("USAGE_CHECK", EMPTY_STRING));
        config.dynamic_key = parse_from_env("DYNAMIC_KEY", false);
        config.share_token = parse_from_env("SHARED_TOKEN", EMPTY_STRING).into_owned();
        IS_SHARE.store(!config.share_token.is_empty(), ::core::sync::atomic::Ordering::Relaxed);
        config.web_refs = parse_from_env("INCLUDE_WEB_REFERENCES", false);
        config.fetch_models =
            FetchMode::from_str(&parse_from_env("FETCH_RAW_MODELS", EMPTY_STRING));
    }

    config_methods! {
        slow_pool: bool, false;
        long_context: bool, false;
        dynamic_key: bool, false;
        web_refs: bool, false;
        vision_ability: VisionAbility, VisionAbility::default();
        fetch_models: FetchMode, FetchMode::default();
    }

    config_methods_clone! {
        usage_check: UsageCheck, UsageCheck::default();
    }

    #[inline]
    pub fn get_share_token() -> String { APP_CONFIG.read().share_token.clone() }

    #[inline]
    pub fn share_token_eq(s: &str) -> bool { APP_CONFIG.read().share_token == s }

    #[inline]
    pub fn update_share_token(value: String) {
        if Self::share_token_eq(&value) {
            IS_SHARE.store(!value.is_empty(), ::core::sync::atomic::Ordering::Relaxed);
            APP_CONFIG.write().share_token = value;
        }
    }

    #[inline]
    pub fn reset_share_token() {
        if Self::is_share() {
            IS_SHARE.store(false, ::core::sync::atomic::Ordering::Relaxed);
            APP_CONFIG.write().share_token.clear();
        }
    }

    #[inline]
    pub fn is_share() -> bool { IS_SHARE.load(::core::sync::atomic::Ordering::Relaxed) }

    // pub fn get_page_content(path: &str) -> Option<PageContent> {
    //     match path {
    //         ROUTE_ROOT_PATH => Some(APP_CONFIG.read().pages.root_content.clone()),
    //         ROUTE_LOGS_PATH => Some(APP_CONFIG.read().pages.logs_content.clone()),
    //         ROUTE_CONFIG_PATH => Some(APP_CONFIG.read().pages.config_content.clone()),
    //         ROUTE_TOKENS_PATH => Some(APP_CONFIG.read().pages.tokens_content.clone()),
    //         ROUTE_PROXIES_PATH => Some(APP_CONFIG.read().pages.proxies_content.clone()),
    //         ROUTE_SHARED_STYLES_PATH => Some(APP_CONFIG.read().pages.shared_styles_content.clone()),
    //         ROUTE_SHARED_JS_PATH => Some(APP_CONFIG.read().pages.shared_js_content.clone()),
    //         ROUTE_ABOUT_PATH => Some(APP_CONFIG.read().pages.about_content.clone()),
    //         ROUTE_README_PATH => Some(APP_CONFIG.read().pages.readme_content.clone()),
    //         ROUTE_API_PATH => Some(APP_CONFIG.read().pages.api_content.clone()),
    //         ROUTE_BUILD_KEY_PATH => Some(APP_CONFIG.read().pages.build_key_content.clone()),
    //         _ => None,
    //     }
    // }

    // pub fn update_page_content(path: &str, content: PageContent) -> bool {
    //     match path {
    //         ROUTE_ROOT_PATH => APP_CONFIG.write().pages.root_content = content,
    //         ROUTE_LOGS_PATH => APP_CONFIG.write().pages.logs_content = content,
    //         ROUTE_CONFIG_PATH => APP_CONFIG.write().pages.config_content = content,
    //         ROUTE_TOKENS_PATH => APP_CONFIG.write().pages.tokens_content = content,
    //         ROUTE_PROXIES_PATH => APP_CONFIG.write().pages.proxies_content = content,
    //         ROUTE_SHARED_STYLES_PATH => APP_CONFIG.write().pages.shared_styles_content = content,
    //         ROUTE_SHARED_JS_PATH => APP_CONFIG.write().pages.shared_js_content = content,
    //         ROUTE_ABOUT_PATH => APP_CONFIG.write().pages.about_content = content,
    //         ROUTE_README_PATH => APP_CONFIG.write().pages.readme_content = content,
    //         ROUTE_API_PATH => APP_CONFIG.write().pages.api_content = content,
    //         ROUTE_BUILD_KEY_PATH => APP_CONFIG.write().pages.build_key_content = content,
    //         _ => return true,
    //     }
    //     false
    // }

    // pub fn reset_page_content(path: &str) -> bool {
    //     match path {
    //         ROUTE_ROOT_PATH => APP_CONFIG.write().pages.root_content = PageContent::default(),
    //         ROUTE_LOGS_PATH => APP_CONFIG.write().pages.logs_content = PageContent::default(),
    //         ROUTE_CONFIG_PATH => APP_CONFIG.write().pages.config_content = PageContent::default(),
    //         ROUTE_TOKENS_PATH => APP_CONFIG.write().pages.tokens_content = PageContent::default(),
    //         ROUTE_PROXIES_PATH => APP_CONFIG.write().pages.proxies_content = PageContent::default(),
    //         ROUTE_SHARED_STYLES_PATH => {
    //             APP_CONFIG.write().pages.shared_styles_content = PageContent::default()
    //         }
    //         ROUTE_SHARED_JS_PATH => {
    //             APP_CONFIG.write().pages.shared_js_content = PageContent::default()
    //         }
    //         ROUTE_ABOUT_PATH => APP_CONFIG.write().pages.about_content = PageContent::default(),
    //         ROUTE_README_PATH => APP_CONFIG.write().pages.readme_content = PageContent::default(),
    //         ROUTE_API_PATH => APP_CONFIG.write().pages.api_content = PageContent::default(),
    //         ROUTE_BUILD_KEY_PATH => {
    //             APP_CONFIG.write().pages.build_key_content = PageContent::default()
    //         }
    //         _ => return true,
    //     }
    //     false
    // }

    // pub fn save() -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
    //     let pages = APP_CONFIG.read().pages.clone();
    //     let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&pages)?;

    //     let file = OpenOptions::new()
    //         .read(true)
    //         .write(true)
    //         .create(true)
    //         .truncate(true)
    //         .open(&*CONFIG_FILE_PATH)?;

    //     // 添加大小检查
    //     if bytes.len() > usize::MAX >> 1 {
    //         return Err("配置数据过大".into());
    //     }

    //     file.set_len(bytes.len() as u64)?;

    //     let mut mmap = unsafe { MmapMut::map_mut(&file)? };
    //     mmap.copy_from_slice(&bytes);
    //     mmap.flush()?;

    //     Ok(())
    // }

    // pub fn load() -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
    //     let file = match OpenOptions::new().read(true).open(&*CONFIG_FILE_PATH) {
    //         Ok(file) => file,
    //         Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
    //             return Ok(());
    //         }
    //         Err(e) => return Err(Box::new(e)),
    //     };

    //     // 添加文件大小检查
    //     if file.metadata()?.len() > usize::MAX as u64 {
    //         return Err("配置文件过大".into());
    //     }

    //     let mmap = unsafe { MmapOptions::new().map(&file)? };

    //     let pages = unsafe { ::rkyv::from_bytes_unchecked::<Pages, ::rkyv::rancor::Error>(&mmap) }?;
    //     let mut config = APP_CONFIG.write();
    //     config.pages = pages;

    //     Ok(())
    // }
}
