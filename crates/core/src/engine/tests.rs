//! Engine flows against a mocked solidtime.

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use jiff::tz::TimeZone;
use serde_json::{Value, json};
use solidbar_shared::instance::{DEEP_LINK_REDIRECT, InstanceSettings, Redirect};
use solidbar_shared::ipc::{AdjustArgs, EntryDraft, FormArgs, TimerSource};
use solidbar_shared::settings::{Settings, SettingsPatch};
use solidbar_shared::view::{AppView, AuthView, Catalog, EntryKind, Notice};
use wiremock::matchers::{body_json, method, path, query_param, query_param_contains};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::{Engine, EngineDeps};
use crate::auth::{Instance, MemoryTokenStore, TokenSet, TokenStore};
use crate::ports::testing::FixedClock;
use crate::ports::{EventSink, UrlOpener};

const ORG: &str = "org-1";
const MEMBER: &str = "member-1";
/// Friday 2026-10-09, 12:00 in Budapest.
const NOW: &str = "2026-10-09T10:00:00Z";

#[derive(Default)]
struct RecordingSink {
    views: Mutex<Vec<AppView>>,
    notices: Mutex<Vec<Notice>>,
}

impl RecordingSink {
    fn last(&self) -> AppView {
        self.views.lock().unwrap().last().cloned().unwrap()
    }

    fn notices(&self) -> Vec<Notice> {
        self.notices.lock().unwrap().clone()
    }
}

impl EventSink for RecordingSink {
    fn app_changed(&self, view: &AppView) {
        self.views.lock().unwrap().push(view.clone());
    }

    fn catalog_changed(&self, _catalog: &Catalog) {}

    fn notice(&self, notice: &Notice) {
        self.notices.lock().unwrap().push(notice.clone());
    }
}

/// Plays the browser: gets the authorize URL and does what the test says with it.
struct Browser(Box<dyn Fn(String) + Send + Sync>);

impl UrlOpener for Browser {
    fn open(&self, url: &str) -> Result<(), String> {
        (self.0)(url.to_owned());
        Ok(())
    }
}

struct Harness {
    engine: Arc<Engine>,
    sink: Arc<RecordingSink>,
    tokens: Arc<MemoryTokenStore>,
    clock: Arc<FixedClock>,
    account: String,
    directory: tempfile::TempDir,
}

fn instance_settings(server: &MockServer) -> InstanceSettings {
    InstanceSettings {
        url: server.uri(),
        client_id: "client-1".to_owned(),
    }
}

fn harness(
    server: &MockServer,
    signed_in: bool,
    browser: impl Fn(String) + Send + Sync + 'static,
) -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let settings = Settings {
        instance: instance_settings(server),
        ..Settings::default()
    };
    std::fs::write(
        directory.path().join("settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let account = Instance::new(&settings.instance).unwrap().account();
    let tokens = Arc::new(MemoryTokenStore::default());
    if signed_in {
        tokens
            .save(
                &account,
                &TokenSet {
                    access_token: "token-1".to_owned(),
                    refresh_token: "refresh-1".to_owned(),
                    expires_at: "2026-10-09T11:00:00Z".parse().unwrap(),
                },
            )
            .unwrap();
    }
    let sink = Arc::new(RecordingSink::default());
    let clock = Arc::new(FixedClock::at(NOW));
    let engine = Engine::new(EngineDeps {
        http: reqwest::Client::new(),
        clock: clock.clone(),
        tokens: tokens.clone(),
        sink: sink.clone(),
        opener: Arc::new(Browser(Box::new(browser))),
        data_dir: directory.path().to_owned(),
        hour12: false,
        system_zone: TimeZone::get("Europe/Budapest").unwrap(),
    });
    Harness {
        engine,
        sink,
        tokens,
        clock,
        account,
        directory,
    }
}

fn entry(id: &str, start: &str, end: Option<&str>) -> Value {
    json!({
        "id": id, "start": start, "end": end, "duration": null, "description": "",
        "task_id": null, "project_id": "p1", "organization_id": ORG, "user_id": "u1",
        "tags": [], "billable": true, "type": "work",
    })
}

async fn mount_profile(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "id": "u1", "name": "Barnabás", "email": "b@example.com",
            "timezone": "Europe/Budapest", "week_start": "monday",
        }})))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me/memberships"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [{
            "id": MEMBER, "role": "owner",
            "organization": { "id": ORG, "name": "Kuix", "currency": "EUR" },
        }]})))
        .mount(server)
        .await;
    for (resource, item) in [
        (
            "projects",
            json!({ "id": "p1", "name": "Design system", "color": "#ef5350", "client_id": null,
                    "is_archived": false, "billable_rate": null, "is_billable": true,
                    "estimated_time": null, "spent_time": 0, "is_public": false }),
        ),
        (
            "tasks",
            json!({ "id": "t1", "name": "Header", "is_done": false, "project_id": "p1" }),
        ),
        (
            "clients",
            json!({ "id": "c1", "name": "Kuix", "is_archived": false }),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/organizations/{ORG}/{resource}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [item], "meta": { "current_page": 1, "last_page": 1 },
            })))
            .mount(server)
            .await;
    }
}

