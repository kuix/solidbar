//! Projects the engine state into the view models the UI renders.

use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;
use solidbar_shared::duration::format_hm;
use solidbar_shared::view::{
    AppView, AuthView, Catalog, DayView, EntryKind, EntryView, OrgView, TimerView, TrayView,
    WeekView, WorkOption,
};

use super::state::{AuthPhase, CatalogData, State};
use crate::model::TimeEntry;
use crate::model::recents::WorkPair;
use crate::time::{Calendar, Week};

pub(crate) struct Clocks<'a> {
    pub now: Timestamp,
    pub hour12: bool,
    pub system_zone: &'a TimeZone,
}

pub(crate) fn app_view(state: &State, clocks: &Clocks) -> AppView {
    let calendar = state.calendar();
    let today = calendar.date_of(clocks.now);
    let signed_in = matches!(state.auth, AuthPhase::SignedIn { .. });

    let mut orgs: Vec<OrgView> = state
        .data
        .memberships
        .iter()
        .map(|membership| OrgView {
            membership_id: membership.id.clone(),
            name: membership.organization.name.clone(),
        })
        .collect();
    orgs.sort_by_key(|org| org.name.to_lowercase());

    let week = signed_in
        .then(|| week_view(state, &calendar, state.selected_week(), today, clocks))
        .flatten();

    AppView {
        auth: auth_view(state),
        settings: state.settings.clone(),
        orgs,
        active_membership_id: state
            .active_membership()
            .map(|membership| membership.id.clone()),
        choose_org: state.must_choose_org(),
        hour12: clocks.hour12,
        today: today.to_string(),
        selected_date: state.selected_date.to_string(),
        week,
        timer: signed_in.then(|| timer_view(state, clocks.now)).flatten(),
        online: state.online,
        loading: state.loading,
        time_zone_hint: time_zone_hint(state, &calendar, clocks.system_zone),
        tray: tray_view(state, &calendar, today, clocks),
        breaks_enabled: state.breaks_enabled(),
    }
}

fn auth_view(state: &State) -> AuthView {
    match &state.auth {
        AuthPhase::SignedOut { error } => AuthView::SignedOut {
            instance: state.settings.instance.clone(),
            error: error.clone(),
        },
        AuthPhase::SigningIn { instance } => AuthView::SigningIn {
            instance: instance.clone(),
        },
        AuthPhase::SignedIn { .. } => AuthView::SignedIn {
            user_name: state
                .data
                .me
                .as_ref()
                .map(|me| me.name.clone())
                .unwrap_or_default(),
            instance_url: state.settings.instance.url.clone(),
        },
    }
}

/// The week's entries, plus the running timer when the list doesn't have it yet.
fn entries_of_week(state: &State, calendar: &Calendar, week: Week) -> Option<Vec<TimeEntry>> {
    let mut entries = state.week_entries(week)?.clone();
    if let (Some(timer), Some(membership)) = (&state.data.active_timer, state.active_membership())
        && timer.organization_id == membership.organization.id
        && calendar.week_bounds(week).contains(timer.entry.start)
    {
        entries.retain(|entry| entry.id != timer.entry.id);
        entries.push(timer.entry.clone());
    }
    entries.sort_by_key(|entry| entry.start);
    Some(entries)
}

fn week_view(
    state: &State,
    calendar: &Calendar,
    week: Week,
    today: Date,
    clocks: &Clocks,
) -> Option<WeekView> {
    let entries = entries_of_week(state, calendar, week)?;
    let catalog = state.catalog();
    let days: Vec<DayView> = week
        .days()
        .into_iter()
        .map(|date| {
            let bounds = calendar.day_bounds(date);
            let entries: Vec<EntryView> = entries
                .iter()
                .filter(|entry| bounds.contains(entry.start))
                .map(|entry| entry_view(entry, catalog, calendar, state, clocks))
                .collect();
            let total = |kind: EntryKind| {
                entries
                    .iter()
                    .filter(|entry| entry.kind == kind)
                    .map(|entry| entry.duration_seconds)
                    .sum()
            };
            DayView {
                date: date.to_string(),
                weekday_short: date.strftime("%a").to_string(),
                label: date.strftime("%A, %b %-d").to_string(),
                is_today: date == today,
                total_seconds: total(EntryKind::Work),
                break_seconds: total(EntryKind::Break),
                entries,
            }
        })
        .collect();
    Some(WeekView {
        first_day: week.first_day.to_string(),
        label: week_label(week),
        total_seconds: days.iter().map(|day| day.total_seconds).sum(),
        days,
    })
}

