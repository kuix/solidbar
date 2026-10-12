//! Types and pure logic shared between the native backend and the wasm UI.
//!
//! Everything here must compile for both `aarch64-apple-darwin` and `wasm32-unknown-unknown`.

pub mod agents;
pub mod clock;
pub mod duration;
pub mod fuzzy;
pub mod group;
pub mod instance;
pub mod ipc;
pub mod settings;
pub mod shortcuts;
pub mod update;
pub mod view;

pub const APP_NAME: &str = "Solidbar";