async fn mount_active(server: &MockServer, active: Option<Value>) {
    let response = match active {
        Some(entry) => ResponseTemplate::new(200).set_body_json(json!({ "data": entry })),
        None => ResponseTemplate::new(404),
    };
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me/time-entries/active"))
        .respond_with(response)
        .mount(server)
        .await;
}

async fn mount_week(server: &MockServer, entries: Vec<Value>) {
    let total = entries.len();
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
        .and(query_param("member_id", MEMBER))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": entries, "meta": { "total": total },
        })))
        .mount(server)
        .await;
}

async fn signed_in_with_week(server: &MockServer, entries: Vec<Value>) -> Harness {
    mount_profile(server).await;
    mount_active(server, None).await;
    mount_week(server, entries).await;
    let harness = harness(server, true, |_| {});
    harness.engine.refresh(false).await.unwrap();
    harness
}

fn draft(start_time: &str, duration: &str) -> EntryDraft {
    EntryDraft {
        id: None,
        group_ids: Vec::new(),
        kind: EntryKind::Work,
        date: "2026-10-09".to_owned(),
        project_id: Some("p1".to_owned()),
        task_id: Some("t1".to_owned()),
        description: " Header ".to_owned(),
        billable: true,
        duration: duration.to_owned(),
        start_time: start_time.to_owned(),
        start_timer: false,
    }
}

#[tokio::test]
async fn a_restored_session_loads_the_week() {
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

    let view = harness.sink.last();
    assert!(
        matches!(view.auth, AuthView::SignedIn { ref user_name, .. } if user_name == "Barnabás")
    );
    assert_eq!(view.active_membership_id.as_deref(), Some(MEMBER));
    // The fallback choice is saved, so a different membership order can't switch it.
    let saved = std::fs::read_to_string(harness.directory.path().join("settings.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(saved["active_membership_id"], MEMBER);
    let friday = &view.week.unwrap().days[4];
    assert_eq!(friday.total_seconds, 2 * 3600);
    assert_eq!(
        friday.entries[0].project_name.as_deref(),
        Some("Design system")
    );
    assert_eq!(view.tray.title.as_deref(), Some("2:00"));
    assert!(!view.loading);
}

#[tokio::test]
async fn with_several_organizations_the_first_sign_in_asks_which_one() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/me/memberships"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [
            { "id": "member-2", "role": "employee",
              "organization": { "id": "org-2", "name": "ACME", "currency": "EUR" } },
            { "id": MEMBER, "role": "owner",
              "organization": { "id": ORG, "name": "Kuix", "currency": "EUR" } },
        ]})))
        .with_priority(1)
        .mount(&server)
        .await;
    let harness = signed_in_with_week(
        &server,
        vec![entry(
            "e1",
            "2026-10-09T07:00:00Z",
            Some("2026-10-09T09:00:00Z"),
        )],
    )
    .await;

    let view = harness.sink.last();
    assert!(view.choose_org);
    assert_eq!(view.active_membership_id, None);
    // Nothing is loaded from an organization nobody picked.
    let requests = server.received_requests().await.unwrap();
    assert!(
        !requests
            .iter()
            .any(|request| request.url.path().starts_with("/api/v1/organizations/"))
    );

    harness.engine.select_org(MEMBER).await.unwrap();
    let view = harness.sink.last();
    assert!(!view.choose_org);
    assert_eq!(view.week.unwrap().days[4].total_seconds, 2 * 3600);

    // Signing out forgets the choice, so whoever signs in next picks their own.
    harness.engine.logout().await.unwrap();
    assert_eq!(harness.engine.settings().active_membership_id, None);
}

