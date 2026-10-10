//! Solidbar core: everything that talks to solidtime or reasons about time entries.
//!
//! This crate deliberately has no Tauri dependency so it can be tested in isolation.

pub mod api;
pub mod auth;
pub mod engine;
pub mod model;
pub mod ports;
pub mod time;

use std::time::Duration;

/// The HTTP client for solidtime: identifies Solidbar and gives up on unresponsive servers.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("Solidbar/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .expect("the macOS TLS backend is available")
}
