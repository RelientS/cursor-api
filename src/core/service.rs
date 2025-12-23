pub mod cpp;

use crate::{
    app::{
        constant::{
            CHATCMPL_PREFIX, ERR_RESPONSE_RECEIVED, ERR_STREAM_RESPONSE, MSG01_PREFIX,
            OBJECT_CHAT_COMPLETION, OBJECT_CHAT_COMPLETION_CHUNK, THINKING_TAG_CLOSE,
            THINKING_TAG_OPEN, UPSTREAM_FAILURE,
            header::{CHUNKED, EVENT_STREAM, JSON, KEEP_ALIVE, NO_CACHE_REVALIDATE},
        },
        lazy::{AUTH_TOKEN, REAL_USAGE, chat_url, dry_chat_url},
        model::{
            AppConfig, AppState, Chain, ChainUsage, DateTime, ErrorInfo, LogStatus, LogTokenInfo,
            LogUpdate, QueueType, RequestLog, TimingInfo, TokenKey, UsageCheck, log_manager,
        },
    },
    common::{
        client::{AiServiceRequest, build_client_request},
        model::{ApiStatus, GenericError, error::ChatError, tri::TriState},
        utils::{
            CollectBytes, TrimNewlines as _, get_available_models, get_token_profile,
            get_token_usage, new_uuid_v4, string_builder, tokeninfo_to_token,
        },
    },
    core::{
        aiserver::v1::EnvironmentInfo,
        auth::{AuthError, TokenBundleResult, auth},
        config::{KeyConfig, parse_dynamic_token},
        constant::Models,
        error::StreamError,
        model::{
            ExtModel, MessageId, ModelsResponse, RawModelsResponse, Role,
            anthropic::{self, AnthropicError},
            openai::{self, OpenAiError},
        },
        stream::{
            decoder::{StreamDecoder, StreamMessage, Thinking},
            droppable::DroppableStream,
        },
    },
};
use alloc::{borrow::Cow, sync::Arc};
use axum::{
    Json,
    body::Body,
    extract::{Query, State},
    response::Response,
};
use bytes::Bytes;
use core::{
    convert::Infallible,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering},
};
use futures::StreamExt as _;
use http::{
    Extensions, StatusCode,
    header::{CACHE_CONTROL, CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING},
};
use interned::Str;
use tokio::sync::Mutex;

pub async fn handle_raw_models() -> Result<Json<RawModelsResponse>, (StatusCode, Json<GenericError>)>
{
    if let Some(available_models) = Models::to_raw_arc() {
        Ok(Json(RawModelsResponse(available_models)))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(GenericError {
                status: ApiStatus::Error,
                code: Some(StatusCode::NOT_FOUND),
                error: Some(Cow::Borrowed("Models data not available")),
                message: Some(Cow::Borrowed(
                    "Please request /v1/models first to initialize models data",
                )),
            }),
        ))
    }
}

pub async fn handle_models(
    State(state): State<Arc<AppState>>,
    headers: http::HeaderMap,
    Query(request): Query<super::aiserver::v1::AvailableModelsRequest>,
) -> Result<Json<ModelsResponse>, (StatusCode, Json<GenericError>)> {
    // 如果没有认证头，返回默认可用模型
    let Some(auth_token) = auth(&headers) else { return Ok(Json(ModelsResponse)) };

    // 获取token信息
    let (ext_token, use_pri) = (async || {
        // 管理员 Token
        if let Some(part) = auth_token.strip_prefix(&**AUTH_TOKEN) {
            let token_manager = state.token_manager.read().await;

            let bundle = if part.is_empty() {
                token_manager
                    .select(QueueType::PrivilegedFree)
                    .ok_or(AuthError::NoAvailableTokens)?
            } else if let Some(alias) = part.strip_prefix('-') {
                if !token_manager.alias_map().contains_key(alias) {
                    return Err(AuthError::AliasNotFound);
                }
                token_manager
                    .get_by_alias(alias)
                    .map(|token_info| token_info.bundle.clone())
                    .ok_or(AuthError::Unauthorized)?
            } else {
                return Err(AuthError::Unauthorized);
            };

            return Ok((bundle, true));
        } else
        // 共享 Token
        if AppConfig::is_share() && AppConfig::share_token_eq(auth_token) {
            let token_manager = state.token_manager.read().await;
            let bundle =
                token_manager.select(QueueType::NormalFree).ok_or(AuthError::NoAvailableTokens)?;
            return Ok((bundle, true));
        } else
        // 普通用户 Token
        if let Some(key) = TokenKey::from_string(auth_token) {
            if let Some(bundle) = log_manager::get_token(key).await {
                return Ok((bundle, false));
            }
        } else
        // 动态密钥
        if AppConfig::get_dynamic_key() {
            if let Some(parsed_config) = parse_dynamic_token(auth_token) {
                if let Some(ext_token) = parsed_config.token_info.and_then(tokeninfo_to_token) {
                    return Ok((ext_token, false));
                }
            }
        }

        Err(AuthError::Unauthorized)
    })()
    .await
    .map_err(AuthError::into_generic_tuple)?;

    // 获取可用模型列表
    let models = get_available_models(ext_token, use_pri, request).await.ok_or((
        UPSTREAM_FAILURE,
        Json(GenericError {
            status: ApiStatus::Error,
            code: Some(UPSTREAM_FAILURE),
            error: Some(Cow::Borrowed("Failed to fetch available models")),
            message: Some(Cow::Borrowed("Unable to get available models")),
        }),
    ))?;

    // 更新模型列表
    Models::update(models).map_err(|e| {
        (
            UPSTREAM_FAILURE,
            Json(GenericError {
                status: ApiStatus::Error,
                code: Some(UPSTREAM_FAILURE),
                error: Some(Cow::Borrowed("Failed to update models")),
                message: Some(Cow::Borrowed(e)),
            }),
        )
    })?;

    Ok(Json(ModelsResponse))
}