#[tokio::test]
async fn opening_the_form_picks_up_a_project_created_elsewhere() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(&server, vec![]).await;
    let labels = || -> Vec<String> {
        harness
            .engine
            .catalog_view()
            .options
            .into_iter()
            .map(|option| option.label)
            .collect()
    };
    assert!(!labels().contains(&"Random projekt".to_owned()));

    // Someone adds a project on the web.
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/organizations/{ORG}/projects")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "p2", "name": "Random projekt", "color": "#ffa726", "client_id": null,
                       "is_archived": false, "billable_rate": null, "is_billable": false,
                       "estimated_time": null, "spent_time": 0, "is_public": false }],
            "meta": { "current_page": 1, "last_page": 1 },
        })))
        .with_priority(1)
        .mount(&server)
        .await;

    // Right after a refresh the catalog is fresh enough.
    harness.engine.form_opened().await;
    assert!(!labels().contains(&"Random projekt".to_owned()));

    harness.clock.advance(SignedDuration::from_mins(2));
    harness.engine.form_opened().await;
    assert!(labels().contains(&"Random projekt".to_owned()));
}

#[tokio::test]
async fn new_entries_stack_after_the_last_one_of_the_day() {
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

    let form = harness
        .engine
        .prepare_form(&FormArgs {
            date: "2026-10-09".to_owned(),
            entry_id: None,
            group_ids: Vec::new(),
        })
        .unwrap();
    assert_eq!(form.start_time, "11:00");

    Mock::given(method("POST"))
        .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
        .and(body_json(json!({
            "member_id": MEMBER, "start": "2026-10-09T09:00:00Z", "end": "2026-10-09T11:00:00Z",
            "billable": true, "project_id": "p1", "task_id": "t1", "description": "Header",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "data": entry("e2", "2026-10-09T09:00:00Z", Some("2026-10-09T11:00:00Z")),
        })))
        .expect(1)
        .mount(&server)
        .await;
    harness
        .engine
        .save_entry(draft(&form.start_time, "2"))
        .await
        .unwrap();

    let view = harness.sink.last();
    let friday = &view.week.unwrap().days[4];
    let ids: Vec<_> = friday
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(ids, ["e1", "e2"]);
    assert!(friday.entries.iter().all(|entry| !entry.pending));
    assert_eq!(harness.sink.notices().last().unwrap().text, "Added 2 hours");
    assert_eq!(
        harness.engine.catalog_view().recents[0].label,
        "Design system › Header"
    );
}

#[tokio::test]
async fn a_rejected_entry_disappears_again_with_the_reason() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(&server, vec![]).await;
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": true, "key": "overlapping_time_entry",
            "message": "Time entry overlaps with another time entry",
        })))
        .mount(&server)
        .await;

    let error = harness
        .engine
        .save_entry(draft("09:00", "1"))
        .await
        .unwrap_err();
    assert_eq!(error.message, "Time entry overlaps with another time entry");
    let view = harness.sink.last();
    assert!(view.week.unwrap().days[4].entries.is_empty());
    // The UI shows the returned error; nothing is announced as if it had worked.
    assert!(harness.sink.notices().is_empty());
}

#[tokio::test]
async fn invalid_durations_never_reach_solidtime() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(&server, vec![]).await;
    let error = harness
        .engine
        .save_entry(draft("23:00", "2"))
        .await
        .unwrap_err();
    assert_eq!(error.message, "A time entry can't continue past midnight");
    let error = harness
        .engine
        .save_entry(draft("09:00", "soon"))
        .await
        .unwrap_err();
    assert!(error.message.starts_with("Use a duration like"));
}

