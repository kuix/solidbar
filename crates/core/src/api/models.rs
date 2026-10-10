//! JSON shapes of the solidtime v1 API (`app/Http/Resources/V1/*`).

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::model::TimeEntry;
use crate::time::{WeekStart, to_api};

#[derive(Deserialize)]
pub(crate) struct Data<T> {
    pub data: T,
}

#[derive(Deserialize)]
pub(crate) struct TimeEntryList {
    pub data: Vec<TimeEntryResource>,
    pub meta: ListMeta,
}

#[derive(Deserialize)]
pub(crate) struct ListMeta {
    pub total: u64,
}

/// A Laravel `paginate()` response.
#[derive(Deserialize)]
pub(crate) struct Page<T> {
    pub data: Vec<T>,
    pub meta: PageMeta,
}

#[derive(Deserialize)]
pub(crate) struct PageMeta {
    pub current_page: u32,
    pub last_page: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Me {
    pub id: String,
    pub name: String,
    pub email: String,
    pub timezone: String,
    pub week_start: WeekStart,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    /// The member id used in `member_id` filters and payloads.
    pub id: String,
    pub organization: Organization,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Organization {
    pub id: String,
    pub name: String,
    pub currency: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct TimeEntryResource {
    pub id: String,
    pub start: Timestamp,
    pub end: Option<Timestamp>,
    pub description: Option<String>,
    pub task_id: Option<String>,
    pub project_id: Option<String>,
    pub organization_id: String,
    pub billable: bool,
}

impl From<TimeEntryResource> for TimeEntry {
    fn from(resource: TimeEntryResource) -> Self {
        TimeEntry {
            id: resource.id,
            start: resource.start,
            end: resource.end,
            project_id: resource.project_id,
            task_id: resource.task_id,
            description: resource.description.unwrap_or_default(),
            billable: resource.billable,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub color: String,
    pub client_id: Option<String>,
    pub is_archived: bool,
    /// Default for the `billable` flag of new entries on this project.
    pub is_billable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub is_done: bool,
    pub project_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Client {
    pub id: String,
    pub name: String,
    pub is_archived: bool,
}

/// Body of `POST /organizations/{org}/time-entries`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NewTimeEntry {
    pub member_id: String,
    pub start: String,
    /// `None` starts a timer.
    pub end: Option<String>,
    pub billable: bool,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub description: Option<String>,
}

impl NewTimeEntry {
    pub fn new(member_id: &str, start: Timestamp, end: Option<Timestamp>) -> Self {
        Self {
            member_id: member_id.to_owned(),
            start: to_api(start),
            end: end.map(to_api),
            billable: false,
            project_id: None,
            task_id: None,
            description: None,
        }
    }
}

/// Body of `PUT /organizations/{org}/time-entries/{id}`. Fields left out stay unchanged.
/// `start` and `end` are always sent together: the API validates `end >= start` only against
/// a `start` in the same request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TimeEntryChanges {
    pub start: String,
    pub end: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billable: Option<bool>,
    /// `Some(None)` clears the project (and requires clearing the task too).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
}

impl TimeEntryChanges {
    pub fn times(start: Timestamp, end: Option<Timestamp>) -> Self {
        Self {
            start: to_api(start),
            end: end.map(to_api),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_changes_send_start_and_end_but_nothing_else() {
        let changes = TimeEntryChanges::times(
            "2026-10-09T07:00:00Z".parse().unwrap(),
            Some("2026-10-09T08:00:00Z".parse().unwrap()),
        );
        assert_eq!(
            serde_json::to_value(&changes).unwrap(),
            serde_json::json!({ "start": "2026-10-09T07:00:00Z", "end": "2026-10-09T08:00:00Z" })
        );
    }

    #[test]
    fn clearing_a_project_sends_explicit_nulls() {
        let changes = TimeEntryChanges {
            project_id: Some(None),
            task_id: Some(None),
            ..TimeEntryChanges::times("2026-10-09T07:00:00Z".parse().unwrap(), None)
        };
        let json = serde_json::to_value(&changes).unwrap();
        assert_eq!(json["project_id"], serde_json::Value::Null);
        assert_eq!(json["task_id"], serde_json::Value::Null);
        assert!(json.get("description").is_none());
        assert_eq!(json["end"], serde_json::Value::Null);
    }

    #[test]
    fn time_entries_map_a_missing_description_to_empty() {
        let resource: TimeEntryResource = serde_json::from_value(serde_json::json!({
            "id": "e1", "start": "2026-10-09T07:00:00Z", "end": null, "duration": null,
            "description": null, "task_id": null, "project_id": "p1",
            "organization_id": "o1", "user_id": "u1", "tags": [], "billable": true, "type": "work",
        }))
        .unwrap();
        let entry = TimeEntry::from(resource);
        assert!(entry.is_running());
        assert_eq!(entry.description, "");
        assert_eq!(entry.project_id.as_deref(), Some("p1"));
    }
}
