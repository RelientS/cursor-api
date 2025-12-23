use crate::{
    AppConfig,
    app::{
        lazy::KEY_PREFIX,
        model::{Randomness, RawToken, Subject, TokenDuration, UserId},
    },
    common::utils::from_base64,
};

// include!(concat!(env!("OUT_DIR"), "/key.rs"));
include!("config/key.rs");

impl ConfiguredKey {
    pub fn move_without_auth_token(&mut self, config: &mut KeyConfig) {
        if self.usage_check_models.is_some() {
            config.usage_check_models = self.usage_check_models.take();
        }
        if self.disable_vision.is_some() {
            config.disable_vision = self.disable_vision.take();
        }
        if self.enable_slow_pool.is_some() {
            config.enable_slow_pool = self.enable_slow_pool.take();
        }
        if self.include_web_references.is_some() {
            config.include_web_references = self.include_web_references.take();
        }
    }
}

impl configured_key::token_info::Token {
    #[inline]
    pub fn from_raw(raw: RawToken) -> Self {
        Self {
            provider: raw.subject.provider.to_string(),
            signature: raw.signature,
            sub_id: raw.subject.id.to_bytes(),
            randomness: raw.randomness.to_bytes(),
            start: raw.duration.start,
            end: raw.duration.end,
            is_session: raw.is_session,
        }
    }

    #[inline]
    pub fn into_raw(self) -> Option<RawToken> {
        Some(RawToken {
            subject: Subject {
                provider: self.provider.parse().ok()?,
                id: UserId::from_bytes(self.sub_id),
            },
            randomness: Randomness::from_bytes(self.randomness),
            signature: self.signature,
            duration: TokenDuration { start: self.start, end: self.end },
            is_session: self.is_session,
        })
    }
}

pub fn parse_dynamic_token(auth_token: &str) -> Option<ConfiguredKey> {
    auth_token.strip_prefix(&**KEY_PREFIX).and_then(from_base64).and_then(|decoded_bytes| {
        let mut decoder = ::minicbor::Decoder::new(&decoded_bytes);
        decoder.decode().ok()
    })
}

#[derive(Clone)]
pub struct KeyConfig {
    pub usage_check_models: Option<configured_key::UsageCheckModel>,
    pub disable_vision: Option<bool>,
    pub enable_slow_pool: Option<bool>,
    pub include_web_references: Option<bool>,
}

impl KeyConfig {
    pub const fn new() -> Self {
        Self {
            usage_check_models: None,
            disable_vision: None,
            enable_slow_pool: None,
            include_web_references: None,
        }
    }

    pub fn with_global(&mut self) {
        if self.disable_vision.is_some() {
            self.disable_vision = Some(AppConfig::get_vision_ability().is_none());
        }
        if self.enable_slow_pool.is_some() {
            self.enable_slow_pool = Some(AppConfig::get_slow_pool());
        }
        if self.include_web_references.is_some() {
            self.include_web_references = Some(AppConfig::get_web_refs());
        }
    }
}
