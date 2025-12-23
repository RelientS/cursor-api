#[cfg(not(feature = "use-minified"))]
use sha2::{Digest, Sha256};
#[cfg(not(feature = "use-minified"))]
use std::collections::HashMap;
#[cfg(not(feature = "use-minified"))]
use std::fs;
#[cfg(not(debug_assertions))]
#[cfg(feature = "__preview")]
use std::fs::File;
use std::io::Result;
#[cfg(not(debug_assertions))]
#[cfg(feature = "__preview")]
use std::io::{Read, Write};
#[cfg(not(feature = "use-minified"))]
use std::path::Path;
#[cfg(not(feature = "use-minified"))]
use std::path::PathBuf;
#[cfg(not(feature = "use-minified"))]
use std::process::Command;

// 支持的文件类型
// #[cfg(not(feature = "use-minified"))]
// const SUPPORTED_EXTENSIONS: [&str; 4] = ["html", "js", "css", "md"];

// 需要处理的 Markdown 文件列表
#[cfg(not(feature = "use-minified"))]
const MARKDOWN_FILES: [&str; 2] = ["README.md", "LICENSE.md"];

#[cfg(not(feature = "use-minified"))]
fn check_and_install_deps() -> Result<()> {
    let scripts_dir = Path::new("scripts");
    let node_modules = scripts_dir.join("node_modules");

    if !node_modules.exists() {
        println!("cargo:warning=Installing minifier dependencies...");
        let status = Command::new("npm").current_dir(scripts_dir).arg("install").status()?;

        if !status.success() {
            panic!("Failed to install npm dependencies");
        }
        println!("cargo:warning=Dependencies installed successfully");
    }
    Ok(())
}

#[cfg(not(feature = "use-minified"))]
fn get_files_hash() -> Result<HashMap<PathBuf, String>> {
    let mut file_hashes = HashMap::new();
    // let static_dir = Path::new("static");

    pub const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    #[inline]
    pub fn to_str<'buf>(bytes: &[u8], buf: &'buf mut [u8]) -> &'buf mut str {
        for (i, &byte) in bytes.iter().enumerate() {
            buf[i * 2] = HEX_CHARS[(byte >> 4) as usize];
            buf[i * 2 + 1] = HEX_CHARS[(byte & 0x0f) as usize];
        }

        // SAFETY: 输出都是有效的 ASCII 字符
        unsafe { core::str::from_utf8_unchecked_mut(buf) }
    }

    // 处理根目录的 Markdown 文件
    for md_file in MARKDOWN_FILES {
        let md_path = Path::new(md_file);
        if md_path.exists() {
            let content = fs::read(md_path)?;
            #[allow(deprecated)]
            let hash =
                to_str(Sha256::new().chain_update(&content).finalize().as_slice(), &mut [0; 64])
                    .to_string();
            file_hashes.insert(md_path.to_path_buf(), hash);
        }
    }

    // 处理 static 目录中的文件
    // if static_dir.exists() {
    //     for entry in fs::read_dir(static_dir)? {
    //         let entry = entry?;
    //         let path = entry.path();

    //         // 检查是否是支持的文件类型，且不是已经压缩的文件
    //         if let Some(ext) = path.extension().and_then(|e| e.to_str())
    //             && SUPPORTED_EXTENSIONS.contains(&ext)
    //             && !path.to_string_lossy().contains(".min.")
    //         {
    //             let content = fs::read(&path)?;
    //             #[allow(deprecated)]
    //             let hash =
    //                 to_str(Sha256::new().chain_update(&content).finalize().as_slice(), &mut [0; 64])
    //                     .to_string();
    //             file_hashes.insert(path, hash);
    //         }
    //     }
    // }

    Ok(file_hashes)
}

#[cfg(not(feature = "use-minified"))]
fn load_saved_hashes() -> Result<HashMap<PathBuf, String>> {
    let hash_file = Path::new("scripts/.asset-hashes.json");
    if hash_file.exists() {
        let content = fs::read_to_string(hash_file)?;
        let hash_map: HashMap<String, String> = serde_json::from_str(&content)?;
        Ok(hash_map.into_iter().map(|(k, v)| (PathBuf::from(k), v)).collect())
    } else {
        Ok(HashMap::new())
    }
}

