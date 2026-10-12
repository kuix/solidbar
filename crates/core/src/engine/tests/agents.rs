//! AI agents over MCP, end to end: the HTTP server with its key, the tools, and the stdio
//! bridge, against a mocked solidtime.

use std::sync::atomic::{AtomicBool, Ordering};

use solidbar_shared::view::AgentServerView;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use super::*;
use crate::mcp::{AgentServer, bridge};

const BRIDGE: &str = "/Applications/Solidbar.app/Contents/MacOS/solidbar";

struct Agent {
    http: reqwest::Client,
    url: String,
    key: String,
}

impl Agent {
    async fn post(&self, message: Value) -> reqwest::Response {
        self.http
            .post(&self.url)
            .bearer_auth(&self.key)
            .json(&message)
            .send()
            .await
            .unwrap()
    }

    async fn call(&self, tool: &str, arguments: Value) -> Value {
        let response = self
            .post(json!({
                "jsonrpc": "2.0", "id": 7, "method": "tools/call",
                "params": { "name": tool, "arguments": arguments },
            }))
            .await;
        assert_eq!(response.status(), 200);
        response.json::<Value>().await.unwrap()["result"].clone()
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// Turns agents on as Settings does, and starts the server.
async fn serve(harness: &Harness) -> (Arc<AgentServer>, Agent) {
    harness.engine.set_agent_access(true).unwrap();
    let server = AgentServer::new(harness.engine.clone(), BRIDGE.to_owned());
    server.sync().await;
    let AgentServerView::Running { url, bridge } = harness.sink.last().agents else {
        panic!("not running: {:?}", harness.sink.last().agents);
    };
    assert_eq!(bridge, BRIDGE);
    let key = harness.engine.agent_access().key.unwrap();
    (
        server,
        Agent {
            http: client(),
            url,
            key,
        },
    )
}

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": { "name": "test", "version": "1" },
        },
    })
}

#[tokio::test]
async fn agents_need_the_key_and_get_four_tools() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(&server, vec![]).await;
    let (_server, agent) = serve(&harness).await;

    let anonymous = agent
        .http
        .post(&agent.url)
        .json(&initialize())
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), 401);

    let response = agent.post(initialize()).await;
    assert_eq!(response.status(), 200);
    let answer: Value = response.json().await.unwrap();
    assert_eq!(answer["id"], 1);
    assert_eq!(answer["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(answer["result"]["serverInfo"]["name"], "solidbar");
    assert!(answer["result"]["capabilities"]["tools"].is_object());

    let initialized = agent
        .post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    assert_eq!(initialized.status(), 202);

    let tools: Value = agent
        .post(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
        .await
        .json()
        .await
        .unwrap();
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "list_projects",
            "list_time_entries",
            "add_time_entries",
            "update_time_entry"
        ]
    );

    let listen = agent
        .http
        .get(&agent.url)
        .bearer_auth(&agent.key)
        .send()
        .await
        .unwrap();
    assert_eq!(listen.status(), 405);

    let projects = agent.call("list_projects", json!({})).await;
    assert_eq!(projects["isError"], false);
    let listing = &projects["structuredContent"];
    assert_eq!(listing["organization"], "Kuix");
    assert_eq!(listing["time_zone"], "Europe/Budapest");
    assert_eq!(listing["today"], "2026-10-09");
    assert_eq!(listing["projects"][0]["name"], "Design system");
    assert_eq!(listing["projects"][0]["tasks"][0]["name"], "Header");
    // The same, as text, for agents that only read that.
    let text = projects["content"][0]["text"].as_str().unwrap();
    assert_eq!(&serde_json::from_str::<Value>(text).unwrap(), listing);

    let unknown = agent
        .post(json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "delete_everything", "arguments": {} },
        }))
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(unknown["error"]["code"], -32602);
}

#[tokio::test]
async fn an_agent_adds_entries_after_the_days_last_one() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(
        &server,
        vec![entry(
            "e1",
            "2026-10-09T07:00:00Z",
            Some("2026-10-09T09:00:00Z"),
        )],
    )
    .await;
    let (_server, agent) = serve(&harness).await;

    // A mistake in one entry: nothing reaches solidtime, and the agent learns which one.
    let mistaken = agent
        .call(
            "add_time_entries",
            json!({ "entries": [
                { "date": "2026-10-09", "duration": "1" },
                { "date": "2026-10-09", "duration": "1", "project_id": "nope" },
            ]}),
        )
        .await;
    assert_eq!(mistaken["isError"], true);
    assert_eq!(
        mistaken["content"][0]["text"],
        "entry 2: there's no project nope; list_projects lists them"
    );

    for (body, created) in [
        (
            json!({
                "member_id": MEMBER, "start": "2026-10-09T09:00:00Z", "end": "2026-10-09T10:30:00Z",
                "billable": true, "project_id": "p1", "task_id": "t1", "description": "Header",
            }),
            entry("e2", "2026-10-09T09:00:00Z", Some("2026-10-09T10:30:00Z")),
        ),
        (
            json!({
                "member_id": MEMBER, "start": "2026-10-09T10:30:00Z", "end": "2026-10-09T11:00:00Z",
                "billable": false, "project_id": null, "task_id": null, "description": "Standup",
            }),
            entry("e3", "2026-10-09T10:30:00Z", Some("2026-10-09T11:00:00Z")),
        ),
    ] {
        Mock::given(method("POST"))
            .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
            .and(body_json(body))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({ "data": created })))
            .expect(1)
            .mount(&server)
            .await;
    }

    let added = agent
        .call(
            "add_time_entries",
            json!({ "entries": [
                { "date": "2026-10-09", "duration": "1:30", "task_id": "t1", "description": "Header" },
                { "date": "2026-10-09", "duration": "30m", "description": "Standup" },
            ]}),
        )
        .await;
    assert_eq!(added["isError"], false, "{added}");
    let added = &added["structuredContent"]["added"];
    assert_eq!(added[0]["id"], "e2");
    assert_eq!(added[0]["start_time"], "11:00");
    assert_eq!(added[1]["end_time"], "13:00");

    let notices = harness.sink.notices();
    assert_eq!(
        notices.last().unwrap().text,
        "An AI agent added 2 time entries"
    );
    // The panel shows them right away.
    let friday = harness.sink.last().week.unwrap().days[4].clone();
    let ids: Vec<&str> = friday
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(ids, ["e1", "e2", "e3"]);
}

