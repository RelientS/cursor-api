use super::*;

pub struct BuildInfo;

impl BuildInfo {
  pub fn write_to<W: Write>(self, mut writer: W) -> io::Result<()> {
    write_generated(&mut writer)?;
    writer.write_all(b"use crate::app::model::version::{Version, ReleaseStage::*};\n\n")?;
    let version_number = version_number();
    if version_number != 0 {
      writeln!(writer, "pub const BUILD_VERSION: u32 = {version_number};")?;
    }
    let build_timestamp = build_timestamp();
    writeln!(
      writer,
      "pub const BUILD_TIMESTAMP: &'static str = {:?};",
      chrono::DateTime::from_timestamp_secs(build_timestamp as i64)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    )?;
    writeln!(
      writer,
      "/// pub const VERSION_STR: &'static str = \"{version}\";\npub const VERSION: Version = {version:?};",
      version = pkg_version()
    )?;
    let is_preview = is_preview();
    let is_debug = cfg!(debug_assertions);
    write!(
      writer,
      "pub const IS_PRERELEASE: bool = {is_preview};\npub const IS_DEBUG: bool = {is_debug};\n\n"
    )?;
    write!(
      writer,
      r#"#[cfg(unix)]
pub const BUILD_EPOCH: std::time::SystemTime =
    unsafe {{ ::core::intrinsics::transmute(({build_timestamp}i64, 0u32)) }};

#[cfg(windows)]
pub const BUILD_EPOCH: std::time::SystemTime = unsafe {{
    const INTERVALS_PER_SEC: u64 = 10_000_000;
    const INTERVALS_TO_UNIX_EPOCH: u64 = 11_644_473_600 * INTERVALS_PER_SEC;
    const TARGET_INTERVALS: u64 = INTERVALS_TO_UNIX_EPOCH + {build_timestamp} * INTERVALS_PER_SEC;

    ::core::intrinsics::transmute((
        TARGET_INTERVALS as u32,
        (TARGET_INTERVALS >> 32) as u32,
    ))
}};
"#
    )?;
    Ok(())
  }
}
