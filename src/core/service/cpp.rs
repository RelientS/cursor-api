use crate::{
    app::{
        constant::header::{
            CHUNKED, CLIENT_KEY, EVENT_STREAM, JSON, KEEP_ALIVE, NO_CACHE_REVALIDATE,
        },
        lazy::{cpp_config_url, cpp_models_url},
        model::CppService,
    },
    common::{
        client::{AiServiceRequest, build_client_request},
        model::{GenericError, error::ChatError},
        utils::{
            CollectBytes, CollectBytesParts, encode_message, encode_message_framed, new_uuid_v4,
        },
    },
    core::{
        aiserver::v1::{
            AvailableCppModelsResponse, CppConfigRequest, CppConfigResponse, FsSyncFileRequest,
            FsSyncFileResponse, FsUploadFileRequest, FsUploadFileResponse, StreamCppRequest,
        },
        auth::TokenBundle,
        stream::decoder::{
            cpp::{StreamDecoder, StreamMessage},
            direct,
            types::{DecodedMessage, DecoderError},
        },
    },
};
use alloc::borrow::Cow;
use axum::{
    Json,
    body::Body,
    response::{IntoResponse as _, Response},
};
use bytes::Bytes;
use core::convert::Infallible;
use futures::StreamExt as _;
use http::{
    Extensions, HeaderMap, StatusCode,
    header::{
        ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS, CACHE_CONTROL, CONNECTION,
        CONTENT_LENGTH, CONTENT_TYPE, COOKIE, TRANSFER_ENCODING, VARY,
    },
};

pub async fn handle_cpp_config(
    mut headers: HeaderMap,
    mut extensions: Extensions,
    Json(request): Json<CppConfigRequest>,
) -> Result<Json<CppConfigResponse>, Response> {
    let (ext_token, use_pri) = __unwrap!(extensions.remove::<TokenBundle>());

    let (data, compressed) = match encode_message(&request) {
        Ok(o) => o,
        Err(e) => return Err(e.into_response()),
    };

    let req = build_client_request(AiServiceRequest {
        ext_token: &ext_token,
        fs_client_key: headers.remove(CLIENT_KEY),
        url: cpp_config_url(use_pri),
        stream: false,
        compressed,
        trace_id: new_uuid_v4(),
        use_pri,
        cookie: headers.remove(COOKIE),
    });

    match CollectBytes(req.body(data)).await {
        Ok(bytes) => match direct::decode::<CppConfigResponse>(&bytes) {
            Ok(DecodedMessage::Protobuf(data)) => Ok(Json(data)),
            Ok(DecodedMessage::Text(s)) => Err(__unwrap!(
                Response::builder()
                    .header(CONTENT_TYPE, JSON)
                    .header(CONTENT_LENGTH, s.len())
                    .body(Body::from(s))
            )),
            Err(DecoderError::Internal(e)) => Err((
                StatusCode::BAD_GATEWAY,
                Json(ChatError::ProcessingFailed(Cow::Borrowed(e)).to_generic()),
            )
                .into_response()),
        },
        Err(mut e) => {
            e = e.without_url();

            Err((
                if e.is_timeout() {
                    StatusCode::GATEWAY_TIMEOUT
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                },
                Json(ChatError::RequestFailed(Cow::Owned(e.to_string())).to_generic()),
            )
                .into_response())
        }
    }
}

pub async fn handle_cpp_models(
    mut headers: HeaderMap,
    mut extensions: Extensions,
) -> Result<Json<AvailableCppModelsResponse>, Response> {
    let (ext_token, use_pri) = __unwrap!(extensions.remove::<TokenBundle>());

    let req = build_client_request(AiServiceRequest {
        ext_token: &ext_token,
        fs_client_key: headers.remove(CLIENT_KEY),
        url: cpp_models_url(use_pri),
        stream: false,
        compressed: false,
        trace_id: new_uuid_v4(),
        use_pri,
        cookie: headers.remove(COOKIE),
    });

    match CollectBytes(req).await {
        Ok(bytes) => match direct::decode::<AvailableCppModelsResponse>(&bytes) {
            Ok(DecodedMessage::Protobuf(data)) => Ok(Json(data)),
            Ok(DecodedMessage::Text(s)) => Err(__unwrap!(
                Response::builder()
                    .header(CONTENT_TYPE, JSON)
                    .header(CONTENT_LENGTH, s.len())
                    .body(Body::from(s))
            )),
            Err(DecoderError::Internal(e)) => Err((
                StatusCode::BAD_GATEWAY,
                Json(ChatError::ProcessingFailed(Cow::Borrowed(e)).to_generic()),
            )
                .into_response()),
        },
        Err(mut e) => {
            e = e.without_url();

            Err((
                if e.is_timeout() {
                    StatusCode::GATEWAY_TIMEOUT
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                },
                Json(ChatError::RequestFailed(Cow::Owned(e.to_string())).to_generic()),
            )
                .into_response())
        }
    }
}

