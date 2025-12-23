use axum::body::Body;
use axum::response::Response;
use http::StatusCode;
use http::header::{CONTENT_TYPE, LOCATION};

use crate::app::constant::{
    HEADER_VALUE_TEXT_CSS_UTF8, HEADER_VALUE_TEXT_HTML_UTF8, HEADER_VALUE_APPLICATION_JS_UTF8,
    HEADER_VALUE_TEXT_PLAIN_UTF8,
};

// 页面内容类型枚举
#[derive(
    Clone, serde::Serialize, serde::Deserialize, rkyv::Archive, rkyv::Deserialize, rkyv::Serialize,
)]
#[serde(tag = "type", content = "value")]
#[serde(rename_all = "snake_case")]
pub enum PageContent {
    Default,           // 默认行为
    NotFound,          // 404页面
    Redirect(String),  // 重定向到指定URL
    PlainText(String), // 纯文本内容
    Html(String),      // HTML内容
    Css(String),       // Css内容
    Js(String),        // Js内容
}

impl const Default for PageContent {
    #[inline(always)]
    fn default() -> Self {
        Self::Default
    }
}

impl PageContent {
    /// 根据内容类型生成Response，接受默认行为闭包
    pub fn into_response<F>(self, default_handler: F) -> Response
    where
        F: FnOnce() -> Result<Response<Body>, http::Error>,
    {
        __unwrap!(match self {
            PageContent::Default => default_handler(),
            PageContent::NotFound => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(CONTENT_TYPE, HEADER_VALUE_TEXT_PLAIN_UTF8)
                .body(Body::from("Not Found")),
            PageContent::Redirect(url) => Response::builder()
                .status(StatusCode::TEMPORARY_REDIRECT)
                .header(LOCATION, url)
                .body(Body::empty()),
            PageContent::PlainText(content) => Response::builder()
                .header(CONTENT_TYPE, HEADER_VALUE_TEXT_PLAIN_UTF8)
                .body(Body::from(content)),
            PageContent::Html(content) => Response::builder()
                .header(CONTENT_TYPE, HEADER_VALUE_TEXT_HTML_UTF8)
                .body(Body::from(content)),
            PageContent::Css(content) => Response::builder()
                .header(CONTENT_TYPE, HEADER_VALUE_TEXT_CSS_UTF8)
                .body(Body::from(content)),
            PageContent::Js(content) => Response::builder()
                .header(CONTENT_TYPE, HEADER_VALUE_APPLICATION_JS_UTF8)
                .body(Body::from(content)),
        })
    }
}

#[derive(Clone, Default, rkyv::Archive, rkyv::Deserialize, rkyv::Serialize)]
pub struct Pages {
    pub root_content: PageContent,
    pub logs_content: PageContent,
    pub config_content: PageContent,
    pub tokens_content: PageContent,
    pub proxies_content: PageContent,
    pub shared_styles_content: PageContent,
    pub shared_js_content: PageContent,
    pub about_content: PageContent,
    pub readme_content: PageContent,
    pub api_content: PageContent,
    pub build_key_content: PageContent,
}
