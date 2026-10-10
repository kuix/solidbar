//! Which solidtime instance to sign in to, and how the OAuth redirect comes back.

use serde::{Deserialize, Serialize};

/// The hosted solidtime service.
pub const CLOUD_URL: &str = "https://app.solidtime.io";
/// The public OAuth client of the official desktop app, registered on app.solidtime.io with
/// the `solidtime://oauth/callback` redirect.
pub const CLOUD_CLIENT_ID: &str = "9c994748-c593-4a6d-951b-6849c829bc4e";
pub const DEEP_LINK_REDIRECT: &str = "solidtime://oauth/callback";
pub const DEFAULT_LOOPBACK_PORT: u16 = 47_821;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RedirectMode {
    /// `solidtime://oauth/callback`, handled by macOS URL scheme routing. Needed for
    /// app.solidtime.io's public client; may collide with the official desktop app.
    DeepLink,
    /// `http://127.0.0.1:<port>/callback`, served by Solidbar while signing in. Needs an OAuth
    /// client registered with exactly this redirect URI.
    Loopback { port: u16 },
}

impl RedirectMode {
    pub fn redirect_uri(self) -> String {
        match self {
            Self::DeepLink => DEEP_LINK_REDIRECT.to_owned(),
            Self::Loopback { port } => format!("http://127.0.0.1:{port}/callback"),
        }
    }
}

/// What the person enters in the instance settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceSettings {
    pub url: String,
    pub client_id: String,
    pub redirect: RedirectMode,
}

impl InstanceSettings {
    pub fn cloud() -> Self {
        Self {
            url: CLOUD_URL.to_owned(),
            client_id: CLOUD_CLIENT_ID.to_owned(),
            redirect: RedirectMode::DeepLink,
        }
    }
}

impl Default for InstanceSettings {
    fn default() -> Self {
        Self::cloud()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_uris() {
        assert_eq!(
            RedirectMode::DeepLink.redirect_uri(),
            "solidtime://oauth/callback"
        );
        assert_eq!(
            RedirectMode::Loopback { port: 47_821 }.redirect_uri(),
            "http://127.0.0.1:47821/callback"
        );
    }
}