const TO_REMOVE_HEADERS: [http::HeaderName; 5] = [
    CONTENT_TYPE,
    CONTENT_LENGTH,
    VARY,
    ACCESS_CONTROL_ALLOW_CREDENTIALS,
    ACCESS_CONTROL_ALLOW_HEADERS,
];

pub async fn handle_upload_file(
    mut headers: HeaderMap,
    mut extensions: Extensions,
    Json(request): Json<FsUploadFileRequest>,
) -> Result<Response, Response> {
    let (ext_token, use_pri) = __unwrap!(extensions.remove::<TokenBundle>());

    let (data, compressed) = match encode_message(&request) {
        Ok(o) => o,
        Err(e) => return Err(e.into_response()),
    };

    let req = build_client_request(AiServiceRequest {
        ext_token: &ext_token,
        fs_client_key: headers.remove(CLIENT_KEY),
        url: ext_token.get_gcpp_host().get_url(CppService::FSUploadFile, use_pri),
        stream: false,
        compressed,
        trace_id: new_uuid_v4(),
        use_pri,
        cookie: headers.remove(COOKIE),
    });

    let mut e = match CollectBytesParts(req.body(data)).await {
        Ok((mut parts, bytes)) => {
            for key in TO_REMOVE_HEADERS {
                let _ = parts.headers.remove(key);
            }
            return match direct::decode::<FsUploadFileResponse>(&bytes) {
                Ok(DecodedMessage::Protobuf(data)) => Ok(Response::from_parts(
                    parts,
                    Body::from(__unwrap!(serde_json::to_vec(&data))),
                )),
                Ok(DecodedMessage::Text(s)) => Err(__unwrap!(
                    Response::builder()
                        .header(CONTENT_TYPE, JSON)
                        .header(CONTENT_LENGTH, s.len())
                        .body(Body::from(s))
                )),
                Err(DecoderError::Internal(e)) => Err((
                    StatusCode::BAD_GATEWAY,
                    Json(ChatError::ProcessingFailed(Cow::Borrowed(e)).to_generic()),
                )
                    .into_response()),
            };
        }
        Err(e) => e,
    };
    e = e.without_url();
    let status_code = if e.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    Err((status_code, Json(ChatError::RequestFailed(Cow::Owned(e.to_string())).to_generic()))
        .into_response())
}

pub async fn handle_sync_file(
    mut headers: HeaderMap,
    mut extensions: Extensions,
    Json(request): Json<FsSyncFileRequest>,
) -> Result<Response, Response> {
    let (ext_token, use_pri) = __unwrap!(extensions.remove::<TokenBundle>());

    let (data, compressed) = match encode_message(&request) {
        Ok(o) => o,
        Err(e) => return Err(e.into_response()),
    };

    let req = build_client_request(AiServiceRequest {
        ext_token: &ext_token,
        fs_client_key: headers.remove(CLIENT_KEY),
        url: ext_token.get_gcpp_host().get_url(CppService::FSSyncFile, use_pri),
        stream: false,
        compressed,
        trace_id: new_uuid_v4(),
        use_pri,
        cookie: headers.remove(COOKIE),
    });

    let mut e = match CollectBytesParts(req.body(data)).await {
        Ok((mut parts, bytes)) => {
            for key in TO_REMOVE_HEADERS {
                let _ = parts.headers.remove(key);
            }
            return match direct::decode::<FsSyncFileResponse>(&bytes) {
                Ok(DecodedMessage::Protobuf(data)) => Ok(Response::from_parts(
                    parts,
                    Body::from(__unwrap!(serde_json::to_vec(&data))),
                )),
                Ok(DecodedMessage::Text(s)) => Err(__unwrap!(
                    Response::builder()
                        .header(CONTENT_TYPE, JSON)
                        .header(CONTENT_LENGTH, s.len())
                        .body(Body::from(s))
                )),
                Err(DecoderError::Internal(e)) => Err((
                    StatusCode::BAD_GATEWAY,
                    Json(ChatError::ProcessingFailed(Cow::Borrowed(e)).to_generic()),
                )
                    .into_response()),
            };
        }
        Err(e) => e,
    };
    e = e.without_url();
    let status_code = if e.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    Err((status_code, Json(ChatError::RequestFailed(Cow::Owned(e.to_string())).to_generic()))
        .into_response())
}