fn entry_view(
    entry: &TimeEntry,
    catalog: Option<&CatalogData>,
    calendar: &Calendar,
    state: &State,
    clocks: &Clocks,
) -> EntryView {
    let project = catalog
        .zip(entry.project_id.as_deref())
        .and_then(|(catalog, id)| catalog.project(id));
    let task = catalog
        .zip(entry.task_id.as_deref())
        .and_then(|(catalog, id)| catalog.task(id));
    let client = catalog
        .zip(project.and_then(|project| project.client_id.as_deref()))
        .and_then(|(catalog, id)| catalog.client(id));
    EntryView {
        id: entry.id.clone(),
        kind: entry.kind,
        project_id: entry.project_id.clone(),
        task_id: entry.task_id.clone(),
        project_name: project.map(|project| project.name.clone()),
        task_name: task.map(|task| task.name.clone()),
        client_name: client.map(|client| client.name.clone()),
        color: project.map(|project| project.color.clone()),
        description: entry.description.clone(),
        billable: entry.billable,
        start_label: time_label(entry.start, calendar, clocks.hour12),
        end_label: entry
            .end
            .map(|end| time_label(end, calendar, clocks.hour12)),
        duration_seconds: entry.duration_seconds(clocks.now),
        running: entry.is_running(),
        pending: state.pending.contains(&entry.id),
    }
}

fn timer_view(state: &State, now: Timestamp) -> Option<TimerView> {
    let timer = state.data.active_timer.as_ref()?;
    let membership = state.membership_of_org(&timer.organization_id);
    let project_name = state
        .data
        .catalogs
        .get(&timer.organization_id)
        .zip(timer.entry.project_id.as_deref())
        .and_then(|(catalog, id)| catalog.project(id))
        .map(|project| project.name.clone());
    let resume_label = timer
        .entry
        .is_break()
        .then(|| state.last_work_entry(now))
        .flatten()
        .map(|work| work_label(work, state.catalog()));
    Some(TimerView {
        entry_id: timer.entry.id.clone(),
        membership_id: membership.map(|membership| membership.id.clone()),
        org_name: membership.map(|membership| membership.organization.name.clone()),
        started_at: timer.entry.start.to_string(),
        description: timer.entry.description.clone(),
        project_name,
        in_active_org: membership.map(|membership| &membership.id)
            == state.active_membership().map(|active| &active.id),
        kind: timer.entry.kind,
        resume_label,
    })
}

/// What an entry is about in a few words: its notes, else its project and task.
fn work_label(entry: &TimeEntry, catalog: Option<&CatalogData>) -> String {
    if !entry.description.is_empty() {
        return entry.description.clone();
    }
    let project = catalog
        .zip(entry.project_id.as_deref())
        .and_then(|(catalog, id)| catalog.project(id));
    let task = catalog
        .zip(entry.task_id.as_deref())
        .and_then(|(catalog, id)| catalog.task(id));
    match (project, task) {
        (Some(project), Some(task)) => format!("{} › {}", project.name, task.name),
        (Some(project), None) => project.name.clone(),
        (None, _) => "No project".to_owned(),
    }
}

fn tray_view(state: &State, calendar: &Calendar, today: Date, clocks: &Clocks) -> TrayView {
    if !matches!(state.auth, AuthPhase::SignedIn { .. }) {
        return TrayView {
            timer_running: false,
            title: None,
            tooltip: "Solidbar — not signed in".to_owned(),
        };
    }
    let (timer_running, time, tooltip) =
        match timer_view(state, clocks.now).zip(state.data.active_timer.as_ref()) {
            Some((_, active)) if active.entry.is_break() => {
                let elapsed = format_hm(active.entry.duration_seconds(clocks.now));
                (
                    true,
                    format!("☕ {elapsed}"),
                    format!("On a break, {elapsed}"),
                )
            }
            Some((timer, active)) => {
                let elapsed = format_hm(active.entry.duration_seconds(clocks.now));
                let what = timer
                    .project_name
                    .or_else(|| Some(timer.description).filter(|text| !text.is_empty()))
                    .unwrap_or_else(|| "No project".to_owned());
                (
                    true,
                    elapsed.clone(),
                    format!("Timer running: {what}, {elapsed}"),
                )
            }
            None => {
                let total = entries_of_week(state, calendar, calendar.week_of(today))
                    .unwrap_or_default()
                    .iter()
                    .filter(|entry| {
                        !entry.is_break() && calendar.day_bounds(today).contains(entry.start)
                    })
                    .map(|entry| entry.duration_seconds(clocks.now))
                    .sum();
                let total = format_hm(total);
                (false, total.clone(), format!("Today: {total}"))
            }
        };
    TrayView {
        timer_running,
        title: state.settings.menu_bar_text.then_some(time),
        tooltip,
    }
}