// 聊天处理函数的签名
pub async fn handle_chat_completions(
    State(state): State<Arc<AppState>>,
    mut extensions: Extensions,
    Json(request): Json<openai::ChatRequest>,
) -> Result<Response<Body>, (StatusCode, Json<OpenAiError>)> {
    let (ext_token, use_pri) =
        __unwrap!(extensions.remove::<TokenBundleResult>()).map_err(|e| e.into_openai_tuple())?;

    // 验证模型是否支持并获取模型信息
    let model = if let Some(model) = ExtModel::from_str(&request.model) {
        model
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ChatError::ModelNotSupported(request.model).to_openai()),
        ));
    };

    // 验证请求
    if request.messages.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(ChatError::EmptyMessages.to_openai())));
    }

    let current_config = __unwrap!(extensions.remove::<KeyConfig>());

    let environment_info = __unwrap!(extensions.remove::<EnvironmentInfo>());

    let current_id: u64;
    let mut usage_check = None;

    let request_time = __unwrap!(extensions.remove::<DateTime>());

    // 更新请求日志
    state.increment_total();
    state.increment_active();
    if log_manager::is_enabled() {
        // let mut need_profile_check = false;

        // {
        //     let log_manager = state.log_manager_lock().await;
        //     for log in log_manager.logs().iter().rev() {
        //         if log_manager
        //             .get_token(&log.token_info.key)
        //             .expect(ERR_LOG_TOKEN_NOT_FOUND)
        //             .primary_token
        //             == ext_token.primary_token
        //             && let (Some(stripe), Some(usage)) =
        //                 (&log.token_info.stripe, &log.token_info.usage)
        //         {
        //             if stripe.membership_type == MembershipType::Free {
        //                 need_profile_check = if FREE_MODELS.contains(&model.id) {
        //                     usage
        //                         .standard
        //                         .max_requests
        //                         .is_some_and(|max| usage.standard.num_requests >= max)
        //                 } else {
        //                     usage
        //                         .premium
        //                         .max_requests
        //                         .is_some_and(|max| usage.premium.num_requests >= max)
        //                 };
        //             }
        //             break;
        //         }
        //     }
        // }

        // // 处理检查结果
        // if need_profile_check {
        //     state.decrement_active();
        //     state.increment_error();
        //     return Err((
        //         StatusCode::UNAUTHORIZED,
        //         Json(ChatError::Unauthorized.to_openai()),
        //     ));
        // }

        let next_id = log_manager::get_next_log_id().await;
        current_id = next_id;

        log_manager::add_log(
            RequestLog {
                id: next_id,
                timestamp: request_time,
                model: model.id,
                token_info: LogTokenInfo {
                    key: ext_token.primary_token.key(),
                    usage: None,
                    user: None,
                    stripe: None,
                },
                chain: Chain { delays: None, usage: None, think: None },
                timing: TimingInfo { total: 0.0 },
                stream: request.stream,
                status: LogStatus::Pending,
                error: ErrorInfo::Empty,
            },
            ext_token.clone(),
        )
        .await;

        // 如果需要获取用户使用情况,创建后台任务获取profile
        if model
            .is_usage_check(current_config.usage_check_models.as_ref().map(UsageCheck::from_proto))
        {
            let unext = ext_token.store_unext();
            let state = state.clone();
            let log_id = next_id;
            let client = ext_token.get_client();

            usage_check = Some(async move {
                let (usage, stripe, user, ..) =
                    get_token_profile(client, unext.as_ref(), use_pri, false).await;

                // 更新日志中的profile
                log_manager::update_log(
                    log_id,
                    LogUpdate::TokenProfile(user.clone(), usage, stripe),
                )
                .await;

                let mut alias_updater = None;

                // 更新token manager中的profile
                if let Some(id) = {
                    state
                        .token_manager_read()
                        .await
                        .id_map()
                        .get(&unext.primary_token.key())
                        .copied()
                } {
                    let alias_is_unnamed = unsafe {
                        state
                            .token_manager_read()
                            .await
                            .id_to_alias()
                            .get_unchecked(id)
                            .as_ref()
                            .unwrap_unchecked()
                            .is_unnamed()
                    };
                    let mut token_manager = state.token_manager_write().await;
                    let token_info = unsafe { token_manager.tokens_mut().get_unchecked_mut(id) };
                    if alias_is_unnamed
                        && let Some(ref user) = user
                        && let Some(alias) = user.alias()
                    {
                        alias_updater = Some((id, alias.clone()));
                    }
                    token_info.user = user;
                    token_info.usage = usage;
                    token_info.stripe = stripe;
                };

                if let Some((id, alias)) = alias_updater {
                    let _ = state.token_manager_write().await.set_alias(id, alias);
                }
            });
        }
    } else {
        current_id = 0;
    }

    let disable_vision = __unwrap!(current_config.disable_vision);
    let enable_slow_pool = __unwrap!(current_config.enable_slow_pool);

    // 将消息转换为hex格式
    let msg_id = uuid::Uuid::new_v4();
    let hex_data = match super::adapter::openai::encode_create_params(
        request.messages,
        ext_token.now(),
        model,
        msg_id,
        environment_info,
        disable_vision,
        enable_slow_pool,
    )
    .await
    {
        Ok(data) => data,
        Err(e) => {
            log_manager::update_log(current_id, LogUpdate::Failure(e.to_log_error())).await;
            state.decrement_active();
            state.increment_error();
            return Err(e.into_openai_tuple());
        }
    };
    let msg_id = MessageId::new(msg_id.as_u128());

    // 构建请求客户端
    let req = build_client_request(AiServiceRequest {
        ext_token: &ext_token,
        fs_client_key: None,
        url: chat_url(use_pri),
        stream: true,
        compressed: true,
        trace_id: new_uuid_v4(),
        use_pri,
        cookie: None,
    });
    // 发送请求
    let response = req.body(hex_data).send().await;

    // 处理请求结果
    let response = match response {
        Ok(resp) => {
            // 更新请求日志为成功
            log_manager::update_log(current_id, LogUpdate::Success).await;
            resp
        }
        Err(mut e) => {
            if let Some(url) = e.url_mut() {
                let _ = url.set_host(None);
            }

            // 根据错误类型返回不同的状态码
            let status_code = if e.is_timeout() {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let e = e.to_string();

            // 更新请求日志为失败
            let error = Str::new(&e);
            log_manager::update_log(current_id, LogUpdate::Failure(ErrorInfo::Simple(error))).await;
            state.decrement_active();
            state.increment_error();

            return Err((status_code, Json(ChatError::RequestFailed(Cow::Owned(e)).to_openai())));
        }
    };

    // 释放活动请求计数
    state.decrement_active();

    let convert_web_ref = __unwrap!(current_config.include_web_references);

    if request.stream {
        let response_id = Arc::new({
            let mut buf = [0; 22];
            let mut s = String::with_capacity(31);
            s.push_str(CHATCMPL_PREFIX);
            s.push_str(msg_id.to_str(&mut buf));
            s
        });
        let is_start = Arc::new(AtomicBool::new(true));
        let meet_thinking = Arc::new(AtomicBool::new(false));
        let start_time = std::time::Instant::now();
        let decoder = Arc::new(Mutex::new(StreamDecoder::new()));
        let is_end = Arc::new(AtomicBool::new(false));
        let is_need = request.stream_options.is_some_and(|opt| opt.include_usage);

        // 定义消息处理器的上下文结构体
        struct MessageProcessContext<'a> {
            response_id: &'a str,
            model: &'static str,
            is_start: &'a AtomicBool,
            meet_thinking: &'a AtomicBool,
            start_time: std::time::Instant,
            current_id: u64,
            created: i64,
            is_end: &'a AtomicBool,
            is_need: bool,
        }

        #[inline]
        fn extend_from_slice(vector: &mut Vec<u8>, value: &openai::ChatResponse) {
            vector.extend_from_slice(b"data: ");
            let vector = {
                let mut ser = serde_json::Serializer::new(vector);
                __unwrap!(serde::Serialize::serialize(value, &mut ser));
                ser.into_inner()
            };
            vector.extend_from_slice(b"\n\n");
        }

        // 处理消息并生成响应数据的辅助函数
        async fn process_messages<I>(
            messages: impl IntoIterator<Item = I::Item, IntoIter = I>,
            ctx: &MessageProcessContext<'_>,
        ) -> Vec<u8>
        where
            I: Iterator<Item = StreamMessage>,
        {
            let mut response_data = Vec::with_capacity(128);

            for message in messages {
                match message {
                    StreamMessage::Content(text) => {
                        let is_first = ctx.is_start.load(Ordering::Acquire);
                        let meet_thinking = ctx.meet_thinking.load(Ordering::Acquire);

                        if meet_thinking {
                            ctx.meet_thinking.store(false, Ordering::Release);
                            let response = openai::ChatResponse {
                                id: ctx.response_id,
                                object: OBJECT_CHAT_COMPLETION_CHUNK,
                                created: ctx.created,
                                model: None,
                                choices: Some(openai::Choice {
                                    index: 0,
                                    message: None,
                                    delta: Some(openai::Delta {
                                        role: None,
                                        content: Some(Cow::Borrowed(*THINKING_TAG_CLOSE)),
                                    }),
                                    finish_reason: false,
                                }),
                                usage: TriState::Null(ctx.is_need),
                            };
                            extend_from_slice(&mut response_data, &response);
                        }

                        let response = openai::ChatResponse {
                            id: ctx.response_id,
                            object: OBJECT_CHAT_COMPLETION_CHUNK,
                            created: ctx.created,
                            model: if is_first { Some(ctx.model) } else { None },
                            choices: Some(openai::Choice {
                                index: 0,
                                message: None,
                                delta: Some(openai::Delta {
                                    role: if is_first { Some(Role::Assistant) } else { None },
                                    content: Some(Cow::Owned(if is_first {
                                        ctx.is_start.store(false, Ordering::Release);
                                        text.trim_leading_newlines()
                                    } else {
                                        text
                                    })),
                                }),
                                finish_reason: false,
                            }),
                            usage: TriState::Null(ctx.is_need),
                        };

                        extend_from_slice(&mut response_data, &response);
                    }
                    StreamMessage::Thinking(Thinking::Text(text)) => {
                        let is_first = ctx.is_start.load(Ordering::Acquire);
                        let meet_thinking = ctx.meet_thinking.load(Ordering::Acquire);

                        if !meet_thinking {
                            ctx.meet_thinking.store(true, Ordering::Release);
                            let response = openai::ChatResponse {
                                id: ctx.response_id,
                                object: OBJECT_CHAT_COMPLETION_CHUNK,
                                created: ctx.created,
                                model: if is_first { Some(ctx.model) } else { None },
                                choices: Some(openai::Choice {
                                    index: 0,
                                    message: None,
                                    delta: Some(openai::Delta {
                                        role: if is_first { Some(Role::Assistant) } else { None },
                                        content: Some(Cow::Borrowed(*THINKING_TAG_OPEN)),
                                    }),
                                    finish_reason: false,
                                }),
                                usage: TriState::Null(ctx.is_need),
                            };
                            extend_from_slice(&mut response_data, &response);
                        }

                        let response = openai::ChatResponse {
                            id: ctx.response_id,
                            object: OBJECT_CHAT_COMPLETION_CHUNK,
                            created: ctx.created,
                            model: None,
                            choices: Some(openai::Choice {
                                index: 0,
                                message: None,
                                delta: Some(openai::Delta {
                                    role: None,
                                    content: Some(Cow::Owned(if is_first {
                                        ctx.is_start.store(false, Ordering::Release);
                                        text.trim_leading_newlines()
                                    } else {
                                        text
                                    })),
                                }),
                                finish_reason: false,
                            }),
                            usage: TriState::Null(ctx.is_need),
                        };
                        extend_from_slice(&mut response_data, &response);
                    }
                    StreamMessage::StreamEnd => {
                        // 计算总时间和首次片段时间
                        let total_time = ctx.start_time.elapsed().as_secs_f64();

                        log_manager::update_log(ctx.current_id, LogUpdate::Timing(total_time))
                            .await;

                        let response = openai::ChatResponse {
                            id: ctx.response_id,
                            object: OBJECT_CHAT_COMPLETION_CHUNK,
                            created: ctx.created,
                            model: None,
                            choices: Some(openai::Choice {
                                index: 0,
                                message: None,
                                delta: Some(openai::Delta { role: None, content: None }),
                                finish_reason: true,
                            }),
                            usage: TriState::Null(ctx.is_need),
                        };
                        extend_from_slice(&mut response_data, &response);

                        ctx.is_end.store(true, Ordering::Release);
                        break;
                    }
                    // StreamMessage::Debug(debug_prompt) => {
                    //     log_manager::update_log(ctx.current_id, |log| {
                    //         if log.chain.is_some() {
                    //             __cold_path!();
                    //             crate::debug!("UB!1 {debug_prompt:?}");
                    //             // chain.prompt.push_str(&debug_prompt);
                    //         } else {
                    //             log.chain = Some(Chain {
                    //                 prompt: Prompt::new(debug_prompt),
                    //                 delays: None,
                    //                 usage: None,
                    //                 think: None,
                    //             });
                    //         }
                    //     })
                    //     .await;
                    // }
                    _ => {} // 忽略其他消息类型
                }
            }

            response_data
        }

        // 首先处理stream直到获得第一个结果
        let (mut stream, drop_handle) = DroppableStream::new(response.bytes_stream());
        {
            let mut decoder = decoder.lock().await;
            while !decoder.is_first_result_ready() {
                match stream.next().await {
                    Some(Ok(chunk)) => {
                        if let Err(StreamError::Upstream(error)) =
                            decoder.decode(&chunk, convert_web_ref)
                        {
                            let canonical = error.canonical();
                            // 更新请求日志为失败
                            log_manager::update_log(
                                current_id,
                                LogUpdate::Failure2(
                                    canonical.to_error_info(),
                                    start_time.elapsed().as_secs_f64(),
                                ),
                            )
                            .await;
                            state.increment_error();
                            return Err((
                                canonical.status_code(),
                                Json(canonical.into_openai().wrapped()),
                            ));
                        }
                    }
                    Some(Err(e)) => {
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(
                                ChatError::RequestFailed(Cow::Owned(format!(
                                    "Failed to read response chunk: {e}"
                                )))
                                .to_openai(),
                            ),
                        ));
                    }
                    None => {
                        // 更新请求日志为失败
                        log_manager::update_log(
                            current_id,
                            LogUpdate::Failure(ErrorInfo::Simple(Str::from_static(
                                ERR_STREAM_RESPONSE,
                            ))),
                        )
                        .await;
                        state.increment_error();
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(
                                ChatError::RequestFailed(Cow::Borrowed(ERR_STREAM_RESPONSE))
                                    .to_openai(),
                            ),
                        ));
                    }
                }
            }
        }

        let created = Arc::new(std::sync::OnceLock::new());
        let created_clone = created.clone();
        let response_id_clone = response_id.clone();

        let decoder_clone = decoder.clone();

        fn timestamp() -> i64 { DateTime::utc_now().timestamp() }

        // 处理后续的stream
        let stream = stream
            .then(move |chunk| {
                let decoder = decoder_clone.clone();
                let response_id = response_id_clone.clone();
                let is_start = is_start.clone();
                let meet_thinking = meet_thinking.clone();
                let created = created_clone.clone();
                let is_end = is_end.clone();
                let drop_handle = drop_handle.clone();

                async move {
                    let chunk = match chunk {
                        Ok(c) => c,
                        Err(_) => {
                            // crate::debug_println!("Find chunk error: {e:?}");
                            return Ok::<_, Infallible>(Bytes::new());
                        }
                    };

                    let ctx = MessageProcessContext {
                        response_id: &response_id,
                        model: model.id,
                        is_start: &is_start,
                        meet_thinking: &meet_thinking,
                        start_time,
                        current_id,
                        created: *created.get_or_init(timestamp),
                        is_end: &is_end,
                        is_need,
                    };

                    // 使用decoder处理chunk
                    let messages = match decoder.lock().await.decode(&chunk, convert_web_ref) {
                        Ok(msgs) => msgs,
                        Err(e) => {
                            match e {
                                // 处理普通空流错误
                                StreamError::EmptyStream => {
                                    let empty_stream_count = decoder.lock().await.get_empty_stream_count();
                                    if empty_stream_count > 1 {
                                        eprintln!("[警告] Stream error: empty stream (连续计数: {empty_stream_count})");
                                    }
                                    return Ok(Bytes::new());
                                }
                                // 罕见
                                StreamError::Upstream(e) => {
                                    let message = __unwrap!(serde_json::to_string(&e.canonical().into_openai().wrapped()));
                                    let messages = [StreamMessage::Content(message), StreamMessage::StreamEnd];
                                    return Ok(Bytes::from(process_messages(messages, &ctx).await));
                                }
                            }
                        }
                    };

                    // crate::debug!("{messages:?}");

                    let mut first_response = None;

                    if let Some(first_msg) = decoder.lock().await.take_first_result() {
                        first_response = Some(process_messages(first_msg, &ctx).await);
                    }

                    let current_response = process_messages(messages, &ctx).await;

                    let response_data = if let Some(mut first_response) = first_response {
                        first_response.extend(current_response);
                        first_response
                    } else {
                        current_response
                    };

                    if is_end.load(Ordering::Acquire) {
                        drop_handle.drop_stream();
                    }

                    // crate::debug!("{:?}", unsafe{str::from_utf8_unchecked(&response_data)});

                    Ok(Bytes::from(response_data))
                }
            })
            .chain(futures::stream::once(async move {
                // 更新delays
                let mut decoder_guard = decoder.lock().await;
                let content_delays = decoder_guard.take_content_delays();
                let thinking_content = decoder_guard.take_thinking_content();

                log_manager::update_log(current_id, LogUpdate::Delays(content_delays, thinking_content))
                    .await;

                let usage = if *REAL_USAGE {
                    let usage =
                        get_token_usage(ext_token, use_pri, request_time, model.id)
                            .await;
                    if let Some(usage) = usage {
                        log_manager::update_log(current_id, LogUpdate::Usage(usage))
                            .await;
                    }
                    usage.map(ChainUsage::into_openai)
                } else {
                    None
                };

                let mut response_data = Vec::with_capacity(128);

                if is_need {
                    let value = openai::ChatResponse {
                        id: &response_id,
                        object: OBJECT_CHAT_COMPLETION_CHUNK,
                        created: *created.get_or_init(timestamp),
                        model: None,
                        choices: None,
                        usage: TriState::Value(usage.unwrap_or_default()),
                    };
                    extend_from_slice(&mut response_data, &value);
                }

                response_data.extend_from_slice(b"data: [DONE]\n\n");

                if let Some(usage_check) = usage_check {
                    tokio::spawn(usage_check);
                }

                Ok(Bytes::from(response_data))
            }));

        Ok(__unwrap!(
            Response::builder()
                .header(CACHE_CONTROL, NO_CACHE_REVALIDATE)
                .header(CONNECTION, KEEP_ALIVE)
                .header(CONTENT_TYPE, EVENT_STREAM)
                .header(TRANSFER_ENCODING, CHUNKED)
                .body(Body::from_stream(stream))
        ))
    } else {
        // 非流式响应
        let start_time = std::time::Instant::now();
        let mut decoder = StreamDecoder::new().no_first_cache();
        let mut thinking_text = String::with_capacity(128);
        let mut full_text = String::with_capacity(128);
        let mut stream = response.bytes_stream();
        // let mut prompt = Prompt::None;

        // 逐个处理chunks
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        ChatError::RequestFailed(Cow::Owned(format!(
                            "Failed to read response chunk: {e}"
                        )))
                        .to_openai(),
                    ),
                )
            })?;

            // 立即处理当前chunk
            match decoder.decode(&chunk, convert_web_ref) {
                Ok(messages) => {
                    for message in messages {
                        match message {
                            StreamMessage::Content(text) => {
                                full_text.push_str(&text);
                            }
                            StreamMessage::Thinking(Thinking::Text(text)) => {
                                thinking_text.push_str(&text);
                            }
                            // StreamMessage::Debug(debug_prompt) => {
                            //     if prompt.is_none() {
                            //         prompt = Prompt::new(debug_prompt);
                            //     } else {
                            //         __cold_path!();
                            //         crate::debug!("UB!2 {debug_prompt:?}");
                            //     }
                            // }
                            _ => {}
                        }
                    }
                }
                Err(StreamError::Upstream(error)) => {
                    let canonical = error.canonical();
                    log_manager::update_log(
                        current_id,
                        LogUpdate::Failure(canonical.to_error_info()),
                    )
                    .await;
                    state.increment_error();
                    return Err((canonical.status_code(), Json(canonical.into_openai().wrapped())));
                }
                Err(StreamError::EmptyStream) => {
                    let empty_stream_count = decoder.get_empty_stream_count();
                    if empty_stream_count > 1 {
                        eprintln!(
                            "[警告] Stream error: empty stream (连续计数: {})",
                            decoder.get_empty_stream_count()
                        );
                    }
                }
            }
        }

        full_text = if !thinking_text.is_empty() {
            thinking_text = thinking_text.trim_leading_newlines();
            string_builder::StringBuilder::with_capacity(4)
                .append(*THINKING_TAG_OPEN)
                .append(&thinking_text)
                .append(*THINKING_TAG_CLOSE)
                .append(&full_text)
                .build()
        } else {
            full_text.trim_leading_newlines()
        };

        // 检查响应是否为空
        if full_text.is_empty() {
            // 更新请求日志为失败
            log_manager::update_log(
                current_id,
                LogUpdate::Failure(ErrorInfo::Simple(Str::from_static(ERR_RESPONSE_RECEIVED))),
            )
            .await;
            state.increment_error();
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ChatError::RequestFailed(Cow::Borrowed(ERR_RESPONSE_RECEIVED)).to_openai()),
            ));
        }

        let (chain_usage, openai_usage) = if *REAL_USAGE {
            let usage = get_token_usage(ext_token, use_pri, request_time, model.id).await;
            let openai = usage.map(ChainUsage::into_openai);
            (usage, openai)
        } else {
            (None, None)
        };

        let response_data = openai::ChatResponse {
            id: &{
                let mut buf = [0; 22];
                let mut s = String::with_capacity(31);
                s.push_str(CHATCMPL_PREFIX);
                s.push_str(msg_id.to_str(&mut buf));
                s
            },
            object: OBJECT_CHAT_COMPLETION,
            created: DateTime::utc_now().timestamp(),
            model: Some(model.id),
            choices: Some(openai::Choice {
                index: 0,
                message: Some(openai::Message {
                    role: Role::Assistant,
                    content: openai::MessageContent::String(full_text),
                }),
                delta: None,
                finish_reason: true,
            }),
            usage: TriState::Value(openai_usage.unwrap_or_default()),
        };

        // 更新请求日志时间信息和状态
        let total_time = start_time.elapsed().as_secs_f64();
        let content_delays = decoder.take_content_delays();
        let thinking_content = decoder.take_thinking_content();

        log_manager::update_log(
            current_id,
            LogUpdate::TimingChain(
                total_time,
                Chain { delays: content_delays, usage: chain_usage, think: thinking_content },
            ),
        )
        .await;

        if let Some(usage_check) = usage_check {
            tokio::spawn(usage_check);
        }

        let data = __unwrap!(serde_json::to_vec(&response_data));
        Ok(__unwrap!(
            Response::builder()
                .header(CACHE_CONTROL, NO_CACHE_REVALIDATE)
                .header(CONNECTION, KEEP_ALIVE)
                .header(CONTENT_TYPE, JSON)
                .header(CONTENT_LENGTH, data.len())
                .body(Body::from(data))
        ))
    }
}

