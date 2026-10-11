//! Receiving the OAuth redirect: parsing it, and the loopback listener that serves
//! `http://127.0.0.1:<port>/callback`.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use super::AuthError;

/// Extracts the authorization code from the redirect, checking it answers our request.
pub fn code_from_redirect(redirect: &Url, expected_state: &str) -> Result<String, AuthError> {
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = None;
    for (key, value) in redirect.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => description = Some(value.into_owned()),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Err(AuthError::Denied(description.unwrap_or(error)));
    }
    if state.as_deref() != Some(expected_state) {
        return Err(AuthError::StateMismatch);
    }
    code.ok_or_else(|| AuthError::Protocol("the redirect carried no authorization code".to_owned()))
}

pub struct LoopbackListener {
    listener: TcpListener,
}

impl LoopbackListener {
    /// Listens on a port the system picks, so it's always free: authorization servers accept
    /// a loopback redirect on any port (RFC 8252, section 7.3).
    pub async fn bind() -> Result<Self, AuthError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|error| AuthError::Network(error.to_string()))?;
        Ok(Self { listener })
    }

    pub fn port(&self) -> u16 {
        self.listener
            .local_addr()
            .map(|address| address.port())
            .unwrap_or_default()
    }

    /// Serves requests until the browser arrives at `/callback`, then returns the code.
    /// Wrap in a timeout; dropping the future closes the port.
    pub async fn wait_for_code(self, expected_state: &str) -> Result<String, AuthError> {
        loop {
            let (mut stream, _) = self
                .listener
                .accept()
                .await
                .map_err(|error| AuthError::Network(error.to_string()))?;
            let Some(target) = read_request_target(&mut stream).await else {
                continue;
            };
            if !target.starts_with("/callback") {
                respond(&mut stream, "404 Not Found", "Not found").await;
                continue;
            }
            let redirect = Url::parse(&format!("http://127.0.0.1{target}"))
                .map_err(|error| AuthError::Protocol(error.to_string()))?;
            let result = code_from_redirect(&redirect, expected_state);
            let page = match &result {
                Ok(_) => SUCCESS_PAGE,
                Err(_) => FAILURE_PAGE,
            };
            respond(&mut stream, "200 OK", page).await;
            return result;
        }
    }
}

const SUCCESS_PAGE: &str = "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Solidbar</title>\
<body style=\"font:15px -apple-system,sans-serif;margin:3em\"><h1>You're signed in</h1>\
<p>You can close this tab and return to Solidbar in the menu bar.</p></body></html>";

const FAILURE_PAGE: &str = "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Solidbar</title>\
<body style=\"font:15px -apple-system,sans-serif;margin:3em\"><h1>Sign-in didn't complete</h1>\
<p>Return to Solidbar in the menu bar to see what went wrong and try again.</p></body></html>";

/// Reads the request head and returns the request target, e.g. `/callback?code=…`.
async fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::with_capacity(1024);
    let mut buffer = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 || head.len() > 16 * 1024 {
            return None;
        }
        head.extend_from_slice(&buffer[..read]);
    }
    let head = String::from_utf8_lossy(&head);
    let mut request_line = head.lines().next()?.split_whitespace();
    match (request_line.next(), request_line.next()) {
        (Some("GET"), Some(target)) => Some(target.to_owned()),
        _ => None,
    }
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
Cache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn accepts_a_matching_state() {
        let redirect = url("solidtime://oauth/callback?code=abc&state=s1");
        assert_eq!(code_from_redirect(&redirect, "s1"), Ok("abc".to_owned()));
    }

    #[test]
    fn rejects_a_foreign_state() {
        let redirect = url("solidtime://oauth/callback?code=abc&state=other");
        assert_eq!(
            code_from_redirect(&redirect, "s1"),
            Err(AuthError::StateMismatch)
        );
    }

    #[test]
    fn surfaces_errors_from_the_authorization_server() {
        let redirect = url(
            "solidtime://oauth/callback?error=access_denied&error_description=The+user+denied+the+request&state=s1",
        );
        assert_eq!(
            code_from_redirect(&redirect, "s1"),
            Err(AuthError::Denied("The user denied the request".to_owned()))
        );
    }

    #[tokio::test]
    async fn loopback_serves_the_callback_and_ignores_other_requests() {
        let listener = LoopbackListener::bind().await.unwrap();
        let port = listener.port();
        assert_ne!(port, 0);
        let waiting = tokio::spawn(async move { listener.wait_for_code("s1").await });

        let client = reqwest::Client::new();
        let favicon = client
            .get(format!("http://127.0.0.1:{port}/favicon.ico"))
            .send()
            .await
            .unwrap();
        assert_eq!(favicon.status(), 404);
        let page = client
            .get(format!(
                "http://127.0.0.1:{port}/callback?code=abc&state=s1"
            ))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(page.contains("You're signed in"));
        assert_eq!(waiting.await.unwrap(), Ok("abc".to_owned()));
    }

    #[tokio::test]
    async fn every_sign_in_gets_a_free_port_of_its_own() {
        let first = LoopbackListener::bind().await.unwrap();
        let second = LoopbackListener::bind().await.unwrap();
        assert_ne!(first.port(), second.port());
    }
}
