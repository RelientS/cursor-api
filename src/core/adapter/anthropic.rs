use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use uuid::Uuid;

use crate::app::constant::EMPTY_STRING;
use crate::app::model::{AppConfig, DEFAULT_INSTRUCTIONS, VisionAbility, create_explicit_context};
use crate::app::model::proxy_pool::get_general_client;
use crate::common::utils::encode_message_framed;
use crate::core::aiserver::v1::{
    AzureState, ClientSideToolV2, ClientSideToolV2Call, ClientSideToolV2Result,
    ComposerExternalLink, ConversationMessage, CursorPosition, CursorRange, EnvironmentInfo,
    ImageProto, McpParams, McpResult, ModelDetails, StreamUnifiedChatRequest,
    StreamUnifiedChatRequestWithTools, ToolResultError, conversation_message, image_proto,
    mcp_params, stream_unified_chat_request,
};
use crate::core::constant::LONG_CONTEXT_MODELS;
use crate::core::model::{ExtModel, Role, ToolId};
use crate::core::model::anthropic::{
    ContentBlockParam, ImageSource, MediaType, MessageContent, MessageCreateParams, MessageParam,
    SystemContent, ToolResultContent, ToolResultContentBlock,
};
use super::{
    AGENT_MODE_NAME, ASK_MODE_NAME, AdapterError, BaseUuid, Messages, NEWLINE, ToOpt as _,
    WEB_SEARCH_MODE, extract_external_links, extract_web_references_info, sanitize_tool_name,
};

