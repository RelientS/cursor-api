mod queue;

use crate::app::{
    constant::{UNNAMED, UNNAMED_PATTERN},
    lazy::TOKENS_FILE_PATH,
    model::{Alias, ExtToken, TokenInfo, TokenInfoHelper, TokenKey},
};
use alloc::{borrow::Cow, collections::VecDeque};
use memmap2::{Mmap, MmapMut};
pub use queue::{QueueType, TokenHealth, TokenQueue};
use tokio::fs::OpenOptions;

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;

#[derive(Debug)]
pub enum TokenError {
    AliasExists,
    InvalidId,
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TokenError::AliasExists => "别名已存在",
            TokenError::InvalidId => "无效的Token ID",
        })
    }
}

impl core::error::Error for TokenError {}

/// 高性能Token管理器
///
/// 设计特点：
/// - **零拷贝查询**：所有查询方法返回引用，避免clone
/// - **紧凑存储**：Vec<Option<T>>密集布局，缓存友好
/// - **O(1)操作**：通过HashMap+Vec实现常数时间增删改查
/// - **ID重用**：FIFO队列管理空闲ID，减少内存碎片
///   - 优先重用最早释放的ID，提高cache locality
///   - Vec不会无限增长，删除后的槽位会被新token复用
/// - **多索引**：支持ID/别名/TokenKey三种查询方式
/// - **无锁设计**：单线程优化，避免同步开销
///
/// 数据结构不变性：
/// - `tokens`, `id_to_alias` 长度始终相同
/// - `id_map`, `alias_map` 中的id值始终 < tokens.len()
/// - `id_map`, `alias_map` 中的id指向的 `tokens[id]` 必为 Some
/// - `free_ids` 中的id必 < tokens.len() 且 `tokens[id]` 为 None
///
/// 性能关键路径已使用unsafe消除边界检查
pub struct TokenManager {
    /// 主存储：ID -> TokenInfo，使用Option支持删除后的空槽位
    tokens: Vec<Option<TokenInfo>>,
    /// TokenKey -> ID映射，用于通过token内容查找
    id_map: HashMap<TokenKey, usize>,
    /// 别名 -> ID映射，用于用户友好的查找
    alias_map: HashMap<Alias, usize>,
    /// ID -> 别名反向索引，与tokens同步维护
    id_to_alias: Vec<Option<Alias>>,
    /// 可重用的ID队列（FIFO），优先重用最早释放的ID
    free_ids: VecDeque<usize>,
    /// Round-robin token选择队列
    queue: TokenQueue,
}

impl TokenManager {
    #[inline]
    pub fn new(capacity: usize) -> Self {
        let r = ahash::RandomState::new();
        Self {
            tokens: Vec::with_capacity(capacity),
            id_map: HashMap::with_capacity_and_hasher(capacity, r.clone()),
            alias_map: HashMap::with_capacity_and_hasher(capacity, r),
            id_to_alias: Vec::with_capacity(capacity),
            free_ids: VecDeque::with_capacity(capacity / 10), // 假设10%的删除率
            queue: TokenQueue::with_capacity(capacity),
        }
    }

    #[inline(never)]
    pub fn add<'a, S: Into<Cow<'a, str>>>(
        &mut self,
        token_info: TokenInfo,
        alias: S,
    ) -> Result<usize, TokenError> {
        // 处理未命名或冲突的别名，自动生成唯一别名
        let mut alias: Cow<'_, str> = alias.into();
        if alias == UNNAMED || alias.starts_with(UNNAMED_PATTERN) {
            let id = self.free_ids.front().copied().unwrap_or(self.tokens.len());
            alias = Cow::Owned(generate_unnamed_alias(id));
        }

        if self.alias_map.contains_key(&*alias) {
            return Err(TokenError::AliasExists);
        }

        // ID分配策略：优先重用空闲ID（FIFO顺序），否则扩展vec
        let id = if let Some(reused_id) = self.free_ids.pop_front() {
            reused_id
        } else {
            let new_id = self.tokens.len();
            self.tokens.push(None);
            self.id_to_alias.push(None);
            new_id
        };

        let key = token_info.bundle.primary_token.key();
        self.id_map.insert(key, id);
        self.queue.push(key, id);

        // SAFETY: id要么是reused_id（来自free_ids，必定<len），要么是刚push后的新索引
        unsafe { *self.tokens.get_unchecked_mut(id) = Some(token_info) };

        let alias = Alias::new(alias);
        self.alias_map.insert(alias.clone(), id);

        // SAFETY: 同上，id有效且id_to_alias与tokens长度同步
        unsafe { *self.id_to_alias.get_unchecked_mut(id) = Some(alias) };

        Ok(id)
    }

    #[cfg(not(feature = "horizon"))]
    /// 热路径：通过ID查询Token
    #[inline]
    pub fn get_by_id(&self, id: usize) -> Option<&TokenInfo> {
        self.tokens.get(id).and_then(|o| o.as_ref())
    }

