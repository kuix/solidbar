use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, RETRY_AFTER};
use reqwest::{Method, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::ApiError;
use super::models::{
    Client, Data, Me, Membership, NewTimeEntry, Page, Project, Task, TimeEntryChanges,
    TimeEntryList, TimeEntryResource,
};
use crate::auth::{AuthError, Session};
use crate::model::TimeEntry;
use crate::time::{Bounds, to_api};

/// The API's maximum `limit` for time entry lists.
const ENTRY_PAGE_SIZE: u64 = 500;

/// solidtime v1 API, authenticated through a [`Session`].
#[derive(Clone)]
pub struct Api {
    session: Arc<Session>,
}

impl Api {
    pub fn new(session: Arc<Session>) -> Self {
        Self { session }
    }

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    pub async fn me(&self) -> Result<Me, ApiError> {
        let me: Data<Me> = self.get("api/v1/users/me", &[]).await?;
        Ok(me.data)
    }

    pub async fn memberships(&self) -> Result<Vec<Membership>, ApiError> {
        let memberships: Data<Vec<Membership>> =
            self.get("api/v1/users/me/memberships", &[]).await?;
        Ok(memberships.data)
    }

    /// The running timer in any organization.
    pub async fn active_time_entry(&self) -> Result<Option<(String, TimeEntry)>, ApiError> {
        match self
            .get::<Data<TimeEntryResource>>("api/v1/users/me/time-entries/active", &[])
            .await
        {
            Ok(active) => Ok(Some((
                active.data.organization_id.clone(),
                active.data.into(),
            ))),
            Err(ApiError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The member's entries that start within `bounds`.
    pub async fn time_entries(
        &self,
        organization_id: &str,
        member_id: &str,
        bounds: Bounds,
    ) -> Result<Vec<TimeEntry>, ApiError> {
        let path = format!("api/v1/organizations/{organization_id}/time-entries");
        let mut entries = Vec::new();
        let mut offset = 0;
        loop {
            let query = [
                ("member_id", member_id.to_owned()),
                ("start", to_api(bounds.start)),
                ("end", to_api(bounds.end)),
                ("limit", ENTRY_PAGE_SIZE.to_string()),
                ("offset", offset.to_string()),
            ];
            let page: TimeEntryList = self.get(&path, &query).await?;
            let received = page.data.len() as u64;
            entries.extend(page.data.into_iter().map(TimeEntry::from));
            offset += received;
            if received == 0 || offset >= page.meta.total {
                break;
            }
        }
        // The `end` filter is inclusive; keep the interval half-open.
        entries.retain(|entry| bounds.contains(entry.start));
        Ok(entries)
    }

    pub async fn create_time_entry(
        &self,
        organization_id: &str,
        entry: &NewTimeEntry,
    ) -> Result<TimeEntry, ApiError> {
        let path = format!("api/v1/organizations/{organization_id}/time-entries");
        let created: Data<TimeEntryResource> =
            self.send(Method::POST, &path, &[], Some(entry)).await?;
        Ok(created.data.into())
    }

    pub async fn update_time_entry(
        &self,
        organization_id: &str,
        id: &str,
        changes: &TimeEntryChanges,
    ) -> Result<TimeEntry, ApiError> {
        let path = format!("api/v1/organizations/{organization_id}/time-entries/{id}");
        let updated: Data<TimeEntryResource> =
            self.send(Method::PUT, &path, &[], Some(changes)).await?;
        Ok(updated.data.into())
    }

    pub async fn delete_time_entry(&self, organization_id: &str, id: &str) -> Result<(), ApiError> {
        let path = format!("api/v1/organizations/{organization_id}/time-entries/{id}");
        self.respond(Method::DELETE, &path, &[], None::<&()>)
            .await?;
        Ok(())
    }

    pub async fn projects(&self, organization_id: &str) -> Result<Vec<Project>, ApiError> {
        self.all_pages(&format!("api/v1/organizations/{organization_id}/projects"))
            .await
    }

    pub async fn tasks(&self, organization_id: &str) -> Result<Vec<Task>, ApiError> {
        self.all_pages(&format!("api/v1/organizations/{organization_id}/tasks"))
            .await
    }

    pub async fn clients(&self, organization_id: &str) -> Result<Vec<Client>, ApiError> {
        self.all_pages(&format!("api/v1/organizations/{organization_id}/clients"))
            .await
    }

    async fn all_pages<T: DeserializeOwned>(&self, path: &str) -> Result<Vec<T>, ApiError> {
        let mut items = Vec::new();
        let mut page_number = 1;
        loop {
            let page: Page<T> = self.get(path, &[("page", page_number.to_string())]).await?;
            items.extend(page.data);
            if page.meta.current_page >= page.meta.last_page {
                return Ok(items);
            }
            page_number = page.meta.current_page + 1;
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T, ApiError> {
        self.send(Method::GET, path, query, None::<&()>).await
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&impl serde::Serialize>,
    ) -> Result<T, ApiError> {
        let response = self.respond(method, path, query, body).await?;
        response
            .json()
            .await
            .map_err(|error| ApiError::Unexpected(error.to_string()))
    }

    /// Sends the request, retrying once with a refreshed token if the token was rejected.
    async fn respond(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&impl serde::Serialize>,
    ) -> Result<Response, ApiError> {
        let token = self.session.access_token().await?;
        let mut response = self.dispatch(&method, path, query, body, &token).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            let token = self.session.refresh_after_rejection(&token).await?;
            response = self.dispatch(&method, path, query, body, &token).await?;
        }
        check(response).await
    }

    async fn dispatch(
        &self,
        method: &Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&impl serde::Serialize>,
        token: &str,
    ) -> Result<Response, ApiError> {
        let mut request = self
            .session
            .http()
            .request(method.clone(), self.session.instance().endpoint(path))
            .bearer_auth(token)
            .header(ACCEPT, "application/json")
            .query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        Ok(request.send().await?)
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    key: Option<String>,
    message: Option<String>,
}

async fn check(response: Response) -> Result<Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok())
        .map(Duration::from_secs);
    let body: Option<ErrorBody> = response.json().await.ok();
    let message = body.as_ref().and_then(|body| body.message.clone());
    Err(match status {
        // Still rejected after a refresh: the session was revoked.
        StatusCode::UNAUTHORIZED => ApiError::Auth(AuthError::SessionExpired),
        StatusCode::FORBIDDEN => ApiError::Forbidden,
        StatusCode::NOT_FOUND => ApiError::NotFound,
        StatusCode::UNPROCESSABLE_ENTITY => {
            ApiError::Invalid(message.unwrap_or_else(|| "solidtime rejected the change".to_owned()))
        }
        StatusCode::TOO_MANY_REQUESTS => ApiError::RateLimited { retry_after },
        StatusCode::BAD_REQUEST => match body {
            Some(ErrorBody {
                key: Some(key),
                message,
            }) => ApiError::Rejected {
                message: message.unwrap_or_else(|| key.clone()),
                key,
            },
            _ => ApiError::Unexpected(status.to_string()),
        },
        _ => ApiError::Unexpected(status.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{Instance, MemoryTokenStore, TokenSet};
    use crate::ports::testing::FixedClock;
    use solidbar_shared::instance::{InstanceSettings, RedirectMode};
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ORG: &str = "org-1";

    async fn api(server: &MockServer) -> Api {
        api_at(server.uri())
    }

    fn api_at(url: String) -> Api {
        let instance = Instance::new(&InstanceSettings {
            url,
            client_id: "client-1".to_owned(),
            redirect: RedirectMode::DeepLink,
        })
        .unwrap();
        let session = Session::start(
            reqwest::Client::new(),
            instance,
            Arc::new(MemoryTokenStore::default()),
            Arc::new(FixedClock::at("2026-10-09T10:00:00Z")),
            TokenSet {
                access_token: "token-1".to_owned(),
                refresh_token: "refresh-1".to_owned(),
                expires_at: "2026-10-09T11:00:00Z".parse().unwrap(),
            },
        )
        .unwrap();
        Api::new(Arc::new(session))
    }

    fn entry_json(id: &str, start: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "start": start, "end": null, "duration": null, "description": "Work",
            "task_id": null, "project_id": null, "organization_id": ORG, "user_id": "u1",
            "tags": [], "billable": false, "type": "work",
        })
    }

    fn week() -> Bounds {
        Bounds {
            start: "2026-10-04T22:00:00Z".parse().unwrap(),
            end: "2026-10-11T22:00:00Z".parse().unwrap(),
        }
    }

    #[tokio::test]
    async fn sends_the_bearer_token_and_reads_memberships() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/me/memberships"))
            .and(header("authorization", "Bearer token-1"))
            .and(header("accept", "application/json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "id": "member-1", "role": "owner",
                           "organization": { "id": ORG, "name": "Kuix", "currency": "EUR" } }]
            })))
            .mount(&server)
            .await;
        let memberships = api(&server).await.memberships().await.unwrap();
        assert_eq!(memberships[0].id, "member-1");
        assert_eq!(memberships[0].organization.name, "Kuix");
    }

    #[tokio::test]
    async fn retries_once_with_a_refreshed_token_after_a_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/me"))
            .and(header("authorization", "Bearer token-1"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "expires_in": 3600, "access_token": "token-2", "refresh_token": "refresh-2",
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/me"))
            .and(header("authorization", "Bearer token-2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "id": "u1", "name": "Barnabás", "email": "b@example.com",
                          "timezone": "Europe/Budapest", "week_start": "monday" }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let me = api(&server).await.me().await.unwrap();
        assert_eq!(me.timezone, "Europe/Budapest");
    }

    #[tokio::test]
    async fn pages_through_time_entries_with_limit_and_offset() {
        let server = MockServer::start().await;
        let first: Vec<_> = (0..500)
            .map(|index| entry_json(&format!("a{index}"), "2026-10-06T08:00:00Z"))
            .collect();
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
            .and(query_param("member_id", "member-1"))
            .and(query_param("start", "2026-10-04T22:00:00Z"))
            .and(query_param("end", "2026-10-11T22:00:00Z"))
            .and(query_param("limit", "500"))
            .and(query_param("offset", "0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": first, "meta": { "total": 502 }
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
            .and(query_param("offset", "500"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    entry_json("b1", "2026-10-07T08:00:00Z"),
                    entry_json("next-week", "2026-10-11T22:00:00Z"),
                ],
                "meta": { "total": 502 }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let entries = api(&server)
            .await
            .time_entries(ORG, "member-1", week())
            .await
            .unwrap();
        assert_eq!(entries.len(), 501);
        assert!(entries.iter().all(|entry| entry.id != "next-week"));
    }

    #[tokio::test]
    async fn follows_laravel_pagination_for_projects() {
        let server = MockServer::start().await;
        for page in 1..=2 {
            Mock::given(method("GET"))
                .and(path(format!("/api/v1/organizations/{ORG}/projects")))
                .and(query_param("page", page.to_string()))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": [{ "id": format!("p{page}"), "name": "Design", "color": "#ef5350",
                               "client_id": null, "is_archived": false, "billable_rate": null,
                               "is_billable": true, "estimated_time": null, "spent_time": 0,
                               "is_public": false }],
                    "links": {},
                    "meta": { "current_page": page, "last_page": 2, "per_page": 15, "total": 2 }
                })))
                .expect(1)
                .mount(&server)
                .await;
        }
        let projects = api(&server).await.projects(ORG).await.unwrap();
        let ids: Vec<_> = projects.iter().map(|project| project.id.as_str()).collect();
        assert_eq!(ids, ["p1", "p2"]);
    }

    #[tokio::test]
    async fn no_active_timer_is_not_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/users/me/time-entries/active"))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "No active time entry"
            })))
            .mount(&server)
            .await;
        assert_eq!(api(&server).await.active_time_entry().await, Ok(None));
    }

    #[tokio::test]
    async fn creates_entries_with_the_expected_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/api/v1/organizations/{ORG}/time-entries")))
            .and(body_json(serde_json::json!({
                "member_id": "member-1", "start": "2026-10-09T07:00:00Z",
                "end": "2026-10-09T09:00:00Z", "billable": true, "project_id": "p1",
                "task_id": null, "description": "Header",
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "data": entry_json("new", "2026-10-09T07:00:00Z")
            })))
            .expect(1)
            .mount(&server)
            .await;
        let entry = NewTimeEntry {
            billable: true,
            project_id: Some("p1".to_owned()),
            description: Some("Header".to_owned()),
            ..NewTimeEntry::new(
                "member-1",
                "2026-10-09T07:00:00Z".parse().unwrap(),
                Some("2026-10-09T09:00:00Z".parse().unwrap()),
            )
        };
        let created = api(&server)
            .await
            .create_time_entry(ORG, &entry)
            .await
            .unwrap();
        assert_eq!(created.id, "new");
    }

    #[tokio::test]
    async fn maps_business_rule_rejections() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path(format!("/api/v1/organizations/{ORG}/time-entries/e1")))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": true, "key": "overlapping_time_entry",
                "message": "Time entry overlaps with another time entry",
            })))
            .mount(&server)
            .await;
        let changes = TimeEntryChanges::times("2026-10-09T07:00:00Z".parse().unwrap(), None);
        let error = api(&server)
            .await
            .update_time_entry(ORG, "e1", &changes)
            .await
            .unwrap_err();
        assert!(error.is_overlap());
        assert_eq!(
            error.to_string(),
            "Time entry overlaps with another time entry"
        );
    }

    #[tokio::test]
    async fn reports_rate_limits_and_validation_errors() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path(format!("/api/v1/organizations/{ORG}/time-entries/e1")))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "12"))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(format!("/api/v1/organizations/{ORG}/time-entries/e2")))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "The end must be after the start.", "errors": {}
            })))
            .mount(&server)
            .await;
        let api = api(&server).await;
        assert_eq!(
            api.delete_time_entry(ORG, "e1").await,
            Err(ApiError::RateLimited {
                retry_after: Some(Duration::from_secs(12))
            })
        );
        assert_eq!(
            api.delete_time_entry(ORG, "e2").await,
            Err(ApiError::Invalid(
                "The end must be after the start.".to_owned()
            ))
        );
    }

    #[tokio::test]
    async fn unreachable_servers_read_as_offline() {
        // Bind and release a port so nothing listens on it.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let api = api_at(format!("http://127.0.0.1:{port}"));
        let error = api.memberships().await.unwrap_err();
        assert!(error.is_offline(), "{error:?}");
    }
}