#[tokio::test]
async fn adding_an_hour_moves_later_entries_and_undoes_partial_failures() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(
        &server,
        vec![
            entry("a", "2026-10-09T07:00:00Z", Some("2026-10-09T08:00:00Z")),
            entry("b", "2026-10-09T08:00:00Z", Some("2026-10-09T09:00:00Z")),
        ],
    )
    .await;
    let put = |id: &str| path(format!("/api/v1/organizations/{ORG}/time-entries/{id}"));
    // "b" moves first, out of the way…
    Mock::given(method("PUT"))
        .and(put("b"))
        .and(body_json(
            json!({ "start": "2026-10-09T09:00:00Z", "end": "2026-10-09T10:00:00Z" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": entry("b", "2026-10-09T09:00:00Z", Some("2026-10-09T10:00:00Z")),
        })))
        .expect(1)
        .mount(&server)
        .await;
    // …then "a" grows, which solidtime rejects…
    Mock::given(method("PUT"))
        .and(put("a"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": true, "key": "overlapping_time_entry", "message": "Overlap",
        })))
        .expect(1)
        .mount(&server)
        .await;
    // …so "b" goes back where it was.
    Mock::given(method("PUT"))
        .and(put("b"))
        .and(body_json(
            json!({ "start": "2026-10-09T08:00:00Z", "end": "2026-10-09T09:00:00Z" }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": entry("b", "2026-10-09T08:00:00Z", Some("2026-10-09T09:00:00Z")),
        })))
        .expect(1)
        .mount(&server)
        .await;

    let error = harness
        .engine
        .adjust_entry(&AdjustArgs {
            id: "a".to_owned(),
            delta_minutes: 60,
        })
        .await
        .unwrap_err();
    assert_eq!(error.message, "Overlap");
    let friday = harness.sink.last().week.unwrap().days[4].clone();
    assert_eq!(friday.total_seconds, 2 * 3600);
    assert!(friday.entries.iter().all(|entry| !entry.pending));
}