    /// 热路径：通过别名查询Token
    #[inline]
    pub fn get_by_alias(&self, alias: &str) -> Option<&TokenInfo> {
        let &id = self.alias_map.get(alias)?;
        // SAFETY: alias_map中的id由add/remove维护，保证<tokens.len()且对应Some
        Some(unsafe { self.tokens.get_unchecked(id).as_ref().unwrap_unchecked() })
    }

    #[inline(never)]
    pub fn remove(&mut self, id: usize) -> Option<TokenInfo> {
        let token_info = self.tokens.get_mut(id)?.take()?;

        // 清理所有索引
        let key = token_info.bundle.primary_token.key();
        self.id_map.remove(&key);
        #[cfg(not(feature = "horizon"))]
        self.queue.remove(&queue);
        #[cfg(feature = "horizon")]
        self.queue.remove(&key, &self.tokens);

        // SAFETY: 能走到这里说明id<len且Some，id_to_alias同步长度，必有对应别名
        unsafe {
            let alias = self.id_to_alias.get_unchecked_mut(id).take().unwrap_unchecked();
            self.alias_map.remove(&alias);
        }

        // 将ID加入空闲队列末尾，等待重用
        self.free_ids.push_back(id);
        Some(token_info)
    }

    #[inline(never)]
    pub fn set_alias<'a, S: Into<Cow<'a, str>>>(
        &mut self,
        id: usize,
        alias: S,
    ) -> Result<(), TokenError> {
        if self.tokens.get(id).is_none_or(Option::is_none) {
            return Err(TokenError::InvalidId);
        }

        let mut alias: Cow<'_, str> = alias.into();
        if alias == UNNAMED || alias.starts_with(UNNAMED_PATTERN) {
            alias = Cow::Owned(generate_unnamed_alias(id));
        }
        if self.alias_map.contains_key(&*alias) {
            return Err(TokenError::AliasExists);
        }

        // SAFETY: 前面已检查id有效且Some
        unsafe {
            let old_alias = self.id_to_alias.get_unchecked_mut(id).take().unwrap_unchecked();
            self.alias_map.remove(&old_alias);
        }

        let alias = Alias::new(alias);
        self.alias_map.insert(alias.clone(), id);

        // SAFETY: id仍然有效
        unsafe { *self.id_to_alias.get_unchecked_mut(id) = Some(alias) };

        Ok(())
    }

    pub fn tokens(&self) -> &Vec<Option<TokenInfo>> { &self.tokens }

    pub fn tokens_mut(&mut self) -> TokensWriter<'_> {
        TokensWriter { tokens: &mut self.tokens, id_map: &mut self.id_map, queue: &mut self.queue }
    }

    pub fn id_map(&self) -> &HashMap<TokenKey, usize> { &self.id_map }

    pub fn alias_map(&self) -> &HashMap<Alias, usize> { &self.alias_map }

    pub fn id_to_alias(&self) -> &Vec<Option<Alias>> { &self.id_to_alias }

    pub fn select(&self, queue_type: QueueType) -> Option<ExtToken> {
        self.queue.select(queue_type, self)
    }

    #[inline(never)]
    pub fn list(&self) -> Vec<(usize, Alias, TokenInfo)> {
        // SAFETY: enumerate保证id<len，filter_map只处理Some分支，id_to_alias同步维护
        unsafe {
            self.tokens
                .iter()
                .enumerate()
                .filter_map(|(id, token_opt)| {
                    token_opt.as_ref().map(|token| {
                        let alias = self.id_to_alias.get_unchecked(id).as_ref().unwrap_unchecked();
                        (id, alias.clone(), token.clone())
                    })
                })
                .collect()
        }
    }

    /// 更新所有token的客户端密钥，用于安全性刷新
    #[inline(always)]
    pub fn update_client_key(&mut self) {
        for token_info in self.tokens.iter_mut().flatten() {
            token_info.bundle.client_key = super::super::Hash::random();
            token_info.bundle.session_id = uuid::Uuid::new_v4();
        }
    }

    #[inline(never)]
    pub async fn save(&self) -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
        // SAFETY: enumerate保证id<len，filter_map只处理Some，id_to_alias同步维护
        let helpers: Vec<TokenInfoHelper> = unsafe {
            self.tokens
                .iter()
                .enumerate()
                .filter_map(|(id, token_opt)| {
                    token_opt.as_ref().map(|token_info| {
                        let alias = self
                            .id_to_alias
                            .get_unchecked(id)
                            .as_ref()
                            .map(|a| a.to_string())
                            .unwrap_unchecked();

                        TokenInfoHelper::new(token_info, alias)
                    })
                })
                .collect()
        };

        let bytes = ::rkyv::to_bytes::<::rkyv::rancor::Error>(&helpers)?;
        if bytes.len() > usize::MAX >> 1 {
            return Err("令牌数据过大".into());
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&*TOKENS_FILE_PATH)
            .await?;
        file.set_len(bytes.len() as u64).await?;

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };
        mmap.copy_from_slice(&bytes);
        mmap.flush()?;

        Ok(())
    }

    #[inline(never)]
    pub async fn load() -> Result<Self, Box<dyn core::error::Error + Send + Sync + 'static>> {
        let file = match OpenOptions::new().read(true).open(&*TOKENS_FILE_PATH).await {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::new(0));
            }
            Err(e) => return Err(Box::new(e)),
        };

        if file.metadata().await?.len() > usize::MAX as u64 {
            return Err("令牌文件过大".into());
        }

        let mmap = unsafe { Mmap::map(&file)? };
        let helpers = unsafe {
            ::rkyv::from_bytes_unchecked::<Vec<TokenInfoHelper>, ::rkyv::rancor::Error>(&mmap)
        }
        .map_err(|_| "加载令牌失败")?;
        let mut manager = Self::new(helpers.len());

        for helper in helpers {
            let (token_info, alias) = helper.extract();
            let _ = manager.add(token_info, alias)?;
        }

        Ok(manager)
    }
}

