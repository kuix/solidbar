//! What the engine knows, and the part of it that survives a restart.

use std::collections::{BTreeMap, HashSet};

use jiff::Timestamp;
use jiff::civil::Date;
use serde::{Deserialize, Serialize};
use solidbar_shared::instance::InstanceSettings;
use solidbar_shared::settings::Settings;

use crate::api::Api;
use crate::api::models::{Client, Me, Membership, Project, Task};
use crate::model::TimeEntry;
use crate::model::recents::WorkPair;
use crate::time::{Calendar, Week, WeekStart};

pub(crate) enum AuthPhase {
    SignedOut { error: Option<String> },
    SigningIn { instance: InstanceSettings },
    SignedIn { api: Api },
}

/// Data loaded from solidtime; cached on disk so the panel opens instantly.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Data {
    pub me: Option<Me>,
    pub memberships: Vec<Membership>,
    /// Membership id → first day of the week → that week's entries.
    pub weeks: BTreeMap<String, BTreeMap<Date, Vec<TimeEntry>>>,
    /// Organization id → its projects, tasks and clients.
    pub catalogs: BTreeMap<String, CatalogData>,
    /// Membership id → recently used project/task pairs.
    pub recents: BTreeMap<String, Vec<WorkPair>>,
    pub active_timer: Option<ActiveTimer>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CatalogData {
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub clients: Vec<Client>,
    pub fetched_at: Timestamp,
}

impl CatalogData {
    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|project| project.id == id)
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn client(&self, id: &str) -> Option<&Client> {
        self.clients.iter().find(|client| client.id == id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActiveTimer {
    pub organization_id: String,
    pub entry: TimeEntry,
}

pub(crate) struct State {
    pub settings: Settings,
    pub auth: AuthPhase,
    pub data: Data,
    pub selected_date: Date,
    /// Entries changed locally and not yet confirmed by solidtime.
    pub pending: HashSet<String>,
    pub online: bool,
    pub loading: bool,
    pub last_refresh: Option<Timestamp>,
}

impl State {
    pub fn calendar(&self) -> Calendar {
        match &self.data.me {
            Some(me) => Calendar::from_profile(&me.timezone, me.week_start),
            None => Calendar::new(jiff::tz::TimeZone::system(), WeekStart::Monday),
        }
    }

    pub fn api(&self) -> Option<Api> {
        match &self.auth {
            AuthPhase::SignedIn { api } => Some(api.clone()),
            _ => None,
        }
    }

    /// The membership new entries go to: the saved choice if it still exists, else the first.
    /// The organization new entries go to: the one chosen, or the only one. With several and
    /// none chosen (yet, or no longer available) there is none, and the UI asks.
    pub fn active_membership(&self) -> Option<&Membership> {
        let memberships = &self.data.memberships;
        let chosen = self
            .settings
            .active_membership_id
            .as_deref()
            .and_then(|id| memberships.iter().find(|membership| membership.id == id));
        match memberships.as_slice() {
            [only] => Some(chosen.unwrap_or(only)),
            _ => chosen,
        }
    }

    /// Signed in to several organizations without having picked one.
    pub fn must_choose_org(&self) -> bool {
        self.api().is_some()
            && self.data.memberships.len() > 1
            && self.active_membership().is_none()
    }

    pub fn membership_of_org(&self, organization_id: &str) -> Option<&Membership> {
        self.data
            .memberships
            .iter()
            .find(|membership| membership.organization.id == organization_id)
    }

    pub fn selected_week(&self) -> Week {
        self.calendar().week_of(self.selected_date)
    }

    /// Entries of a week of the active membership, if loaded.
    pub fn week_entries(&self, week: Week) -> Option<&Vec<TimeEntry>> {
        let membership = self.active_membership()?;
        self.data.weeks.get(&membership.id)?.get(&week.first_day)
    }

    /// Finds a loaded entry of the active membership by id, with the week it belongs to.
    pub fn find_entry(&self, id: &str) -> Option<(Week, &TimeEntry)> {
        let membership = self.active_membership()?;
        self.data
            .weeks
            .get(&membership.id)?
            .iter()
            .find_map(|(first_day, entries)| {
                entries.iter().find(|entry| entry.id == id).map(|entry| {
                    (
                        Week {
                            first_day: *first_day,
                        },
                        entry,
                    )
                })
            })
    }

    pub fn catalog(&self) -> Option<&CatalogData> {
        let membership = self.active_membership()?;
        self.data.catalogs.get(&membership.organization.id)
    }
}