async fn process_message_params(
    mut inputs: Vec<MessageParam>,
    system: Option<SystemContent>,
    supported_tools: Vec<i32>,
    now_with_tz: chrono::DateTime<chrono_tz::Tz>,
    image_support: bool,
    is_agentic: bool,
) -> Result<(String, Messages, Vec<ComposerExternalLink>), AdapterError> {
    // 收集 system 指令
    let instructions = system.map(|content| match content {
        SystemContent::String(text) => text,
        SystemContent::Array(contents) => {
            contents.into_iter().map(|c| c.text).collect::<Vec<String>>().join(NEWLINE)
        }
    });

    // 使用默认指令或收集到的指令
    let instructions = if let Some(instructions) = instructions {
        instructions
    } else {
        DEFAULT_INSTRUCTIONS.get().get(now_with_tz)
    };

    // 处理空对话情况
    if inputs.is_empty() {
        return Ok((
            instructions,
            Messages::from_single(ConversationMessage {
                r#type: conversation_message::MessageType::Human as i32,
                bubble_id: Uuid::new_v4().to_string().into(),
                unified_mode: Some(stream_unified_chat_request::UnifiedMode::Chat as i32),
                // is_simple_looping_message: Some(false),
                ..Default::default()
            }),
            vec![],
        ));
    }

    // 如果第一条是 assistant，插入空的 user 消息
    if inputs.first().is_some_and(|input| input.role == Role::Assistant) {
        inputs.insert(
            0,
            MessageParam { role: Role::User, content: MessageContent::String(EMPTY_STRING.into()) },
        );
    }

    // 确保最后一条是 user
    // if inputs.last().is_some_and(|input| input.role == Role::Assistant) {
    //     inputs.push(MessageParam {
    //         role: Role::User,
    //         content: MessageContent::String(EMPTY_STRING.into()),
    //     });
    // }

    // 转换为 proto messages
    let mut messages = Messages::with_capacity(inputs.len());
    let mut base_uuid = BaseUuid::new();
    let mut inputs = inputs.into_iter().peekable();

    while let Some(input) = inputs.next() {
        let (text, images, thinking, next) = match input.content {
            MessageContent::String(text) => (text, vec![], vec![], None),
            MessageContent::Array(contents) if input.role == Role::User => {
                let text_parts_len = contents
                    .iter()
                    .filter(|c| matches!(**c, ContentBlockParam::Text { .. }))
                    .count();
                let images_len = if image_support { contents.len() - text_parts_len } else { 0 };
                let mut text_parts = Vec::with_capacity(text_parts_len);
                let mut images = Vec::with_capacity(images_len);

                for content in contents {
                    match content {
                        ContentBlockParam::Text { text } => text_parts.push(text),
                        ContentBlockParam::Image { source } => {
                            if image_support {
                                let res = {
                                    let vision_ability = AppConfig::get_vision_ability();

                                    match vision_ability {
                                        VisionAbility::None => Err(AdapterError::VisionDisabled),
                                        VisionAbility::Base64 => match source {
                                            ImageSource::Base64 { media_type, data } => {
                                                process_base64_image(media_type, &data)
                                            }
                                            ImageSource::Url { .. } => {
                                                Err(AdapterError::Base64Only)
                                            }
                                        },
                                        VisionAbility::All => match source {
                                            ImageSource::Base64 { media_type, data } => {
                                                process_base64_image(media_type, &data)
                                            }
                                            ImageSource::Url { url } => {
                                                super::process_http_image(
                                                    &url,
                                                    get_general_client(),
                                                )
                                                .await
                                            }
                                        },
                                    }
                                };
                                match res {
                                    Ok((image_data, dimension)) => {
                                        images.push(ImageProto {
                                            data: image_data,
                                            dimension,
                                            uuid: base_uuid.add_and_to_string(),
                                            // task_specific_description: None,
                                        });
                                    }
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                        _ => {}
                    }
                }

                (text_parts.join(NEWLINE), images, vec![], None)
            }
            MessageContent::Array(mut contents) if input.role == Role::Assistant => {
                let mut text_parts = Vec::new();
                let mut all_thinking_blocks = Vec::new();
                let mut next = None;

                if matches!(contents.last(), Some(ContentBlockParam::ToolUse { .. }))
                    && let ContentBlockParam::ToolUse { id, name, input } =
                        __unwrap!(contents.pop())
                    && let Some(peek_input) = inputs.peek()
                    && peek_input.role == Role::User
                    && let MessageContent::Array(ref contents) = peek_input.content
                    && contents.len() == 1
                    && matches!(contents[0], ContentBlockParam::ToolResult { .. })
                    && let MessageContent::Array(contents) = __unwrap!(inputs.next()).content
                    && let ContentBlockParam::ToolResult { tool_use_id, content, is_error } =
                        __unwrap!(contents.into_iter().next())
                {
                    let tool_name: prost::ByteStr =
                        format!("mcp_{}_{name}", sanitize_tool_name(&name)).into();
                    let (text, images) = match content {
                        None => (String::new(), vec![]),
                        Some(content) => {
                            match content {
                                ToolResultContent::String(text) => (text, vec![]),
                                ToolResultContent::Array(contents) => {
                                    let text_parts_len = contents
                                        .iter()
                                        .filter(|c| {
                                            matches!(**c, ToolResultContentBlock::Text { .. })
                                        })
                                        .count();
                                    let images_len = if image_support {
                                        contents.len() - text_parts_len
                                    } else {
                                        0
                                    };
                                    let mut text_parts = Vec::with_capacity(text_parts_len);
                                    let mut images = Vec::with_capacity(images_len);
                                    for content in contents {
                                        match content {
                                            ToolResultContentBlock::Text { text } => {
                                                text_parts.push(text)
                                            }
                                            ToolResultContentBlock::Image { source } => {
                                                if image_support {
                                                    let res = {
                                                        let vision_ability =
                                                            AppConfig::get_vision_ability();

                                                        match vision_ability {
                                                            VisionAbility::None => {
                                                                Err(AdapterError::VisionDisabled)
                                                            }
                                                            VisionAbility::Base64 => match source {
                                                                ImageSource::Base64 {
                                                                    media_type,
                                                                    data,
                                                                } => process_base64_image(
                                                                    media_type, &data,
                                                                ),
                                                                ImageSource::Url { .. } => {
                                                                    Err(AdapterError::Base64Only)
                                                                }
                                                            },
                                                            VisionAbility::All => match source {
                                                                ImageSource::Base64 {
                                                                    media_type,
                                                                    data,
                                                                } => process_base64_image(
                                                                    media_type, &data,
                                                                ),
                                                                ImageSource::Url { url } => {
                                                                    super::process_http_image(
                                                                        &url,
                                                                        get_general_client(),
                                                                    )
                                                                    .await
                                                                }
                                                            },
                                                        }
                                                    };
                                                    match res {
                                                        Ok((image_data, dimension)) => {
                                                            images.push(ImageProto {
                                                                data: image_data,
                                                                dimension,
                                                                uuid: base_uuid.add_and_to_string(),
                                                                // task_specific_description: None,
                                                            });
                                                        }
                                                        Err(e) => return Err(e),
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    (text_parts.join(NEWLINE), images)
                                }
                            }
                        }
                    };
                    let text: prost::ByteStr = text.into();
                    let tool_id = ToolId::parse(tool_use_id);
                    let (result, error) = if is_error {
                        let error =
                            Some(ToolResultError { model_visible_error_message: text.clone() });
                        (
                            Some(ClientSideToolV2Result {
                                tool: ClientSideToolV2::Mcp as i32,
                                tool_call_id: tool_id.tool_call_id.clone(),
                                error: error.clone(),
                                model_call_id: tool_id.model_call_id.clone(),
                                tool_index: Some(1),
                                result: Some(Result::McpResult(McpResult {
                                    selected_tool: name.clone(),
                                    result: text,
                                })),
                            }),
                            error,
                        )
                    } else {
                        (
                            Some(ClientSideToolV2Result {
                                tool: ClientSideToolV2::Mcp as i32,
                                tool_call_id: tool_id.tool_call_id.clone(),
                                error: None,
                                model_call_id: tool_id.model_call_id.clone(),
                                tool_index: Some(1),
                                result: Some(Result::McpResult(McpResult {
                                    selected_tool: name.clone(),
                                    result: text,
                                })),
                            }),
                            None,
                        )
                    };
                    use crate::core::aiserver::v1::client_side_tool_v2_call::Params;
                    use crate::core::aiserver::v1::client_side_tool_v2_result::Result;
                    use crate::core::aiserver::v1::conversation_message::ToolResult;
                    let raw_args: prost::ByteStr = __unwrap!(serde_json::to_string(&input)).into();
                    let tool_call = Some(ClientSideToolV2Call {
                        tool: ClientSideToolV2::Mcp as i32,
                        tool_call_id: id,
                        name: tool_name.clone(),
                        tool_index: Some(1),
                        params: Some(Params::McpParams(McpParams {
                            tools: vec![mcp_params::Tool {
                                name,
                                parameters: raw_args.clone(),
                                ..Default::default()
                            }],
                        })),
                        ..Default::default()
                    });
                    let result = ToolResult {
                        tool_call_id: tool_id.tool_call_id,
                        tool_name,
                        tool_index: 1,
                        model_call_id: tool_id.model_call_id,
                        raw_args,
                        result,
                        error,
                        images,
                        tool_call,
                    };
                    next = Some(ConversationMessage {
                        r#type: conversation_message::MessageType::Ai as i32,
                        bubble_id: Uuid::new_v4().to_string().into(),
                        server_bubble_id: Some(Uuid::new_v4().to_string().into()),
                        tool_results: vec![result],
                        unified_mode: Some(if is_agentic {
                            stream_unified_chat_request::UnifiedMode::Agent
                        } else {
                            stream_unified_chat_request::UnifiedMode::Chat
                        } as i32),
                        ..Default::default()
                    });
                }

                for content in contents {
                    match content {
                        ContentBlockParam::Text { text } => {
                            text_parts.push(text);
                        }
                        ContentBlockParam::Thinking { thinking, signature } => {
                            all_thinking_blocks.push(conversation_message::Thinking {
                                text: thinking,
                                signature,
                                redacted_thinking: String::new(),
                            });
                        }
                        ContentBlockParam::RedactedThinking { data } => {
                            all_thinking_blocks.push(conversation_message::Thinking {
                                text: String::new(),
                                signature: String::new(),
                                redacted_thinking: data,
                            });
                        }
                        _ => {}
                    }
                }

                (text_parts.join(NEWLINE), vec![], all_thinking_blocks, next)
            }
            _ => __unreachable!(),
        };

        // 处理消息内容和相关字段
        let (final_text, web_references, use_web, external_links) = match input.role {
            Role::Assistant => {
                let (text, web_refs, has_web) = extract_web_references_info(text);
                (text, web_refs, has_web.to_opt(), vec![])
            }
            Role::User => {
                let external_links = extract_external_links(&text, &mut base_uuid);
                (text, vec![], None, external_links)
            }
            _ => __unreachable!(),
        };

        let is_user = input.role == Role::User;
        let r#type = if is_user {
            conversation_message::MessageType::Human as i32
        } else {
            conversation_message::MessageType::Ai as i32
        };

        messages.push(ConversationMessage {
            text: final_text,
            r#type,
            images,
            bubble_id: Uuid::new_v4().to_string().into(),
            server_bubble_id: if is_user { None } else { Some(Uuid::new_v4().to_string().into()) },
            tool_results: vec![],
            is_agentic: is_agentic && is_user,
            web_references,
            thinking: match thinking.len() {
                0 => None,
                1 => thinking.into_iter().next(),
                _ => Some(conversation_message::Thinking {
                    text: thinking
                        .into_iter()
                        .map(|t| t.text)
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(EMPTY_STRING),
                    signature: String::new(),
                    redacted_thinking: String::new(),
                }),
            },
            unified_mode: Some(if is_agentic {
                stream_unified_chat_request::UnifiedMode::Agent
            } else {
                stream_unified_chat_request::UnifiedMode::Chat
            } as i32),
            supported_tools: vec![],
            external_links,
            use_web,
            // is_simple_looping_message: Some(false),
        });

        if let Some(next) = next {
            messages.push(next);
        }
    }

    // 获取最后一条用户消息的URLs
    let external_links = messages
        .last_mut()
        .map(|msg| {
            msg.supported_tools = supported_tools;
            msg.external_links.clone()
        })
        .unwrap_or_default();

    Ok((instructions, messages, external_links))
}

/// 处理 base64 编码的图片
fn process_base64_image(
    media_type: MediaType,
    data: &str,
) -> Result<(bytes::Bytes, Option<image_proto::Dimension>), AdapterError> {
    let image_data = BASE64.decode(data).map_err(|_| AdapterError::Base64DecodeFailed)?;
    let format = match media_type {
        MediaType::ImagePng => image::ImageFormat::Png,
        MediaType::ImageJpeg => image::ImageFormat::Jpeg,
        MediaType::ImageGif => image::ImageFormat::Gif,
        MediaType::ImageWebp => image::ImageFormat::WebP,
    };

    // 检查是否为动态 GIF
    if format == image::ImageFormat::Gif
        && let Ok(frames) = gif::DecodeOptions::new().read_info(std::io::Cursor::new(&image_data))
        && frames.into_iter().nth(1).is_some()
    {
        return Err(AdapterError::UnsupportedAnimatedGif);
    }

    // 获取图片尺寸
    let dimensions = image::load_from_memory_with_format(&image_data, format)
        .ok()
        .and_then(|img| img.try_into().ok());

    Ok((image_data.into(), dimensions))
}

pub async fn encode_create_params(
    params: MessageCreateParams,
    now_with_tz: chrono::DateTime<chrono_tz::Tz>,
    model: ExtModel,
    msg_id: Uuid,
    environment_info: EnvironmentInfo,
    disable_vision: bool,
    enable_slow_pool: bool,
) -> Result<Vec<u8>, AdapterError> {
    let is_chat = params.tools.is_empty();
    let is_agentic = !is_chat;
    let supported_tools = if is_agentic { vec![ClientSideToolV2::Mcp as i32] } else { vec![] };

    let (instructions, messages, external_links) = process_message_params(
        params.messages,
        params.system,
        supported_tools.clone(),
        now_with_tz,
        !disable_vision && model.is_image,
        is_agentic,
    )
    .await?;

    let explicit_context = create_explicit_context(instructions.into());

    let long_context = AppConfig::get_long_context() || LONG_CONTEXT_MODELS.contains(&model.id);

    let message = StreamUnifiedChatRequestWithTools {
        request: Some(crate::core::aiserver::v1::stream_unified_chat_request_with_tools::Request::StreamUnifiedChatRequest(Box::new(StreamUnifiedChatRequest {
            conversation: messages.inner,
            full_conversation_headers_only: messages.headers,
            // allow_long_file_scan: Some(false),
            explicit_context,
            // can_handle_filenames_after_language_ids: Some(false),
            model_details: Some(ModelDetails {
                model_name: Some(model.id()),
                azure_state: Some(AzureState::default()),
                enable_slow_pool: enable_slow_pool.to_opt(),
                max_mode: Some(model.max),
            }),
            use_web: if model.web {
                Some(::prost::ByteStr::from_static(WEB_SEARCH_MODE))
            } else {
                None
            },
            external_links,
            should_cache: Some(true),
            current_file: Some(crate::core::aiserver::v1::CurrentFileInfo {
                contents_start_at_line: 1,
                cursor_position: Some(CursorPosition::default()),
                total_number_of_lines: 1,
                selection: Some(CursorRange {
                    start_position: Some(CursorPosition::default()),
                    end_position: Some(CursorPosition::default()),
                }),
                ..Default::default()
            }),
            // use_reference_composer_diff_prompt: Some(false),
            use_new_compression_scheme: Some(true),
            is_chat,
            conversation_id: msg_id.to_string(),
            environment_info: Some(environment_info),
            is_agentic,
            supported_tools: supported_tools.clone(),
            mcp_tools: params.tools.into_iter().map(|t| mcp_params::Tool {
                server_name: sanitize_tool_name(&t.name),
                name: t.name.into(),
                description: t.description.unwrap_or_default(),
                parameters: __unwrap!(serde_json::to_string(&t.input_schema)).into()
            }).collect(),
            use_full_inputs_context: long_context.to_opt(),
            // is_resume: Some(false),
            allow_model_fallbacks: Some(false),
            // number_of_times_shown_fallback_model_warning: Some(0),
            unified_mode: Some(if is_agentic { stream_unified_chat_request::UnifiedMode::Agent } else { stream_unified_chat_request::UnifiedMode::Chat } as i32),
            // tools_requiring_accepted_return: supported_tools,
            should_disable_tools: Some(is_chat),
            thinking_level: Some(if model.is_thinking {
                stream_unified_chat_request::ThinkingLevel::High
            } else {
                stream_unified_chat_request::ThinkingLevel::Unspecified
            } as i32),
            uses_rules: Some(false),
            // mode_uses_auto_apply: Some(false),
            unified_mode_name: Some(::prost::ByteStr::from_static(if is_chat { ASK_MODE_NAME } else { AGENT_MODE_NAME })),
        })))
    };

    encode_message_framed(&message).map_err(Into::into)
}