pub struct TokensWriter<'w> {
    tokens: &'w mut Vec<Option<TokenInfo>>,
    id_map: &'w mut HashMap<TokenKey, usize>,
    queue: &'w mut TokenQueue,
}

impl<'w> TokensWriter<'w> {
    // SAFETY: 调用者必须保证id < tokens.len()且tokens[id].is_some()
    #[inline]
    pub unsafe fn get_unchecked_mut(self, id: usize) -> &'w mut TokenInfo {
        unsafe { self.tokens.get_unchecked_mut(id).as_mut().unwrap_unchecked() }
    }

    // SAFETY: 调用者必须保证id < tokens.len()且tokens[id].is_some()
    #[inline]
    pub unsafe fn into_token_writer(self, id: usize) -> TokenWriter<'w> {
        let token =
            unsafe { &mut self.tokens.get_unchecked_mut(id).as_mut().unwrap_unchecked().bundle };
        TokenWriter {
            key: token.primary_token.key(),
            token,
            id_map: self.id_map,
            queue: self.queue,
        }
    }
}

/// Token写入器，通过Drop自动同步key变化
///
/// 使用场景：当需要修改token的key时，通过此类型确保：
/// 1. 修改完成后自动更新id_map
/// 2. 修改完成后自动更新queue中的key
/// 3. 防止忘记手动同步导致的索引不一致
pub struct TokenWriter<'w> {
    pub key: TokenKey,
    token: &'w mut ExtToken,
    id_map: &'w mut HashMap<TokenKey, usize>,
    queue: &'w mut TokenQueue,
}

impl<'w> core::ops::Deref for TokenWriter<'w> {
    type Target = &'w mut ExtToken;
    fn deref(&self) -> &Self::Target { &self.token }
}

impl<'w> core::ops::DerefMut for TokenWriter<'w> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.token }
}

impl Drop for TokenWriter<'_> {
    fn drop(&mut self) {
        use core::hint::{assert_unchecked, unreachable_unchecked};
        let key = self.token.primary_token.key();

        // 检测key是否变化，变化则更新所有索引
        if key != self.key {
            // SAFETY: TokenWriter只能通过into_token_writer创建，那时token必存在
            // self.key是创建时的token key，必在id_map中
            unsafe {
                let i = if let hashbrown::hash_map::EntryRef::Occupied(entry) = self.id_map.entry_ref(&self.key) {
                    entry.remove()
                } else {
                    unreachable_unchecked()
                };
                self.id_map.insert(key, i);
                assert_unchecked(self.queue.set_key(&self.key, key));
            }
        }
    }
}

/// 生成未命名token的默认别名
/// 格式：UNNAMED_PATTERN + ID（如"unnamed_42"）
#[inline]
fn generate_unnamed_alias(id: usize) -> String {
    // 预分配容量：pattern长度 + 6位数字
    // 6位数字可表示0-999999，覆盖百万级token
    // 超过百万时String会自动扩容（额外一次realloc）
    const CAPACITY: usize = UNNAMED_PATTERN.len() + 6;
    let mut s = String::with_capacity(CAPACITY);
    s.push_str(UNNAMED_PATTERN);

    if id == 0 {
        s.push('0');
    } else {
        let start = s.len();
        let mut n = id;
        // 倒序push数字，最后反转
        while n > 0 {
            s.push((b'0' + (n % 10) as u8) as char);
            n /= 10;
        }
        // SAFETY: 只push了ASCII数字，UTF-8有效
        unsafe { s[start..].as_bytes_mut().reverse() };
    }

    s
}
