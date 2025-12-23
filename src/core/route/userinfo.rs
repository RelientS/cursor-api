use crate::{
    app::model::proxy_pool::get_general_client,
    common::{
        model::{ApiStatus, userinfo::GetUserInfo},
        utils::{extract_token, get_new_token, get_token_profile},
    },
    core::constant::ERR_NODATA,
};
use axum::Json;

use super::token::TokenRequest;

pub async fn handle_user_info(Json(request): Json<TokenRequest>) -> Json<GetUserInfo> {
    let auth_token = match request.token {
        Some(token) => token,
        None => {
            return Json(GetUserInfo::Error {
                error: ERR_NODATA.to_string(),
            });
        }
    };

    let token = match extract_token(&auth_token) {
        Some(token) => token,
        None => {
            return Json(GetUserInfo::Error {
                error: ERR_NODATA.to_string(),
            });
        }
    };

    match get_token_profile(get_general_client(), token, false).await {
        Some(usage) => Json(GetUserInfo::Usage(Box::new(usage))),
        None => Json(GetUserInfo::Error {
            error: ERR_NODATA.to_string(),
        }),
    }
}

#[derive(serde::Serialize)]
pub struct TokenUpgradeResponse {
    status: ApiStatus,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<String>,
}
