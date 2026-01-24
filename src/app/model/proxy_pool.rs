use crate::app::lazy::{PROXIES_FILE_PATH, SERVICE_TIMEOUT, TCP_KEEPALIVE, TCP_KEEPALIVE_INTERVAL, TCP_KEEPALIVE_RETRIES};
use alloc::sync::Arc;
use arc_swap::{ArcSwap, ArcSwapAny};
use core::{str::FromStr, time::Duration};
use manually_init::ManuallyInit;
use memmap2::{MmapMut, MmapOptions};
use reqwest::Client;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
mod proxy_url;
use proxy_url::ProxyUrl;

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;
type HashSet<K> = hashbrown::HashSet<K, ahash::RandomState>;

// 代理值常量
const NON_PROXY: &str = "non";
const SYS_PROXY: &str = "sys";

/// 创建默认的代理配置
///
/// 包含一个系统代理配置
#[inline]
pub fn default_proxies() -> HashMap<String, SingleProxy> {
    HashMap::from_iter([(SYS_PROXY.to_string(), SingleProxy::Sys)])
}

/// 名称到代理配置的映射
static PROXIES: ManuallyInit<ArcSwap<HashMap<String, SingleProxy>>> = ManuallyInit::new();

/// 通用代理名称
static GENERAL_NAME: ManuallyInit<ArcSwap<String>> = ManuallyInit::new();

// /// 获取图像代理名称
// static FETCH_IMAGE_NAME: ArcSwapOption<String> = ArcSwapOption::const_empty();

/// 代理配置到客户端实例的映射
///
/// 缓存已创建的客户端，避免重复创建相同配置的客户端
static CLIENTS: ManuallyInit<ArcSwap<HashMap<SingleProxy, Client>>> = ManuallyInit::new();

/// 通用客户端
///
/// 用于未指定特定代理的请求，指向 GENERAL_NAME 对应的客户端
static GENERAL_CLIENT: ManuallyInit<ArcSwapAny<Client>> = ManuallyInit::new();

// /// 获取图像客户端
// ///
// /// 用于获取HTTP图像的请求，指向 FETCH_IMAGE_NAME 对应的客户端
// static FETCH_IMAGE_CLIENT: ArcSwapAny<Option<Client>> = unsafe {
//     core::intrinsics::transmute_unchecked::<ArcSwapOption<()>, _>(ArcSwapOption::const_empty())
// };

/// 代理配置管理器
///
/// 负责管理所有代理配置及其对应的客户端
#[derive(Clone, Deserialize, Serialize, Archive, RkyvDeserialize, RkyvSerialize)]
pub struct Proxies {
    /// 名称到代理配置的映射
    proxies: HashMap<String, SingleProxy>,
    /// 默认使用的代理名称
    general: String,
}

impl Default for Proxies {
    fn default() -> Self { Self { proxies: default_proxies(), general: SYS_PROXY.to_string() } }
}

impl Proxies {
    /// 初始化全局代理系统
    ///
    /// 验证配置的完整性并创建所有必要的客户端
    #[inline]
    pub fn init(mut self) {
        // 确保至少有默认代理
        if self.proxies.is_empty() {
            self.proxies = default_proxies();
            if self.general.as_str() != SYS_PROXY {
                self.general = SYS_PROXY.to_string();
            }
        } else if !self.proxies.contains_key(&self.general) {
            // 通用代理名称无效，使用第一个可用的代理
            self.general = __unwrap!(self.proxies.keys().next()).clone();
        }

        // 收集所有唯一的代理配置
        let proxies = self.proxies.values().collect::<HashSet<_>>();
        let mut clients =
            HashMap::with_capacity_and_hasher(proxies.len(), ::ahash::RandomState::new());

        // 为每个代理配置创建客户端
        for proxy in proxies {
            proxy.insert_to(&mut clients);
        }

        // 初始化全局静态变量
        // Safety: 前面的逻辑已确保 general 存在于 proxies 中，
        // 且所有 proxies 中的代理都有对应的客户端
        GENERAL_CLIENT.init(ArcSwapAny::from(
            __unwrap!(clients.get(__unwrap!(self.proxies.get(&self.general)))).clone(),
        ));
        CLIENTS.init(ArcSwap::from_pointee(clients));
        PROXIES.init(ArcSwap::from_pointee(self.proxies));
        GENERAL_NAME.init(ArcSwap::from_pointee(self.general));
    }

