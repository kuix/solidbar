//! The solidtime v1 REST API.

mod client;
mod error;
pub mod models;

pub use client::Api;
pub use error::ApiError;
