//! 认证提供者模块
//!
//! 本模块管理可配置的认证提供者列表，
//! 可以通过环境变量 `ALLOWED_PROVIDERS` 自定义支持的提供者。

use core::fmt;
use std::str::FromStr;

crate::def_pub_const!(
    /// Auth0认证提供者标识符
    AUTH0 = "auth0",
    /// Google OAuth2认证提供者标识符
    GOOGLE_OAUTH2 = "google-oauth2",
    /// GitHub认证提供者标识符
    GITHUB = "github",
);

/// 默认支持的认证提供者列表
const DEFAULT_PROVIDERS: &'static [&'static str] = &[AUTH0, GOOGLE_OAUTH2, GITHUB];
static mut PROVIDERS: &'static [&'static str] = DEFAULT_PROVIDERS;

/// 表示一个认证提供者
///
/// 这是一个对静态字符串标识符的包装，
/// 该标识符会与支持的提供者列表进行验证
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Provider(usize);

impl PartialEq for Provider {
    #[inline]
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}

impl Eq for Provider {}

impl ::core::hash::Hash for Provider {
    #[inline]
    fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) { self.0.hash(state) }
}

impl fmt::Display for Provider {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(self.as_str()) }
}

impl Provider {
    #[inline]
    #[allow(static_mut_refs)]
    pub fn as_str(self) -> &'static str { unsafe { PROVIDERS.get_unchecked(self.0) } }

    #[inline]
    pub(super) fn from_str(s: &str) -> Result<Self, super::SubjectError> {
        unsafe { PROVIDERS }
            .iter()
            .position(|&provider| s == provider)
            .map(Self)
            .ok_or(super::SubjectError::UnsupportedProvider)
    }

    #[inline]
    pub(super) fn to_helper(self) -> super::ProviderHelper {
        match self.as_str() {
            AUTH0 => super::ProviderHelper::Auth0,
            GITHUB => super::ProviderHelper::Github,
            GOOGLE_OAUTH2 => super::ProviderHelper::Google,
            s => super::ProviderHelper::Other(s.to_string()),
        }
    }
}

impl FromStr for Provider {
    type Err = super::SubjectError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> { Self::from_str(s) }
}

impl ::serde::Serialize for Provider {
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: ::serde::Serializer {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> ::serde::Deserialize<'de> for Provider {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: ::serde::Deserializer<'de> {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// 从环境配置初始化支持的提供者列表
///
/// 如果设置了环境变量 `ALLOWED_PROVIDERS`，则从中读取，否则保持默认提供者列表。
/// 环境变量应包含以逗号分隔的提供者标识符列表。
///
/// # 环境变量示例
/// ```text
/// ALLOWED_PROVIDERS=auth0,google-oauth2,github,custom-provider
/// ```
///
/// # 注意
/// 此函数应在应用程序启动时调用一次。
/// 任何未知的提供者字符串都会泄漏到静态内存中。
pub fn parse_providers() {
    if let Ok(env) = std::env::var("ALLOWED_PROVIDERS") {
        // 使用位标志跟踪默认提供者
        const AUTH0_FLAG: u8 = 1 << 0;
        const GOOGLE_FLAG: u8 = 1 << 1;
        const GITHUB_FLAG: u8 = 1 << 2;
        const ALL_DEFAULT: u8 = AUTH0_FLAG | GOOGLE_FLAG | GITHUB_FLAG;

        let mut default_flags = 0u8;
        let mut custom_count = 0;

        let v = env
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| match s {
                AUTH0 => {
                    default_flags |= AUTH0_FLAG;
                    AUTH0
                }
                GOOGLE_OAUTH2 => {
                    default_flags |= GOOGLE_FLAG;
                    GOOGLE_OAUTH2
                }
                GITHUB => {
                    default_flags |= GITHUB_FLAG;
                    GITHUB
                }
                s => {
                    custom_count += 1;
                    Box::leak(Box::from(s))
                }
            })
            .collect::<Vec<_>>();

        // 如果恰好是3个默认提供者且没有自定义提供者，保持默认值
        if custom_count == 0 && default_flags == ALL_DEFAULT {
            return;
        }

        unsafe { PROVIDERS = Box::leak(v.into_boxed_slice()) };
    }
}