    /// 更新全局代理配置（不更新客户端池）
    #[inline]
    pub fn update_global(self) {
        proxies().store(Arc::new(self.proxies));
        general_name().store(Arc::new(self.general));
    }

    /// 更新全局代理池
    ///
    /// 智能更新客户端池：
    /// - 移除不再使用的客户端
    /// - 为新的代理配置创建客户端
    /// - 保留仍在使用的客户端
    fn update_global_pool() {
        let proxies = proxies().load();
        let mut general_name = general_name().load_full();
        let mut clients = (*clients().load_full()).clone();

        // 确保配置有效性
        if proxies.is_empty() {
            self::proxies().store(Arc::new(default_proxies()));
            if general_name.as_str() != SYS_PROXY {
                general_name = Arc::new(SYS_PROXY.to_string());
            }
        } else if !proxies.contains_key(&*general_name) {
            // 通用代理名称无效，选择第一个可用的
            general_name = Arc::new(__unwrap!(proxies.keys().next()).clone());
        }

        // 收集当前配置中的所有唯一代理
        let current_proxies: HashSet<&SingleProxy> = proxies.values().collect();

        // 移除不再使用的客户端
        let to_remove: Vec<SingleProxy> =
            clients.keys().filter(|proxy| !current_proxies.contains(proxy)).cloned().collect();

        for proxy in to_remove {
            clients.remove(&proxy);
        }

        // 为新的代理配置创建客户端
        for proxy in current_proxies {
            if !clients.contains_key(proxy) {
                proxy.insert_to(&mut clients);
            }
        }

        // 更新全局状态
        self::clients().store(Arc::new(clients));
        self::general_name().store(general_name);
        set_general();
    }

    /// 保存代理配置到文件
    pub async fn save() -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&Self {
            proxies: (*proxies().load_full()).clone(),
            general: (*general_name().load_full()).clone(),
        })?;

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&*PROXIES_FILE_PATH)
            .await?;

        // 防止文件过大
        if bytes.len() > usize::MAX >> 1 {
            return Err("代理数据过大".into());
        }

        file.set_len(bytes.len() as u64).await?;
        let mut mmap = unsafe { MmapMut::map_mut(&file)? };
        mmap.copy_from_slice(&bytes);
        mmap.flush()?;

        Ok(())
    }

    /// 从文件加载代理配置
    pub async fn load() -> Result<Self, Box<dyn core::error::Error + Send + Sync + 'static>> {
        let file = match OpenOptions::new().read(true).open(&*PROXIES_FILE_PATH).await {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(e) => return Err(Box::new(e)),
        };

        if file.metadata().await?.len() > usize::MAX as u64 {
            return Err("代理文件过大".into());
        }

        let mmap = unsafe { MmapOptions::new().map(&file)? };

        // Safety: 文件内容由我们自己控制，格式保证正确
        unsafe {
            ::rkyv::from_bytes_unchecked::<Self, ::rkyv::rancor::Error>(&mmap)
                .map_err(|_| "加载代理失败".into())
        }
    }

    /// 更新全局代理池并保存配置
    #[inline]
    pub async fn update_and_save() -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>>
    {
        Self::update_global_pool();
        Self::save().await
    }
}

/// 单个代理配置
#[derive(Clone, Archive, RkyvDeserialize, RkyvSerialize, PartialEq, Eq, Hash)]
#[rkyv(compare(PartialEq))]
pub enum SingleProxy {
    /// 不使用代理
    Non,
    /// 使用系统代理
    Sys,
    /// 使用指定URL的代理
    Url(ProxyUrl),
}

impl SingleProxy {
    /// 根据代理配置创建对应的客户端并插入到映射中
    #[inline]
    fn insert_to(&self, clients: &mut HashMap<SingleProxy, Client>) {
        let builder = Client::builder()
            .https_only(true)
            .tcp_keepalive(TCP_KEEPALIVE.to_duration())
            .tcp_keepalive_interval(TCP_KEEPALIVE_INTERVAL.to_duration())
            .tcp_keepalive_retries(TCP_KEEPALIVE_RETRIES.to_count())
            .connect_timeout(Duration::from_secs(*SERVICE_TIMEOUT as _))
            .webpki_roots_only();
        let client = match self {
            SingleProxy::Non => builder.no_proxy().build().expect("创建无代理客户端失败"),
            SingleProxy::Sys => builder.build().expect("创建默认客户端失败"),
            SingleProxy::Url(url) => {
                builder.proxy(url.to_proxy()).build().expect("创建代理客户端失败")
            }
        };

        clients.insert(self.clone(), client);
    }
}

