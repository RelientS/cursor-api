use std::env;

use crate::version::ReleaseStage::*;
use crate::version::Version;

generate_static_variable!(PKG_VERSION Version);

fn _initialize() -> Version {
  let mut ver: Version = env::var("CARGO_PKG_VERSION").unwrap().parse().unwrap();
  if let Preview { ref mut build, .. } = ver.stage {
    *build = Some(*unsafe { super::version_number::get_unchecked() });
  }
  ver
}