pub async fn handle_stream_cpp(
    mut headers: HeaderMap,
    mut extensions: Extensions,
    Json(request): Json<StreamCppRequest>,
) -> Result<Response, (StatusCode, Json<GenericError>)> {
    let (ext_token, use_pri) = __unwrap!(extensions.remove::<TokenBundle>());

    let data = match encode_message_framed(&request) {
        Ok(o) => o,
        Err(e) => return Err(e.into_response_tuple()),
    };

    let req = build_client_request(AiServiceRequest {
        ext_token: &ext_token,
        fs_client_key: headers.remove(CLIENT_KEY),
        url: ext_token.get_gcpp_host().get_url(CppService::StreamCpp, use_pri),
        stream: true,
        compressed: true,
        trace_id: new_uuid_v4(),
        use_pri,
        cookie: headers.remove(COOKIE),
    });

    let res = match req.body(data).send().await {
        Ok(r) => r,
        Err(mut e) => {
            e = e.without_url();

            return Err((
                if e.is_timeout() {
                    StatusCode::GATEWAY_TIMEOUT
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                },
                Json(ChatError::RequestFailed(Cow::Owned(e.to_string())).to_generic()),
            ));
        }
    };

    // SSE 事件格式化
    #[inline]
    fn format_sse_event(vector: &mut Vec<u8>, message: &StreamMessage) {
        vector.extend_from_slice(b"event: ");
        vector.extend_from_slice(message.type_name().as_bytes());
        vector.extend_from_slice(b"\ndata: ");
        let vector = {
            let mut ser = serde_json::Serializer::new(vector);
            __unwrap!(serde::Serialize::serialize(message, &mut ser));
            ser.into_inner()
        };
        vector.extend_from_slice(b"\n\n");
    }

    fn process_messages<I>(messages: impl IntoIterator<Item = I::Item, IntoIter = I>) -> Vec<u8>
    where I: Iterator<Item = StreamMessage> {
        let mut response_data = Vec::with_capacity(128);
        for message in messages {
            format_sse_event(&mut response_data, &message);
        }
        response_data
    }

    let mut decoder = StreamDecoder::new();

    let stream = res.bytes_stream().map(move |chunk| {
        let chunk = match chunk {
            Ok(c) => c,
            Err(_) => return Ok::<_, Infallible>(Bytes::new()),
        };

        let messages = match decoder.decode(&chunk) {
            Ok(msgs) => msgs,
            Err(()) => {
                let count = decoder.get_empty_stream_count();
                if count > 1 {
                    eprintln!("[警告] 连续空流: {count} 次");
                    return Ok(Bytes::from_static(
                        b"event: error\ndata: {\"type\":\"error\",\"error\":{\"code\":533,\"type\":\"unknown\",\"details\":{\"title\":\"Empty\",\"detail\":\"Empty stream\"}}}\n\n",
                    ));
                }
                return Ok(Bytes::new());
            }
        };

        if messages.is_empty() {
            return Ok(Bytes::new());
        }

        Ok(Bytes::from(process_messages(messages)))
    });

    Ok(__unwrap!(
        Response::builder()
            .header(CACHE_CONTROL, NO_CACHE_REVALIDATE)
            .header(CONNECTION, KEEP_ALIVE)
            .header(CONTENT_TYPE, EVENT_STREAM)
            .header(TRANSFER_ENCODING, CHUNKED)
            .body(Body::from_stream(stream))
    ))
}
