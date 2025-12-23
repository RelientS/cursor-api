#![allow(internal_features)]
#![feature(
    addr_parse_ascii,
    cold_path,
    hasher_prefixfree_extras,
    const_trait_impl,
    const_default,
    core_intrinsics,
    associated_type_defaults,
    sized_type_properties,
    str_from_raw_parts,
    trim_prefix_suffix,
    unboxed_closures,
    fn_traits
)]
#![allow(
    clippy::redundant_static_lifetimes,
    clippy::enum_variant_names,
    clippy::let_and_return
)]

#[macro_use]
extern crate cursor_api;

extern crate alloc;

mod app;
mod common;
mod core;
mod leak;
mod natural_args;

use app::{
    constant::{EMPTY_STRING, ExeName, VERSION},
    model::{AppConfig, AppState},
};
use common::utils::parse_from_env;
use natural_args::{DEFAULT_LISTEN_HOST, ENV_HOST, ENV_PORT};
use tokio::signal;

#[tokio::main]
async fn main() {
    // 设置自定义 panic hook
    #[cfg(not(debug_assertions))]
    ::std::panic::set_hook(Box::new(|info| {
        __cold_path!(); // panic 是异常路径
        // std::env::set_var("RUST_BACKTRACE", "1");
        if let Some(msg) = info.payload().downcast_ref::<String>() {
            __eprint!(msg);
            __eprintln!();
        } else if let Some(msg) = info.payload().downcast_ref::<&str>() {
            __eprint!(msg);
            __eprintln!();
        }
    }));

    // 处理自然语言参数
    {
        let current_exe = __unwrap_panic!(std::env::current_exe());
        let file_name = __unwrap_panic!(
            current_exe
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("filename")
        );
        let current_dir = app::lazy::init_paths(
            __unwrap_panic!(current_exe.parent().ok_or("parent")).to_path_buf(),
        );
        unsafe {
            app::constant::IS_TERMINAL = std::io::IsTerminal::is_terminal(&std::io::stdout());
        }
        let expect = current_dir.join(ExeName::EXE_NAME).is_file();

        if file_name != ExeName::EXE_NAME {
            if expect {
                println!(
                    "Oh, I see you already have a {} sitting there. Multiple versions? How adventurous of you!",
                    ExeName::YELLOW
                )
            } else {
                println!(
                    "{file_name}? Really? {} was literally right there!",
                    ExeName::BRIGHT_RED
                );
            };
        }

        natural_args::process_args(file_name);
    }

    // tracing_subscriber::fmt::init();
    {
        use app::{
            constant::MIN_COMPAT_VERSION,
            lazy::{AUTH_TOKEN, DATA_DIR},
            model::Version,
        };
        AUTH_TOKEN.init(parse_from_env("AUTH_TOKEN", EMPTY_STRING));
        if AUTH_TOKEN.is_empty() {
            __cold_path!();
            __eprintln!("AUTH_TOKEN must be set\n");
            std::process::exit(1);
        };

        let path = DATA_DIR.join("version.bin");
        if let Ok(mut f) = std::fs::File::open(&path) {
            if let Ok(ver) = Version::read_from(&mut f)
                && ver < MIN_COMPAT_VERSION
            {
                eprintln!(
                    "数据兼容版本不匹配，目标需要: v{ver}，当前需要: v{MIN_COMPAT_VERSION}\n"
                );
                std::process::exit(1);
            }
        } else {
            println!("数据兼容版本标识不存在，当前需要: v{MIN_COMPAT_VERSION}");
            if let Ok(mut f) = std::fs::File::create(&path) {
                if let Err(e) = MIN_COMPAT_VERSION.write_to(&mut f) {
                    eprintln!("{e}");
                }
            }
        }
    }

    // 初始化全局配置
    AppConfig::init();

    // 初始化应用状态
    let state = alloc::sync::Arc::new(__unwrap_panic!(AppState::load().await));

    // 尝试加载保存的配置
    // if let Err(e) = AppConfig::load() {
    //     __cold_path!();
    //     eprintln!("加载保存的配置失败: {e}");
    // }

    // 初始化NTP
    let ntp = tokio::spawn(common::model::ntp::init_sync());

    // 创建一个克隆用于后台任务
    let state_for_reload = state.clone();

    // 启动后台任务在每个整1000秒时更新 checksum
    tokio::spawn(async move {
        use crate::app::model::timestamp_header;
        let state = state_for_reload;
        let mut counter = 29u8;

        loop {
            let now = common::utils::now_secs();
            let current_kilo = now / 1000;

            // 更新为当前千秒
            timestamp_header::update_global_with(current_kilo);

            // 等待到下一个千秒
            let wait_duration = (current_kilo + 1) * 1000 - now;
            ::tokio::time::sleep(::core::time::Duration::from_secs(wait_duration)).await;

            // 每30次循环才更新一次client_key
            counter += 1;
            if counter >= 30 {
                state.update_client_key().await;
                counter = 0;
            }
        }
    });

    // 创建一个克隆用于信号处理
    let state_for_shutdown = state.clone();

    // 设置关闭信号处理
    let shutdown_signal = async move {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        {
            let terminate = async {
                let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("failed to install signal handler");
                sigterm.recv().await;
            };

            tokio::select! {
                _ = ctrl_c => {},
                _ = terminate => {},
            }
        }

        #[cfg(windows)]
        {
            let ctrl_break = async {
                let mut signal =
                    signal::windows::ctrl_break().expect("failed to install Ctrl+Break handler");
                signal.recv().await;
            };

            let ctrl_close = async {
                let mut signal =
                    signal::windows::ctrl_close().expect("failed to install Ctrl+Close handler");
                signal.recv().await;
            };

            tokio::select! {
                _ = ctrl_c => {},
                _ = ctrl_break => {},
                _ = ctrl_close => {},
            }
        }

        #[cfg(not(any(unix, windows)))]
        ctrl_c.await;

        __println!("正在关闭服务器...");

        // 保存配置
        // if let Err(e) = AppConfig::save() {
        //     __cold_path!(); // 配置保存失败是错误路径
        //     eprintln!("保存配置失败: {e}");
        // } else {
        //     __println!("配置已保存");
        // }

        // 保存状态
        if let Err(e) = state_for_shutdown.save().await {
            __cold_path!(); // 状态保存失败是错误路径
            eprintln!("保存状态失败: {e}");
        } else {
            __println!("状态已保存");
        }

        app::lazy::log::flush_all_debug_logs().await;
    };

    // 设置路由
    let make_service = app::route::create_router(state);

    // 启动服务器
    let listener = {
        use std::net::{IpAddr, Ipv4Addr, SocketAddr};
        let port = {
            std::env::var(ENV_PORT)
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(3000)
        };
        let addr = SocketAddr::new(
            IpAddr::parse_ascii(parse_from_env(ENV_HOST, DEFAULT_LISTEN_HOST).as_bytes())
                .unwrap_or_else(|e| {
                    __cold_path!(); // IP解析失败是错误路径
                    eprintln!("无法解析IP: {e}");
                    IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))
                }),
            port,
        );
        println!("服务器运行在 {addr}");
        tokio::net::TcpListener::bind(addr)
            .await
            .unwrap_or_else(|e| {
                __cold_path!();
                eprintln!("无法绑定到地址 {addr}: {e}");
                std::process::exit(1);
            })
    };

    print!("时间同步中...");
    let _ = std::io::Write::flush(&mut std::io::stdout().lock());
    if let Err(e) = ntp.await {
        eprintln!("{e}");
        return;
    }

    crate::app::lazy::init_start_time();
    crate::app::model::DefaultInstructions::init();
    println!("当前版本: v{VERSION}");
    #[cfg(feature = "__preview")]
    {
        __println!("当前是测试版，有问题及时反馈哦~");
    }
    common::time::print_project_age();
    common::time::print_build_age();

    let server = axum::serve(listener, make_service);
    tokio::select! {
        result = server => {
            if let Err(e) = result {
                __cold_path!(); // 服务器错误是异常路径
                eprintln!("服务器错误: {e}");
            }
        }
        _ = shutdown_signal => {
            println!(
                "运行时间: {}",
                common::utils::duration_fmt::human(__unwrap!(
                    app::model::DateTime::naive_now()
                        .signed_duration_since(app::lazy::START_TIME.naive())
                        .to_std()
                ))
                .format(parse_from_env("DURATION_FORMAT", common::utils::duration_fmt::DurationFormat::Random))
                .language(parse_from_env("DURATION_LANGUAGE", common::utils::duration_fmt::Language::Random))
            );
            common::time::print_project_age();
            common::time::print_build_age();
            __println!("服务器已关闭");
        }
    }
}