pub async fn handle_messages(
    State(state): State<Arc<AppState>>,
    mut extensions: Extensions,
    Json(mut request): Json<anthropic::MessageCreateParams>,
) -> Result<Response<Body>, (StatusCode, Json<AnthropicError>)> {
    let (ext_token, use_pri) = __unwrap!(extensions.remove::<TokenBundleResult>())
        .map_err(|e| e.into_anthropic_tuple())?;

    // 验证模型是否支持并获取模型信息
    let model = &mut request.model;
    if matches!(request.thinking, Some(anthropic::ThinkingConfig::Enabled { .. })) {
        let prefix = model.trim_suffix("-online").trim_suffix("-max");
        if !prefix.ends_with("-thinking") {
            model.insert_str(prefix.len(), "-thinking");
        }
    }
    let model = if let Some(model) = ExtModel::from_str(model.as_str()) {
        model
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ChatError::ModelNotSupported(request.model).to_anthropic()),
        ));
    };
    let params = request;

    // 验证请求
    if params.messages.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(ChatError::EmptyMessages.to_anthropic())));
    }

    let current_config = __unwrap!(extensions.remove::<KeyConfig>());

    let environment_info = __unwrap!(extensions.remove::<EnvironmentInfo>());

    let current_id: u64;
    let mut usage_check = None;

    let request_time = __unwrap!(extensions.remove::<DateTime>());

    // 更新请求日志
    state.increment_total();
    state.increment_active();
    if log_manager::is_enabled() {
        // let mut need_profile_check = false;

        // {
        //     let log_manager = state.log_manager_lock().await;
        //     for log in log_manager.logs().iter().rev() {
        //         if log_manager
        //             .get_token(&log.token_info.key)
        //             .expect(ERR_LOG_TOKEN_NOT_FOUND)
        //             .primary_token
        //             == ext_token.primary_token
        //             && let (Some(stripe), Some(usage)) =
        //                 (&log.token_info.stripe, &log.token_info.usage)
        //         {
        //             if stripe.membership_type == MembershipType::Free {
        //                 need_profile_check = if FREE_MODELS.contains(&model.id) {
        //                     usage
        //                         .standard
        //                         .max_requests
        //                         .is_some_and(|max| usage.standard.num_requests >= max)
        //                 } else {
        //                     usage
        //                         .premium
        //                         .max_requests
        //                         .is_some_and(|max| usage.premium.num_requests >= max)
        //                 };
        //             }
        //             break;
        //         }
        //     }
        // }

        // // 处理检查结果
        // if need_profile_check {
        //     state.decrement_active();
        //     state.increment_error();
        //     return Err((
        //         StatusCode::UNAUTHORIZED,
        //         Json(ChatError::Unauthorized.to_generic()),
        //     ));
        // }

        let next_id = log_manager::get_next_log_id().await;
        current_id = next_id;

        log_manager::add_log(
            RequestLog {
                id: next_id,
                timestamp: request_time,
                model: model.id,
                token_info: LogTokenInfo {
                    key: ext_token.primary_token.key(),
                    usage: None,
                    user: None,
                    stripe: None,
                },
                chain: Chain { delays: None, usage: None, think: None },
                timing: TimingInfo { total: 0.0 },
                stream: params.stream,
                status: LogStatus::Pending,
                error: ErrorInfo::Empty,
            },
            ext_token.clone(),
        )
        .await;

        // 如果需要获取用户使用情况,创建后台任务获取profile
        if model
            .is_usage_check(current_config.usage_check_models.as_ref().map(UsageCheck::from_proto))
        {
            let unext = ext_token.store_unext();
            let state = state.clone();
            let log_id = next_id;
            let client = ext_token.get_client();

            usage_check = Some(async move {
                let (usage, stripe, user, ..) =
                    get_token_profile(client, unext.as_ref(), use_pri, false).await;

                // 更新日志中的profile
                log_manager::update_log(
                    log_id,
                    LogUpdate::TokenProfile(user.clone(), usage, stripe),
                )
                .await;

                let mut alias_updater = None;

                // 更新token manager中的profile
                if let Some(id) = {
                    state
                        .token_manager_read()
                        .await
                        .id_map()
                        .get(&unext.primary_token.key())
                        .copied()
                } {
                    let alias_is_unnamed = unsafe {
                        state
                            .token_manager_read()
                            .await
                            .id_to_alias()
                            .get_unchecked(id)
                            .as_ref()
                            .unwrap_unchecked()
                            .is_unnamed()
                    };
                    let mut token_manager = state.token_manager_write().await;
                    let token_info = unsafe { token_manager.tokens_mut().get_unchecked_mut(id) };
                    if alias_is_unnamed
                        && let Some(ref user) = user
                        && let Some(alias) = user.alias()
                    {
                        alias_updater = Some((id, alias.clone()));
                    }
                    token_info.user = user;
                    token_info.usage = usage;
                    token_info.stripe = stripe;
                };

                if let Some((id, alias)) = alias_updater {
                    let _ = state.token_manager_write().await.set_alias(id, alias);
                }
            });
        }
    } else {
        current_id = 0;
    }

    let disable_vision = __unwrap!(current_config.disable_vision);
    let enable_slow_pool = __unwrap!(current_config.enable_slow_pool);

    // 将消息转换为hex格式
    let stream = params.stream;
    let msg_id = uuid::Uuid::new_v4();
    let hex_data = match super::adapter::anthropic::encode_create_params(
        params,
        ext_token.now(),
        model,
        msg_id,
        environment_info,
        disable_vision,
        enable_slow_pool,
    )
    .await
    {
        Ok(data) => data,
        Err(e) => {
            log_manager::update_log(current_id, LogUpdate::Failure(e.to_log_error())).await;
            state.decrement_active();
            state.increment_error();
            return Err(e.into_anthropic_tuple());
        }
    };
    let msg_id = MessageId::new(msg_id.as_u128());

    // 构建请求客户端
    let req = build_client_request(AiServiceRequest {
        ext_token: &ext_token,
        fs_client_key: None,
        url: chat_url(use_pri),
        stream: true,
        compressed: true,
        trace_id: new_uuid_v4(),
        use_pri,
        cookie: None,
    });
    // 发送请求
    let response = req.body(hex_data).send().await;

    // 处理请求结果
    let response = match response {
        Ok(resp) => {
            // 更新请求日志为成功
            log_manager::update_log(current_id, LogUpdate::Success).await;
            resp
        }
        Err(mut e) => {
            if let Some(url) = e.url_mut() {
                let _ = url.set_host(None);
            }

            // 根据错误类型返回不同的状态码
            let status_code = if e.is_timeout() {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let e = e.to_string();

            // 更新请求日志为失败
            let error = Str::new(&e);
            log_manager::update_log(current_id, LogUpdate::Failure(ErrorInfo::Simple(error))).await;
            state.decrement_active();
            state.increment_error();

            return Err((
                status_code,
                Json(ChatError::RequestFailed(Cow::Owned(e)).to_anthropic()),
            ));
        }
    };

    // 释放活动请求计数
    state.decrement_active();

    let convert_web_ref = __unwrap!(current_config.include_web_references);

    if stream {
        let msg_id = Arc::new({
            let mut buf = [0; 22];
            let mut s = String::with_capacity(28);
            s.push_str(MSG01_PREFIX);
            s.push_str(msg_id.to_str(&mut buf));
            s
        });
        let index = Arc::new(AtomicU32::new(0));
        let start_time = std::time::Instant::now();
        let decoder = Arc::new(Mutex::new(StreamDecoder::new()));
        let stream_state = Arc::new(AtomicU8::new(0));
        let last_content_type = Arc::new(AtomicU8::new(0));

        #[repr(u8)]
        #[derive(Clone, Copy, PartialEq)]
        enum StreamState {
            /// 初始状态，什么都未开始
            NotStarted = 0,
            // /// message_start 已完成，等待 content_block_start
            // MessageStarted = 1,
            /// content_block_start 已完成，正在接收 content_block_delta
            ContentBlockActive = 2,
            // /// content_block_stop 已完成，等待下一个 content_block_start 或 message_delta
            // BetweenBlocks = 3,
            // /// message_delta 已完成，等待 message_stop
            // MessageEnding = 4,
            /// message_stop 已完成，流结束
            Completed = 5,
        }

        #[repr(u8)]
        #[derive(Clone, Copy, PartialEq)]
        enum LastContentType {
            None = 0,
            Thinking = 1,
            Text = 2,
            InputJson = 3,
        }

        // 定义消息处理器的上下文结构体
        struct MessageProcessContext<'a> {
            msg_id: &'a str,
            model: &'static str,
            index: &'a AtomicU32,
            start_time: std::time::Instant,
            stream_state: &'a AtomicU8,
            last_content_type: &'a AtomicU8,
            current_id: u64,
        }

        #[inline]
        fn extend_from_slice(vector: &mut Vec<u8>, value: &anthropic::RawMessageStreamEvent) {
            vector.extend_from_slice(b"event: ");
            vector.extend_from_slice(value.type_name().as_bytes());
            vector.extend_from_slice(b"\ndata: ");
            let vector = {
                let mut ser = serde_json::Serializer::new(vector);
                __unwrap!(serde::Serialize::serialize(value, &mut ser));
                ser.into_inner()
            };
            vector.extend_from_slice(b"\n\n");
        }

        // 处理消息并生成响应数据的辅助函数
        async fn process_messages(
            messages: Vec<StreamMessage>,
            ctx: &MessageProcessContext<'_>,
        ) -> Vec<u8> {
            let mut response_data = Vec::with_capacity(128);

            for message in messages {
                match message {
                    StreamMessage::Thinking(thinking) => {
                        // 检查是否需要开始消息
                        let current_state = ctx.stream_state.load(Ordering::Acquire);
                        let is_start = current_state == StreamState::NotStarted as u8;
                        if is_start {
                            let event = anthropic::RawMessageStreamEvent::MessageStart {
                                message: anthropic::Message {
                                    content: vec![],
                                    usage: anthropic::Usage::default(),
                                    id: ctx.msg_id,
                                    model: ctx.model,
                                    stop_reason: None,
                                },
                            };
                            extend_from_slice(&mut response_data, &event);
                        }

                        // 检查是否需要切换或开始内容块
                        let last_type = ctx.last_content_type.load(Ordering::Acquire);

                        if last_type != LastContentType::Thinking as u8 {
                            // 如果上次不是思考类型，需要结束上个块(如果有的话)
                            if last_type != LastContentType::None as u8 {
                                let event = anthropic::RawMessageStreamEvent::ContentBlockStop {
                                    index: ctx.index.load(Ordering::Acquire),
                                };
                                extend_from_slice(&mut response_data, &event);
                                ctx.index.fetch_add(1, Ordering::AcqRel);
                            }

                            // 开始新的思考块
                            let event = anthropic::RawMessageStreamEvent::ContentBlockStart {
                                index: ctx.index.load(Ordering::Acquire),
                                content_block: anthropic::ContentBlock::Thinking {
                                    thinking: String::new(),
                                    signature: None,
                                },
                            };
                            extend_from_slice(&mut response_data, &event);

                            // 如果是刚开始，发送ping事件
                            if is_start {
                                let event = anthropic::RawMessageStreamEvent::Ping;
                                extend_from_slice(&mut response_data, &event);
                            }

                            ctx.last_content_type
                                .store(LastContentType::Thinking as u8, Ordering::Release);
                            ctx.stream_state
                                .store(StreamState::ContentBlockActive as u8, Ordering::Release);
                        }

                        match thinking {
                            Thinking::Text(text) => {
                                let event = anthropic::RawMessageStreamEvent::ContentBlockDelta {
                                    index: ctx.index.load(Ordering::Acquire),
                                    delta: anthropic::RawContentBlockDelta::ThinkingDelta {
                                        thinking: text,
                                    },
                                };
                                extend_from_slice(&mut response_data, &event);
                            }
                            Thinking::Signature(signature) => {
                                let event = anthropic::RawMessageStreamEvent::ContentBlockDelta {
                                    index: ctx.index.load(Ordering::Acquire),
                                    delta: anthropic::RawContentBlockDelta::SignatureDelta {
                                        signature,
                                    },
                                };
                                extend_from_slice(&mut response_data, &event);
                            }
                            _ => {}
                        }
                    }
                    StreamMessage::Content(text) => {
                        // 检查是否需要开始消息
                        let current_state = ctx.stream_state.load(Ordering::Acquire);
                        let is_start = current_state == StreamState::NotStarted as u8;
                        if is_start {
                            let event = anthropic::RawMessageStreamEvent::MessageStart {
                                message: anthropic::Message {
                                    content: vec![],
                                    usage: anthropic::Usage::default(),
                                    id: ctx.msg_id,
                                    model: ctx.model,
                                    stop_reason: None,
                                },
                            };
                            extend_from_slice(&mut response_data, &event);
                        }

                        // 检查是否需要切换或开始内容块
                        let last_type = ctx.last_content_type.load(Ordering::Acquire);

                        if last_type != LastContentType::Text as u8 {
                            // 如果上次不是文本类型，需要结束上个块(如果有的话)
                            if last_type != LastContentType::None as u8 {
                                let event = anthropic::RawMessageStreamEvent::ContentBlockStop {
                                    index: ctx.index.load(Ordering::Acquire),
                                };
                                extend_from_slice(&mut response_data, &event);
                                ctx.index.fetch_add(1, Ordering::AcqRel);
                            }

                            // 开始新的文本块
                            let event = anthropic::RawMessageStreamEvent::ContentBlockStart {
                                index: ctx.index.load(Ordering::Acquire),
                                content_block: anthropic::ContentBlock::Text {
                                    text: String::new(),
                                },
                            };
                            extend_from_slice(&mut response_data, &event);

                            // 如果是刚开始，发送ping事件
                            if is_start {
                                let event = anthropic::RawMessageStreamEvent::Ping;
                                extend_from_slice(&mut response_data, &event);
                            }

                            ctx.last_content_type
                                .store(LastContentType::Text as u8, Ordering::Release);
                            ctx.stream_state
                                .store(StreamState::ContentBlockActive as u8, Ordering::Release);
                        }

                        let event = anthropic::RawMessageStreamEvent::ContentBlockDelta {
                            index: ctx.index.load(Ordering::Acquire),
                            delta: anthropic::RawContentBlockDelta::TextDelta { text },
                        };
                        extend_from_slice(&mut response_data, &event);
                    }
                    StreamMessage::ToolCall(tool_call) => {
                        // 检查是否需要切换或开始内容块
                        let last_type = ctx.last_content_type.load(Ordering::Acquire);

                        if last_type != LastContentType::InputJson as u8 {
                            // 如果上次不是InputJson类型，需要结束上个块(如果有的话)
                            if last_type != LastContentType::None as u8 {
                                let event = anthropic::RawMessageStreamEvent::ContentBlockStop {
                                    index: ctx.index.load(Ordering::Acquire),
                                };
                                extend_from_slice(&mut response_data, &event);
                                ctx.index.fetch_add(1, Ordering::AcqRel);
                            }

                            // 开始新的InputJson块
                            let event = anthropic::RawMessageStreamEvent::ContentBlockStart {
                                index: ctx.index.load(Ordering::Acquire),
                                content_block: anthropic::ContentBlock::ToolUse {
                                    id: tool_call.id,
                                    name: tool_call.name,
                                    input: indexmap::IndexMap::with_hasher(
                                        ahash::RandomState::new(),
                                    ),
                                },
                            };
                            extend_from_slice(&mut response_data, &event);

                            let event = anthropic::RawMessageStreamEvent::ContentBlockDelta {
                                index: ctx.index.load(Ordering::Acquire),
                                delta: anthropic::RawContentBlockDelta::InputJsonDelta {
                                    partial_json: String::new(),
                                },
                            };
                            extend_from_slice(&mut response_data, &event);

                            ctx.last_content_type
                                .store(LastContentType::Text as u8, Ordering::Release);
                            ctx.stream_state
                                .store(StreamState::ContentBlockActive as u8, Ordering::Release);
                        }

                        let event = anthropic::RawMessageStreamEvent::ContentBlockDelta {
                            index: ctx.index.load(Ordering::Acquire),
                            delta: anthropic::RawContentBlockDelta::InputJsonDelta {
                                partial_json: tool_call.input,
                            },
                        };
                        extend_from_slice(&mut response_data, &event);
                    }
                    StreamMessage::StreamEnd => {
                        // 计算总时间和首次片段时间
                        let total_time = ctx.start_time.elapsed().as_secs_f64();

                        log_manager::update_log(ctx.current_id, LogUpdate::Timing(total_time))
                            .await;

                        // 结束当前内容块(如果有的话)
                        let last_type = ctx.last_content_type.load(Ordering::Acquire);
                        if last_type != LastContentType::None as u8 {
                            let event = anthropic::RawMessageStreamEvent::ContentBlockStop {
                                index: ctx.index.load(Ordering::Acquire),
                            };
                            extend_from_slice(&mut response_data, &event);
                        }

                        ctx.stream_state.store(StreamState::Completed as u8, Ordering::Release);
                        break;
                    }
                    // StreamMessage::Debug(debug_prompt) => {
                    //     log_manager::update_log(ctx.current_id, |log| {
                    //         if log.chain.is_some() {
                    //             __cold_path!();
                    //             crate::debug!("UB!1 {debug_prompt:?}");
                    //             // chain.prompt.push_str(&debug_prompt);
                    //         } else {
                    //             log.chain = Some(Chain {
                    //                 prompt: Prompt::new(debug_prompt),
                    //                 delays: None,
                    //                 usage: None,
                    //                 think: None,
                    //             });
                    //         }
                    //     })
                    //     .await;
                    // }
                    _ => {} // 忽略其他消息类型
                }
            }

            response_data
        }

        // 首先处理stream直到获得第一个结果
        let (mut stream, drop_handle) = DroppableStream::new(response.bytes_stream());
        {
            let mut decoder = decoder.lock().await;
            while !decoder.is_first_result_ready() {
                match stream.next().await {
                    Some(Ok(chunk)) => {
                        if let Err(StreamError::Upstream(error)) =
                            decoder.decode(&chunk, convert_web_ref)
                        {
                            let canonical = error.canonical();
                            // 更新请求日志为失败
                            log_manager::update_log(
                                current_id,
                                LogUpdate::Failure2(
                                    canonical.to_error_info(),
                                    start_time.elapsed().as_secs_f64(),
                                ),
                            )
                            .await;
                            state.increment_error();
                            return Err((
                                canonical.status_code(),
                                Json(canonical.into_anthropic().wrapped()),
                            ));
                        }
                    }
                    Some(Err(e)) => {
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(
                                ChatError::RequestFailed(Cow::Owned(format!(
                                    "Failed to read response chunk: {e}"
                                )))
                                .to_anthropic(),
                            ),
                        ));
                    }
                    None => {
                        // 更新请求日志为失败
                        log_manager::update_log(
                            current_id,
                            LogUpdate::Failure(ErrorInfo::Simple(Str::from_static(
                                ERR_STREAM_RESPONSE,
                            ))),
                        )
                        .await;
                        state.increment_error();
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(
                                ChatError::RequestFailed(Cow::Borrowed(ERR_STREAM_RESPONSE))
                                    .to_anthropic(),
                            ),
                        ));
                    }
                }
            }
        }

        let decoder_clone = decoder.clone();

        // 处理后续的stream
        let stream = stream
            .then(move |chunk| {
                let decoder = decoder_clone.clone();
                let msg_id = msg_id.clone();
                let index = index.clone();
                let stream_state = stream_state.clone();
                let last_content_type = last_content_type.clone();
                let drop_handle = drop_handle.clone();

                async move {
                    let chunk = match chunk {
                        Ok(c) => c,
                        Err(_) => {
                            // crate::debug_println!("Find chunk error: {e:?}");
                            return Ok::<_, Infallible>(Bytes::new());
                        }
                    };

                    let ctx = MessageProcessContext {
                        msg_id: &msg_id,
                        model: model.id,
                        index: &index,
                        start_time,
                        stream_state: &stream_state,
                        last_content_type: &last_content_type,
                        current_id,
                    };

                    // 使用decoder处理chunk
                    let messages = match decoder.lock().await.decode(&chunk, convert_web_ref) {
                        Ok(msgs) => msgs,
                        Err(e) => {
                            match e {
                                // 处理普通空流错误
                                StreamError::EmptyStream => {
                                    let empty_stream_count = decoder.lock().await.get_empty_stream_count();
                                    if empty_stream_count > 1 {
                                        eprintln!("[警告] Stream error: empty stream (连续计数: {empty_stream_count})");
                                    }
                                    return Ok(Bytes::new());
                                }
                                // 罕见
                                StreamError::Upstream(e) => {
                                    let canonical = e.canonical();
                                    let mut buf = Vec::with_capacity(128);
                                    extend_from_slice(&mut buf, &anthropic::RawMessageStreamEvent::Error {
                                        error: canonical.into_anthropic(),
                                    });
                                    return Ok(Bytes::from(buf));
                                }
                            }
                        }
                    };

                    let mut first_response = None;

                    if let Some(first_msg) = decoder.lock().await.take_first_result() {
                        first_response = Some(process_messages(first_msg, &ctx).await);
                    }

                    let current_response = process_messages(messages, &ctx).await;
                    let response_data = if let Some(mut first_response) = first_response {
                        first_response.extend_from_slice(&current_response);
                        first_response
                    } else {
                        current_response
                    };

                    // 检查是否已完成
                    if ctx.stream_state.load(Ordering::Acquire) == StreamState::Completed as u8 {
                        drop_handle.drop_stream();
                    }

                    Ok(Bytes::from(response_data))
                }
            })
            .chain(futures::stream::once(async move {
                // 更新delays
                let mut decoder_guard = decoder.lock().await;
                let content_delays = decoder_guard.take_content_delays();
                let thinking_content = decoder_guard.take_thinking_content();

                log_manager::update_log(current_id, LogUpdate::Delays(content_delays, thinking_content))
                    .await;

                // 处理使用量统计
                let usage = if *REAL_USAGE {
                    let usage =
                        get_token_usage(ext_token, use_pri, request_time, model.id).await;
                    if let Some(usage) = usage {
                        log_manager::update_log(current_id, LogUpdate::Usage(usage))
                            .await;
                    }
                    usage.map(ChainUsage::into_anthropic_delta)
                } else {
                    None
                };

                let mut response_data = Vec::with_capacity(128);

                extend_from_slice(&mut response_data, &anthropic::RawMessageStreamEvent::MessageDelta {
                    delta: anthropic::MessageDelta {
                        stop_reason: if decoder_guard.is_tool_processed() {
                            anthropic::StopReason::ToolUse
                        } else {
                            anthropic::StopReason::EndTurn
                        },
                    },
                    usage: usage.unwrap_or_default(),
                });
                response_data.extend_from_slice(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");

                if let Some(usage_check) = usage_check {
                    tokio::spawn(usage_check);
                }

                Ok(Bytes::from(response_data))
            }));

        Ok(__unwrap!(
            Response::builder()
                .header(CACHE_CONTROL, NO_CACHE_REVALIDATE)
                .header(CONNECTION, KEEP_ALIVE)
                .header(CONTENT_TYPE, EVENT_STREAM)
                .header(TRANSFER_ENCODING, CHUNKED)
                .body(Body::from_stream(stream))
        ))
    } else {
        // 非流式响应
        let start_time = std::time::Instant::now();
        let mut decoder = StreamDecoder::new().no_first_cache();
        let mut content = Vec::with_capacity(16);
        let mut stream = response.bytes_stream();
        // let mut prompt = Prompt::None;

        // 逐个处理chunks
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(
                        ChatError::RequestFailed(Cow::Owned(format!(
                            "Failed to read response chunk: {e}"
                        )))
                        .to_anthropic(),
                    ),
                )
            })?;

            // 立即处理当前chunk
            match decoder.decode(&chunk, convert_web_ref) {
                Ok(messages) => {
                    let mut input_json = String::with_capacity(64);
                    for message in messages {
                        match message {
                            StreamMessage::Thinking(thinking) => match thinking {
                                Thinking::Text(text) => {
                                    if let Some(anthropic::ContentBlock::Thinking {
                                        thinking,
                                        ..
                                    }) = content.last_mut()
                                    {
                                        thinking.reserve_exact(text.len() * 2);
                                        thinking.push_str(&text);
                                    } else {
                                        content.push(anthropic::ContentBlock::Thinking {
                                            thinking: text,
                                            signature: None,
                                        });
                                    }
                                }
                                Thinking::Signature(signature) => {
                                    if let Some(anthropic::ContentBlock::Thinking {
                                        signature: signature_ref,
                                        ..
                                    }) = content.last_mut()
                                    {
                                        *signature_ref = Some(signature);
                                    } else {
                                        crate::debug!("up!3 {signature:?}");
                                        content.push(anthropic::ContentBlock::Thinking {
                                            thinking: String::new(),
                                            signature: Some(signature),
                                        });
                                    }
                                }
                                Thinking::RedactedThinking(redacted_thinking) => {
                                    content.push(anthropic::ContentBlock::RedactedThinking {
                                        data: redacted_thinking,
                                    });
                                }
                            },
                            StreamMessage::Content(atext) => {
                                if let Some(anthropic::ContentBlock::Text { text }) =
                                    content.last_mut()
                                {
                                    text.reserve_exact(atext.len() * 2);
                                    text.push_str(&atext);
                                } else {
                                    let mut text = atext;
                                    text.reserve_exact(text.len());
                                    content.push(anthropic::ContentBlock::Text { text });
                                }
                            }
                            StreamMessage::ToolCall(tool_call) => {
                                input_json.push_str(&tool_call.input);
                                if decoder.is_tool_processed()
                                    && let Ok(input) = serde_json::from_str(&input_json)
                                {
                                    content.push(anthropic::ContentBlock::ToolUse {
                                        id: tool_call.id,
                                        name: tool_call.name,
                                        input,
                                    });
                                    input_json.clear();
                                }
                            }
                            // StreamMessage::Debug(debug_prompt) => {
                            //     if prompt.is_none() {
                            //         prompt = Prompt::new(debug_prompt);
                            //     } else {
                            //         __cold_path!();
                            //         crate::debug!("UB!2 {debug_prompt:?}");
                            //     }
                            // }
                            _ => {}
                        }
                    }
                }
                Err(StreamError::Upstream(error)) => {
                    let canonical = error.canonical();
                    log_manager::update_log(
                        current_id,
                        LogUpdate::Failure(canonical.to_error_info()),
                    )
                    .await;
                    state.increment_error();
                    return Err((
                        canonical.status_code(),
                        Json(canonical.into_anthropic().wrapped()),
                    ));
                }
                Err(StreamError::EmptyStream) => {
                    let empty_stream_count = decoder.get_empty_stream_count();
                    if empty_stream_count > 1 {
                        eprintln!(
                            "[警告] Stream error: empty stream (连续计数: {})",
                            decoder.get_empty_stream_count()
                        );
                    }
                }
            }
        }

        drop(stream);

        let (chain_usage, anthropic_usage) = if *REAL_USAGE {
            let usage = get_token_usage(ext_token, use_pri, request_time, model.id).await;
            let anthropic = usage.map(ChainUsage::into_anthropic);
            (usage, anthropic)
        } else {
            (None, None)
        };

        let response_data = anthropic::Message {
            stop_reason: Some(if decoder.is_tool_processed() {
                anthropic::StopReason::ToolUse
            } else {
                anthropic::StopReason::EndTurn
            }),
            content,
            usage: anthropic_usage.unwrap_or_default(),
            id: &{
                let mut buf = [0; 22];
                let mut s = String::with_capacity(28);
                s.push_str(MSG01_PREFIX);
                s.push_str(msg_id.to_str(&mut buf));
                s
            },
            model: model.id,
        };

        // 更新请求日志时间信息和状态
        let total_time = start_time.elapsed().as_secs_f64();
        let content_delays = decoder.take_content_delays();
        let thinking_content = decoder.take_thinking_content();

        log_manager::update_log(
            current_id,
            LogUpdate::TimingChain(
                total_time,
                Chain { delays: content_delays, usage: chain_usage, think: thinking_content },
            ),
        )
        .await;

        if let Some(usage_check) = usage_check {
            tokio::spawn(usage_check);
        }

        let data = __unwrap!(serde_json::to_vec(&response_data));
        Ok(__unwrap!(
            Response::builder()
                .header(CACHE_CONTROL, NO_CACHE_REVALIDATE)
                .header(CONNECTION, KEEP_ALIVE)
                .header(CONTENT_TYPE, JSON)
                .header(CONTENT_LENGTH, data.len())
                .body(Body::from(data))
        ))
    }
}

