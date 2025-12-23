use core::cmp::Ordering;
use std::io;

/// 版本发布阶段
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum ReleaseStage {
    /// 正式发布版本
    Release,
    /// 预览版本，格式如 `-pre.6` 或 `-pre.6+build.8`
    Preview {
        /// 预览版本号
        version: u16,
        /// 构建号（可选）
        build: Option<u16>,
    },
}

impl PartialOrd for ReleaseStage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for ReleaseStage {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            // 预览版 < 正式版
            (ReleaseStage::Preview { .. }, ReleaseStage::Release) => Ordering::Less,
            (ReleaseStage::Release, ReleaseStage::Preview { .. }) => Ordering::Greater,

            // 正式版之间相等
            (ReleaseStage::Release, ReleaseStage::Release) => Ordering::Equal,

            // 预览版之间：先比较 version，再比较 build
            (
                ReleaseStage::Preview { version: v1, build: b1 },
                ReleaseStage::Preview { version: v2, build: b2 },
            ) => v1.cmp(v2).then_with(|| b1.cmp(b2)),
        }
    }
}

impl core::fmt::Display for ReleaseStage {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ReleaseStage::Release => Ok(()),
            ReleaseStage::Preview { version, build: None } => {
                write!(f, "-pre.{version}")
            }
            ReleaseStage::Preview { version, build: Some(build) } => {
                write!(f, "-pre.{version}+build.{build}")
            }
        }
    }
}

/// 遵循格式：v0.4.0-pre.6+build.8
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub stage: ReleaseStage,
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // 依次比较 major -> minor -> patch -> stage
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| self.stage.cmp(&other.stage))
    }
}

impl core::fmt::Display for Version {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}{}", self.major, self.minor, self.patch, self.stage)
    }
}

impl Version {
    /// 写入到 writer
    ///
    /// 二进制格式（使用原生字节序）：
    /// - [0-1] major: u16
    /// - [2-3] minor: u16
    /// - [4-5] patch: u16
    /// - [6-7] len: u16 (0=Release, 1=Preview, 2=PreviewBuild)
    /// - [8-9] (可选) pre_version: u16
    /// - [10-11] (可选) build: u16
    ///
    /// # Errors
    ///
    /// 如果写入失败，返回 I/O 错误
    pub fn write_to<W: io::Write>(&self, writer: &mut W) -> io::Result<()> {
        // 写入固定头部
        writer.write_all(&self.major.to_ne_bytes())?;
        writer.write_all(&self.minor.to_ne_bytes())?;
        writer.write_all(&self.patch.to_ne_bytes())?;

        // 根据 stage 写入 len 和 metadata
        match self.stage {
            ReleaseStage::Release => {
                writer.write_all(&0u16.to_ne_bytes())?;
            }
            ReleaseStage::Preview { version, build: None } => {
                writer.write_all(&1u16.to_ne_bytes())?;
                writer.write_all(&version.to_ne_bytes())?;
            }
            ReleaseStage::Preview { version, build: Some(build) } => {
                writer.write_all(&2u16.to_ne_bytes())?;
                writer.write_all(&version.to_ne_bytes())?;
                writer.write_all(&build.to_ne_bytes())?;
            }
        }

        Ok(())
    }

    /// 从 reader 读取
    ///
    /// # Errors
    ///
    /// - `UnexpectedEof`: 数据不足
    /// - `InvalidData`: len 值非法（>2）
    /// - 其他 I/O 错误
    pub fn read_from<R: io::Read>(reader: &mut R) -> io::Result<Self> {
        let mut buf = [0u8; 2];

        // 读取固定头部
        reader.read_exact(&mut buf)?;
        let major = u16::from_ne_bytes(buf);

        reader.read_exact(&mut buf)?;
        let minor = u16::from_ne_bytes(buf);

        reader.read_exact(&mut buf)?;
        let patch = u16::from_ne_bytes(buf);

        reader.read_exact(&mut buf)?;
        let len = u16::from_ne_bytes(buf);

        // 根据 len 读取 metadata
        let stage = match len {
            0 => ReleaseStage::Release,
            1 => {
                reader.read_exact(&mut buf)?;
                let version = u16::from_ne_bytes(buf);
                ReleaseStage::Preview { version, build: None }
            }
            2 => {
                reader.read_exact(&mut buf)?;
                let version = u16::from_ne_bytes(buf);
                reader.read_exact(&mut buf)?;
                let build = u16::from_ne_bytes(buf);
                ReleaseStage::Preview { version, build: Some(build) }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid version length field: {len}"),
                ));
            }
        };

        Ok(Version { major, minor, patch, stage })
    }
}

// 辅助函数：创建正式版本
#[allow(dead_code)]
pub const fn release(major: u16, minor: u16, patch: u16) -> Version {
    Version { major, minor, patch, stage: ReleaseStage::Release }
}

// 辅助函数：创建预览版本（无 build）
#[allow(dead_code)]
pub const fn preview(major: u16, minor: u16, patch: u16, version: u16) -> Version {
    Version { major, minor, patch, stage: ReleaseStage::Preview { version, build: None } }
}

// 辅助函数：创建预览版本（带 build）
#[allow(dead_code)]
pub const fn preview_build(
    major: u16,
    minor: u16,
    patch: u16,
    version: u16,
    build: u16,
) -> Version {
    Version { major, minor, patch, stage: ReleaseStage::Preview { version, build: Some(build) } }
}
