use std::{borrow::Cow, sync::Arc};

use super::{
    ApiStatus, DeleteResponseExpectation,
    proxy_pool::{Proxies, SingleProxy},
};
use serde::{Deserialize, Serialize};

type HashMap<K, V> = hashbrown::HashMap<K, V, ahash::RandomState>;
type HashSet<K> = hashbrown::HashSet<K, ahash::RandomState>;

// 代理信息响应
#[derive(Serialize)]
pub struct ProxyInfoResponse {
    pub status: ApiStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxies: Option<Arc<HashMap<String, SingleProxy>>>,
    pub proxies_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub general_proxy: Option<Arc<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Cow<'static, str>>,
}

// 更新代理配置请求
pub type ProxyUpdateRequest = Proxies;

// 添加代理请求
#[derive(Deserialize)]
pub struct ProxyAddRequest {
    pub proxies: HashMap<String, SingleProxy>,
}

// 删除代理请求
#[derive(Deserialize)]
pub struct ProxiesDeleteRequest {
    #[serde(default)]
    pub names: HashSet<String>,
    #[serde(default)]
    pub expectation: DeleteResponseExpectation,
}

// 删除代理响应
#[derive(Serialize)]
pub struct ProxiesDeleteResponse {
    pub status: ApiStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_proxies: Option<Arc<HashMap<String, SingleProxy>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_names: Option<Vec<String>>,
}

// 设置通用代理请求
#[derive(Deserialize)]
pub struct SetGeneralProxyRequest {
    pub name: String,
}
