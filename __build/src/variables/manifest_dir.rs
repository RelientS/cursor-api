use std::env;
use std::path::PathBuf;

generate_static_variable!(CARGO_MANIFEST_DIR PathBuf);

fn _initialize() -> PathBuf {
  env::var_os("CARGO_MANIFEST_DIR").unwrap().into()
}
