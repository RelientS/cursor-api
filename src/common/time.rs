//! 跨平台项目和构建时间追踪
//!
//! 提供项目启动时间（天级）和构建时间（分钟级）的优雅显示。

pub use super::build::BUILD_EPOCH;
use crate::common::utils::now_with_epoch;
use chrono::{DateTime, Utc};
use core::fmt;

/// 项目基准时间点（2024-12-23 01:30:48 UTC）
#[cfg(unix)]
pub const EPOCH: std::time::SystemTime =
    unsafe { core::intrinsics::transmute((1734915448i64, 0u32)) };

/// 项目基准时间点（2024-12-23 01:30:48 UTC）
///
/// Windows使用FILETIME格式（100纳秒间隔数）
#[cfg(windows)]
pub const EPOCH: std::time::SystemTime = unsafe {
    const INTERVALS_PER_SEC: u64 = 10_000_000;
    const INTERVALS_TO_UNIX_EPOCH: u64 = 11_644_473_600 * INTERVALS_PER_SEC;
    const TARGET_INTERVALS: u64 = INTERVALS_TO_UNIX_EPOCH + 1734915448 * INTERVALS_PER_SEC;

    core::intrinsics::transmute((TARGET_INTERVALS as u32, (TARGET_INTERVALS >> 32) as u32))
};

const EPOCH_DATETIME: DateTime<Utc> =
    unsafe { DateTime::from_timestamp(1734915448i64, 0u32).unwrap_unchecked() };

/// 打印项目运行时长（年月日）
pub fn print_project_age() {
    let age = ProjectAge::since_epoch();
    println!("Project started {age} ago");
}

/// 打印构建时长（分钟级）
pub fn print_build_age() {
    let age = BuildAge::since_build();
    println!("Program built {age} ago");
}

/// 项目年龄（天级精度）
#[derive(Debug, Clone, Copy)]
struct ProjectAge {
    years: u32,
    months: u32,
    days: u32,
}

impl ProjectAge {
    /// 基于日历算法计算年龄
    ///
    /// 处理月末边界情况：起始日31号且目标月不足31天时，
    /// 计算到目标月末，剩余日期计入days
    fn from_duration(duration: core::time::Duration) -> Self {
        use chrono::Datelike as _;

        // SAFETY: duration来自NTP同步模块，保证不会导致无效时间戳
        let current_datetime = EPOCH_DATETIME + __unwrap!(chrono::TimeDelta::from_std(duration));

        let current_year = current_datetime.year();
        let current_month = current_datetime.month();
        let current_day = current_datetime.day();
        unsafe { core::hint::assert_unchecked(current_year >= 2025) }

        let mut years = current_year - 2024;
        let mut months = current_month as i32 - 12;
        let mut days = current_day as i32 - 23;

        // 日期借位
        if days < 0 {
            months -= 1;
            let (year, month) = if current_month == 1 {
                (current_year - 1, 12)
            } else {
                (current_year, current_month - 1)
            };
            let last_day_of_prev_month = unsafe {
                chrono::NaiveDate::from_ymd_opt(year, month, 1)
                    .unwrap_unchecked()
                    .pred_opt()
                    .unwrap_unchecked()
            };
            days += last_day_of_prev_month.day() as i32;
        }

        // 月份借位
        if months < 0 {
            years -= 1;
            months += 12;
        }

        Self { years: years as u32, months: months as u32, days: days as u32 }
    }

    /// # Panics
    /// 系统时间早于项目EPOCH时panic
    #[inline]
    pub fn since_epoch() -> Self {
        let duration = now_with_epoch(EPOCH, "system time before program epoch");
        Self::from_duration(duration)
    }
}

/// 构建年龄（分钟级精度）
#[derive(Debug, Clone, Copy)]
struct BuildAge {
    minutes: u64,
}

impl BuildAge {
    /// # Panics
    /// 系统时间早于构建EPOCH时panic
    #[inline]
    pub fn since_build() -> Self {
        let duration = now_with_epoch(BUILD_EPOCH, "system time before build epoch");
        Self { minutes: duration.as_secs() / 60 }
    }
}

struct Unit {
    word: &'static str,
    count: core::num::NonZeroU32,
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.count.get();
        write!(f, "{n} {}", self.word)?;
        if n > 1 { f.write_str("s") } else { Ok(()) }
    }
}

macro_rules! unit_fn {
    ($($name:ident),* $(,)?) => {
        $(
            #[inline]
            fn $name(n: u32) -> Unit {
                Unit {
                    word: stringify!($name),
                    count: unsafe { core::num::NonZeroU32::new_unchecked(n) }
                }
            }
        )*
    };
}

unit_fn!(year, month, day, hour, minute);

impl fmt::Display for ProjectAge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.years, self.months, self.days) {
            (0, 0, 0) => f.write_str("less than 1 day"),
            (0, 0, d) => day(d).fmt(f),
            (0, m, 0) => month(m).fmt(f),
            (0, m, d) => write!(f, "{} and {}", month(m), day(d)),
            (y, 0, 0) => year(y).fmt(f),
            (y, 0, d) => write!(f, "{} and {}", year(y), day(d)),
            (y, m, 0) => write!(f, "{} and {}", year(y), month(m)),
            (y, m, d) => write!(f, "{}, {}, and {}", year(y), month(m), day(d)),
        }
    }
}

impl fmt::Display for BuildAge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.minutes {
            0 => f.write_str("just now"),
            m @ 1..60 => minute(m as u32).fmt(f),
            m @ 60..120 => {
                let m = (m % 60) as u32;
                if m == 0 { hour(1).fmt(f) } else { write!(f, "{} and {}", hour(1), minute(m)) }
            }
            m @ 120..1440 => {
                let h = (m / 60) as u32;
                let m = (m % 60) as u32;
                if m == 0 { hour(h).fmt(f) } else { write!(f, "{} and {}", hour(h), minute(m)) }
            }
            m => {
                let d = (m / 1440) as u32;
                let h = ((m % 1440) / 60) as u32;
                if h == 0 { day(d).fmt(f) } else { write!(f, "{} and {}", day(d), hour(h)) }
            }
        }
    }
}
