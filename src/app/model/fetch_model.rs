/// 模型数据获取模式
#[derive(Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum FetchMode {
    /// 覆盖现有数据
    Truncate,

    /// 追加新数据并部分覆盖现有数据
    AppendTruncate,

    /// 追加新数据
    Append,
}

impl FetchMode {
    /// 表示截断模式的字符串常量
    const TRUNCATE: &'static str = "truncate";

    /// 表示追加截断模式的字符串常量
    const APPEND_TRUNCATE: &'static str = "append:truncate";

    /// 表示追加模式的字符串常量
    const APPEND: &'static str = "append";

    /// 从字符串解析获取模式
    #[inline]
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            Self::TRUNCATE => Self::Truncate,
            Self::APPEND_TRUNCATE => Self::AppendTruncate,
            Self::APPEND => Self::Append,
            _ => Self::default(),
        }
    }

    /// 返回获取模式的字符串表示
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Truncate => Self::TRUNCATE,
            Self::AppendTruncate => Self::APPEND_TRUNCATE,
            Self::Append => Self::APPEND,
        }
    }
}

impl const Default for FetchMode {
    #[inline(always)]
    fn default() -> Self { Self::Truncate }
}

impl ::serde::Serialize for FetchMode {
    /// 序列化获取模式
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> ::serde::Deserialize<'de> for FetchMode {
    /// 反序列化获取模式
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_str(&s))
    }
}
