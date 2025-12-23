use crate::app::constant::header::{HEADER_VALUE_TEXT_HTML_UTF8, HEADER_VALUE_TEXT_PLAIN_UTF8};
use axum::{body::Body, http::header::CONTENT_TYPE, response::Response};

// const MAX_FILE_SIZE_BYTES: u64 = 4_000_000_000;

pub async fn handle_env_example() -> Response {
    __unwrap!(
        Response::builder()
            .header(CONTENT_TYPE, HEADER_VALUE_TEXT_PLAIN_UTF8)
            .body(Body::from(include_str!("../../../.env.example")))
    )
}

// // 配置页面处理函数
// pub async fn handle_config_page() -> Response {
//     AppConfig::get_page_content(ROUTE_CONFIG_PATH).unwrap_or_default().into_response(|| {
//         Response::builder()
//             .header(CONTENT_TYPE, HEADER_VALUE_TEXT_HTML_UTF8)
//             .body(Body::from(include_str!("../../../static/config.min.html")))
//     })
// }

// pub async fn handle_static(Path(path): Path<String>) -> Response {
//     match path.as_str() {
//         "shared-styles.css" => AppConfig::get_page_content(ROUTE_SHARED_STYLES_PATH)
//             .unwrap_or_default()
//             .into_response(|| {
//                 Response::builder()
//                     .header(CONTENT_TYPE, HEADER_VALUE_TEXT_CSS_UTF8)
//                     .body(Body::from(include_str!("../../../static/shared-styles.min.css")))
//             }),
//         "shared.js" => AppConfig::get_page_content(ROUTE_SHARED_JS_PATH)
//             .unwrap_or_default()
//             .into_response(|| {
//                 Response::builder()
//                     .header(CONTENT_TYPE, HEADER_VALUE_APPLICATION_JS_UTF8)
//                     .body(Body::from(include_str!("../../../static/shared.min.js")))
//             }),
//         s => {
//             if !s.contains("..")
//                 && STATIC_DIR.is_dir()
//                 && let file_path = STATIC_DIR.join(s)
//                 && let Ok(metadata) = std::fs::metadata(&file_path)
//                 && metadata.is_file()
//                 && metadata.len() <= MAX_FILE_SIZE_BYTES
//                 && let Some(content_type) = file_path
//                     .extension()
//                     .and_then(|e| e.to_str())
//                     .map(get_content_type_by_extension)
//                 && let Ok(file) = tokio::fs::File::open(&file_path).await
//             {
//                 let stream = tokio_util::io::ReaderStream::new(file);
//                 return __unwrap!(
//                     Response::builder()
//                         .header(CONTENT_TYPE, content_type)
//                         .header(CONTENT_LENGTH, metadata.len())
//                         .body(Body::from_stream(stream))
//                 );
//             };
//             __unwrap!(
//                 Response::builder().status(StatusCode::NOT_FOUND).body(Body::from("Not found"))
//             )
//         }
//     }
// }

pub async fn handle_readme() -> Response {
    __unwrap!(
        Response::builder()
            .header(CONTENT_TYPE, HEADER_VALUE_TEXT_HTML_UTF8)
            .body(Body::from(include_str!("../../../static/readme.min.html")))
    )
}

pub async fn handle_license() -> Response {
    __unwrap!(
        Response::builder()
            .header(CONTENT_TYPE, HEADER_VALUE_TEXT_HTML_UTF8)
            .body(Body::from(include_str!("../../../static/license.min.html")))
    )
}

// pub async fn handle_about() -> Response {
//     AppConfig::get_page_content(ROUTE_ABOUT_PATH).unwrap_or_default().into_response(|| {
//         Response::builder()
//             .status(StatusCode::TEMPORARY_REDIRECT)
//             .header(LOCATION, ROUTE_LICENSE_PATH)
//             .body(Body::empty())
//     })
// }

// pub async fn handle_build_key_page() -> Response {
//     AppConfig::get_page_content(ROUTE_BUILD_KEY_PATH).unwrap_or_default().into_response(|| {
//         Response::builder()
//             .header(CONTENT_TYPE, HEADER_VALUE_TEXT_HTML_UTF8)
//             .body(Body::from(include_str!("../../../static/build_key.min.html")))
//     })
// }

// pub async fn handle_tokens_page() -> Response {
//     AppConfig::get_page_content(ROUTE_TOKENS_PATH).unwrap_or_default().into_response(|| {
//         Response::builder()
//             .header(CONTENT_TYPE, HEADER_VALUE_TEXT_HTML_UTF8)
//             .body(Body::from(include_str!("../../../static/tokens.min.html")))
//     })
// }

// pub async fn handle_proxies_page() -> Response {
//     AppConfig::get_page_content(ROUTE_PROXIES_PATH).unwrap_or_default().into_response(|| {
//         Response::builder()
//             .header(CONTENT_TYPE, HEADER_VALUE_TEXT_HTML_UTF8)
//             .body(Body::from(include_str!("../../../static/proxies.min.html")))
//     })
// }

// pub async fn handle_api_page() -> Response {
//     AppConfig::get_page_content(ROUTE_API_PATH).unwrap_or_default().into_response(|| {
//         Response::builder()
//             .header(CONTENT_TYPE, HEADER_VALUE_TEXT_HTML_UTF8)
//             .body(Body::from(include_str!("../../../static/api.min.html")))
//     })
// }
