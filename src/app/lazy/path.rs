use crate::common::utils::parse_from_env;
use manually_init::ManuallyInit;
use std::path::{Path, PathBuf};

pub fn init(current_dir: PathBuf) -> &'static Path {
    CURRENT_DIR.init(current_dir);
    DATA_DIR.init({
        let data_dir = parse_from_env("DATA_DIR", "data");
        let path = CURRENT_DIR.join(&*data_dir);
        if !path.exists() {
            std::fs::create_dir_all(&path).expect("无法创建数据目录");
        }
        path
    });
    LOGS_DIR.init({
        let logs_dir = parse_from_env("LOGS_DIR", "logs");
        let path = DATA_DIR.join(&*logs_dir);
        if !path.exists() {
            std::fs::create_dir_all(&path).expect("无法创建数据目录");
        }
        path
    });
    LOGS_FILE_PATH.init(DATA_DIR.join("logs.bin"));
    TOKENS_FILE_PATH.init(DATA_DIR.join("tokens.bin"));
    PROXIES_FILE_PATH.init(DATA_DIR.join("proxies.bin"));
    CURRENT_DIR.get()
}

pub static CURRENT_DIR: ManuallyInit<PathBuf> = ManuallyInit::new();

pub static DATA_DIR: ManuallyInit<PathBuf> = ManuallyInit::new();

pub static LOGS_DIR: ManuallyInit<PathBuf> = ManuallyInit::new();

pub static LOGS_FILE_PATH: ManuallyInit<PathBuf> = ManuallyInit::new();
pub static TOKENS_FILE_PATH: ManuallyInit<PathBuf> = ManuallyInit::new();
pub static PROXIES_FILE_PATH: ManuallyInit<PathBuf> = ManuallyInit::new();
