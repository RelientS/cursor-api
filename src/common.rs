pub mod client;
pub mod model;
pub mod time;
// pub mod traits;
pub mod utils;

pub mod build {
    include!(concat!(env!("OUT_DIR"), "/build_info.rs"));
}

pub mod platform {
    include!(concat!(env!("OUT_DIR"), "/platform_info.rs"));
}
