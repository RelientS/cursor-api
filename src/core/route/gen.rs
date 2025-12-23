use std::sync::LazyLock;

use axum::body::Body;
use axum::http::HeaderMap;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse as _, Response};

use crate::app::constant::HEADER_VALUE_TEXT_PLAIN_UTF8;
use crate::app::model::{Checksum, Hash, timestamp_header};

static HEADERS_TEXT_PLAIN: LazyLock<HeaderMap> =
    LazyLock::new(|| HeaderMap::from_iter([(CONTENT_TYPE, HEADER_VALUE_TEXT_PLAIN_UTF8)]));

pub async fn handle_gen_uuid() -> Response {
    let mut buf = vec![0u8; 36];
    ::uuid::Uuid::new_v4().hyphenated().encode_lower(&mut buf);
    let body = Body::from(buf);

    (HEADERS_TEXT_PLAIN.clone(), body).into_response()
}

pub async fn handle_gen_hash() -> Response {
    let mut buf = vec![0u8; 64];
    Hash::random().to_str(unsafe { &mut *buf.as_mut_ptr().cast() });
    let body = Body::from(buf);

    (HEADERS_TEXT_PLAIN.clone(), body).into_response()
}

pub async fn handle_gen_checksum() -> Response {
    let mut buf = vec![0u8; 137];
    Checksum::random().to_str(unsafe { &mut *buf.as_mut_ptr().cast() });
    let body = Body::from(buf);

    (HEADERS_TEXT_PLAIN.clone(), body).into_response()
}

pub async fn handle_get_timestamp_header() -> Response {
    static TIMESTAMP_HEADER: ::bytes::Bytes = ::bytes::Bytes::from_static(unsafe {
        core::slice::from_raw_parts(timestamp_header::as_ptr(), 8)
    });
    let body = Body::from(TIMESTAMP_HEADER.clone());
    (HEADERS_TEXT_PLAIN.clone(), body).into_response()
}