pub async fn handle_messages_count_tokens(
    mut extensions: Extensions,
    Json(mut request): Json<anthropic::MessageCreateParams>,
) -> Result<Response<Body>, (StatusCode, Json<AnthropicError>)> {
    let (ext_token, use_pri) = __unwrap!(extensions.remove::<TokenBundleResult>())
        .map_err(|e| e.into_anthropic_tuple())?;

    // 验证模型是否支持并获取模型信息
    let model = &mut request.model;
    if matches!(request.thinking, Some(anthropic::ThinkingConfig::Enabled { .. })) {
        let prefix = model.trim_suffix("-online").trim_suffix("-max");
        if !prefix.ends_with("-thinking") {
            model.insert_str(prefix.len(), "-thinking");
        }
    }
    let model = if let Some(model) = ExtModel::from_str(model.as_str()) {
        model
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ChatError::ModelNotSupported(request.model).to_anthropic()),
        ));
    };
    let params = request;

    // 验证请求
    if params.messages.is_empty() {
        return Err((StatusCode::BAD_REQUEST, Json(ChatError::EmptyMessages.to_anthropic())));
    }

    let current_config = __unwrap!(extensions.remove::<KeyConfig>());

    let environment_info = __unwrap!(extensions.remove::<EnvironmentInfo>());

    let disable_vision = __unwrap!(current_config.disable_vision);
    let enable_slow_pool = __unwrap!(current_config.enable_slow_pool);

    // 将消息转换为hex格式
    let msg_id = uuid::Uuid::new_v4();
    let hex_data = match super::adapter::anthropic::encode_create_params(
        params,
        ext_token.now(),
        model,
        msg_id,
        environment_info,
        disable_vision,
        enable_slow_pool,
    )
    .await
    {
        Ok(data) => data,
        Err(e) => return Err(e.into_anthropic_tuple()),
    };

    // 构建请求客户端
    let req = build_client_request(AiServiceRequest {
        ext_token: &ext_token,
        fs_client_key: None,
        url: dry_chat_url(use_pri),
        stream: true,
        compressed: true,
        trace_id: new_uuid_v4(),
        use_pri,
        cookie: None,
    });
    // 请求
    let response = match CollectBytes(req.body(hex_data)).await {
        Ok(resp) => {
            use super::{aiserver::v1::GetPromptDryRunResponse, error::CursorError};
            use prost::Message as _;
            match GetPromptDryRunResponse::decode(resp.clone()) {
                Ok(resp) => resp,
                Err(_) => {
                    if prost::encoding::is_vaild_utf8(&resp)
                        && let Ok(error) = CursorError::from_slice(resp.as_ref())
                    {
                        let canonical = error.canonical();
                        return Err((
                            canonical.status_code(),
                            Json(canonical.into_anthropic().wrapped()),
                        ));
                    }
                    return Err((UPSTREAM_FAILURE, Json(ChatError::EmptyMessages.to_anthropic())));
                }
            }
        }
        Err(mut e) => {
            if let Some(url) = e.url_mut() {
                let _ = url.set_host(None);
            }

            // 根据错误类型返回不同的状态码
            let status_code = if e.is_timeout() {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let e = e.to_string();

            return Err((
                status_code,
                Json(ChatError::RequestFailed(Cow::Owned(e)).to_anthropic()),
            ));
        }
    };

    let response_data = anthropic::MessagesCountTokens {
        input_tokens: {
            if let Some(c) = response.full_conversation_token_count
                && let Some(t) = c.num_tokens
            {
                t
            } else if let Some(c) = response.user_message_token_count
                && let Some(t) = c.num_tokens
            {
                t
            } else {
                0
            }
        },
    };

    let data = __unwrap!(serde_json::to_vec(&response_data));
    Ok(__unwrap!(
        Response::builder()
            .header(CACHE_CONTROL, NO_CACHE_REVALIDATE)
            .header(CONNECTION, KEEP_ALIVE)
            .header(CONTENT_TYPE, JSON)
            .header(CONTENT_LENGTH, data.len())
            .body(Body::from(data))
    ))
}
