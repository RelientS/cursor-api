mod token;

use super::{
    log::{LogManager, create_task},
    proxy_pool::Proxies,
};
use core::sync::atomic::{AtomicU64, Ordering};
pub use token::{QueueType, TokenError, TokenHealth, TokenManager, TokenWriter};
use tokio::sync::RwLock;

pub struct AppState {
    pub token_manager: RwLock<TokenManager>,
    pub total_requests: AtomicU64,
    pub active_requests: AtomicU64,
    pub error_requests: AtomicU64,
}

impl AppState {
    pub async fn load() -> Result<Self, Box<dyn core::error::Error + Send + Sync + 'static>> {
        // 并行加载日志、令牌和代理
        let (log_manager_result, token_manager_result, proxies_result) =
            tokio::join!(LogManager::load(), TokenManager::load(), Proxies::load());

        // 获取结果，处理错误
        let log_manager = log_manager_result?;
        let token_manager = token_manager_result?;

        // 处理代理
        let proxies = proxies_result.unwrap_or_default();
        proxies.init();

        // 计算初始统计信息
        let error_count = log_manager.error_count();
        let total_count = log_manager.total_count();

        create_task(log_manager);

        Ok(Self {
            token_manager: RwLock::new(token_manager),
            total_requests: AtomicU64::new(total_count),
            active_requests: AtomicU64::new(0),
            error_requests: AtomicU64::new(error_count),
        })
    }

    /// 增加总请求计数
    #[inline(always)]
    pub fn increment_total(&self) { self.total_requests.fetch_add(1, Ordering::Relaxed); }

    /// 增加活跃请求计数
    #[inline(always)]
    pub fn increment_active(&self) { self.active_requests.fetch_add(1, Ordering::Relaxed); }

    /// 减少活跃请求计数
    #[inline(always)]
    pub fn decrement_active(&self) { self.active_requests.fetch_sub(1, Ordering::Relaxed); }

    /// 增加错误请求计数
    #[inline(always)]
    pub fn increment_error(&self) { self.error_requests.fetch_add(1, Ordering::Relaxed); }

    /// 获取TokenManager的读锁
    #[inline]
    pub async fn token_manager_read(&self) -> tokio::sync::RwLockReadGuard<'_, TokenManager> {
        self.token_manager.read().await
    }

    /// 获取TokenManager的写锁
    #[inline]
    pub async fn token_manager_write(&self) -> tokio::sync::RwLockWriteGuard<'_, TokenManager> {
        self.token_manager.write().await
    }

    pub async fn save(&self) -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
        // 并行保存日志、令牌和代理
        let (log_result, tokens_result, proxies_result) =
            tokio::join!(LogManager::save(), self.save_tokens(), Proxies::save());

        log_result?;
        tokens_result?;
        proxies_result?;
        Ok(())
    }

    async fn save_tokens(&self) -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
        self.token_manager.read().await.save().await
    }

    /// 更新token manager中的client key
    pub async fn update_client_key(&self) { self.token_manager.write().await.update_client_key() }
}