fn time_zone_hint(state: &State, calendar: &Calendar, system_zone: &TimeZone) -> Option<String> {
    state.data.me.as_ref()?;
    let profile = calendar.time_zone.iana_name()?;
    if Some(profile) == system_zone.iana_name() {
        return None;
    }
    Some(format!(
        "Times are shown in {profile}, the time zone of your solidtime profile."
    ))
}

pub(crate) fn catalog_view(state: &State) -> solidbar_shared::view::Catalog {
    let Some(catalog) = state.catalog() else {
        return Catalog::default();
    };
    let mut projects: Vec<_> = catalog
        .projects
        .iter()
        .filter(|project| !project.is_archived)
        .collect();
    projects.sort_by_key(|project| project.name.to_lowercase());

    let mut options = Vec::new();
    for project in projects {
        options.push(work_option(catalog, &project.id, None).expect("the project exists"));
        let mut tasks: Vec<_> = catalog
            .tasks
            .iter()
            .filter(|task| task.project_id == project.id && !task.is_done)
            .collect();
        tasks.sort_by_key(|task| task.name.to_lowercase());
        options.extend(
            tasks
                .into_iter()
                .filter_map(|task| work_option(catalog, &project.id, Some(&task.id))),
        );
    }
    options.push(no_project_option());
    if catalog.breaks_enabled {
        options.push(WorkOption {
            kind: EntryKind::Break,
            label: "Break".to_owned(),
            ..no_project_option()
        });
    }

    let recents = state
        .active_membership()
        .and_then(|membership| state.data.recents.get(&membership.id))
        .map(|pairs| {
            pairs
                .iter()
                .filter_map(
                    |WorkPair {
                         project_id,
                         task_id,
                     }| {
                        work_option(catalog, project_id, task_id.as_deref())
                    },
                )
                .collect()
        })
        .unwrap_or_default();

    Catalog { options, recents }
}

/// A picker option for an active project, optionally with one of its open tasks.
fn work_option(
    catalog: &CatalogData,
    project_id: &str,
    task_id: Option<&str>,
) -> Option<WorkOption> {
    let project = catalog
        .project(project_id)
        .filter(|project| !project.is_archived)?;
    let task = match task_id {
        Some(id) => Some(catalog.task(id).filter(|task| !task.is_done)?),
        None => None,
    };
    let label = match task {
        Some(task) => format!("{} › {}", project.name, task.name),
        None => project.name.clone(),
    };
    Some(WorkOption {
        kind: EntryKind::Work,
        project_id: Some(project.id.clone()),
        task_id: task.map(|task| task.id.clone()),
        label,
        client_name: project
            .client_id
            .as_deref()
            .and_then(|id| catalog.client(id))
            .map(|client| client.name.clone()),
        color: Some(project.color.clone()),
        billable: project.is_billable,
    })
}

fn no_project_option() -> WorkOption {
    WorkOption {
        kind: EntryKind::Work,
        project_id: None,
        task_id: None,
        label: "No project".to_owned(),
        client_name: None,
        color: None,
        billable: false,
    }
}

pub(crate) fn time_label(instant: Timestamp, calendar: &Calendar, hour12: bool) -> String {
    let zoned = instant.to_zoned(calendar.time_zone.clone());
    let format = if hour12 { "%-I:%M %p" } else { "%H:%M" };
    zoned.strftime(format).to_string()
}

