use super::{HashMap, TokenManager};
#[cfg(not(feature = "horizon"))]
use crate::app::model::{Randomness, UserId};
use crate::{
    app::model::{ExtToken, TokenKey},
    common::utils::now_secs,
};
use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(
    Clone,
    Copy,
    serde::Serialize,
    serde::Deserialize,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct TokenHealth {
    pub backoff_until: u64,
    pub consecutive_failures: u32,
}

impl TokenHealth {
    pub const fn new() -> Self { Self { backoff_until: 0, consecutive_failures: 0 } }

    #[inline]
    pub fn is_available(&self) -> bool { self.backoff_until <= now_secs() }

    #[inline]
    pub fn set_backoff(&mut self, seconds: u64) { self.backoff_until = now_secs() + seconds; }

    #[inline]
    pub fn set_permanent_backoff(&mut self) { self.backoff_until = u64::MAX; }

    #[inline]
    pub fn clear_backoff(&mut self) {
        self.backoff_until = 0;
        self.consecutive_failures = 0;
    }

    #[inline]
    pub fn inc_failures(&mut self) -> u32 {
        self.consecutive_failures += 1;
        self.consecutive_failures
    }
}

#[cfg(not(feature = "horizon"))]
/// 队列内部使用的复合键，将TokenKey和manager索引绑定
/// 作为hint加速查找：如果token未被修改，index可以直接定位
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct TokenManagerKey {
    user_id: UserId,
    randomness: Randomness,
    index: usize, // token在manager.tokens中的位置（hint）
}

#[cfg(not(feature = "horizon"))]
impl TokenManagerKey {
    #[inline]
    pub fn new(token_key: TokenKey, index: usize) -> Self {
        Self { user_id: token_key.user_id, randomness: token_key.randomness, index }
    }

    #[inline]
    pub fn token_key(&self) -> TokenKey {
        TokenKey { user_id: self.user_id, randomness: self.randomness }
    }

    #[inline]
    pub fn set_token_key(&mut self, new_key: TokenKey) {
        self.user_id = new_key.user_id;
        self.randomness = new_key.randomness;
    }
}

/// 队列优先级类型，决定token选择顺序
/// 数值越小优先级越高
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum QueueType {
    PrivilegedPaid = 0, // 最高优先级
    PrivilegedFree = 1,
    NormalPaid = 2,
    NormalFree = 3, // 最低优先级
}

impl QueueType {
    #[inline]
    pub const fn as_index(self) -> usize { self as usize }
}

/// 全局队列头指针数组，每个队列类型独立维护轮询位置
/// 使用静态全局变量而非存储在TokenQueue中，避免每次select时的借用检查
static QUEUE_HEADS: [AtomicUsize; 4] =
    [AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0)];

/// Round-robin token选择队列
///
/// 设计要点：
/// - 所有token共享同一个vec，不同队列类型通过head指针区分轮询位置
/// - 每次select从当前head开始遍历，跳过不可用token，找到后更新head
/// - remove时需要调整所有head，保证指针不会越界
pub struct TokenQueue {
    #[cfg(not(feature = "horizon"))]
    vec: Vec<TokenManagerKey>,
    #[cfg(feature = "horizon")]
    vec: Vec<usize>,
    map: HashMap<TokenKey, usize>, // TokenKey -> vec索引，用于O(1)查找和删除
}

