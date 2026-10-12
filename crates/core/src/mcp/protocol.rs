//! JSON-RPC 2.0 and the MCP lifecycle, for a server that offers tools and nothing else.

use serde_json::{Map, Value, json};
use solidbar_shared::agents::SERVER_NAME;

use super::tools;
use crate::engine::Engine;

/// The protocol revisions this server speaks, newest first. A client asking for one of them
/// gets it; any other gets the newest.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// Solidbar can't serve the request right now (turned off, not running).
pub const UNAVAILABLE: i64 = -32000;

const INSTRUCTIONS: &str = "Solidbar is the person's solidtime time tracker on this Mac. \
These tools work in their solidtime account, as them, in the organization chosen in Solidbar. \
Dates are YYYY-MM-DD and times HH:MM (24-hour), in the person's solidtime time zone; every \
result names the zone and today's date. Find project and task ids with list_projects. A new \
entry without start_time starts where the day's last entry ends (at the start of the workday on \
an empty day), so \"two hours on X on Monday\" needs only date, duration and project. Entries \
can't cross midnight. Check what's already tracked with list_time_entries before adding \
entries, so nothing is logged twice.";

/// Answers a message, or a batch of them; `None` when there's nothing to answer
/// (notifications, and responses to requests this server never makes).
pub async fn handle(engine: &Engine, message: Value) -> Option<Value> {
    match message {
        Value::Array(batch) => {
            let mut answers = Vec::new();
            for message in batch {
                answers.extend(handle_one(engine, message).await);
            }
            (!answers.is_empty()).then_some(Value::Array(answers))
        }
        message => handle_one(engine, message).await,
    }
}

async fn handle_one(engine: &Engine, message: Value) -> Option<Value> {
    let Value::Object(mut message) = message else {
        return Some(error(
            Value::Null,
            INVALID_REQUEST,
            "Not a JSON-RPC message",
        ));
    };
    let id = message.remove("id");
    let Some(method) = message
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        let is_response = message.contains_key("result") || message.contains_key("error");
        return match id {
            Some(id) if !is_response => Some(error(id, INVALID_REQUEST, "No method")),
            _ => None,
        };
    };
    // Notifications (no id), like `notifications/initialized`, need no answer.
    let id = id?;
    let params = message.remove("params").unwrap_or(Value::Null);
    let result = match method.as_str() {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(Value::Object(Map::new())),
        "tools/list" => Ok(tools::list()),
        "tools/call" => tools::call(engine, &params).await,
        _ => Err((METHOD_NOT_FOUND, format!("There's no method {method}"))),
    };
    Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, text)) => error(id, code, &text),
    })
}

fn initialize(params: &Value) -> Value {
    let version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .filter(|version| PROTOCOL_VERSIONS.contains(version))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "Solidbar",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": INSTRUCTIONS,
    })
}

pub fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The ids of the requests in a message or batch, which an error answers.
pub fn request_ids(message: &Value) -> Vec<Value> {
    let ids = |message: &Value| {
        message
            .get("method")
            .and(message.get("id"))
            .cloned()
            .into_iter()
    };
    match message {
        Value::Array(batch) => batch.iter().flat_map(ids).collect(),
        message => ids(message).collect(),
    }
}
