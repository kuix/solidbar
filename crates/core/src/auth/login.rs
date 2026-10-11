//! One sign-in attempt: the authorize URL to open, and finishing with the returned code.

use std::sync::Arc;

use solidbar_shared::instance::Redirect;
use url::Url;

use super::pkce::{Pkce, random_token};
use super::{AuthError, Instance, Session, TokenStore, callback, probe, token};
use crate::ports::Clock;

pub struct LoginRequest {
    instance: Instance,
    pkce: Pkce,
    state: String,
}

impl LoginRequest {
    pub fn new(instance: Instance) -> Self {
        Self {
            instance,
            pkce: Pkce::generate(),
            state: random_token(),
        }
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn state(&self) -> &str {
        &self.state
    }

    /// How the browser can return: the loopback address on `port` if the OAuth client allows
    /// it, otherwise the solidtime:// link.
    pub async fn choose_redirect(&self, port: u16) -> Result<Redirect, AuthError> {
        probe::choose_redirect(self, port).await
    }

    /// The page to open in the browser.
    pub fn authorize_url(&self, redirect: Redirect) -> Url {
        let mut url = self.instance.endpoint("oauth/authorize");
        url.query_pairs_mut()
            .append_pair("client_id", self.instance.client_id())
            .append_pair("redirect_uri", &redirect.uri())
            .append_pair("response_type", "code")
            .append_pair("state", &self.state)
            .append_pair("code_challenge", &self.pkce.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("scope", "*");
        url
    }

    /// Checks a `solidtime://oauth/callback?…` deep link against this request.
    pub fn code_from_redirect(&self, redirect: &Url) -> Result<String, AuthError> {
        callback::code_from_redirect(redirect, &self.state)
    }

    /// Exchanges the code; `redirect` is the one the authorize URL carried.
    pub async fn finish(
        self,
        http: reqwest::Client,
        redirect: Redirect,
        code: &str,
        store: Arc<dyn TokenStore>,
        clock: Arc<dyn Clock>,
    ) -> Result<Session, AuthError> {
        let tokens = token::exchange_code(
            &http,
            &self.instance,
            &redirect.uri(),
            code,
            &self.pkce.verifier,
            clock.now(),
        )
        .await?;
        Session::start(http, self.instance, store, clock, tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidbar_shared::instance::InstanceSettings;

    #[test]
    fn authorize_url_carries_pkce_and_state() {
        let request = LoginRequest::new(Instance::new(&InstanceSettings::cloud()).unwrap());
        let url = request.authorize_url(Redirect::DeepLink);
        assert_eq!(url.path(), "/oauth/authorize");
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], "9c994748-c593-4a6d-951b-6849c829bc4e");
        assert_eq!(query["redirect_uri"], "solidtime://oauth/callback");
        assert_eq!(query["response_type"], "code");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["state"], request.state());
        assert_eq!(query["code_challenge"].len(), 43);
    }
}
