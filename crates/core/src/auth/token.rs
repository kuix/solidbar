//! The OAuth token endpoint (Laravel Passport): code exchange and refresh.

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

use super::{AuthError, Instance};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Timestamp,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

#[derive(Deserialize)]
struct OAuthErrorBody {
    error: String,
    error_description: Option<String>,
}

pub async fn exchange_code(
    http: &reqwest::Client,
    instance: &Instance,
    code: &str,
    verifier: &str,
    now: Timestamp,
) -> Result<TokenSet, AuthError> {
    let redirect_uri = instance.redirect_uri();
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", instance.client_id()),
        ("redirect_uri", redirect_uri.as_str()),
        ("code_verifier", verifier),
        ("code", code),
    ];
    request(http, instance, &form, None, now).await
}

pub async fn refresh(
    http: &reqwest::Client,
    instance: &Instance,
    refresh_token: &str,
    now: Timestamp,
) -> Result<TokenSet, AuthError> {
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", instance.client_id()),
        ("refresh_token", refresh_token),
    ];
    request(http, instance, &form, Some(refresh_token), now).await
}

async fn request(
    http: &reqwest::Client,
    instance: &Instance,
    form: &[(&str, &str)],
    previous_refresh_token: Option<&str>,
    now: Timestamp,
) -> Result<TokenSet, AuthError> {
    let response = http
        .post(instance.endpoint("oauth/token"))
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await?;
    let status = response.status();
    if status.is_success() {
        let body: TokenResponse = response.json().await?;
        let refresh_token = body
            .refresh_token
            .or_else(|| previous_refresh_token.map(str::to_owned))
            .ok_or_else(|| AuthError::Protocol("no refresh token in the response".to_owned()))?;
        return Ok(TokenSet {
            access_token: body.access_token,
            refresh_token,
            expires_at: now + SignedDuration::from_secs(body.expires_in),
        });
    }
    match response.json::<OAuthErrorBody>().await {
        // Expired, revoked or already used grant: the session is over.
        Ok(body) if body.error == "invalid_grant" => Err(AuthError::SessionExpired),
        Ok(body) => Err(AuthError::Denied(
            body.error_description.unwrap_or(body.error),
        )),
        Err(_) => Err(AuthError::Protocol(format!(
            "token endpoint returned {status}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidbar_shared::instance::{InstanceSettings, RedirectMode};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn instance(server: &MockServer) -> Instance {
        Instance::new(&InstanceSettings {
            url: server.uri(),
            client_id: "client-1".to_owned(),
            redirect: RedirectMode::Loopback { port: 47_821 },
        })
        .unwrap()
    }

    fn now() -> Timestamp {
        "2026-10-09T10:00:00Z".parse().unwrap()
    }

    #[tokio::test]
    async fn exchanges_the_code_with_the_verifier() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code_verifier=verifier-1"))
            .and(body_string_contains("code=code-1"))
            .and(body_string_contains(
                "redirect_uri=http%3A%2F%2F127.0.0.1%3A47821%2Fcallback",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "token_type": "Bearer",
                "expires_in": 3600,
                "access_token": "access-1",
                "refresh_token": "refresh-1",
            })))
            .expect(1)
            .mount(&server)
            .await;

        let tokens = exchange_code(
            &reqwest::Client::new(),
            &instance(&server),
            "code-1",
            "verifier-1",
            now(),
        )
        .await
        .unwrap();
        assert_eq!(tokens.access_token, "access-1");
        assert_eq!(tokens.refresh_token, "refresh-1");
        assert_eq!(tokens.expires_at, "2026-10-09T11:00:00Z".parse().unwrap());
    }

    #[tokio::test]
    async fn keeps_the_old_refresh_token_when_none_is_returned() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "expires_in": 60, "access_token": "access-2",
            })))
            .mount(&server)
            .await;
        let tokens = refresh(
            &reqwest::Client::new(),
            &instance(&server),
            "refresh-1",
            now(),
        )
        .await
        .unwrap();
        assert_eq!(tokens.refresh_token, "refresh-1");
    }

    #[tokio::test]
    async fn invalid_grant_ends_the_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "The refresh token is invalid.",
            })))
            .mount(&server)
            .await;
        let result = refresh(&reqwest::Client::new(), &instance(&server), "old", now()).await;
        assert_eq!(result, Err(AuthError::SessionExpired));
    }

    #[tokio::test]
    async fn other_oauth_errors_are_shown_to_the_person() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": "invalid_client",
                "error_description": "Client authentication failed",
            })))
            .mount(&server)
            .await;
        let result =
            exchange_code(&reqwest::Client::new(), &instance(&server), "c", "v", now()).await;
        assert_eq!(
            result,
            Err(AuthError::Denied("Client authentication failed".to_owned()))
        );
    }
}
