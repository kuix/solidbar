//! Which solidtime instance to sign in to, and how the OAuth redirect comes back.

use serde::{Deserialize, Serialize};

/// The hosted solidtime service.
pub const CLOUD_URL: &str = "https://app.solidtime.io";
/// The public OAuth client of the official desktop app, registered on app.solidtime.io with
/// the `solidtime://oauth/callback` redirect.
pub const CLOUD_CLIENT_ID: &str = "9c994748-c593-4a6d-951b-6849c829bc4e";
pub const DEEP_LINK_REDIRECT: &str = "solidtime://oauth/callback";
/// The loopback redirect URI to register for Solidbar. Authorization servers ignore the port
/// of loopback redirects (RFC 8252, section 7.3), so Solidbar can listen on any free port.
pub const LOOPBACK_REDIRECT: &str = "http://127.0.0.1/callback";

/// How the browser returns to Solidbar after signing in. Worked out for every sign-in from the
/// redirect URIs the OAuth client allows; the person doesn't choose it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Redirect {
    /// `http://127.0.0.1:<port>/callback`, served by Solidbar while signing in, on a port the
    /// system picks. Preferred: it can't be taken over by another app.
    Loopback { port: u16 },
    /// `solidtime://oauth/callback`, handled by macOS URL scheme routing. Needed for
    /// app.solidtime.io's public client; may collide with the official desktop app.
    DeepLink,
}

impl Redirect {
    pub fn uri(self) -> String {
        match self {
            Self::Loopback { port } => format!("http://127.0.0.1:{port}/callback"),
            Self::DeepLink => DEEP_LINK_REDIRECT.to_owned(),
        }
    }
}

/// What the person enters to sign in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceSettings {
    pub url: String,
    pub client_id: String,
}

impl InstanceSettings {
    pub fn cloud() -> Self {
        Self {
            url: CLOUD_URL.to_owned(),
            client_id: CLOUD_CLIENT_ID.to_owned(),
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
        assert_eq!(Redirect::DeepLink.uri(), "solidtime://oauth/callback");
        assert_eq!(
            Redirect::Loopback { port: 52_107 }.uri(),
            "http://127.0.0.1:52107/callback"
        );
    }

    #[test]
    fn settings_saved_with_a_chosen_redirect_still_load() {
        let saved = r#"{"url":"https://time.example.com","client_id":"c1","redirect":{"mode":"loopback","port":47821}}"#;
        let settings: InstanceSettings = serde_json::from_str(saved).unwrap();
        assert_eq!(settings.url, "https://time.example.com");
        assert_eq!(settings.client_id, "c1");
    }
}
