use core::num::NonZeroUsize;

/// 请求日志限制枚举
#[derive(Debug, Clone, Copy)]
pub enum LogsLimit {
    /// 禁用日志记录
    Disabled,
    /// 有限制的日志记录，参数为最大日志数量
    Limited(NonZeroUsize),
}

impl LogsLimit {
    #[inline]
    pub fn from_usize(limit: usize) -> Self {
        match NonZeroUsize::new(limit) {
            None => Self::Disabled,
            Some(n) => Self::Limited(n),
        }
    }

    /// 检查是否需要保存日志
    #[inline(always)]
    pub fn should_log(&self) -> bool { !matches!(self, Self::Disabled) }

    /// 获取日志限制
    #[inline(always)]
    pub fn get_limit(self) -> usize {
        match self {
            Self::Disabled => 0,
            Self::Limited(limit) => limit.get(),
        }
    }
}
