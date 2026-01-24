use super::*;
use std::fs;

#[allow(non_camel_case_types)]
#[derive(Clone, Copy)]
pub enum PlatformType {
  Windows,
  macOS,
  Linux,
  Android,
  FreeBSD,
  Unknown,
}

impl PlatformType {
  #[inline]
  pub const fn as_str(self) -> &'static str {
    match self {
      PlatformType::Windows => "Windows",
      PlatformType::macOS => "macOS",
      PlatformType::Linux => "Linux",
      PlatformType::Android => "Android",
      PlatformType::FreeBSD => "FreeBSD",
      PlatformType::Unknown => "Unknown",
    }
  }
  #[inline]
  pub const fn or_default(self) -> Self {
    match self {
      PlatformType::Windows | PlatformType::macOS | PlatformType::Linux => self,
      _ => PlatformType::Windows,
    }
  }
}

pub const CURRENT: PlatformType = cfg_select! {
  target_os = "windows" => {PlatformType::Windows}
  target_os = "macos" => {PlatformType::macOS}
  target_os = "linux" => {PlatformType::Linux}
  target_os = "android" => {PlatformType::Android}
  target_os = "freebsd" => {PlatformType::FreeBSD}
  _ => {PlatformType::Unknown}
};

pub struct PlatformInfo;

impl PlatformInfo {
  pub fn write_to<W: Write>(self, mut writer: W) -> io::Result<()> {
    write_generated(&mut writer)?;
    writer
      .write_all(b"use crate::app::model::platform::PlatformType;\n\n")?;
    let default = CURRENT.or_default();
    writeln!(writer, "pub const DEFAULT: PlatformType = PlatformType::{};", default.as_str())?;
    writeln!(
      writer,
      "pub const CONFIG_EXAMPLE: &'static str = {:?};",
      fs::read_to_string(manifest_dir().join("config.example.toml"))
        .unwrap()
        .replace("{DEFAULT_PLATFORM}", default.as_str())
    )?;
    Ok(())
  }
}