impl Serialize for SingleProxy {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        match self {
            Self::Non => serializer.serialize_str(NON_PROXY),
            Self::Sys => serializer.serialize_str(SYS_PROXY),
            Self::Url(url) => serializer.serialize_str(&url.to_string()),
        }
    }
}

impl<'de> Deserialize<'de> for SingleProxy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where D: serde::Deserializer<'de> {
        struct SingleProxyVisitor;

        impl serde::de::Visitor<'_> for SingleProxyVisitor {
            type Value = SingleProxy;

            fn expecting(&self, formatter: &mut core::fmt::Formatter) -> core::fmt::Result {
                formatter.write_str("a string representing 'non', 'sys', or a valid URL")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where E: serde::de::Error {
                match value {
                    NON_PROXY => Ok(Self::Value::Non),
                    SYS_PROXY => Ok(Self::Value::Sys),
                    url_str => Ok(Self::Value::Url(
                        ProxyUrl::from_str(url_str)
                            .map_err(|e| E::custom(format_args!("Invalid URL: {e}")))?,
                    )),
                }
            }
        }

        deserializer.deserialize_str(SingleProxyVisitor)
    }
}

impl core::fmt::Display for SingleProxy {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Non => f.write_str(NON_PROXY),
            Self::Sys => f.write_str(SYS_PROXY),
            Self::Url(url) => f.write_str(url),
        }
    }
}

impl FromStr for SingleProxy {
    type Err = reqwest::Error;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            NON_PROXY => Ok(Self::Non),
            SYS_PROXY => Ok(Self::Sys),
            url_str => Ok(Self::Url(ProxyUrl::from_str(url_str)?)),
        }
    }
}

/// 根据名称获取对应的客户端
///
/// 如果找不到指定名称的代理，返回通用客户端
#[inline]
pub fn get_client(name: &str) -> Client {
    // 先通过名称查找代理配置
    if let Some(proxy) = proxies().load().get(name) {
        // 然后通过代理配置查找客户端
        if let Some(client) = clients().load().get(proxy) {
            return client.clone();
        }
    }

    // 返回通用客户端
    get_general_client()
}

/// 获取通用客户端
#[inline]
pub fn get_general_client() -> Client { general_client().load_full() }

/// 根据可选的名称获取客户端
#[inline]
pub fn get_client_or_general(name: Option<&str>) -> Client {
    match name {
        Some(name) => get_client(name),
        None => get_general_client(),
    }
}

/// 获取请求图像客户端
#[inline]
pub fn get_fetch_image_client() -> Client {
    // fetch_image_client().load_full().unwrap_or_else(get_general_client)
    get_general_client()
}

/// 更新通用客户端引用
///
/// 前置条件：general_name 必须存在于 proxies 中，
/// 且对应的代理必须存在于 clients 中
#[inline]
fn set_general() {
    general_client().store(unsafe {
        clients()
            .load()
            .get(proxies().load().get(&*general_name().load_full()).unwrap_unchecked())
            .unwrap_unchecked()
            .clone()
    });
}

// 访问器函数
#[inline]
pub fn proxies() -> &'static ArcSwap<HashMap<String, SingleProxy>> { PROXIES.get() }

#[inline]
pub fn general_name() -> &'static ArcSwap<String> { GENERAL_NAME.get() }

// #[inline]
// pub fn fetch_image_name() -> &'static ArcSwapOption<String> { &FETCH_IMAGE_NAME }

#[inline]
fn clients() -> &'static ArcSwap<HashMap<SingleProxy, Client>> { CLIENTS.get() }

#[inline]
fn general_client() -> &'static ArcSwapAny<Client> { GENERAL_CLIENT.get() }

// #[inline]
// fn fetch_image_client() -> &'static ArcSwapAny<Option<Client>> { &FETCH_IMAGE_CLIENT }
