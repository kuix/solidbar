//! Engine flows against a mocked solidtime.

use std::sync::{Arc, Mutex};

use jiff::SignedDuration;
use jiff::tz::TimeZone;
use serde_json::{Value, json};
use solidbar_shared::instance::{InstanceSettings, RedirectMode};
use solidbar_shared::ipc::{AdjustArgs, EntryDraft, FormArgs, TimerSource};
use solidbar_shared::settings::Settings;
use solidbar_shared::view::{AppView, AuthView, Catalog, Notice};
use wiremock::matchers::{body_json, method, path, query_param};
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

fn instance_settings(server: &MockServer, redirect: RedirectMode) -> InstanceSettings {
    InstanceSettings {
        url: server.uri(),
        client_id: "client-1".to_owned(),
        redirect,
    }
}

fn harness(
    server: &MockServer,
    redirect: RedirectMode,
    signed_in: bool,
    browser: impl Fn(String) + Send + Sync + 'static,
) -> Harness {
    let directory = tempfile::tempdir().unwrap();
    let settings = Settings {
        instance: instance_settings(server, redirect),
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
    let harness = harness(server, RedirectMode::DeepLink, true, |_| {});
    harness.engine.refresh(false).await.unwrap();
    harness
}

fn draft(start_time: &str, duration: &str) -> EntryDraft {
    EntryDraft {
        id: None,
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
    let harness = harness(&server, RedirectMode::DeepLink, true, |_| {});
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
    let harness = harness(&server, RedirectMode::DeepLink, true, |_| {});
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
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let harness = harness(
        &server,
        RedirectMode::Loopback { port },
        false,
        |authorize| {
            // The browser follows the redirect once the person approves.
            tokio::spawn(async move {
                let redirect = format!(
                    "{}?code=code-1&state={}",
                    query(&authorize, "redirect_uri"),
                    query(&authorize, "state")
                );
                reqwest::get(redirect).await.unwrap();
            });
        },
    );

    let settings = instance_settings(&server, RedirectMode::Loopback { port });
    harness.engine.login(settings).await.unwrap();

    assert!(matches!(
        harness.sink.last().auth,
        AuthView::SignedIn { .. }
    ));
    assert!(harness.tokens.load(&harness.account).unwrap().is_some());
    assert!(harness.engine.settings().instance.redirect == RedirectMode::Loopback { port });
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
    let harness = harness(&server, RedirectMode::DeepLink, false, move |url| {
        recorder.lock().unwrap().push(url);
    });
    let settings = instance_settings(&server, RedirectMode::DeepLink);

    // Cancelled: back to the sign-in screen without an error.
    let engine = harness.engine.clone();
    let attempt = tokio::spawn({
        let settings = settings.clone();
        async move { engine.login(settings).await }
    });
    while opened.lock().unwrap().is_empty() {
        tokio::task::yield_now().await;
    }
    assert!(matches!(
        harness.sink.last().auth,
        AuthView::SigningIn { .. }
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
