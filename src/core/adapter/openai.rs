use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use uuid::Uuid;

use crate::app::constant::EMPTY_STRING;
use crate::app::model::{AppConfig, DEFAULT_INSTRUCTIONS, VisionAbility, create_explicit_context};
use crate::app::model::proxy_pool::get_general_client;
use crate::common::utils::encode_message_framed;
use crate::core::aiserver::v1::{
    AzureState, ComposerExternalLink, ConversationMessage, CursorPosition, CursorRange,
    EnvironmentInfo, ImageProto, ModelDetails, StreamUnifiedChatRequest,
    StreamUnifiedChatRequestWithTools, conversation_message, image_proto,
    stream_unified_chat_request,
};
use crate::core::constant::LONG_CONTEXT_MODELS;
use crate::core::model::{ExtModel, Role, openai};
use super::{
    ASK_MODE_NAME, AdapterError, BaseUuid, Messages, NEWLINE, ToOpt as _, WEB_SEARCH_MODE,
    extract_external_links, extract_web_references_info,
};

crate::define_typed_constants! {
    &'static str => {
        /// 支持的图片格式
        FORMAT_PNG = "png",
        FORMAT_JPEG = "jpeg",
        FORMAT_JPG = "jpg",
        FORMAT_WEBP = "webp",
        FORMAT_GIF = "gif",
        /// Data URL 前缀
        DATA_IMAGE_PREFIX = "data:image/",
        /// Base64 分隔符
        BASE64_SEPARATOR = ";base64,",
        /// 双换行符用于分隔指令
        DOUBLE_NEWLINE = "\n\n",
    }
}

