//! `solidbar mcp`: MCP over stdio, for agents that start their servers themselves, like
//! Claude Desktop. Each line on stdin is a JSON-RPC message; it goes to the running app's
//! server with the address and key from Solidbar's settings (read again for every message, so
//! a new key or port needs nothing from the agent), and the answer comes back on stdout.
//! When Solidbar isn't running, the bridge starts it.

use std::path::Path;
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;
use solidbar_shared::agents::server_url;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use super::protocol::{self, UNAVAILABLE};
use crate::engine::files::Files;

/// How long to wait for Solidbar to start and answer.
const LAUNCH_WAIT: Duration = Duration::from_secs(20);
const RETRY_EVERY: Duration = Duration::from_millis(250);

/// Relays stdin to the server in `data_dir`'s settings until stdin closes. `launch` starts
/// Solidbar; it's called at most once.
pub async fn run(
    data_dir: &Path,
    input: impl AsyncRead + Unpin,
    mut output: impl AsyncWrite + Unpin,
    launch: impl Fn(),
) -> std::io::Result<()> {
    let files = Files::new(data_dir);
    let http = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("the macOS TLS backend is available");
    let mut launch = Some(launch);
    let mut lines = BufReader::new(input).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let answers = match serde_json::from_str::<Value>(&line) {
            Err(_) => vec![protocol::error(
                Value::Null,
                protocol::PARSE_ERROR,
                "That isn't JSON",
            )],
            Ok(message) => match relay(&http, &files, &line, &mut launch).await {
                Ok(Some(answer)) => vec![answer],
                Ok(None) => Vec::new(),
                Err(problem) => protocol::request_ids(&message)
                    .into_iter()
                    .map(|id| protocol::error(id, UNAVAILABLE, &problem))
                    .collect(),
            },
        };
        for answer in answers {
            // One message per line: serialized compactly, JSON has no raw newlines.
            output.write_all(answer.to_string().as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
    Ok(())
}

async fn relay(
    http: &reqwest::Client,
    files: &Files,
    line: &str,
    launch: &mut Option<impl Fn()>,
) -> Result<Option<Value>, String> {
    let access = files.load_settings().agents;
    let (true, Some(port), Some(key)) = (access.enabled, access.port, access.key) else {
        return Err(
            "AI agents are turned off in Solidbar; turn them on in Solidbar's Settings › AI agents"
                .to_owned(),
        );
    };
    let deadline = Instant::now() + LAUNCH_WAIT;
    loop {
        let sent = http
            .post(server_url(port))
            .header(AUTHORIZATION, format!("Bearer {key}"))
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .body(line.to_owned())
            .send()
            .await;
        match sent {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.map_err(|error| error.to_string())?;
                return match status {
                    StatusCode::ACCEPTED => Ok(None),
                    status if status.is_success() || status == StatusCode::BAD_REQUEST => {
                        serde_json::from_str(&body)
                            .map(Some)
                            .map_err(|_| format!("Solidbar's answer wasn't JSON: {body}"))
                    }
                    status => Err(format!("Solidbar answered {status}: {body}")),
                };
            }
            Err(error) if error.is_connect() && Instant::now() < deadline => {
                if let Some(launch) = launch.take() {
                    launch();
                }
                tokio::time::sleep(RETRY_EVERY).await;
            }
            Err(error) if error.is_connect() => {
                return Err(
                    "Solidbar isn't running and didn't start; open Solidbar and try again"
                        .to_owned(),
                );
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}
