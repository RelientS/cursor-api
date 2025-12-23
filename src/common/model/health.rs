use serde::Serialize;

use crate::app::model::DateTime;
use super::ApiStatus;

#[derive(Serialize)]
pub struct HealthCheckResponse {
    pub status: ApiStatus,
    pub service: ServiceInfo,
    pub runtime: RuntimeStats,
    pub system: Option<SystemStats>,
    pub capabilities: Capabilities,
}

#[derive(Serialize)]
pub struct ServiceInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub is_debug: bool,
    pub build: BuildInfo,
}

#[derive(Serialize)]
pub struct BuildInfo {
    // pub commit: Option<&'static str>,
    #[cfg(feature = "__preview")]
    pub version: u32,
    pub timestamp: &'static str,
    pub is_debug: bool,
    pub is_prerelease: bool,
}

#[derive(Serialize)]
pub struct RuntimeStats {
    pub started_at: DateTime,
    pub uptime_seconds: i64,
    pub requests: RequestStats,
}

#[derive(Serialize)]
pub struct RequestStats {
    pub total: u64,
    pub active: u64,
    pub errors: u64,
}

#[derive(Serialize)]
pub struct SystemStats {
    pub memory: MemoryInfo,
    pub cpu: CpuInfo,
}

#[derive(Serialize)]
pub struct MemoryInfo {
    pub used_bytes: u64,
    pub used_percentage: f32,
    pub available_bytes: u64,
}

#[derive(Serialize)]
pub struct CpuInfo {
    pub usage_percentage: f32,
    pub load_average: [f64; 3], // 1min, 5min, 15min
}

#[derive(Serialize)]
pub struct Capabilities {
    pub models: alloc::sync::Arc<Vec<&'static str>>,
    pub endpoints: &'static [&'static str],
    pub features: &'static [&'static str],
}
