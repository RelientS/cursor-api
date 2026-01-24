use std::env;
use std::path::PathBuf;

generate_static_variable!(OUT_DIR PathBuf);

fn _initialize() -> PathBuf {
  env::var_os("OUT_DIR").unwrap().into()
}
