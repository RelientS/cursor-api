use core::num::NonZeroU16;

use serde::Serialize;

use super::CanonicalError;
use crate::core::aiserver::v1::CustomErrorDetails;

#[derive(Serialize)]
pub struct CppError {
    code: NonZeroU16,
    r#type: &'static str,
    details: Option<CustomErrorDetails>,
}

impl From<CanonicalError> for CppError {
    #[inline]
    fn from(error: CanonicalError) -> Self {
        Self { code: error.status_code, r#type: error.r#type, details: error.details }
    }
}