async fn process_chat_inputs(
    inputs: Vec<openai::Message>,
    now_with_tz: chrono::DateTime<chrono_tz::Tz>,
    image_support: bool,
) -> Result<(String, Messages, Vec<ComposerExternalLink>), AdapterError> {
    // 分别收集 system 指令和 user/assistant 对话
    let (system_messages, chat_messages): (Vec<_>, Vec<_>) =
        inputs.into_iter().partition(|input| input.role == Role::System);

    // 收集 system 指令
    let instructions = system_messages
        .into_iter()
        .map(|input| match input.content {
            openai::MessageContent::String(text) => text,
            openai::MessageContent::Array(contents) => contents
                .into_iter()
                .filter_map(openai::MessageContentObject::into_text)
                .collect::<Vec<String>>()
                .join(NEWLINE),
        })
        .collect::<Vec<String>>()
        .join(DOUBLE_NEWLINE);

    // 使用默认指令或收集到的指令
    let instructions = if instructions.is_empty() {
        DEFAULT_INSTRUCTIONS.get().get(now_with_tz)
    } else {
        instructions
    };

    // 过滤出 user 和 assistant 对话
    let mut chat_inputs = chat_messages;

    // 处理空对话情况
    if chat_inputs.is_empty() {
        return Ok((
            instructions,
            Messages::from_single(ConversationMessage {
                r#type: conversation_message::MessageType::Human as i32,
                bubble_id: Uuid::new_v4().to_string().into(),
                unified_mode: Some(stream_unified_chat_request::UnifiedMode::Chat as i32),
                ..Default::default()
            }),
            vec![],
        ));
    }

    // 如果第一条是 assistant，插入空的 user 消息
    if chat_inputs.first().is_some_and(|input| input.role == Role::Assistant) {
        chat_inputs.insert(
            0,
            openai::Message {
                role: Role::User,
                content: openai::MessageContent::String(EMPTY_STRING.into()),
            },
        );
    }

    // 确保最后一条是 user
    // if chat_inputs.last().is_some_and(|input| input.role == Role::Assistant) {
    //     chat_inputs.push(openai::Message {
    //         role: Role::User,
    //         content: openai::MessageContent::String(EMPTY_STRING.into()),
    //     });
    // }

    // 转换为 proto messages
    let mut messages = Messages::with_capacity(chat_inputs.len());
    let mut base_uuid = BaseUuid::new();

    for input in chat_inputs {
        let (text, images) = match input.content {
            openai::MessageContent::String(text) => (text, vec![]),
            openai::MessageContent::Array(contents) if input.role == Role::User => {
                let mut text_parts = Vec::new();
                let mut images = Vec::new();

                for content in contents {
                    match content {
                        openai::MessageContentObject::Text { text } => text_parts.push(text),
                        openai::MessageContentObject::ImageUrl { image_url } => {
                            if image_support {
                                let url = image_url.url;
                                let res = {
                                    let vision_ability = AppConfig::get_vision_ability();

                                    match vision_ability {
                                        VisionAbility::None => Err(AdapterError::VisionDisabled),
                                        VisionAbility::Base64 => {
                                            if let Some(url) = url.strip_prefix(DATA_IMAGE_PREFIX) {
                                                process_base64_image(url)
                                            } else {
                                                Err(AdapterError::Base64Only)
                                            }
                                        }
                                        VisionAbility::All => {
                                            if let Some(url) = url.strip_prefix(DATA_IMAGE_PREFIX) {
                                                process_base64_image(url)
                                            } else {
                                                super::process_http_image(
                                                    &url,
                                                    get_general_client(),
                                                )
                                                .await
                                            }
                                        }
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
            openai::MessageContent::Array(contents) => {
                let mut text_parts = Vec::new();

                for content in contents {
                    if let Some(text) = content.into_text() {
                        text_parts.push(text);
                    }
                }

                (text_parts.join(NEWLINE), vec![])
            }
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

        let r#type = if input.role == Role::User {
            conversation_message::MessageType::Human as i32
        } else {
            conversation_message::MessageType::Ai as i32
        };

        messages.push(ConversationMessage {
            text: final_text,
            r#type,
            images,
            bubble_id: Uuid::new_v4().to_string().into(),
            server_bubble_id: if input.role == Role::User {
                None
            } else {
                Some(Uuid::new_v4().to_string().into())
            },
            is_agentic: false,
            // existed_subsequent_terminal_command: false,
            // existed_previous_terminal_command: false,
            web_references,
            // git_context: None,
            // cached_conversation_summary: None,
            // attached_human_changes: false,
            thinking: None,
            unified_mode: Some(stream_unified_chat_request::UnifiedMode::Chat as i32),
            external_links,
            use_web,
            ..Default::default()
        });
    }

    // 获取最后一条用户消息的URLs
    let external_links = messages.last().map(|msg| msg.external_links.clone()).unwrap_or_default();

    Ok((instructions, messages, external_links))
}

/// 处理 base64 编码的图片
fn process_base64_image(
    url: &str,
) -> Result<(bytes::Bytes, Option<image_proto::Dimension>), AdapterError> {
    let (format, data) =
        url.split_once(BASE64_SEPARATOR).ok_or(AdapterError::Base64DecodeFailed)?;

    // 检查图片格式
    let format = match format {
        FORMAT_PNG => image::ImageFormat::Png,
        FORMAT_JPG | FORMAT_JPEG => image::ImageFormat::Jpeg,
        FORMAT_GIF => image::ImageFormat::Gif,
        FORMAT_WEBP => image::ImageFormat::WebP,
        _ => return Err(AdapterError::UnsupportedImageFormat),
    };
    let image_data = BASE64.decode(data).map_err(|_| AdapterError::Base64DecodeFailed)?;

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
    inputs: Vec<openai::Message>,
    now_with_tz: chrono::DateTime<chrono_tz::Tz>,
    model: ExtModel,
    msg_id: Uuid,
    environment_info: EnvironmentInfo,
    disable_vision: bool,
    enable_slow_pool: bool,
) -> Result<Vec<u8>, AdapterError> {
    let (instructions, messages, external_links) =
        process_chat_inputs(inputs, now_with_tz, !disable_vision && model.is_image).await?;

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
                cursor_position: Some(CursorPosition { line: 0, column: 0 }),
                total_number_of_lines: 1,
                selection: Some(CursorRange {
                    start_position: Some(CursorPosition { line: 0, column: 0 }),
                    end_position: Some(CursorPosition { line: 0, column: 0 }),
                }),
                ..Default::default()
            }),
            // use_reference_composer_diff_prompt: Some(false),
            use_new_compression_scheme: Some(true),
            is_chat: false,
            conversation_id: msg_id.to_string(),
            environment_info: Some(environment_info),
            is_agentic: false,
            supported_tools: vec![],
            // use_unified_chat_prompt: false,
            mcp_tools: vec![],
            use_full_inputs_context: long_context.to_opt(),
            // is_resume: Some(false),
            allow_model_fallbacks: Some(false),
            // number_of_times_shown_fallback_model_warning: Some(0),
            // is_headless: false,
            unified_mode: Some(stream_unified_chat_request::UnifiedMode::Chat as i32),
            // tools_requiring_accepted_return: vec![],
            should_disable_tools: Some(true),
            thinking_level: Some(if model.is_thinking {
                stream_unified_chat_request::ThinkingLevel::High
            } else {
                stream_unified_chat_request::ThinkingLevel::Unspecified
            } as i32),
            // should_use_chat_prompt: None,
            uses_rules: Some(false),
            // mode_uses_auto_apply: Some(false),
            unified_mode_name: Some(::prost::ByteStr::from_static(ASK_MODE_NAME)),
        })))
    };

    encode_message_framed(&message).map_err(Into::into)
}
