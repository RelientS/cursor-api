use std::env;

generate_static_variable!(CFG_FEATURE String);

fn _initialize() -> String {
  let s = env::var("CARGO_CFG_FEATURE").unwrap_or_default();
  println!("{s}");
  s
}
