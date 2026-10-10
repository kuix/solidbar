//! A signed-in session: hands out access tokens and refreshes them, one refresh at a time.

use std::sync::Arc;

use jiff::SignedDuration;
use tokio::sync::{Mutex, MutexGuard};

use super::{AuthError, Instance, TokenSet, TokenStore, token};
use crate::ports::Clock;

/// Refresh this long before the access token expires, so in-flight requests don't race it.
pub const REFRESH_MARGIN: SignedDuration = SignedDuration::from_secs(60);

pub struct Session {
    http: reqwest::Client,
    instance: Instance,
    store: Arc<dyn TokenStore>,
    clock: Arc<dyn Clock>,
    /// Held across the refresh request, so concurrent callers share one refresh. Passport
    /// rotates refresh tokens, so a second refresh with the old token would end the session.
    tokens: Mutex<Option<TokenSet>>,
}

impl Session {
    /// The session saved by a previous launch, if any.
    pub fn restore(
        http: reqwest::Client,
        instance: Instance,
        store: Arc<dyn TokenStore>,
        clock: Arc<dyn Clock>,
    ) -> Result<Option<Self>, AuthError> {
        let Some(tokens) = store.load(&instance.account())? else {
            return Ok(None);
        };
        Ok(Some(Self::with_tokens(
            http, instance, store, clock, tokens,
        )))
    }

    /// A new session from a completed sign-in; persists the tokens.
    pub fn start(
        http: reqwest::Client,
        instance: Instance,
        store: Arc<dyn TokenStore>,
        clock: Arc<dyn Clock>,
        tokens: TokenSet,
    ) -> Result<Self, AuthError> {
        store.save(&instance.account(), &tokens)?;
        Ok(Self::with_tokens(http, instance, store, clock, tokens))
    }

