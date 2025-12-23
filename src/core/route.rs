mod health;
mod logs;
mod page;
mod proxies;
mod token;
mod tokens;
mod utils;

pub use health::{handle_health, init_endpoints};
pub use logs::{handle_get_logs, handle_get_logs_tokens};
pub use page::{handle_env_example, handle_license, handle_readme};
pub use proxies::{
    handle_add_proxy, handle_delete_proxies, handle_get_proxies, handle_set_general_proxy,
    handle_set_proxies,
};
pub use token::{handle_build_key, handle_get_config_version, handle_get_token_profile};
pub use tokens::{
    handle_add_tokens, handle_delete_tokens, handle_get_tokens, handle_merge_tokens,
    handle_refresh_tokens, handle_set_tokens, handle_set_tokens_alias, handle_set_tokens_proxy,
    handle_set_tokens_status, handle_set_tokens_timezone, handle_update_tokens_config_version,
    handle_update_tokens_profile,
};
pub use utils::{
    handle_gen_checksum, handle_gen_hash, handle_gen_uuid, handle_get_checksum_header,
    handle_ntp_sync_once,
};
