use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

generate_static_variable!(BUILD_TIMESTAMP u64);

fn _initialize() -> u64 {
  if let Some(s) = env::var_os("BUILD_TIMESTAMP") {
    s.to_str().unwrap().parse().unwrap()
  } else {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
  }
}
