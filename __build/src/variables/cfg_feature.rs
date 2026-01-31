use std::env;

generate_static_variable!(CFG_FEATURE String);

fn _initialize() -> String {
  env::var("CARGO_CFG_FEATURE").unwrap_or_default()
}
