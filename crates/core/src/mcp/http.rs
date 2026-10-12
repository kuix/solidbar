//! MCP's Streamable HTTP transport, as much of it as a tools-only server needs: `POST /mcp`
//! with a JSON-RPC message, answered with JSON. There are no sessions and no server-sent
//! events, so `GET` and `DELETE` are refused, as the specification allows.
//!
//! Only this Mac can connect (loopback interface), and every request needs the key. Browsers
//! are kept out on top of that: a page can't name the key, and the `Host` and `Origin` checks
//! stop DNS rebinding.

use std::convert::Infallible;
use std::net::Ipv4Addr;
use std::sync::Arc;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::protocol;
use crate::engine::Engine;

/// Tool calls are small; a hundred entries are a few kilobytes.
const MAX_BODY: usize = 1024 * 1024;

pub struct Server {
    pub port: u16,
    stop: Option<oneshot::Sender<()>>,
    accepting: Option<JoinHandle<()>>,
}

impl Server {
    /// Starts answering on `127.0.0.1:port`.
    pub async fn start(engine: Arc<Engine>, port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
        let port = listener.local_addr()?.port();
        let (stop, mut stopped) = oneshot::channel::<()>();
        let accepting = tokio::spawn(async move {
            loop {
                let stream = tokio::select! {
                    _ = &mut stopped => break,
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) => stream,
                        Err(error) => {
                            log::warn!("the MCP server couldn't accept a connection: {error}");
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            continue;
                        }
                    },
                };
                let engine = engine.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request| {
                        let engine = engine.clone();
                        async move { Ok::<_, Infallible>(respond(&engine, port, request).await) }
                    });
                    if let Err(error) = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await
                    {
                        log::debug!("MCP connection ended: {error}");
                    }
                });
            }
        });
        Ok(Self {
            port,
            stop: Some(stop),
            accepting: Some(accepting),
        })
    }

    /// Stops listening; the port is free again when this returns. Connections still open
    /// get turned away, as every request checks that access is on.
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(accepting) = self.accepting.take() {
            let _ = accepting.await;
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

async fn respond(engine: &Engine, port: u16, request: Request<Incoming>) -> Response<Full<Bytes>> {
    if request.uri().path() != "/mcp" {
        return text(
            StatusCode::NOT_FOUND,
            "Solidbar's MCP server answers at /mcp",
        );
    }
    let headers = request.headers();
    if !host_allowed(headers, port) || !origin_allowed(headers) {
        return text(StatusCode::FORBIDDEN, "Only apps on this Mac may connect");
    }
    let access = engine.agent_access();
    if !access.enabled {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "AI agents are turned off in Solidbar's Settings",
        );
    }
    if !authorized(headers, access.key.as_deref()) {
        let mut response = text(
            StatusCode::UNAUTHORIZED,
            "Missing or wrong key; copy it from Solidbar's Settings › AI agents",
        );
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Bearer realm=\"Solidbar\""),
        );
        return response;
    }
    if request.method() != Method::POST {
        let mut response = text(
            StatusCode::METHOD_NOT_ALLOWED,
            "Send MCP messages with POST",
        );
        response
            .headers_mut()
            .insert(header::ALLOW, HeaderValue::from_static("POST"));
        return response;
    }
    let body = match Limited::new(request.into_body(), MAX_BODY).collect().await {
        Ok(body) => body.to_bytes(),
        Err(_) => return text(StatusCode::PAYLOAD_TOO_LARGE, "That message is too long"),
    };
    let message: Value = match serde_json::from_slice(&body) {
        Ok(message) => message,
        Err(_) => {
            let answer = protocol::error(Value::Null, protocol::PARSE_ERROR, "That isn't JSON");
            return json(StatusCode::BAD_REQUEST, &answer);
        }
    };
    match protocol::handle(engine, message).await {
        Some(answer) => json(StatusCode::OK, &answer),
        None => empty(StatusCode::ACCEPTED),
    }
}

/// `Host` names this server: a DNS-rebound page would send its own domain.
fn host_allowed(headers: &HeaderMap, port: u16) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
    else {
        return false;
    };
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

/// Browsers send `Origin`; agents don't. One from this Mac is fine, any other is a web page.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Some(url) = origin
        .to_str()
        .ok()
        .and_then(|origin| url::Url::parse(origin).ok())
    else {
        return false;
    };
    matches!(url.scheme(), "http" | "https")
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

fn authorized(headers: &HeaderMap, key: Option<&str>) -> bool {
    let (Some(key), Some(sent)) = (key, headers.get(header::AUTHORIZATION)) else {
        return false;
    };
    let expected = format!("Bearer {key}");
    constant_time_eq(sent.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

fn text(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from_static(body.as_bytes())));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn json(status: StatusCode, body: &Value) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(body.to_string())));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

fn empty(status: StatusCode) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(name.clone(), HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn only_this_servers_own_host_names() {
        assert!(host_allowed(
            &headers(&[(header::HOST, "127.0.0.1:5000")]),
            5000
        ));
        assert!(host_allowed(
            &headers(&[(header::HOST, "localhost:5000")]),
            5000
        ));
        assert!(!host_allowed(
            &headers(&[(header::HOST, "evil.example:5000")]),
            5000
        ));
        assert!(!host_allowed(
            &headers(&[(header::HOST, "127.0.0.1:5001")]),
            5000
        ));
        assert!(!host_allowed(&HeaderMap::new(), 5000));
    }

    #[test]
    fn web_pages_are_turned_away() {
        assert!(origin_allowed(&HeaderMap::new()));
        assert!(origin_allowed(&headers(&[(
            header::ORIGIN,
            "http://localhost:6274"
        )])));
        assert!(!origin_allowed(&headers(&[(
            header::ORIGIN,
            "https://evil.example"
        )])));
        assert!(!origin_allowed(&headers(&[(header::ORIGIN, "null")])));
    }

    #[test]
    fn the_key_has_to_match_exactly() {
        let sent = headers(&[(header::AUTHORIZATION, "Bearer abc")]);
        assert!(authorized(&sent, Some("abc")));
        assert!(!authorized(&sent, Some("abd")));
        assert!(!authorized(&sent, Some("ab")));
        assert!(!authorized(&sent, None));
        assert!(!authorized(&HeaderMap::new(), Some("abc")));
    }
}