#[tokio::test]
async fn an_agent_changes_an_entry_it_listed() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(
        &server,
        vec![entry(
            "e1",
            "2026-10-09T07:00:00Z",
            Some("2026-10-09T09:00:00Z"),
        )],
    )
    .await;
    let (_server, agent) = serve(&harness).await;

    let listed = agent.call("list_time_entries", json!({})).await;
    let listing = &listed["structuredContent"];
    assert_eq!(
        (&listing["from"], &listing["to"]),
        (&json!("2026-10-09"), &json!("2026-10-09"))
    );
    assert_eq!(listing["total"], "2:00");
    assert_eq!(listing["entries"][0]["start_time"], "09:00");

    let unknown = agent
        .call(
            "update_time_entry",
            json!({ "id": "e9", "description": "?" }),
        )
        .await;
    assert_eq!(unknown["isError"], true);

    let mut changed = entry("e1", "2026-10-09T08:00:00Z", Some("2026-10-09T10:00:00Z"));
    changed["description"] = json!("Header work");
    Mock::given(method("PUT"))
        .and(path(format!("/api/v1/organizations/{ORG}/time-entries/e1")))
        .and(body_partial_json(json!({
            "start": "2026-10-09T08:00:00Z", "end": "2026-10-09T10:00:00Z",
            "project_id": "p1", "description": "Header work",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": changed })))
        .expect(1)
        .mount(&server)
        .await;

    // A new start keeps the two hours.
    let updated = agent
        .call(
            "update_time_entry",
            json!({ "id": "e1", "start_time": "10:00", "description": "Header work" }),
        )
        .await;
    assert_eq!(updated["isError"], false, "{updated}");
    let entry = &updated["structuredContent"]["entry"];
    assert_eq!(
        (&entry["start_time"], &entry["end_time"]),
        (&json!("10:00"), &json!("12:00"))
    );
    assert_eq!(entry["description"], "Header work");
    assert_eq!(
        harness.sink.notices().last().unwrap().text,
        "An AI agent changed a time entry"
    );
}

#[tokio::test]
async fn turning_agents_off_or_a_new_key_shuts_them_out() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(&server, vec![]).await;
    let (server, agent) = serve(&harness).await;

    harness.engine.reset_agent_key();
    assert_eq!(agent.post(initialize()).await.status(), 401);
    let agent = Agent {
        key: harness.engine.agent_access().key.unwrap(),
        ..agent
    };
    assert_eq!(agent.post(initialize()).await.status(), 200);

    harness.engine.set_agent_access(false).unwrap();
    // Even before the server stops, a connection still open gets nothing.
    assert_eq!(agent.post(initialize()).await.status(), 503);
    server.sync().await;
    assert_eq!(harness.sink.last().agents, AgentServerView::Off);
    // A new connection finds the port closed.
    let refused = client()
        .post(&agent.url)
        .bearer_auth(&agent.key)
        .json(&initialize())
        .send()
        .await
        .unwrap_err();
    assert!(refused.is_connect(), "{refused}");

    // On again: the same address and key as before.
    harness.engine.set_agent_access(true).unwrap();
    server.sync().await;
    assert_eq!(agent.post(initialize()).await.status(), 200);
}

#[tokio::test]
async fn the_bridge_relays_stdio_and_starts_solidbar_when_needed() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(&server, vec![]).await;
    harness.engine.set_agent_access(true).unwrap();
    // Not running yet: the bridge starts it.
    let agents = AgentServer::new(harness.engine.clone(), BRIDGE.to_owned());
    let launched = Arc::new(AtomicBool::new(false));

    let (client, bridge_side) = tokio::io::duplex(64 * 1024);
    let (input, output) = tokio::io::split(bridge_side);
    let directory = harness.directory.path().to_owned();
    let launch = {
        let agents = agents.clone();
        let launched = launched.clone();
        move || {
            launched.store(true, Ordering::SeqCst);
            let agents = agents.clone();
            tokio::spawn(async move { agents.sync().await });
        }
    };
    let relay = tokio::spawn(async move { bridge::run(&directory, input, output, launch).await });

    let (reader, mut writer) = tokio::io::split(client);
    let mut lines = BufReader::new(reader).lines();
    for message in [
        initialize(),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    ] {
        writer
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }
    let initialized: Value =
        serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(initialized["result"]["serverInfo"]["name"], "solidbar");
    // The notification has no answer; the next line answers tools/list.
    let tools: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(tools["id"], 2);
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 4);
    assert!(launched.load(Ordering::SeqCst));

    // Turned off in Settings: the agent hears why.
    harness.engine.set_agent_access(false).unwrap();
    writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\"}\n")
        .await
        .unwrap();
    let refused: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
    assert_eq!(refused["id"], 3);
    assert_eq!(refused["error"]["code"], -32000);
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Settings › AI agents"),
        "{refused}"
    );

    writer.shutdown().await.unwrap();
    relay.await.unwrap().unwrap();
}