    fn with_tokens(
        http: reqwest::Client,
        instance: Instance,
        store: Arc<dyn TokenStore>,
        clock: Arc<dyn Clock>,
        tokens: TokenSet,
    ) -> Self {
        Self {
            http,
            instance,
            store,
            clock,
            tokens: Mutex::new(Some(tokens)),
        }
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// A usable access token, refreshed first when it is about to expire.
    pub async fn access_token(&self) -> Result<String, AuthError> {
        let mut tokens = self.tokens.lock().await;
        let current = tokens.as_ref().ok_or(AuthError::SignedOut)?;
        if current.expires_at - REFRESH_MARGIN > self.clock.now() {
            return Ok(current.access_token.clone());
        }
        self.refresh_locked(&mut tokens).await
    }

    /// Called after the API rejected `rejected`. Refreshes, unless another request already
    /// replaced that token in the meantime.
    pub async fn refresh_after_rejection(&self, rejected: &str) -> Result<String, AuthError> {
        let mut tokens = self.tokens.lock().await;
        let current = tokens.as_ref().ok_or(AuthError::SignedOut)?;
        if current.access_token != rejected {
            return Ok(current.access_token.clone());
        }
        self.refresh_locked(&mut tokens).await
    }

    pub async fn sign_out(&self) -> Result<(), AuthError> {
        *self.tokens.lock().await = None;
        self.store.clear(&self.instance.account())
    }

    async fn refresh_locked(
        &self,
        tokens: &mut MutexGuard<'_, Option<TokenSet>>,
    ) -> Result<String, AuthError> {
        let refresh_token = tokens
            .as_ref()
            .ok_or(AuthError::SignedOut)?
            .refresh_token
            .clone();
        let account = self.instance.account();
        match token::refresh(&self.http, &self.instance, &refresh_token, self.clock.now()).await {
            Ok(fresh) => {
                // Persist before anyone else can use the rotated token. If the keychain fails,
                // keep working with the in-memory tokens; the next launch will ask to sign in.
                if let Err(error) = self.store.save(&account, &fresh) {
                    log::warn!("could not persist refreshed tokens: {error}");
                }
                let access = fresh.access_token.clone();
                **tokens = Some(fresh);
                Ok(access)
            }
            Err(AuthError::SessionExpired) => {
                **tokens = None;
                if let Err(error) = self.store.clear(&account) {
                    log::warn!("could not clear the expired session: {error}");
                }
                Err(AuthError::SessionExpired)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryTokenStore;
    use crate::ports::testing::FixedClock;
    use solidbar_shared::instance::{InstanceSettings, RedirectMode};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    struct Fixture {
        session: Arc<Session>,
        store: Arc<MemoryTokenStore>,
        clock: Arc<FixedClock>,
        account: String,
    }

    async fn fixture(server: &MockServer, expires_at: &str) -> Fixture {
        let instance = Instance::new(&InstanceSettings {
            url: server.uri(),
            client_id: "client-1".to_owned(),
            redirect: RedirectMode::DeepLink,
        })
        .unwrap();
        let store = Arc::new(MemoryTokenStore::default());
        let clock = Arc::new(FixedClock::at("2026-10-09T10:00:00Z"));
        let tokens = TokenSet {
            access_token: "stale".to_owned(),
            refresh_token: "refresh-1".to_owned(),
            expires_at: expires_at.parse().unwrap(),
        };
        let account = instance.account();
        let session = Session::start(
            reqwest::Client::new(),
            instance,
            store.clone(),
            clock.clone(),
            tokens,
        )
        .unwrap();
        Fixture {
            session: Arc::new(session),
            store,
            clock,
            account,
        }
    }

    async fn mount_refresh(server: &MockServer, expected_calls: u64) {
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("refresh_token=refresh-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "expires_in": 3600, "access_token": "fresh", "refresh_token": "refresh-2",
            })))
            .expect(expected_calls)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn uses_the_cached_token_while_it_is_valid() {
        let server = MockServer::start().await;
        mount_refresh(&server, 0).await;
        let fixture = fixture(&server, "2026-10-09T11:00:00Z").await;
        assert_eq!(fixture.session.access_token().await.unwrap(), "stale");
    }

    #[tokio::test]
    async fn refreshes_shortly_before_expiry_and_persists_the_rotated_tokens() {
        let server = MockServer::start().await;
        mount_refresh(&server, 1).await;
        let fixture = fixture(&server, "2026-10-09T11:00:00Z").await;
        fixture
            .clock
            .advance(SignedDuration::from_mins(59) + SignedDuration::from_secs(1));

        assert_eq!(fixture.session.access_token().await.unwrap(), "fresh");
        let saved = fixture.store.load(&fixture.account).unwrap().unwrap();
        assert_eq!(saved.refresh_token, "refresh-2");
        // The new token is cached.
        assert_eq!(fixture.session.access_token().await.unwrap(), "fresh");
    }

    #[tokio::test]
    async fn concurrent_rejections_share_a_single_refresh() {
        let server = MockServer::start().await;
        mount_refresh(&server, 1).await;
        let fixture = fixture(&server, "2026-10-09T11:00:00Z").await;

        let attempts: Vec<_> = (0..8)
            .map(|_| {
                let session = fixture.session.clone();
                tokio::spawn(async move { session.refresh_after_rejection("stale").await })
            })
            .collect();
        for attempt in attempts {
            assert_eq!(attempt.await.unwrap().unwrap(), "fresh");
        }
    }

    #[tokio::test]
    async fn an_invalid_grant_signs_out() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({ "error": "invalid_grant" })),
            )
            .mount(&server)
            .await;
        let fixture = fixture(&server, "2026-10-09T10:00:30Z").await;

        assert_eq!(
            fixture.session.access_token().await,
            Err(AuthError::SessionExpired)
        );
        assert_eq!(fixture.store.load(&fixture.account).unwrap(), None);
        assert_eq!(
            fixture.session.access_token().await,
            Err(AuthError::SignedOut)
        );
    }

    #[tokio::test]
    async fn network_errors_keep_the_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let fixture = fixture(&server, "2026-10-09T10:00:30Z").await;
        assert!(matches!(
            fixture.session.access_token().await,
            Err(AuthError::Protocol(_))
        ));
        assert!(fixture.store.load(&fixture.account).unwrap().is_some());
    }
}