#[tokio::test]
async fn adding_an_hour_announces_what_moved() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(
        &server,
        vec![
            entry("a", "2026-10-09T07:00:00Z", Some("2026-10-09T08:00:00Z")),
            entry("b", "2026-10-09T08:00:00Z", Some("2026-10-09T09:00:00Z")),
        ],
    )
    .await;
    for (id, start, end) in [
        ("b", "2026-10-09T09:00:00Z", "2026-10-09T10:00:00Z"),
        ("a", "2026-10-09T07:00:00Z", "2026-10-09T09:00:00Z"),
    ] {
        Mock::given(method("PUT"))
            .and(path(format!(
                "/api/v1/organizations/{ORG}/time-entries/{id}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": entry(id, start, Some(end)),
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    harness
        .engine
        .adjust_entry(&AdjustArgs {
            id: "a".to_owned(),
            delta_minutes: 60,
        })
        .await
        .unwrap();
    assert_eq!(
        harness.sink.notices().last().unwrap().text,
        "Added 1 hour; moved 1 later entry"
    );
    assert_eq!(
        harness.sink.last().week.unwrap().days[4].total_seconds,
        3 * 3600
    );
}

#[tokio::test]
async fn continuing_an_entry_stops_the_running_timer_first() {
    let server = MockServer::start().await;
    mount_profile(&server).await;
    mount_active(&server, Some(entry("live", "2026-10-09T09:00:00Z", None))).await;
    mount_week(
        &server,
        vec![
            entry("done", "2026-10-09T07:00:00Z", Some("2026-10-09T08:00:00Z")),
            entry("live", "2026-10-09T09:00:00Z", None),
        ],
    )
    .await;
    let harness = harness(&server, true, |_| {});
    harness.engine.refresh(false).await.unwrap();
    assert!(harness.sink.last().tray.timer_running);

    Mock::given(method("PUT"))
        .and(path(format!(
            "/api/v1/organizations/{ORG}/time-entries/live"
        )))
        .and(body_json(
            json!({ "start": "2026-10-09T09:00:00Z", "end": NOW }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": entry("live", "2026-10-09T09:00:00Z", Some(NOW)),
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
        .and(body_json(json!({
            "member_id": MEMBER, "start": NOW, "end": null, "billable": true,
            "project_id": "p1", "task_id": null, "description": null,
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "data": entry("next", NOW, None),
        })))
        .expect(1)
        .mount(&server)
        .await;

    harness
        .engine
        .start_timer(&TimerSource::Entry {
            id: "done".to_owned(),
        })
        .await
        .unwrap();
    let view = harness.sink.last();
    assert_eq!(view.timer.unwrap().entry_id, "next");
    let friday = &view.week.unwrap().days[4];
    assert_eq!(
        friday.entries.iter().filter(|entry| entry.running).count(),
        1
    );
}

fn break_entry(id: &str, start: &str, end: Option<&str>) -> Value {
    json!({
        "id": id, "start": start, "end": end, "duration": null, "description": null,
        "task_id": null, "project_id": null, "organization_id": ORG, "user_id": "u1",
        "tags": [], "billable": false, "type": "break",
    })
}

async fn mount_breaks(server: &MockServer, enabled: bool) {
    Mock::given(method("GET"))
        .and(path(format!("/api/v1/organizations/{ORG}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {
            "id": ORG, "name": "Kuix", "currency": "EUR", "breaks_enabled": enabled,
        }})))
        .mount(server)
        .await;
}

/// Signed in with a running timer (or none) and the week's entries.
async fn signed_in_with_timer(
    server: &MockServer,
    breaks: bool,
    active: Option<Value>,
    entries: Vec<Value>,
) -> Harness {
    mount_profile(server).await;
    mount_breaks(server, breaks).await;
    mount_active(server, active).await;
    mount_week(server, entries).await;
    let harness = harness(server, true, |_| {});
    harness.engine.refresh(false).await.unwrap();
    harness
}

#[tokio::test]
async fn taking_a_break_stops_work_and_starts_the_break_at_the_same_instant() {
    let server = MockServer::start().await;
    let live = entry("live", "2026-10-09T08:00:00Z", None);
    let harness = signed_in_with_timer(&server, true, Some(live.clone()), vec![live]).await;
    assert!(harness.sink.last().breaks_enabled);

    Mock::given(method("PUT"))
        .and(path(format!(
            "/api/v1/organizations/{ORG}/time-entries/live"
        )))
        .and(body_json(
            json!({ "start": "2026-10-09T08:00:00Z", "end": NOW }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": entry("live", "2026-10-09T08:00:00Z", Some(NOW)),
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
        .and(body_json(json!({
            "member_id": MEMBER, "start": NOW, "end": null, "billable": false,
            "project_id": null, "task_id": null, "description": null, "type": "break",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "data": break_entry("pause", NOW, None),
        })))
        .expect(1)
        .mount(&server)
        .await;

    harness.engine.start_break().await.unwrap();
    let view = harness.sink.last();
    let timer = view.timer.unwrap();
    assert_eq!(timer.kind, EntryKind::Break);
    assert_eq!(timer.resume_label.as_deref(), Some("Design system"));
    assert_eq!(view.tray.title.as_deref(), Some("☕ 0:00"));
    assert_eq!(view.tray.tooltip, "On a break, 0:00");
    assert_eq!(harness.sink.notices().last().unwrap().text, "Break started");
}

#[tokio::test]
async fn after_a_break_the_timer_shortcut_resumes_the_work_before_it() {
    let server = MockServer::start().await;
    let mut work = entry("work", "2026-10-09T07:00:00Z", Some("2026-10-09T09:00:00Z"));
    work["description"] = json!("Header");
    work["task_id"] = json!("t1");
    let pause = break_entry("pause", "2026-10-09T09:00:00Z", None);
    let harness = signed_in_with_timer(&server, true, Some(pause.clone()), vec![work, pause]).await;
    let view = harness.sink.last();
    assert_eq!(view.timer.unwrap().resume_label.as_deref(), Some("Header"));
    // The break is neither work time nor part of today's total.
    let friday = &view.week.unwrap().days[4];
    assert_eq!(friday.total_seconds, 2 * 3600);
    assert_eq!(friday.break_seconds, 3600);
    assert_eq!(friday.entries[1].kind, EntryKind::Break);

    Mock::given(method("PUT"))
        .and(path(format!(
            "/api/v1/organizations/{ORG}/time-entries/pause"
        )))
        .and(body_json(
            json!({ "start": "2026-10-09T09:00:00Z", "end": NOW }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": break_entry("pause", "2026-10-09T09:00:00Z", Some(NOW)),
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
        .and(body_json(json!({
            "member_id": MEMBER, "start": NOW, "end": null, "billable": true,
            "project_id": "p1", "task_id": "t1", "description": "Header",
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "data": entry("again", NOW, None),
        })))
        .expect(1)
        .mount(&server)
        .await;

    harness.engine.toggle_timer().await.unwrap();
    let view = harness.sink.last();
    let timer = view.timer.unwrap();
    assert_eq!(timer.entry_id, "again");
    assert_eq!(timer.kind, EntryKind::Work);
    assert_eq!(view.week.unwrap().days[4].break_seconds, 3600);
}

#[tokio::test]
async fn breaks_are_offered_only_where_the_organization_tracks_them() {
    let server = MockServer::start().await;
    let harness = signed_in_with_timer(&server, false, None, vec![]).await;
    assert!(!harness.sink.last().breaks_enabled);
    assert!(
        harness
            .engine
            .catalog_view()
            .options
            .iter()
            .all(|option| option.kind == EntryKind::Work)
    );
    let error = harness.engine.start_break().await.unwrap_err();
    assert_eq!(error.message, "This organization doesn't track breaks");

    let server = MockServer::start().await;
    let harness = signed_in_with_timer(&server, true, None, vec![]).await;
    let options = harness.engine.catalog_view().options;
    let last = options.last().unwrap();
    assert_eq!(
        (last.kind, last.label.as_str()),
        (EntryKind::Break, "Break")
    );
}

#[tokio::test]
async fn a_group_saves_its_shared_fields_to_every_entry_and_keeps_their_times() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(
        &server,
        vec![
            entry("a", "2026-10-09T07:00:00Z", Some("2026-10-09T09:00:00Z")),
            entry("b", "2026-10-09T09:05:00Z", Some("2026-10-09T09:35:00Z")),
        ],
    )
    .await;
    for (id, start, end) in [
        ("a", "2026-10-09T07:00:00Z", "2026-10-09T09:00:00Z"),
        ("b", "2026-10-09T09:05:00Z", "2026-10-09T09:35:00Z"),
    ] {
        let mut saved = entry(id, start, Some(end));
        saved["description"] = json!("Review");
        Mock::given(method("PUT"))
            .and(path(format!(
                "/api/v1/organizations/{ORG}/time-entries/{id}"
            )))
            .and(body_json(json!({
                "start": start, "end": end, "billable": true,
                "project_id": "p1", "task_id": null, "description": "Review",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": saved })))
            .expect(1)
            .mount(&server)
            .await;
    }

    let form = harness
        .engine
        .prepare_form(&FormArgs {
            date: "2026-10-09".to_owned(),
            entry_id: None,
            group_ids: vec!["a".to_owned(), "b".to_owned()],
        })
        .unwrap();
    assert_eq!(form.group_ids, ["a", "b"]);
    assert_eq!(form.duration, "2:30");
    harness
        .engine
        .save_entry(EntryDraft {
            group_ids: form.group_ids,
            project_id: Some("p1".to_owned()),
            task_id: None,
            description: "Review".to_owned(),
            // Ignored for a group: its entries keep their times.
            duration: "8".to_owned(),
            ..draft("09:00", "")
        })
        .await
        .unwrap();
    let friday = &harness.sink.last().week.unwrap().days[4];
    assert!(
        friday
            .entries
            .iter()
            .all(|entry| entry.description == "Review")
    );
    assert_eq!(friday.total_seconds, 2 * 3600 + 30 * 60);
    assert_eq!(
        harness.sink.notices().last().unwrap().text,
        "Saved 2 time entries"
    );
}

#[tokio::test]
async fn deleting_a_group_deletes_each_entry_and_restores_what_failed() {
    let server = MockServer::start().await;
    let harness = signed_in_with_week(
        &server,
        vec![
            entry("a", "2026-10-09T07:00:00Z", Some("2026-10-09T08:00:00Z")),
            entry("b", "2026-10-09T08:00:00Z", Some("2026-10-09T09:00:00Z")),
            entry("c", "2026-10-09T09:00:00Z", Some("2026-10-09T09:30:00Z")),
        ],
    )
    .await;
    for (id, status) in [("a", 204), ("b", 204), ("c", 403)] {
        Mock::given(method("DELETE"))
            .and(path(format!(
                "/api/v1/organizations/{ORG}/time-entries/{id}"
            )))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;
    }

    harness
        .engine
        .delete_entries(&["a".to_owned(), "b".to_owned()])
        .await
        .unwrap();
    let ids = |harness: &Harness| -> Vec<String> {
        harness.sink.last().week.unwrap().days[4]
            .entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    };
    assert_eq!(ids(&harness), ["c"]);
    assert_eq!(
        harness.sink.notices().last().unwrap().text,
        "Deleted 2 time entries"
    );

    let error = harness
        .engine
        .delete_entries(&["c".to_owned()])
        .await
        .unwrap_err();
    assert_eq!(
        error.message,
        "You don't have permission to do that in this organization"
    );
    assert_eq!(ids(&harness), ["c"]);
}

#[tokio::test]
async fn an_expired_session_returns_to_the_sign_in_screen() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({ "error": "invalid_grant" })))
        .mount(&server)
        .await;
    let harness = harness(&server, true, |_| {});
    assert!(harness.engine.refresh(false).await.is_err());

    let view = harness.sink.last();
    assert!(matches!(
        view.auth,
        AuthView::SignedOut { error: Some(_), .. }
    ));
    assert!(harness.tokens.load(&harness.account).unwrap().is_none());
    assert!(view.week.is_none());
}

/// Pulls a query parameter out of the authorize URL the engine opened.
fn query(url: &str, key: &str) -> String {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
        .unwrap()
}

/// The authorize page, as an anonymous visitor sees it: sent on to the login page for a
/// redirect the client allows, `invalid_client` for one it doesn't.
async fn mount_authorize(server: &MockServer, loopback: bool, deep_link: bool) {
    let answer = |allowed: bool| {
        if allowed {
            ResponseTemplate::new(302).insert_header("location", "/login")
        } else {
            ResponseTemplate::new(401).set_body_json(json!({
                "error": "invalid_client", "error_description": "Client authentication failed",
            }))
        }
    };
    Mock::given(method("GET"))
        .and(path("/oauth/authorize"))
        .and(query_param("redirect_uri", DEEP_LINK_REDIRECT))
        .respond_with(answer(deep_link))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/oauth/authorize"))
        .and(query_param_contains("redirect_uri", "http://127.0.0.1:"))
        .respond_with(answer(loopback))
        .mount(server)
        .await;
}

async fn mount_token_exchange(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "expires_in": 3600, "access_token": "token-1", "refresh_token": "refresh-1",
        })))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn signs_in_through_the_loopback_redirect() {
    let server = MockServer::start().await;
    mount_profile(&server).await;
    mount_active(&server, None).await;
    mount_week(&server, vec![]).await;
    mount_token_exchange(&server).await;
    mount_authorize(&server, true, true).await;
    let opened = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = opened.clone();
    let harness = harness(&server, false, move |authorize| {
        recorder.lock().unwrap().push(authorize.clone());
        // The browser follows the redirect once the person approves.
        tokio::spawn(async move {
            let redirect = format!(
                "{}?code=code-1&state={}",
                query(&authorize, "redirect_uri"),
                query(&authorize, "state")
            );
            reqwest::get(redirect).await.unwrap();
        });
    });

    harness
        .engine
        .login(instance_settings(&server))
        .await
        .unwrap();

    assert!(matches!(
        harness.sink.last().auth,
        AuthView::SignedIn { .. }
    ));
    assert!(harness.tokens.load(&harness.account).unwrap().is_some());
    // On a port the system picked, and the token request named the same redirect.
    let redirect_uri = query(&opened.lock().unwrap()[0], "redirect_uri");
    let port: u16 = redirect_uri
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix("/callback"))
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(port, 0);
    let token_request = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|request| request.url.path() == "/oauth/token")
        .unwrap();
    let form: std::collections::HashMap<String, String> =
        url::form_urlencoded::parse(&token_request.body)
            .into_owned()
            .collect();
    assert_eq!(form["redirect_uri"], redirect_uri);
}

#[tokio::test]
async fn signs_in_through_a_deep_link_and_can_be_cancelled() {
    let server = MockServer::start().await;
    mount_profile(&server).await;
    mount_active(&server, None).await;
    mount_week(&server, vec![]).await;
    mount_token_exchange(&server).await;
    let opened = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = opened.clone();
    // The client only allows the solidtime:// link.
    mount_authorize(&server, false, true).await;
    let harness = harness(&server, false, move |url| {
        recorder.lock().unwrap().push(url);
    });
    let settings = instance_settings(&server);

    // Cancelled: back to the sign-in screen without an error.
    let engine = harness.engine.clone();
    let attempt = tokio::spawn({
        let settings = settings.clone();
        async move { engine.login(settings).await }
    });
    while opened.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        query(&opened.lock().unwrap()[0], "redirect_uri"),
        DEEP_LINK_REDIRECT
    );
    assert!(matches!(
        harness.sink.last().auth,
        AuthView::SigningIn {
            redirect: Some(Redirect::DeepLink),
            ..
        }
    ));
    harness.engine.cancel_login();
    attempt.await.unwrap().unwrap();
    assert!(matches!(
        harness.sink.last().auth,
        AuthView::SignedOut { error: None, .. }
    ));

    // Completed by the deep link.
    let engine = harness.engine.clone();
    let attempt = tokio::spawn(async move { engine.login(settings).await });
    while opened.lock().unwrap().len() < 2 {
        tokio::task::yield_now().await;
    }
    let authorize = opened.lock().unwrap()[1].clone();
    harness.engine.deliver_redirect(&format!(
        "solidtime://oauth/callback?code=code-1&state={}",
        query(&authorize, "state")
    ));
    attempt.await.unwrap().unwrap();
    assert!(matches!(
        harness.sink.last().auth,
        AuthView::SignedIn { .. }
    ));
}

#[tokio::test]
async fn a_client_solidtime_refuses_is_reported_without_opening_the_browser() {
    let server = MockServer::start().await;
    mount_authorize(&server, false, false).await;
    let opened = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorder = opened.clone();
    let harness = harness(&server, false, move |url| {
        recorder.lock().unwrap().push(url);
    });

    let error = harness
        .engine
        .login(instance_settings(&server))
        .await
        .unwrap_err();

    assert!(
        error
            .message
            .contains("doesn't accept this OAuth client ID")
    );
    assert!(opened.lock().unwrap().is_empty());
    assert!(matches!(
        harness.sink.last().auth,
        AuthView::SignedOut { error: Some(_), .. }
    ));
}

#[tokio::test]
async fn single_key_shortcuts_can_be_turned_off_and_stay_off() {
    let server = MockServer::start().await;
    let harness = harness(&server, false, |_| {});
    assert!(harness.engine.settings().single_key_shortcuts);

    harness
        .engine
        .update_settings(&SettingsPatch {
            single_key_shortcuts: Some(false),
            ..SettingsPatch::default()
        })
        .unwrap();

    assert!(!harness.sink.last().settings.single_key_shortcuts);
    let saved: Settings = serde_json::from_slice(
        &std::fs::read(harness.directory.path().join("settings.json")).unwrap(),
    )
    .unwrap();
    assert!(!saved.single_key_shortcuts);
}
