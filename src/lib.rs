use std::io::Read as _;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use flate2::read::GzDecoder;
use image::guess_format;
use prost::Message as _;
use rand::{thread_rng, Rng};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub mod aiserver;
use aiserver::proto::*;

pub mod message;
use message::*;

// pub mod decoder;
// use decoder::*;

pub mod app;
use app::*;

const LONG_CONTEXT_MODELS: [&str; 4] = [
    "gpt-4o-128k",
    "gemini-1.5-flash-500k",
    "claude-3-haiku-200k",
    "claude-3-5-sonnet-200k",
];

async fn process_chat_inputs(inputs: Vec<Message>) -> (String, Vec<ConversationMessage>) {
    // 收集 system 和 developer 指令
    let instructions = inputs
        .iter()
        .filter(|input| input.role == "system" || input.role == "developer")
        .map(|input| match &input.content {
            MessageContent::Text(text) => text.clone(),
            MessageContent::Vision(contents) => contents
                .iter()
                .filter_map(|content| {
                    if content.content_type == "text" {
                        content.text.clone()
                    } else {
                        None
                    }
                })
                .collect::<Vec<String>>()
                .join("\n"),
        })
        .collect::<Vec<String>>()
        .join("\n\n");

    // 使用默认指令或收集到的指令
    let instructions = if instructions.is_empty() {
        "Respond in Chinese by default".to_string()
    } else {
        instructions
    };

    // 过滤出 user 和 assistant 对话
    let mut chat_inputs: Vec<Message> = inputs
        .into_iter()
        .filter(|input| input.role == "user" || input.role == "assistant")
        .collect();

    // 处理空对话情况
    if chat_inputs.is_empty() {
        return (
            instructions,
            vec![ConversationMessage {
                text: " ".to_string(),
                r#type: conversation_message::MessageType::Human as i32,
                attached_code_chunks: vec![],
                codebase_context_chunks: vec![],
                commits: vec![],
                pull_requests: vec![],
                git_diffs: vec![],
                assistant_suggested_diffs: vec![],
                interpreter_results: vec![],
                images: vec![],
                attached_folders: vec![],
                approximate_lint_errors: vec![],
                bubble_id: Uuid::new_v4().to_string(),
                server_bubble_id: None,
                attached_folders_new: vec![],
                lints: vec![],
                user_responses_to_suggested_code_blocks: vec![],
                relevant_files: vec![],
                tool_results: vec![],
                notepads: vec![],
                is_capability_iteration: Some(false),
                capabilities: vec![],
                edit_trail_contexts: vec![],
                suggested_code_blocks: vec![],
                diffs_for_compressing_files: vec![],
                multi_file_linter_errors: vec![],
                diff_histories: vec![],
                recently_viewed_files: vec![],
                recent_locations_history: vec![],
                is_agentic: false,
                file_diff_trajectories: vec![],
                conversation_summary: None,
            }],
        );
    }

    // 如果第一条是 assistant，插入空的 user 消息
    if chat_inputs
        .first()
        .map_or(false, |input| input.role == "assistant")
    {
        chat_inputs.insert(
            0,
            Message {
                role: "user".to_string(),
                content: MessageContent::Text(" ".to_string()),
            },
        );
    }

    // 处理连续相同角色的情况
    let mut i = 1;
    while i < chat_inputs.len() {
        if chat_inputs[i].role == chat_inputs[i - 1].role {
            let insert_role = if chat_inputs[i].role == "user" {
                "assistant"
            } else {
                "user"
            };
            chat_inputs.insert(
                i,
                Message {
                    role: insert_role.to_string(),
                    content: MessageContent::Text(" ".to_string()),
                },
            );
        }
        i += 1;
    }

    // 确保最后一条是 user
    if chat_inputs
        .last()
        .map_or(false, |input| input.role == "assistant")
    {
        chat_inputs.push(Message {
            role: "user".to_string(),
            content: MessageContent::Text(" ".to_string()),
        });
    }

    // 转换为 proto messages
    let mut messages = Vec::new();
    for input in chat_inputs {
        let (text, images) = match input.content {
            MessageContent::Text(text) => (text, vec![]),
            MessageContent::Vision(contents) => {
                let mut text_parts = Vec::new();
                let mut images = Vec::new();

                for content in contents {
                    match content.content_type.as_str() {
                        "text" => {
                            if let Some(text) = content.text {
                                text_parts.push(text);
                            }
                        }
                        "image_url" => {
                            if let Some(image_url) = &content.image_url {
                                let url = image_url.url.clone();
                                let result =
                                    tokio::spawn(async move { fetch_image_data(&url).await });
                                if let Ok(Ok((image_data, dimensions))) = result.await {
                                    images.push(ImageProto {
                                        data: image_data,
                                        dimension: dimensions,
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
                (text_parts.join("\n"), images)
            }
        };

        messages.push(ConversationMessage {
            text,
            r#type: if input.role == "user" {
                conversation_message::MessageType::Human as i32
            } else {
                conversation_message::MessageType::Ai as i32
            },
            attached_code_chunks: vec![],
            codebase_context_chunks: vec![],
            commits: vec![],
            pull_requests: vec![],
            git_diffs: vec![],
            assistant_suggested_diffs: vec![],
            interpreter_results: vec![],
            images,
            attached_folders: vec![],
            approximate_lint_errors: vec![],
            bubble_id: Uuid::new_v4().to_string(),
            server_bubble_id: None,
            attached_folders_new: vec![],
            lints: vec![],
            user_responses_to_suggested_code_blocks: vec![],
            relevant_files: vec![],
            tool_results: vec![],
            notepads: vec![],
            is_capability_iteration: Some(false),
            capabilities: vec![],
            edit_trail_contexts: vec![],
            suggested_code_blocks: vec![],
            diffs_for_compressing_files: vec![],
            multi_file_linter_errors: vec![],
            diff_histories: vec![],
            recently_viewed_files: vec![],
            recent_locations_history: vec![],
            is_agentic: false,
            file_diff_trajectories: vec![],
            conversation_summary: None,
        });
    }

    (instructions, messages)
}

async fn fetch_image_data(
    url: &str,
) -> Result<(Vec<u8>, Option<image_proto::Dimension>), Box<dyn std::error::Error + Send + Sync>> {
    // 在进入异步操作前获取并释放锁
    let vision_ability = {
        let config = APP_CONFIG.read().unwrap();
        config.vision_ability.clone()
    };

    match vision_ability.as_str() {
        "none" | "disabled" => Err("图片功能已禁用".into()),

        "base64" | "base64-only" => {
            if !url.starts_with("data:image/") {
                return Err("仅支持 base64 编码的图片".into());
            }
            process_base64_image(url)
        }

        "base64-http" | "all" => {
            if url.starts_with("data:image/") {
                process_base64_image(url)
            } else {
                process_http_image(url).await
            }
        }

        _ => Err("无效的 VISION_ABILITY 配置".into()),
    }
}

// 处理 base64 编码的图片
fn process_base64_image(
    url: &str,
) -> Result<(Vec<u8>, Option<image_proto::Dimension>), Box<dyn std::error::Error + Send + Sync>> {
    let parts: Vec<&str> = url.split("base64,").collect();
    if parts.len() != 2 {
        return Err("无效的 base64 图片格式".into());
    }

    // 检查图片格式
    let format = parts[0].to_lowercase();
    if !format.contains("png")
        && !format.contains("jpeg")
        && !format.contains("jpg")
        && !format.contains("webp")
        && !format.contains("gif")
    {
        return Err("不支持的图片格式，仅支持 PNG、JPEG、WEBP 和非动态 GIF".into());
    }

    let image_data = BASE64.decode(parts[1])?;

    // 检查是否为动态 GIF
    if format.contains("gif") {
        if let Ok(frames) = gif::DecodeOptions::new().read_info(std::io::Cursor::new(&image_data)) {
            if frames.into_iter().count() > 1 {
                return Err("不支持动态 GIF".into());
            }
        }
    }

    // 获取图片尺寸
    let dimensions = if let Ok(img) = image::load_from_memory(&image_data) {
        Some(image_proto::Dimension {
            width: img.width() as i32,
            height: img.height() as i32,
        })
    } else {
        None
    };

    Ok((image_data, dimensions))
}

// 处理 HTTP 图片 URL
async fn process_http_image(
    url: &str,
) -> Result<(Vec<u8>, Option<image_proto::Dimension>), Box<dyn std::error::Error + Send + Sync>> {
    let response = reqwest::get(url).await?;
    let image_data = response.bytes().await?.to_vec();
    let format = guess_format(&image_data)?;

    // 检查图片格式
    match format {
        image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP => {
            // 这些格式都支持
        }
        image::ImageFormat::Gif => {
            if let Ok(frames) =
                gif::DecodeOptions::new().read_info(std::io::Cursor::new(&image_data))
            {
                if frames.into_iter().count() > 1 {
                    return Err("不支持动态 GIF".into());
                }
            }
        }
        _ => return Err("不支持的图片格式，仅支持 PNG、JPEG、WEBP 和非动态 GIF".into()),
    }

    // 获取图片尺寸
    let dimensions = if let Ok(img) = image::load_from_memory_with_format(&image_data, format) {
        Some(image_proto::Dimension {
            width: img.width() as i32,
            height: img.height() as i32,
        })
    } else {
        None
    };

    Ok((image_data, dimensions))
}

pub async fn encode_chat_message(
    inputs: Vec<Message>,
    model_name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // 在进入异步操作前获取并释放锁
    let enable_slow_pool = {
        let config = APP_CONFIG.read().unwrap();
        config.enable_slow_pool
    };

    let (instructions, messages) = process_chat_inputs(inputs).await;

    let explicit_context = if !instructions.trim().is_empty() {
        Some(ExplicitContext {
            context: instructions,
            repo_context: None,
        })
    } else {
        None
    };

    let chat = GetChatRequest {
        current_file: None,
        conversation: messages,
        repositories: vec![],
        explicit_context,
        workspace_root_path: None,
        code_blocks: vec![],
        model_details: Some(ModelDetails {
            model_name: Some(model_name.to_string()),
            api_key: None,
            enable_ghost_mode: None,
            azure_state: None,
            enable_slow_pool,
            openai_api_base_url: None,
        }),
        documentation_identifiers: vec![],
        request_id: Uuid::new_v4().to_string(),
        linter_errors: None,
        summary: None,
        summary_up_until_index: None,
        allow_long_file_scan: None,
        is_bash: None,
        conversation_id: Uuid::new_v4().to_string(),
        can_handle_filenames_after_language_ids: None,
        use_web: None,
        quotes: vec![],
        debug_info: None,
        workspace_id: None,
        external_links: vec![],
        commit_notes: vec![],
        long_context_mode: if LONG_CONTEXT_MODELS.contains(&model_name) {
            Some(true)
        } else {
            None
        },
        is_eval: None,
        desired_max_tokens: None,
        context_ast: None,
        is_composer: None,
        runnable_code_blocks: None,
        should_cache: None,
    };

    let mut encoded = Vec::new();
    chat.encode(&mut encoded)?;

    let len_prefix = format!("{:010x}", encoded.len()).to_uppercase();
    let content = hex::encode_upper(&encoded);

    Ok(hex::decode(len_prefix + &content)?)
}

pub async fn decode_response(
    data: &[u8],
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    match decode_proto_messages(data) {
        Ok(decoded) if !decoded.is_empty() => Ok(decoded),
        _ => decompress_response(data).await,
    }
}

fn decode_proto_messages(data: &[u8]) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let hex_str = hex::encode(data);
    let mut pos = 0;
    let mut messages = Vec::new();

    while pos + 10 <= hex_str.len() {
        let msg_len = i64::from_str_radix(&hex_str[pos..pos + 10], 16)?;
        pos += 10;

        if pos + (msg_len * 2) as usize > hex_str.len() {
            break;
        }

        let msg_data = &hex_str[pos..pos + (msg_len * 2) as usize];
        pos += (msg_len * 2) as usize;

        let buffer = hex::decode(msg_data)?;
        let response = StreamChatResponse::decode(&buffer[..])?;
        messages.push(response.text);
    }

    Ok(messages.join(""))
}

async fn decompress_response(
    data: &[u8],
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if data.len() <= 5 {
        return Ok(String::new());
    }

    let mut decoder = GzDecoder::new(&data[5..]);
    let mut text = String::new();

    match decoder.read_to_string(&mut text) {
        Ok(_) => {
            if !text.contains("<|BEGIN_SYSTEM|>") {
                Ok(text)
            } else {
                Ok(String::new())
            }
        }
        Err(e) => Err(Box::new(e)),
    }
}

pub fn generate_random_id(
    size: usize,
    dict_type: Option<&str>,
    custom_chars: Option<&str>,
) -> String {
    let charset = match (dict_type, custom_chars) {
        (_, Some(chars)) => chars,
        (Some("alphabet"), _) => "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
        (Some("max"), _) => "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ_-",
        _ => "0123456789",
    };

    let mut rng = thread_rng();
    (0..size)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset.chars().nth(idx).unwrap()
        })
        .collect()
}

pub fn generate_hash() -> String {
    let random_bytes = rand::thread_rng().gen::<[u8; 32]>();
    let mut hasher = Sha256::new();
    hasher.update(random_bytes);
    hex::encode(hasher.finalize())
}

fn obfuscate_bytes(bytes: &mut [u8]) {
    let mut prev: u8 = 165;
    for (idx, byte) in bytes.iter_mut().enumerate() {
        let old_value = *byte;
        *byte = (old_value ^ prev).wrapping_add((idx % 256) as u8);
        prev = *byte;
    }
}

pub fn generate_checksum(device_id: &str, mac_addr: Option<&str>) -> String {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        / 1_000_000;

    let mut timestamp_bytes = vec![
        ((timestamp >> 40) & 255) as u8,
        ((timestamp >> 32) & 255) as u8,
        ((timestamp >> 24) & 255) as u8,
        ((timestamp >> 16) & 255) as u8,
        ((timestamp >> 8) & 255) as u8,
        (255 & timestamp) as u8,
    ];

    obfuscate_bytes(&mut timestamp_bytes);
    let encoded = BASE64.encode(&timestamp_bytes);

    match mac_addr {
        Some(mac) => format!("{}{}/{}", encoded, device_id, mac),
        None => format!("{}{}", encoded, device_id),
    }
}
