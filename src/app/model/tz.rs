use manually_init::ManuallyInit;

use crate::common::utils::parse_from_env;

pub static TZ: ManuallyInit<chrono_tz::Tz> = ManuallyInit::new();

#[inline(always)]
pub fn __init() {
    use std::str::FromStr as _;
    let tz = match chrono_tz::Tz::from_str(&parse_from_env("TZ", super::EMPTY_STRING)) {
        Ok(tz) => tz,
        Err(_e) => chrono_tz::Tz::UTC,
    };
    println!("时区TZ: '{tz}'");
    TZ.init(tz);
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct DateTime(chrono::DateTime<chrono_tz::Tz>);

/// 从操作系统获取当前时刻，返回一个纯粹的、无时区的`NaiveDateTime`。
fn now_naive() -> chrono::NaiveDateTime {
    use chrono::{NaiveDate, NaiveTime};

    const UNIX_EPOCH_DAY: i64 = 719_163;
    let now = crate::common::utils::now();

    let secs = now.as_secs() as i64;
    let nsecs = now.subsec_nanos();

    let days = secs.div_euclid(86_400) + UNIX_EPOCH_DAY;
    let secs_of_day = secs.rem_euclid(86_400);

    let date = __unwrap!(NaiveDate::from_num_days_from_ce_opt(days as i32));
    let time = __unwrap!(NaiveTime::from_num_seconds_from_midnight_opt(secs_of_day as u32, nsecs));

    date.and_time(time)
}

impl DateTime {
    /// 获取当前时刻，并应用全局静态时区 `TZ`。
    #[inline(always)]
    pub fn now() -> Self {
        use chrono::TimeZone as _;
        Self(TZ.from_utc_datetime(&now_naive()))
    }

    /// 获取当前时刻的 UTC 时间。
    #[inline(always)]
    pub fn utc_now() -> chrono::DateTime<chrono::Utc> {
        now_naive().and_utc()
    }

    #[inline(always)]
    pub fn naive_now() -> chrono::NaiveDateTime {
        now_naive()
    }

    #[inline(always)]
    pub fn naive(&self) -> chrono::NaiveDateTime {
        self.0.naive_utc()
    }

    #[inline(always)]
    pub fn from_naive(naive: &chrono::NaiveDateTime) -> Self {
        use chrono::TimeZone as _;
        Self(TZ.from_utc_datetime(naive))
    }
}

impl ::core::ops::Deref for DateTime {
    type Target = chrono::DateTime<chrono_tz::Tz>;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<chrono::NaiveDateTime> for DateTime {
    #[inline]
    fn from(naive: chrono::NaiveDateTime) -> Self {
        Self::from_naive(&naive)
    }
}

impl From<DateTime> for chrono::NaiveDateTime {
    #[inline]
    fn from(date_time: DateTime) -> Self {
        date_time.naive()
    }
}

mod serde_impls {
    use super::*;

    impl ::serde::Serialize for DateTime {
        #[inline]
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: ::serde::Serializer,
        {
            self.0.serialize(serializer)
        }
    }

    impl<'de> ::serde::Deserialize<'de> for DateTime {
        #[inline]
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: ::serde::Deserializer<'de>,
        {
            deserializer
                .deserialize_str(chrono::serde::DateTimeVisitor)
                .map(|dt| Self(dt.with_timezone(&TZ)))
        }
    }
}

impl ::core::cmp::PartialEq<DateTime> for DateTime {
    #[inline]
    fn eq(&self, other: &DateTime) -> bool {
        self.0 == other.0
    }
}

impl ::core::cmp::Eq for DateTime {}

impl ::core::cmp::PartialOrd<DateTime> for DateTime {
    #[inline]
    fn partial_cmp(&self, other: &DateTime) -> Option<::core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ::core::cmp::Ord for DateTime {
    #[inline]
    fn cmp(&self, other: &DateTime) -> ::core::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}
