//! Signing in to solidtime with OAuth 2 (authorization code + PKCE) and keeping the session.

pub mod callback;
mod error;
mod instance;
mod login;
mod pkce;
mod session;
mod store;
mod token;

pub use error::AuthError;
pub use instance::{Instance, InstanceError};
pub use login::LoginRequest;
pub use session::Session;
pub use store::{FileTokenStore, KeychainTokenStore, MemoryTokenStore, TokenStore};
pub use token::TokenSet;
