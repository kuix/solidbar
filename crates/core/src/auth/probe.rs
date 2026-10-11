//! Finding out, before the browser opens, which redirect the OAuth client allows.
//!
//! The authorization server checks the client and its redirect URI before it asks the person
//! to sign in, so an anonymous request for the authorize page answers the question: being sent
//! on to the login page means yes, `invalid_client` means no (solidtime's OAuth server answers
//! a redirect URI the client doesn't allow with the same error as an unknown client). When a
//! server answers anything else, the sign-in goes ahead with the likelier choice.

use std::time::Duration;

use reqwest::StatusCode;
use solidbar_shared::instance::Redirect;
use url::Url;

use super::token::OAuthErrorBody;
use super::{AuthError, LoginRequest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Accepted,
    Rejected,
    Unclear,
}

pub(super) async fn choose_redirect(
    request: &LoginRequest,
    port: u16,
) -> Result<Redirect, AuthError> {
    let http = client();
    let loopback = Redirect::Loopback { port };
    let loopback_answer = ask(&http, request.authorize_url(loopback)).await?;
    if loopback_answer == Answer::Accepted {
        return Ok(loopback);
    }
    let deep_link_answer = ask(&http, request.authorize_url(Redirect::DeepLink)).await?;
    match (loopback_answer, deep_link_answer) {
        (Answer::Rejected, Answer::Rejected) => Err(AuthError::ClientRejected),
        (Answer::Unclear, Answer::Rejected) => Ok(loopback),
        // Also when neither answer is clear: app.solidtime.io doesn't tell anonymous visitors,
        // and its public client (the official desktop app's) only allows the link.
        _ => Ok(Redirect::DeepLink),
    }
}

async fn ask(http: &reqwest::Client, url: Url) -> Result<Answer, AuthError> {
    let response = http.get(url).send().await?;
    let status = response.status();
    if status.is_success() || status.is_redirection() {
        return Ok(Answer::Accepted);
    }
    if status == StatusCode::NOT_FOUND {
        return Err(AuthError::NoSignInPage);
    }
    let Ok(body) = response.json::<OAuthErrorBody>().await else {
        return Ok(Answer::Unclear);
    };
    match body.error.as_str() {
        "invalid_client" => Ok(Answer::Rejected),
        // Anything else the browser would show too, e.g. an unsupported scope.
        _ if status.is_client_error() => Err(AuthError::Denied(
            body.error_description.unwrap_or(body.error),
        )),
        _ => Ok(Answer::Unclear),
    }
}

/// Doesn't follow redirects: where the authorize page sends an anonymous visitor is the answer.
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("Solidbar/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(15))
        .build()
        .expect("the macOS TLS backend is available")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::Instance;
    use solidbar_shared::instance::{DEEP_LINK_REDIRECT, InstanceSettings};
    use wiremock::matchers::{method, path, query_param, query_param_contains};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PORT: u16 = 52_107;

    fn request(server: &MockServer) -> LoginRequest {
        LoginRequest::new(
            Instance::new(&InstanceSettings {
                url: server.uri(),
                client_id: "client-1".to_owned(),
            })
            .unwrap(),
        )
    }

    fn to_login() -> ResponseTemplate {
        ResponseTemplate::new(302).insert_header("location", "https://time.example.com/login")
    }

    fn invalid_client() -> ResponseTemplate {
        ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "invalid_client",
            "error_description": "Client authentication failed",
        }))
    }

    /// Answers the authorize page for the loopback and the deep link redirect.
    async fn server(loopback: ResponseTemplate, deep_link: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/oauth/authorize"))
            .and(query_param("client_id", "client-1"))
            .and(query_param_contains("redirect_uri", "http://127.0.0.1:"))
            .and(query_param("code_challenge_method", "S256"))
            .respond_with(loopback)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/oauth/authorize"))
            .and(query_param("redirect_uri", DEEP_LINK_REDIRECT))
            .respond_with(deep_link)
            .mount(&server)
            .await;
        server
    }

    async fn choose(server: &MockServer) -> Result<Redirect, AuthError> {
        choose_redirect(&request(server), PORT).await
    }

    #[tokio::test]
    async fn prefers_the_loopback_address_when_the_client_allows_it() {
        let server = server(to_login(), to_login()).await;
        assert_eq!(choose(&server).await, Ok(Redirect::Loopback { port: PORT }));
        // The answer for the loopback address was enough.
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn uses_the_link_when_the_client_only_allows_that() {
        let server = server(invalid_client(), to_login()).await;
        assert_eq!(choose(&server).await, Ok(Redirect::DeepLink));
    }

    #[tokio::test]
    async fn a_client_that_allows_neither_is_reported_before_the_browser_opens() {
        let server = server(invalid_client(), invalid_client()).await;
        assert_eq!(choose(&server).await, Err(AuthError::ClientRejected));
    }

    #[tokio::test]
    async fn without_a_clear_answer_the_link_is_tried_unless_it_was_refused() {
        // app.solidtime.io answers anonymous visitors with an empty 401.
        let server_a = server(ResponseTemplate::new(401), ResponseTemplate::new(401)).await;
        assert_eq!(choose(&server_a).await, Ok(Redirect::DeepLink));

        let server_b = server(ResponseTemplate::new(500), invalid_client()).await;
        assert_eq!(
            choose(&server_b).await,
            Ok(Redirect::Loopback { port: PORT })
        );
    }

    #[tokio::test]
    async fn other_problems_are_shown_right_away() {
        let missing = MockServer::start().await;
        assert_eq!(choose(&missing).await, Err(AuthError::NoSignInPage));

        let scope = server(
            ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_scope",
                "error_description": "The requested scope is invalid, unknown, or malformed",
            })),
            to_login(),
        )
        .await;
        assert_eq!(
            choose(&scope).await,
            Err(AuthError::Denied(
                "The requested scope is invalid, unknown, or malformed".to_owned()
            ))
        );
    }
}