impl TokenQueue {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            vec: Vec::with_capacity(capacity),
            map: HashMap::with_capacity_and_hasher(capacity, ahash::RandomState::new()),
        }
    }

    pub fn push(&mut self, token_key: TokenKey, token_id: usize) {
        #[cfg(not(feature = "horizon"))]
        let mgr_key = TokenManagerKey::new(token_key, token_id);
        #[cfg(feature = "horizon")]
        let mgr_key = token_id;
        let vec_index = self.vec.len();

        self.vec.push(mgr_key);
        self.map.insert(token_key, vec_index);
    }

    pub fn set_key(&mut self, old_key: &TokenKey, new_key: TokenKey) -> bool {
        let Some(vec_index) = self.map.remove(old_key) else { return false };

        #[cfg(not(feature = "horizon"))]
        // SAFETY: vec_index来自map，map中的索引由push/remove维护，保证有效
        unsafe {
            self.vec.get_unchecked_mut(vec_index).set_token_key(new_key);
        };

        self.map.insert(new_key, vec_index).is_none()
    }

    #[cfg(not(feature = "horizon"))]
    pub fn remove(&mut self, token_key: &TokenKey) -> Option<TokenManagerKey> {
        let vec_index = self.map.remove(token_key)?;

        // 调整所有队列的head指针：如果head在被删除元素之后，需要前移一位
        // 这保证了remove后指针仍然指向正确的相对位置
        // SAFETY: QueueType是repr(usize)枚举，值域为0..4，QUEUE_HEADS长度为4
        unsafe {
            for i in 0..4 {
                let head = QUEUE_HEADS.get_unchecked(i);
                let current = head.load(Ordering::Acquire);
                if current > vec_index {
                    head.store(current - 1, Ordering::Release);
                }
            }
        }

        // Vec::remove会将后续元素前移，需要更新它们在map中的索引
        let removed = self.vec.remove(vec_index);

        // 使用指针迭代避免重复的bounds checking
        // SAFETY: vec_index来自map且已remove一个元素，后续元素索引为vec_index..len
        // 这些元素的token_key在map中必然存在（由push/set_key保证）
        unsafe {
            let base = self.vec.as_mut_ptr().add(vec_index);
            for i in 0..(self.vec.len() - vec_index) {
                let key = (*base.add(i)).token_key();
                *self.map.get_mut(&key).unwrap_unchecked() = vec_index + i;
            }
        }

        Some(removed)
    }

    #[cfg(feature = "horizon")]
    pub fn remove(
        &mut self,
        token_key: &TokenKey,
        tokens: &Vec<Option<super::TokenInfo>>,
    ) -> Option<usize> {
        let vec_index = self.map.remove(token_key)?;

        // 调整所有队列的head指针：如果head在被删除元素之后，需要前移一位
        // 这保证了remove后指针仍然指向正确的相对位置
        // SAFETY: QueueType是repr(usize)枚举，值域为0..4，QUEUE_HEADS长度为4
        unsafe {
            for i in 0..4 {
                let head = QUEUE_HEADS.get_unchecked(i);
                let current = head.load(Ordering::Acquire);
                if current > vec_index {
                    head.store(current - 1, Ordering::Release);
                }
            }
        }

        // Vec::remove会将后续元素前移，需要更新它们在map中的索引
        let removed = self.vec.remove(vec_index);

        // 使用指针迭代避免重复的bounds checking
        // SAFETY: vec_index来自map且已remove一个元素，后续元素索引为vec_index..len
        // 这些元素的token_key在map中必然存在（由push/set_key保证）
        unsafe {
            let base = self.vec.as_mut_ptr().add(vec_index);
            for i in 0..(self.vec.len() - vec_index) {
                let key = tokens
                    .get_unchecked(*base.add(i))
                    .as_ref()
                    .unwrap_unchecked()
                    .bundle
                    .primary_token
                    .key();
                *self.map.get_mut(&key).unwrap_unchecked() = vec_index + i;
            }
        }

        Some(removed)
    }

    /// Round-robin选择可用token
    ///
    /// 算法：
    /// 1. 从当前队列的head开始轮询
    /// 2. 检查token是否启用且健康
    /// 3. 找到后更新head到下一个位置
    /// 4. 最多尝试整个vec一轮，避免无限循环
    pub fn select(&self, queue_type: QueueType, manager: &TokenManager) -> Option<ExtToken> {
        if self.vec.is_empty() {
            return None;
        }

        // SAFETY: queue_type.as_index()为0..4，QUEUE_HEADS长度为4
        let head = unsafe { QUEUE_HEADS.get_unchecked(queue_type.as_index()) };
        let start = head.load(Ordering::Relaxed);
        let len = self.vec.len();

        // SAFETY: vec非空，len>=1，base有效
        let base = self.vec.as_ptr();

        for i in 0..len {
            let index = (start + i) % len;
            // SAFETY: index < len
            let mgr_key = unsafe { *base.add(index) };
            #[cfg(not(feature = "horizon"))]
            let token = {
                let token_key = mgr_key.token_key();

                // 先尝试用hint（mgr_key.index）快速查找
                // 如果token的key已变化，hint失效，需要通过id_map查找
                let token_id = if let Some(token) = manager.get_by_id(mgr_key.index)
                    && token.bundle.primary_token.key() == token_key
                {
                    mgr_key.index
                } else {
                    *manager.id_map().get(&token_key)?
                };

                let token = manager.get_by_id(token_id)?;

                if !token.is_enabled() || !token.status.health.is_available() {
                    continue;
                }

                // 找到可用token，更新head到下一个位置
                head.store((index + 1) % len, Ordering::Relaxed);

                token
            };
            #[cfg(feature = "horizon")]
            let token = {
                let token =
                    unsafe { manager.tokens.get_unchecked(mgr_key).as_ref().unwrap_unchecked() };

                if !token.is_enabled() || !token.status.health.is_available() {
                    continue;
                }

                // 找到可用token，更新head到下一个位置
                head.store((index + 1) % len, Ordering::Relaxed);

                token
            };
            return Some(token.bundle.clone());
        }

        None
    }
}