/// `Oct 5 – 11, 2026`, `Sep 28 – Oct 4, 2026` or `Dec 28, 2026 – Jan 3, 2027`.
pub(crate) fn week_label(week: Week) -> String {
    let (first, last) = (week.first_day, week.last_day());
    if first.year() != last.year() {
        format!(
            "{} – {}",
            first.strftime("%b %-d, %Y"),
            last.strftime("%b %-d, %Y")
        )
    } else if first.month() != last.month() {
        format!(
            "{} – {}",
            first.strftime("%b %-d"),
            last.strftime("%b %-d, %Y")
        )
    } else {
        format!(
            "{} – {}",
            first.strftime("%b %-d"),
            last.strftime("%-d, %Y")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{Client, Me, Membership, Organization, Project, Task};
    use crate::engine::state::{ActiveTimer, Data};
    use crate::model::testing::{finished, running, ts};
    use crate::time::WeekStart;
    use jiff::civil::date;
    use solidbar_shared::settings::Settings;
    use std::collections::{BTreeMap, HashSet};

    fn membership(id: &str, org: &str, name: &str) -> Membership {
        Membership {
            id: id.to_owned(),
            organization: Organization {
                id: org.to_owned(),
                name: name.to_owned(),
                currency: "EUR".to_owned(),
            },
            role: "owner".to_owned(),
        }
    }

    fn project(id: &str, name: &str, archived: bool) -> Project {
        Project {
            id: id.to_owned(),
            name: name.to_owned(),
            color: "#ef5350".to_owned(),
            client_id: Some("c1".to_owned()),
            is_archived: archived,
            is_billable: true,
        }
    }

    fn task(id: &str, project: &str, name: &str, done: bool) -> Task {
        Task {
            id: id.to_owned(),
            name: name.to_owned(),
            is_done: done,
            project_id: project.to_owned(),
        }
    }

    fn with_project(mut entry: TimeEntry, project: &str, task: Option<&str>) -> TimeEntry {
        entry.project_id = Some(project.to_owned());
        entry.task_id = task.map(str::to_owned);
        entry
    }

    fn state() -> State {
        let week = date(2026, 10, 5);
        let entries = vec![
            with_project(
                finished("e1", "2026-10-09T07:00:00Z", "2026-10-09T08:30:00Z"),
                "p1",
                Some("t1"),
            ),
            finished("e2", "2026-10-09T09:00:00Z", "2026-10-09T09:30:00Z"),
            finished("monday", "2026-10-05T07:00:00Z", "2026-10-05T15:00:00Z"),
        ];
        let mut weeks = BTreeMap::new();
        weeks.insert("m1".to_owned(), BTreeMap::from([(week, entries)]));
        let mut catalogs = BTreeMap::new();
        catalogs.insert(
            "o1".to_owned(),
            CatalogData {
                projects: vec![
                    project("p1", "Design system", false),
                    project("p0", "Archived", true),
                    project("p2", "Admin", false),
                ],
                tasks: vec![
                    task("t1", "p1", "Header", false),
                    task("t2", "p1", "Done task", true),
                ],
                clients: vec![Client {
                    id: "c1".to_owned(),
                    name: "Kuix".to_owned(),
                    is_archived: false,
                }],
                breaks_enabled: false,
                fetched_at: ts("2026-10-09T07:00:00Z"),
            },
        );
        State {
            settings: Settings {
                active_membership_id: Some("m1".to_owned()),
                ..Settings::default()
            },
            auth: AuthPhase::SignedIn {
                api: crate::api::Api::new(std::sync::Arc::new(test_session())),
            },
            data: Data {
                me: Some(Me {
                    id: "u1".to_owned(),
                    name: "Barnabás".to_owned(),
                    email: "b@example.com".to_owned(),
                    timezone: "Europe/Budapest".to_owned(),
                    week_start: WeekStart::Monday,
                }),
                memberships: vec![
                    membership("m1", "o1", "Kuix"),
                    membership("m2", "o2", "ACME"),
                ],
                weeks,
                catalogs,
                recents: BTreeMap::from([(
                    "m1".to_owned(),
                    vec![
                        WorkPair {
                            project_id: "p1".to_owned(),
                            task_id: Some("t1".to_owned()),
                        },
                        WorkPair {
                            project_id: "p0".to_owned(),
                            task_id: None,
                        },
                    ],
                )]),
                active_timer: None,
            },
            selected_date: date(2026, 10, 9),
            pending: HashSet::from(["e2".to_owned()]),
            online: true,
            loading: false,
            last_refresh: None,
        }
    }

    fn test_session() -> crate::auth::Session {
        use solidbar_shared::instance::InstanceSettings;
        crate::auth::Session::start(
            reqwest::Client::new(),
            crate::auth::Instance::new(&InstanceSettings::cloud()).unwrap(),
            std::sync::Arc::new(crate::auth::MemoryTokenStore::default()),
            std::sync::Arc::new(crate::ports::SystemClock),
            crate::auth::TokenSet {
                access_token: "a".to_owned(),
                refresh_token: "r".to_owned(),
                expires_at: ts("2030-01-01T00:00:00Z"),
            },
        )
        .unwrap()
    }

    fn clocks<'a>(now: &str, zone: &'a TimeZone) -> Clocks<'a> {
        Clocks {
            now: ts(now),
            hour12: false,
            system_zone: zone,
        }
    }

    #[test]
    fn days_carry_entries_labels_and_totals() {
        let zone = TimeZone::get("Europe/Budapest").unwrap();
        let view = app_view(&state(), &clocks("2026-10-09T12:00:00Z", &zone));
        let week = view.week.unwrap();
        assert_eq!(week.label, "Oct 5 – 11, 2026");
        assert_eq!(week.days.len(), 7);
        assert_eq!(week.total_seconds, 10 * 3600);

        let friday = &week.days[4];
        assert_eq!(friday.weekday_short, "Fri");
        assert_eq!(friday.label, "Friday, Oct 9");
        assert!(friday.is_today);
        assert_eq!(friday.total_seconds, 2 * 3600);

        let first = &friday.entries[0];
        assert_eq!(first.project_name.as_deref(), Some("Design system"));
        assert_eq!(first.task_name.as_deref(), Some("Header"));
        assert_eq!(first.client_name.as_deref(), Some("Kuix"));
        assert_eq!(first.start_label, "09:00");
        assert_eq!(first.end_label.as_deref(), Some("10:30"));
        assert!(!first.pending);
        assert!(friday.entries[1].pending);
        assert_eq!(view.time_zone_hint, None);
    }

    #[test]
    fn the_tray_shows_todays_total_or_the_running_timer() {
        let zone = TimeZone::get("Europe/Budapest").unwrap();
        let mut state = state();
        let view = app_view(&state, &clocks("2026-10-09T12:00:00Z", &zone));
        assert_eq!(view.tray.title.as_deref(), Some("2:00"));
        assert_eq!(view.tray.tooltip, "Today: 2:00");

        state.data.active_timer = Some(ActiveTimer {
            organization_id: "o1".to_owned(),
            entry: with_project(running("live", "2026-10-09T11:18:00Z"), "p1", None),
        });
        let view = app_view(&state, &clocks("2026-10-09T12:00:00Z", &zone));
        assert!(view.tray.timer_running);
        assert_eq!(view.tray.title.as_deref(), Some("0:42"));
        assert_eq!(view.tray.tooltip, "Timer running: Design system, 0:42");
        // The running entry shows up in today's list and total.
        assert_eq!(view.week.unwrap().days[4].total_seconds, 2 * 3600 + 42 * 60);
        assert!(view.timer.unwrap().in_active_org);

        state.settings.menu_bar_text = false;
        let view = app_view(&state, &clocks("2026-10-09T12:00:00Z", &zone));
        assert_eq!(view.tray.title, None);
    }

    #[test]
    fn a_timer_in_another_organization_is_flagged() {
        let zone = TimeZone::get("Europe/Budapest").unwrap();
        let mut state = state();
        state.data.active_timer = Some(ActiveTimer {
            organization_id: "o2".to_owned(),
            entry: running("elsewhere", "2026-10-09T11:00:00Z"),
        });
        let view = app_view(&state, &clocks("2026-10-09T12:00:00Z", &zone));
        let timer = view.timer.unwrap();
        assert!(!timer.in_active_org);
        assert_eq!(timer.org_name.as_deref(), Some("ACME"));
        assert_eq!(view.week.unwrap().days[4].entries.len(), 2);
    }

    #[test]
    fn hints_when_the_profile_zone_differs_from_the_mac() {
        let zone = TimeZone::get("America/New_York").unwrap();
        let view = app_view(&state(), &clocks("2026-10-09T12:00:00Z", &zone));
        assert!(view.time_zone_hint.unwrap().contains("Europe/Budapest"));
    }

    #[test]
    fn catalog_lists_active_projects_and_open_tasks() {
        let catalog = catalog_view(&state());
        let labels: Vec<_> = catalog
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect();
        assert_eq!(
            labels,
            [
                "Admin",
                "Design system",
                "Design system › Header",
                "No project"
            ]
        );
        assert!(catalog.options[1].billable);
        assert_eq!(catalog.options[1].client_name.as_deref(), Some("Kuix"));
        // The archived project drops out of the recents.
        let recents: Vec<_> = catalog
            .recents
            .iter()
            .map(|option| option.label.as_str())
            .collect();
        assert_eq!(recents, ["Design system › Header"]);
    }

    #[test]
    fn week_labels_span_months_and_years() {
        let week = |first_day| Week { first_day };
        assert_eq!(week_label(week(date(2026, 9, 28))), "Sep 28 – Oct 4, 2026");
        assert_eq!(
            week_label(week(date(2026, 12, 28))),
            "Dec 28, 2026 – Jan 3, 2027"
        );
    }

    #[test]
    fn time_labels_follow_the_clock_preference() {
        let calendar = Calendar::new(TimeZone::get("Europe/Budapest").unwrap(), WeekStart::Monday);
        let instant = ts("2026-10-09T13:05:00Z");
        assert_eq!(time_label(instant, &calendar, false), "15:05");
        assert_eq!(time_label(instant, &calendar, true), "3:05 PM");
    }
}
