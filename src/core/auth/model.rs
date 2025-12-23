use crate::app::model::ExtToken;
use super::error::AuthError;

pub type TokenBundle = (ExtToken, bool);
pub type TokenBundleResult = Result<TokenBundle, AuthError>;