#[cfg(not(feature = "use-minified"))]
fn save_hashes(hashes: &HashMap<PathBuf, String>) -> Result<()> {
    let hash_file = Path::new("scripts/.asset-hashes.json");
    let string_map: HashMap<String, String> =
        hashes.iter().map(|(k, v)| (k.to_string_lossy().into_owned(), v.clone())).collect();
    let content = serde_json::to_string_pretty(&string_map)?;
    fs::write(hash_file, content)?;
    Ok(())
}

#[cfg(not(feature = "use-minified"))]
fn get_minified_output_path(path: &Path) -> PathBuf {
    let file_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");

    // 检查是否是根目录的 Markdown 文件
    if MARKDOWN_FILES.contains(&file_name) {
        // 将文件名转换为小写并生成对应的 .min.html 文件
        let base_name = path.file_stem().unwrap().to_string_lossy().to_lowercase();
        PathBuf::from(format!("static/{}.min.html", base_name))
    } else {
        // 其他文件保持原有逻辑
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        path.with_file_name(format!("{}.min.{}", path.file_stem().unwrap().to_string_lossy(), ext))
    }
}

#[cfg(not(feature = "use-minified"))]
fn minify_assets() -> Result<()> {
    // 获取现有文件的哈希
    let current_hashes = get_files_hash()?;

    if current_hashes.is_empty() {
        println!("cargo:warning=No files to minify");
        return Ok(());
    }

    // 加载保存的哈希值
    let saved_hashes = load_saved_hashes()?;

    // 找出需要更新的文件
    let files_to_update: Vec<_> = current_hashes
        .iter()
        .filter(|(path, current_hash)| {
            // 获取压缩后的输出路径
            let min_path = get_minified_output_path(path);

            // 检查压缩/转换后的文件是否存在
            if !min_path.exists() {
                return true;
            }

            // 检查原始文件是否发生变化
            saved_hashes.get(*path) != Some(*current_hash)
        })
        .map(|(path, _)| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    if files_to_update.is_empty() {
        println!("cargo:warning=No files need to be updated");
        return Ok(());
    }

    println!("cargo:warning=Minifying {} files...", files_to_update.len());
    println!("cargo:warning=Files: {}", files_to_update.join(" "));

    // 运行压缩脚本
    let status = Command::new("node").arg("scripts/minify.js").args(&files_to_update).status()?;

    if !status.success() {
        panic!("Asset minification failed");
    }

    // 保存新的哈希值
    save_hashes(&current_hashes)?;

    Ok(())
}

include!("build_info.rs");

#[cfg(feature = "__protoc")]
macro_rules! proto_attributes {
    (config: $config:expr, paths: $paths:expr, attributes: [$($attr:expr),* $(,)?]) => {
        for path in $paths {
            $(
                $config.type_attribute(path, $attr);
            )*
        }
    };
}

fn main() -> Result<()> {
    // 更新版本号 - 只在 release 构建时执行
    #[cfg(all(not(debug_assertions), feature = "__preview"))]
    update_version()?;

    #[cfg(feature = "__protoc")]
    {
        // Proto 文件处理
        println!("cargo:rerun-if-changed=src/core/aiserver/v1/lite.proto");
        println!("cargo:rerun-if-changed=src/core/config/key.proto");

        // 获取环境变量 PROTOC 并创建配置
        let mut config = prost_build::Config::new();

        // 检查环境变量是否设置
        match std::env::var_os("PROTOC") {
            Some(path) => {
                // 有环境变量时直接配置
                config.protoc_executable(PathBuf::from(path));
            }
            None => {
                // 无环境变量时输出警告，使用默认 protoc
                println!(
                    "cargo:warning=PROTOC environment variable not set, using default protoc."
                );
                // 这里不需要额外操作，prost-build 会自动使用默认的 protoc
            }
        }

        // config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");
        // config.enum_attribute(".aiserver.v1", "#[allow(clippy::enum_variant_names)]");
        for p in [
            ".aiserver.v1.CppSessionEvent.event.git_context_event",
            ".aiserver.v1.CppTimelineEvent.v.event",
            ".aiserver.v1.StreamAiLintBugResponse.response.bug",
            ".aiserver.v1.StreamChatToolformerResponse.response_type.tool_action",
            ".aiserver.v1.TaskStreamLogResponse.response.streamed_log_item",
            ".aiserver.v1.StreamUnifiedChatRequestWithTools.request.stream_unified_chat_request",
            ".aiserver.v1.StreamUnifiedChatRequestWithTools.request.client_side_tool_v2_result",
            ".aiserver.v1.StreamUnifiedChatResponseWithTools.response.client_side_tool_v2_call",
            ".aiserver.v1.StreamUnifiedChatResponseWithTools.response.stream_unified_chat_response",
        ] {
            config.boxed(p);
        }

        proto_attributes! {
            config: config,
            paths: [
                ".aiserver.v1.CursorPosition",
                ".aiserver.v1.SimplestRange",
                ".aiserver.v1.SimpleRange",
                ".aiserver.v1.LineRange",
                ".aiserver.v1.CursorRange",
                ".aiserver.v1.Diagnostic",
                ".aiserver.v1.BM25Chunk",
                ".aiserver.v1.CurrentFileInfo",
                ".aiserver.v1.DataframeInfo",
                ".aiserver.v1.LinterError",
                ".aiserver.v1.LinterErrors",
                ".aiserver.v1.LspSubgraphPosition",
                ".aiserver.v1.LspSubgraphRange",
                ".aiserver.v1.LspSubgraphContextItem",
                ".aiserver.v1.LspSubgraphFullContext",
                ".aiserver.v1.FSUploadFileRequest",
                ".aiserver.v1.FilesyncUpdateWithModelVersion",
                ".aiserver.v1.SingleUpdateRequest",
                ".aiserver.v1.FSSyncFileRequest",
                ".aiserver.v1.CppIntentInfo",
                ".aiserver.v1.LspSuggestion",
                ".aiserver.v1.LspSuggestedItems",
                ".aiserver.v1.StreamCppRequest",
                ".aiserver.v1.CppConfigRequest",
                ".aiserver.v1.AdditionalFile",
                ".aiserver.v1.AvailableCppModelsRequest",
                ".aiserver.v1.CppFileDiffHistory",
                ".aiserver.v1.CppContextItem",
                ".aiserver.v1.CppParameterHint",
                ".aiserver.v1.IRange",
                ".aiserver.v1.BlockDiffPatch",
                ".aiserver.v1.AvailableModelsRequest",
                ".aiserver.v1.GetFilteredUsageEventsResponse",
                ".aiserver.v1.GetAggregatedUsageEventsResponse",
            ],
            attributes: [
                "#[derive(::serde::Deserialize)]",
            ]
        }

        proto_attributes! {
            config: config,
            paths: &[
                ".aiserver.v1.LineRange",
                ".aiserver.v1.FSUploadErrorType",
                ".aiserver.v1.FSSyncErrorType",
                ".aiserver.v1.FSUploadFileResponse",
                ".aiserver.v1.FSSyncFileResponse",
                ".aiserver.v1.StreamCppResponse",
                ".aiserver.v1.CppConfigResponse",
                ".aiserver.v1.AvailableCppModelsResponse",
                ".aiserver.v1.AvailableModelsResponse",
                ".aiserver.v1.CustomErrorDetails",
                ".aiserver.v1.GetFilteredUsageEventsRequest",
                ".aiserver.v1.GetAggregatedUsageEventsRequest",
            ],
            attributes: [
                "#[derive(::serde::Serialize)]",
            ]
        }

        config
            .compile_protos(&["src/core/aiserver/v1/lite.proto"], &["src/core/aiserver/v1/"])
            .unwrap();
        // config.compile_protos(&["src/core/config/key.proto"], &["src/core/config/"]).unwrap();
    }

    // 静态资源文件处理
    println!("cargo:rerun-if-changed=scripts/minify.js");
    // println!("cargo:rerun-if-changed=scripts/package.json");
    // println!("cargo:rerun-if-changed=static/api.html");
    // println!("cargo:rerun-if-changed=static/build_key.html");
    // println!("cargo:rerun-if-changed=static/config.html");
    // println!("cargo:rerun-if-changed=static/logs.html");
    // println!("cargo:rerun-if-changed=static/proxies.html");
    // println!("cargo:rerun-if-changed=static/shared-styles.css");
    // println!("cargo:rerun-if-changed=static/shared.js");
    // println!("cargo:rerun-if-changed=static/tokens.html");
    println!("cargo:rerun-if-changed=README.md");
    println!("cargo:rerun-if-changed=LICENSE.md");

    // 只在release模式下监控VERSION文件变化
    #[cfg(not(debug_assertions))]
    #[cfg(feature = "__preview")]
    println!("cargo:rerun-if-changed=VERSION");

    #[cfg(not(feature = "use-minified"))]
    {
        // 检查并安装依赖
        check_and_install_deps()?;

        // 运行资源压缩
        minify_assets()?;
    }

    // 生成构建信息文件
    generate_build_info()?;

    Ok(())
}
