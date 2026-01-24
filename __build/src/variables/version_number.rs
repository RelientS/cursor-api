use std::{env, fs};

generate_static_variable!(VERSION_NUMBER u16);

fn _initialize() -> u16 {
  if unsafe { *super::is_preview::get_unchecked() } {
    if let Some(s) = env::var_os("VERSION_NUMBER") {
      s.to_str().unwrap().trim().parse().unwrap()
    } else {
      fs::read_to_string(unsafe { super::manifest_dir::get_unchecked() }.join("VERSION"))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
    }
  } else {
    0
  }
}
