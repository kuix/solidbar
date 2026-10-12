//! What AI agents may do through the MCP server in [`crate::mcp`]: read the projects and time
//! entries, add entries and change them.
//!
//! The rules are the panel's: everything happens in the organization chosen in Solidbar,
//! dates and times are wall-clock times in the person's solidtime time zone, and an entry
//! without a start time starts where the day's last entry ends.

use std::collections::HashMap;

use jiff::civil::{Date, Time};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Deserializer, Serialize};
use solidbar_shared::duration::{format_hm, parse_minutes};
use solidbar_shared::ipc::CommandError;
use solidbar_shared::view::{EntryKind, NoticeKind};

use super::state::CatalogData;
use super::{CATALOG_TTL_FOR_FORM, Engine, changes_between, find_entry, workday_start};
use crate::api::models::Membership;
use crate::model::{TimeEntry, stacking};
use crate::time::{Bounds, Calendar};

/// More than this many entries in one call is more likely a mistake than a plan.
pub const MAX_NEW_ENTRIES: usize = 100;
/// The longest range one listing covers.
pub const MAX_LISTED_DAYS: i32 = 366;
/// Entries agents have seen, so they can change them; forgotten past this many.
const REMEMBERED_ENTRIES: usize = 5000;

/// Where an agent's requests land.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AgentScope {
    pub organization: String,
    /// IANA name of the zone every date and time is in.
    pub time_zone: String,
    /// `YYYY-MM-DD`
    pub today: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectListing {
    #[serde(flatten)]
    pub scope: AgentScope,
    pub projects: Vec<ProjectInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryListing {
    #[serde(flatten)]
    pub scope: AgentScope,
    pub from: String,
    pub to: String,
    /// `H:MM` of work, breaks left out.
    pub total: String,
    pub total_minutes: i64,
    pub entries: Vec<EntryInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AddedEntries {
    #[serde(flatten)]
    pub scope: AgentScope,
    pub added: Vec<EntryInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct UpdatedEntry {
    #[serde(flatten)]
    pub scope: AgentScope,
    pub entry: EntryInfo,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub client: Option<String>,
    /// New entries of the project are billable unless told otherwise.
    pub billable: bool,
    pub archived: bool,
    pub tasks: Vec<TaskInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaskInfo {
    pub id: String,
    pub name: String,
    pub done: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryInfo {
    pub id: String,
    /// `YYYY-MM-DD`, the day the entry starts.
    pub date: String,
    /// `HH:MM`
    pub start_time: String,
    /// `HH:MM`; `None` while the timer runs.
    pub end_time: Option<String>,
    /// Only for an entry that ends on a later day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// `H:MM`
    pub duration: String,
    pub duration_minutes: i64,
    pub running: bool,
    pub kind: EntryKind,
    pub project_id: Option<String>,
    pub project: Option<String>,
    pub task_id: Option<String>,
    pub task: Option<String>,
    pub client: Option<String>,
    pub description: String,
    pub billable: bool,
}

/// A duration as agents write it: `"1:30"`, `"1.5"`, `"90m"`, `"1h30m"`, or a number of hours.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum DurationInput {
    Text(String),
    Hours(f64),
}

impl DurationInput {
    fn minutes(&self) -> Result<u32, String> {
        let text = match self {
            Self::Text(text) => text.clone(),
            Self::Hours(hours) => hours.to_string(),
        };
        parse_minutes(&text).map_err(|error| format!("duration \"{text}\": {error}"))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewEntry {
    pub date: String,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub duration: Option<DurationInput>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub billable: Option<bool>,
}

/// Changes to an entry; what's left out stays as it is.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryChanges {
    pub id: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub start_time: Option<String>,
    #[serde(default)]
    pub end_time: Option<String>,
    #[serde(default)]
    pub duration: Option<DurationInput>,
    /// `Some(None)`, sent as `null`, removes the project.
    #[serde(default, deserialize_with = "present")]
    pub project_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    pub task_id: Option<Option<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub billable: Option<bool>,
}

/// Tells a field sent as `null` from one left out.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(deserializer).map(Some)
}

/// Why adding entries stopped, and the ones added before it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddFailure {
    pub message: String,
    pub added: Vec<EntryInfo>,
}

impl From<CommandError> for AddFailure {
    fn from(error: CommandError) -> Self {
        Self {
            message: error.message,
            added: Vec::new(),
        }
    }
}

#[derive(Default)]
pub(super) struct SeenEntries(HashMap<String, (String, TimeEntry)>);

impl Engine {
    /// The organization, time zone and today, for the agent to read dates and times right.
    pub fn agent_scope(&self) -> Result<AgentScope, CommandError> {
        let state = self.state();
        if state.api().is_none() {
            return Err(signed_out());
        }
        let membership = state.active_membership().ok_or_else(no_organization)?;
        let calendar = state.calendar();
        let time_zone = calendar
            .time_zone
            .iana_name()
            .map(str::to_owned)
            .or_else(|| state.data.me.as_ref().map(|me| me.timezone.clone()))
            .unwrap_or_else(|| "UTC".to_owned());
        Ok(AgentScope {
            organization: membership.organization.name.clone(),
            time_zone,
            today: calendar.date_of(self.deps.clock.now()).to_string(),
        })
    }

    /// The active organization's projects with their tasks, by name.
    pub async fn agent_projects(
        &self,
        include_archived: bool,
    ) -> Result<ProjectListing, CommandError> {
        let scope = self.agent_scope()?;
        let catalog = self.agent_catalog().await?;
        let mut projects: Vec<ProjectInfo> = catalog
            .projects
            .iter()
            .filter(|project| include_archived || !project.is_archived)
            .map(|project| {
                let mut tasks: Vec<TaskInfo> = catalog
                    .tasks
                    .iter()
                    .filter(|task| task.project_id == project.id)
                    .map(|task| TaskInfo {
                        id: task.id.clone(),
                        name: task.name.clone(),
                        done: task.is_done,
                    })
                    .collect();
                tasks.sort_by_key(|task| (task.done, task.name.to_lowercase()));
                ProjectInfo {
                    id: project.id.clone(),
                    name: project.name.clone(),
                    client: project
                        .client_id
                        .as_deref()
                        .and_then(|id| catalog.client(id))
                        .map(|client| client.name.clone()),
                    billable: project.is_billable,
                    archived: project.is_archived,
                    tasks,
                }
            })
            .collect();
        projects.sort_by_key(|project| (project.archived, project.name.to_lowercase()));
        Ok(ProjectListing { scope, projects })
    }

    /// The person's entries starting from `from` through `to` (both default to today).
    pub async fn agent_entries(
        &self,
        from: Option<&str>,
        to: Option<&str>,
    ) -> Result<EntryListing, CommandError> {
        let scope = self.agent_scope()?;
        let today: Date = scope.today.parse().map_err(|_| no_organization())?;
        let from = from.map(|text| parse_date("from", text)).transpose()?;
        let to = to.map(|text| parse_date("to", text)).transpose()?;
        let (from, to) = match (from, to) {
            (None, None) => (today, today),
            (Some(from), None) => (from, from.max(today)),
            (None, Some(to)) => (to.min(today), to),
            (Some(from), Some(to)) => (from, to),
        };
        if to < from {
            return Err(CommandError::new("`to` is before `from`"));
        }
        if (to - from).get_days() >= MAX_LISTED_DAYS {
            return Err(CommandError::new(format!(
                "List at most {MAX_LISTED_DAYS} days at a time"
            )));
        }
        let api = self.api()?;
        let membership = self.agent_membership()?;
        let catalog = self.agent_catalog().await?;
        let calendar = self.state().calendar();
        let bounds = Bounds {
            start: calendar.day_bounds(from).start,
            end: calendar.day_bounds(to).end,
        };
        let mut entries = api
            .time_entries(&membership.organization.id, &membership.id, bounds)
            .await
            .map_err(|error| self.fail(error))?;
        entries.sort_by_key(|entry| entry.start);
        self.remember(&membership.organization.id, &entries);
        let now = self.deps.clock.now();
        let entries: Vec<EntryInfo> = entries
            .iter()
            .map(|entry| entry_info(entry, &calendar, &catalog, now))
            .collect();
        let work_seconds: i64 = entries
            .iter()
            .filter(|entry| entry.kind.is_work())
            .map(|entry| entry.duration_minutes * 60)
            .sum();
        Ok(EntryListing {
            scope,
            from: from.to_string(),
            to: to.to_string(),
            total: format_hm(work_seconds),
            total_minutes: work_seconds / 60,
            entries,
        })
    }

    /// Adds entries in order. Nothing is added when one of them doesn't make sense; when
    /// solidtime refuses one, the ones before it stay added.
    pub async fn agent_add_entries(
        &self,
        requests: &[NewEntry],
    ) -> Result<AddedEntries, AddFailure> {
        if requests.is_empty() {
            return Err(CommandError::new("Give at least one entry").into());
        }
        if requests.len() > MAX_NEW_ENTRIES {
            return Err(CommandError::new(format!(
                "Add at most {MAX_NEW_ENTRIES} entries at a time"
            ))
            .into());
        }
        let _writes = self.writes.lock().await;
        let scope = self.agent_scope()?;
        let api = self.api()?;
        let membership = self.agent_membership()?;
        let catalog = self.agent_catalog().await?;
        let (calendar, workday) = {
            let state = self.state();
            (state.calendar(), workday_start(&state.settings))
        };

        let dates = requests
            .iter()
            .map(|request| parse_date("date", &request.date))
            .collect::<Vec<_>>();
        let problems = numbered_problems(dates.iter().map(|date| date.as_ref().err()));
        if !problems.is_empty() {
            return Err(CommandError::new(problems).into());
        }
        let dates: Vec<Date> = dates.into_iter().map(Result::unwrap).collect();
        let first = *dates.iter().min().expect("at least one entry");
        let last = *dates.iter().max().expect("at least one entry");
        let existing = api
            .time_entries(
                &membership.organization.id,
                &membership.id,
                Bounds {
                    start: calendar.day_bounds(first).start,
                    end: calendar.day_bounds(last).end,
                },
            )
            .await
            .map_err(|error| self.fail(error))?;
        let planned = plan_new_entries(requests, &calendar, &catalog, &existing, workday)
            .map_err(CommandError::new)?;

        let now = self.deps.clock.now();
        let mut added = Vec::new();
        for (index, entry) in planned.into_iter().enumerate() {
            match self.create(&api, &membership, entry).await {
                Ok(created) => added.push(created),
                Err(error) => {
                    self.after_agent_added(&membership, &added);
                    let message = match index {
                        0 => format!("solidtime didn't add the entry: {}", error.message),
                        index => format!(
                            "solidtime didn't add entry {}: {}. The {} before it were added.",
                            index + 1,
                            error.message,
                            index
                        ),
                    };
                    return Err(AddFailure {
                        message,
                        added: added
                            .iter()
                            .map(|entry| entry_info(entry, &calendar, &catalog, now))
                            .collect(),
                    });
                }
            }
        }
        self.after_agent_added(&membership, &added);
        let added = added
            .iter()
            .map(|entry| entry_info(entry, &calendar, &catalog, now))
            .collect();
        Ok(AddedEntries { scope, added })
    }

    /// Changes an entry the agent has seen (in a listing, or one it added).
    pub async fn agent_update_entry(
        &self,
        changes: &EntryChanges,
    ) -> Result<UpdatedEntry, CommandError> {
        let _writes = self.writes.lock().await;
        let scope = self.agent_scope()?;
        let api = self.api()?;
        let membership = self.agent_membership()?;
        let (organization_id, original) = self.recall(&changes.id).ok_or_else(|| {
            CommandError::new(format!(
                "There's no time entry {} among the ones listed; find it with list_time_entries first",
                changes.id
            ))
        })?;
        if organization_id != membership.organization.id {
            return Err(CommandError::new(
                "This entry belongs to another organization than the one chosen in Solidbar",
            ));
        }
        let catalog = self.agent_catalog().await?;
        let calendar = self.state().calendar();
        let now = self.deps.clock.now();
        let edited = apply_changes(&original, changes, &calendar, &catalog, now)
            .map_err(CommandError::new)?;
        if edited == original {
            let entry = entry_info(&original, &calendar, &catalog, now);
            return Ok(UpdatedEntry { scope, entry });
        }
        let change = changes_between(&original, &edited);
        let updated = self
            .update(&api, &organization_id, original, edited, &change)
            .await?;
        self.remember(&organization_id, std::slice::from_ref(&updated));
        self.notice(NoticeKind::Info, "An AI agent changed a time entry");
        self.persist();
        let entry = entry_info(&updated, &calendar, &catalog, now);
        Ok(UpdatedEntry { scope, entry })
    }

    fn after_agent_added(&self, membership: &Membership, added: &[TimeEntry]) {
        if added.is_empty() {
            return;
        }
        self.remember(&membership.organization.id, added);
        let message = match added.len() {
            1 => "An AI agent added a time entry".to_owned(),
            count => format!("An AI agent added {count} time entries"),
        };
        self.notice(NoticeKind::Info, &message);
        self.persist();
    }

    fn agent_membership(&self) -> Result<Membership, CommandError> {
        self.state()
            .active_membership()
            .cloned()
            .ok_or_else(no_organization)
    }

    /// The active organization's projects, tasks and clients, at most a minute old, so a
    /// project created a moment ago is there.
    async fn agent_catalog(&self) -> Result<CatalogData, CommandError> {
        let api = self.api()?;
        match self.load_catalog(&api, CATALOG_TTL_FOR_FORM).await {
            Ok(true) => {
                self.emit_catalog();
                self.persist();
            }
            Ok(false) => {}
            Err(error) => return Err(self.fail(error)),
        }
        self.state().catalog().cloned().ok_or_else(no_organization)
    }

    fn remember(&self, organization_id: &str, entries: &[TimeEntry]) {
        let mut seen = self.agent_seen.lock().expect("no panics while holding it");
        if seen.0.len() + entries.len() > REMEMBERED_ENTRIES {
            seen.0.clear();
        }
        for entry in entries {
            seen.0.insert(
                entry.id.clone(),
                (organization_id.to_owned(), entry.clone()),
            );
        }
    }

    /// An entry agents have seen, or one the panel has loaded, with its organization.
    fn recall(&self, id: &str) -> Option<(String, TimeEntry)> {
        if let Some(found) = find_entry(&self.state(), id) {
            return Some(found);
        }
        let seen = self.agent_seen.lock().expect("no panics while holding it");
        seen.0.get(id).cloned()
    }
}

/// The entries to create, in order: validated, with their times worked out. Entries without a
/// start time stack after the day's last entry, including the ones planned before them.
fn plan_new_entries(
    requests: &[NewEntry],
    calendar: &Calendar,
    catalog: &CatalogData,
    existing: &[TimeEntry],
    workday_start: Time,
) -> Result<Vec<TimeEntry>, String> {
    let mut day_entries = existing.to_vec();
    let mut planned = Vec::new();
    let mut problems = Vec::new();
    for request in requests {
        match plan_entry(request, calendar, catalog, &day_entries, workday_start) {
            Ok(entry) => {
                day_entries.push(entry.clone());
                planned.push(entry);
                problems.push(None);
            }
            Err(problem) => problems.push(Some(problem)),
        }
    }
    let problems = numbered_problems(problems.iter().map(Option::as_ref));
    if problems.is_empty() {
        Ok(planned)
    } else {
        Err(problems)
    }
}

fn plan_entry(
    request: &NewEntry,
    calendar: &Calendar,
    catalog: &CatalogData,
    day_entries: &[TimeEntry],
    workday_start: Time,
) -> Result<TimeEntry, String> {
    let date = parse_date("date", &request.date).map_err(|error| error.message)?;
    let work = resolve_work(catalog, given(&request.project_id), given(&request.task_id))?;
    let start = match given(&request.start_time) {
        Some(text) => calendar.at(date, parse_clock("start_time", text)?),
        None => stacking::default_start(calendar, date, day_entries, workday_start),
    };
    let end = match (&request.duration, given(&request.end_time)) {
        (Some(_), Some(_)) => return Err("give a duration or an end_time, not both".to_owned()),
        (None, None) => return Err("give a duration or an end_time".to_owned()),
        (Some(duration), None) => {
            stacking::interval_on(calendar, date, start, duration.minutes()?)
                .map_err(|error| error.to_string())?
                .end
        }
        (None, Some(text)) => {
            let end = calendar.at(date, parse_clock("end_time", text)?);
            if end <= start {
                return Err(format!(
                    "end_time {text} isn't after the start, {}",
                    wall_clock(start, calendar)
                ));
            }
            end
        }
    };
    Ok(TimeEntry {
        id: String::new(),
        start,
        end: Some(end),
        project_id: work.project_id,
        task_id: work.task_id,
        description: request
            .description
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_owned(),
        billable: request.billable.unwrap_or(work.billable),
        kind: EntryKind::Work,
    })
}

/// `original` with `changes` applied.
fn apply_changes(
    original: &TimeEntry,
    changes: &EntryChanges,
    calendar: &Calendar,
    catalog: &CatalogData,
    now: Timestamp,
) -> Result<TimeEntry, String> {
    let zoned = original.start.to_zoned(calendar.time_zone.clone());
    let date = match given(&changes.date) {
        Some(text) => parse_date("date", text).map_err(|error| error.message)?,
        None => zoned.date(),
    };
    let start_time = match given(&changes.start_time) {
        Some(text) => parse_clock("start_time", text)?,
        None => zoned.time(),
    };
    let start = calendar.at(date, start_time);
    let end_time = given(&changes.end_time);
    let end = match original.end {
        None => {
            if changes.duration.is_some() || end_time.is_some() {
                return Err(
                    "this entry is the running timer; stop it in Solidbar before giving it an end"
                        .to_owned(),
                );
            }
            if start > now {
                return Err("a running timer can't start in the future".to_owned());
            }
            None
        }
        Some(old_end) => {
            let end = match (&changes.duration, end_time) {
                (Some(_), Some(_)) => {
                    return Err("give a duration or an end_time, not both".to_owned());
                }
                (Some(duration), None) => {
                    start + SignedDuration::from_mins(i64::from(duration.minutes()?))
                }
                (None, Some(text)) => calendar.at(date, parse_clock("end_time", text)?),
                (None, None) => start + old_end.duration_since(original.start),
            };
            if end <= start {
                return Err(format!(
                    "the end, {}, isn't after the start, {}",
                    wall_clock(end, calendar),
                    wall_clock(start, calendar)
                ));
            }
            if end > calendar.day_bounds(date).end {
                return Err(stacking::IntervalError::CrossesMidnight.to_string());
            }
            Some(end)
        }
    };

    let (project_id, task_id) = if changes.project_id.is_none() && changes.task_id.is_none() {
        (original.project_id.clone(), original.task_id.clone())
    } else {
        let project_id = match &changes.project_id {
            Some(project_id) => project_id.clone().filter(|id| !id.is_empty()),
            None => original.project_id.clone(),
        };
        let task_id = match &changes.task_id {
            Some(task_id) => task_id.clone().filter(|id| !id.is_empty()),
            // A task of the old project doesn't come along to the new one.
            None => original.task_id.clone().filter(|id| {
                catalog
                    .task(id)
                    .is_some_and(|task| project_id.as_deref() == Some(task.project_id.as_str()))
            }),
        };
        let work = resolve_work(catalog, project_id.as_deref(), task_id.as_deref())?;
        (work.project_id, work.task_id)
    };
    if original.is_break() && (project_id.is_some() || task_id.is_some()) {
        return Err("a break can't have a project or a task".to_owned());
    }
    Ok(TimeEntry {
        id: original.id.clone(),
        start,
        end,
        project_id,
        task_id,
        description: match &changes.description {
            Some(description) => description.trim().to_owned(),
            None => original.description.clone(),
        },
        billable: changes.billable.unwrap_or(original.billable),
        kind: original.kind,
    })
}

struct Work {
    project_id: Option<String>,
    task_id: Option<String>,
    /// The project's default.
    billable: bool,
}

/// Checks a project and task against the catalog; a task alone brings its project.
fn resolve_work(
    catalog: &CatalogData,
    project_id: Option<&str>,
    task_id: Option<&str>,
) -> Result<Work, String> {
    let task = task_id
        .map(|id| {
            catalog
                .task(id)
                .ok_or_else(|| format!("there's no task {id}; list_projects lists them"))
        })
        .transpose()?;
    let project_id = match (project_id, task) {
        (Some(project_id), Some(task)) if task.project_id != project_id => {
            return Err(format!(
                "task {} belongs to another project than {project_id}",
                task.id
            ));
        }
        (None, Some(task)) => Some(task.project_id.as_str()),
        (project_id, _) => project_id,
    };
    let project = project_id
        .map(|id| {
            catalog
                .project(id)
                .ok_or_else(|| format!("there's no project {id}; list_projects lists them"))
        })
        .transpose()?;
    if let Some(project) = project
        && project.is_archived
    {
        return Err(format!("the project {} is archived", project.name));
    }
    Ok(Work {
        project_id: project.map(|project| project.id.clone()),
        task_id: task.map(|task| task.id.clone()),
        billable: project.is_some_and(|project| project.is_billable),
    })
}

fn entry_info(
    entry: &TimeEntry,
    calendar: &Calendar,
    catalog: &CatalogData,
    now: Timestamp,
) -> EntryInfo {
    let start = entry.start.to_zoned(calendar.time_zone.clone());
    let end = entry
        .end
        .map(|end| end.to_zoned(calendar.time_zone.clone()));
    let project = entry
        .project_id
        .as_deref()
        .and_then(|id| catalog.project(id));
    let seconds = entry.duration_seconds(now);
    EntryInfo {
        id: entry.id.clone(),
        date: start.date().to_string(),
        start_time: start.strftime("%H:%M").to_string(),
        end_time: end.as_ref().map(|end| end.strftime("%H:%M").to_string()),
        end_date: end
            .as_ref()
            .filter(|end| end.date() != start.date())
            .map(|end| end.date().to_string()),
        duration: format_hm(seconds),
        duration_minutes: seconds / 60,
        running: entry.is_running(),
        kind: entry.kind,
        project_id: entry.project_id.clone(),
        project: project.map(|project| project.name.clone()),
        task_id: entry.task_id.clone(),
        task: entry
            .task_id
            .as_deref()
            .and_then(|id| catalog.task(id))
            .map(|task| task.name.clone()),
        client: project
            .and_then(|project| project.client_id.as_deref())
            .and_then(|id| catalog.client(id))
            .map(|client| client.name.clone()),
        description: entry.description.clone(),
        billable: entry.billable,
    }
}

/// An optional text, where an empty one means "not given" (agents send `""` for those too).
fn given(text: &Option<String>) -> Option<&str> {
    text.as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn parse_date(field: &str, text: &str) -> Result<Date, CommandError> {
    text.trim()
        .parse()
        .map_err(|_| CommandError::new(format!("{field} \"{text}\" isn't a date like 2026-10-09")))
}

/// `HH:MM`, 24-hour.
fn parse_clock(field: &str, text: &str) -> Result<Time, String> {
    let invalid = || format!("{field} \"{text}\" isn't a 24-hour time like 09:30");
    let (hours, minutes) = text.trim().split_once(':').ok_or_else(invalid)?;
    let hours: i8 = hours.parse().map_err(|_| invalid())?;
    let minutes: i8 = minutes.parse().map_err(|_| invalid())?;
    Time::new(hours, minutes, 0, 0).map_err(|_| invalid())
}

fn wall_clock(instant: Timestamp, calendar: &Calendar) -> String {
    instant
        .to_zoned(calendar.time_zone.clone())
        .strftime("%H:%M")
        .to_string()
}

/// `"Entry 2: …; entry 5: …"` for the requests that have a problem; empty when none has.
fn numbered_problems<'a, P: std::fmt::Display + 'a>(
    problems: impl Iterator<Item = Option<&'a P>>,
) -> String {
    problems
        .enumerate()
        .filter_map(|(index, problem)| {
            problem.map(|problem| format!("entry {}: {problem}", index + 1))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn signed_out() -> CommandError {
    CommandError::new(
        "Solidbar isn't signed in to solidtime; sign in from Solidbar in the menu bar first",
    )
}

fn no_organization() -> CommandError {
    CommandError::new("Choose an organization in Solidbar first")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{Client, Project, Task};
    use crate::model::testing::{budapest, finished, running, ts};
    use jiff::civil::time;

    const NINE: Time = time(9, 0, 0, 0);

    fn catalog() -> CatalogData {
        CatalogData {
            projects: vec![
                Project {
                    id: "p1".into(),
                    name: "Design system".into(),
                    color: "#ef5350".into(),
                    client_id: Some("c1".into()),
                    is_archived: false,
                    is_billable: true,
                },
                Project {
                    id: "p2".into(),
                    name: "Internal".into(),
                    color: "#42a5f5".into(),
                    client_id: None,
                    is_archived: false,
                    is_billable: false,
                },
                Project {
                    id: "old".into(),
                    name: "Old site".into(),
                    color: "#000000".into(),
                    client_id: None,
                    is_archived: true,
                    is_billable: false,
                },
            ],
            tasks: vec![Task {
                id: "t1".into(),
                name: "Header".into(),
                is_done: false,
                project_id: "p1".into(),
            }],
            clients: vec![Client {
                id: "c1".into(),
                name: "Kuix".into(),
                is_archived: false,
            }],
            breaks_enabled: false,
            fetched_at: ts("2026-10-09T10:00:00Z"),
        }
    }

    fn request(date: &str) -> NewEntry {
        NewEntry {
            date: date.to_owned(),
            ..NewEntry::default()
        }
    }

    fn hours(text: &str) -> Option<DurationInput> {
        Some(DurationInput::Text(text.to_owned()))
    }

    #[test]
    fn entries_without_a_start_stack_after_the_day_and_each_other() {
        let existing = [finished(
            "e1",
            "2026-10-09T07:00:00Z",
            "2026-10-09T09:00:00Z",
        )];
        let requests = [
            NewEntry {
                duration: hours("1:30"),
                task_id: Some("t1".into()),
                description: Some("  Header  ".into()),
                ..request("2026-10-09")
            },
            NewEntry {
                duration: Some(DurationInput::Hours(2.0)),
                project_id: Some("p2".into()),
                ..request("2026-10-09")
            },
            NewEntry {
                duration: hours("45m"),
                project_id: Some("p1".into()),
                billable: Some(false),
                ..request("2026-10-12")
            },
        ];
        let planned =
            plan_new_entries(&requests, &budapest(), &catalog(), &existing, NINE).unwrap();

        assert_eq!(planned[0].start, ts("2026-10-09T09:00:00Z"));
        assert_eq!(planned[0].end, Some(ts("2026-10-09T10:30:00Z")));
        // The task brings its project, and the project's billable default.
        assert_eq!(planned[0].project_id.as_deref(), Some("p1"));
        assert!(planned[0].billable);
        assert_eq!(planned[0].description, "Header");
        assert_eq!(planned[1].start, ts("2026-10-09T10:30:00Z"));
        assert_eq!(planned[1].end, Some(ts("2026-10-09T12:30:00Z")));
        assert!(!planned[1].billable);
        // An empty day starts at the workday start (09:00 in Budapest is 07:00 UTC).
        assert_eq!(planned[2].start, ts("2026-10-12T07:00:00Z"));
        assert!(!planned[2].billable);
    }

    #[test]
    fn explicit_times_are_wall_clock_times() {
        let requests = [NewEntry {
            start_time: Some("13:15".into()),
            end_time: Some("14:00".into()),
            ..request("2026-10-09")
        }];
        let planned = plan_new_entries(&requests, &budapest(), &catalog(), &[], NINE).unwrap();
        assert_eq!(planned[0].start, ts("2026-10-09T11:15:00Z"));
        assert_eq!(planned[0].end, Some(ts("2026-10-09T12:00:00Z")));
        assert_eq!(planned[0].project_id, None);
    }

    #[test]
    fn every_problem_is_named_and_nothing_is_planned() {
        let requests = [
            NewEntry {
                duration: hours("1"),
                project_id: Some("p1".into()),
                ..request("2026-10-09")
            },
            NewEntry {
                start_time: Some("23:30".into()),
                duration: hours("1"),
                ..request("2026-10-09")
            },
            NewEntry {
                duration: hours("1"),
                project_id: Some("p2".into()),
                task_id: Some("t1".into()),
                ..request("2026-10-09")
            },
            NewEntry {
                duration: hours("1"),
                project_id: Some("old".into()),
                ..request("2026-10-09")
            },
            request("2026-10-09"),
            NewEntry {
                start_time: Some("9.30".into()),
                duration: hours("1"),
                ..request("2026-10-09")
            },
        ];
        let problems = plan_new_entries(&requests, &budapest(), &catalog(), &[], NINE).unwrap_err();
        assert_eq!(
            problems,
            "entry 2: A time entry can't continue past midnight; \
             entry 3: task t1 belongs to another project than p2; \
             entry 4: the project Old site is archived; \
             entry 5: give a duration or an end_time; \
             entry 6: start_time \"9.30\" isn't a 24-hour time like 09:30"
        );
    }

    #[test]
    fn changes_keep_what_isnt_mentioned() {
        let original = TimeEntry {
            project_id: Some("p1".into()),
            task_id: Some("t1".into()),
            description: "Header".into(),
            ..finished("e1", "2026-10-09T07:00:00Z", "2026-10-09T09:00:00Z")
        };
        let now = ts("2026-10-09T10:00:00Z");
        // Moving the start keeps the duration.
        let moved = apply_changes(
            &original,
            &EntryChanges {
                id: "e1".into(),
                start_time: Some("10:00".into()),
                ..EntryChanges::default()
            },
            &budapest(),
            &catalog(),
            now,
        )
        .unwrap();
        assert_eq!(moved.start, ts("2026-10-09T08:00:00Z"));
        assert_eq!(moved.end, Some(ts("2026-10-09T10:00:00Z")));
        assert_eq!(moved.task_id.as_deref(), Some("t1"));
        assert_eq!(moved.description, "Header");

        // Another project leaves the old project's task behind; null removes the project.
        let changes: EntryChanges =
            serde_json::from_str(r#"{ "id": "e1", "project_id": "p2", "duration": "30m" }"#)
                .unwrap();
        let other = apply_changes(&original, &changes, &budapest(), &catalog(), now).unwrap();
        assert_eq!(other.project_id.as_deref(), Some("p2"));
        assert_eq!(other.task_id, None);
        assert_eq!(other.end, Some(ts("2026-10-09T07:30:00Z")));
        let changes: EntryChanges =
            serde_json::from_str(r#"{ "id": "e1", "project_id": null }"#).unwrap();
        let none = apply_changes(&original, &changes, &budapest(), &catalog(), now).unwrap();
        assert_eq!((none.project_id, none.task_id), (None, None));
    }

    #[test]
    fn a_running_timer_has_no_end_to_change() {
        let timer = running("e1", "2026-10-09T08:00:00Z");
        let now = ts("2026-10-09T10:00:00Z");
        let changes = EntryChanges {
            id: "e1".into(),
            end_time: Some("12:00".into()),
            ..EntryChanges::default()
        };
        assert!(apply_changes(&timer, &changes, &budapest(), &catalog(), now).is_err());
        let earlier = EntryChanges {
            id: "e1".into(),
            start_time: Some("09:30".into()),
            description: Some("Standup".into()),
            ..EntryChanges::default()
        };
        let changed = apply_changes(&timer, &earlier, &budapest(), &catalog(), now).unwrap();
        assert_eq!(changed.start, ts("2026-10-09T07:30:00Z"));
        assert_eq!(changed.end, None);
        assert_eq!(changed.description, "Standup");
    }

    #[test]
    fn unknown_fields_are_refused() {
        let error =
            serde_json::from_str::<NewEntry>(r#"{ "date": "2026-10-09", "start": "09:00" }"#)
                .unwrap_err();
        assert!(
            error.to_string().contains("unknown field `start`"),
            "{error}"
        );
    }

    #[test]
    fn listed_entries_read_like_the_panel() {
        let entry = TimeEntry {
            project_id: Some("p1".into()),
            task_id: Some("t1".into()),
            ..finished("e1", "2026-10-09T07:00:00Z", "2026-10-09T08:30:00Z")
        };
        let info = entry_info(&entry, &budapest(), &catalog(), ts("2026-10-09T10:00:00Z"));
        assert_eq!(info.date, "2026-10-09");
        assert_eq!(info.start_time, "09:00");
        assert_eq!(info.end_time.as_deref(), Some("10:30"));
        assert_eq!(info.end_date, None);
        assert_eq!(info.duration, "1:30");
        assert_eq!(info.duration_minutes, 90);
        assert_eq!(info.project.as_deref(), Some("Design system"));
        assert_eq!(info.task.as_deref(), Some("Header"));
        assert_eq!(info.client.as_deref(), Some("Kuix"));
    }
}
