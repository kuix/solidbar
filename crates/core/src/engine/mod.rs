//! The engine: application state, the calls to solidtime, and the rules tying them together.
//!
//! Commands from the UI land here. Every change is applied locally first (so the panel
//! reacts instantly), then sent to solidtime, then reconciled with the response or rolled
//! back. Writes are serialized so they reach solidtime in the order they were made.

pub mod agent;
pub(crate) mod files;
mod state;
mod view;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use jiff::civil::{Date, Time};
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use solidbar_shared::duration::{format_hm, parse_minutes};
use solidbar_shared::instance::{InstanceSettings, Redirect};
use solidbar_shared::ipc::{
    AdjustArgs, BootstrapView, CommandError, EntryDraft, FormArgs, ShortcutArgs, TimerSource,
};
use solidbar_shared::settings::{AgentAccess, Settings, SettingsPatch};
use solidbar_shared::view::{
    AgentServerView, AppView, Catalog, EntryKind, FormInit, Notice, NoticeKind,
};
use tokio::sync::oneshot;
use url::Url;

use crate::api::models::{Membership, NewTimeEntry, TimeEntryChanges};
use crate::api::{Api, ApiError};
use crate::auth::callback::LoopbackListener;
use crate::auth::{AuthError, Instance, LoginRequest, Session, TokenStore};
use crate::model::recents::{self, WorkPair};
use crate::model::{TimeEntry, cascade, stacking};
use crate::ports::{Clock, EventSink, UrlOpener};
use crate::time::Week;
use files::Files;
use state::{ActiveTimer, AuthPhase, CatalogData, Data, State};

/// How long to wait for the browser to come back from the sign-in page.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Projects and tasks change rarely; reload them after this long (or on ⌘R).
const CATALOG_TTL: SignedDuration = SignedDuration::from_mins(10);
/// The entry form is where a project created a moment ago would be missed.
const CATALOG_TTL_FOR_FORM: SignedDuration = SignedDuration::from_mins(1);
/// Opening the panel refreshes when the data is older than this.
const REFRESH_ON_OPEN_AFTER: SignedDuration = SignedDuration::from_secs(30);
/// Weeks further than this from the current one are dropped from the cache.
const CACHED_WEEKS_AROUND: i64 = 6;

pub struct EngineDeps {
    pub http: reqwest::Client,
    pub clock: Arc<dyn Clock>,
    pub tokens: Arc<dyn TokenStore>,
    pub sink: Arc<dyn EventSink>,
    pub opener: Arc<dyn UrlOpener>,
    /// Where settings and the cache live.
    pub data_dir: PathBuf,
    /// The Mac uses a 12-hour clock.
    pub hour12: bool,
    pub system_zone: TimeZone,
}

#[derive(Default)]
struct LoginControl {
    cancel: Option<oneshot::Sender<()>>,
    /// Waiting for a `solidtime://oauth/callback` deep link.
    redirect: Option<oneshot::Sender<String>>,
}

pub struct Engine {
    deps: EngineDeps,
    files: Files,
    state: Mutex<State>,
    writes: tokio::sync::Mutex<()>,
    login: Mutex<LoginControl>,
    local_ids: AtomicU64,
    agent_seen: Mutex<agent::SeenEntries>,
}

impl Engine {
    pub fn new(deps: EngineDeps) -> Arc<Self> {
        let files = Files::new(&deps.data_dir);
        let settings = files.load_settings();
        let session = Instance::new(&settings.instance).ok().and_then(|instance| {
            Session::restore(
                deps.http.clone(),
                instance,
                deps.tokens.clone(),
                deps.clock.clone(),
            )
            .inspect_err(|error| log::warn!("could not restore the session: {error}"))
            .ok()
            .flatten()
        });
        let (auth, data) = match session {
            Some(session) => {
                let data = files
                    .load_cache(&session.instance().account())
                    .unwrap_or_default();
                let api = Api::new(Arc::new(session));
                (AuthPhase::SignedIn { api }, data)
            }
            None => (AuthPhase::SignedOut { error: None }, Data::default()),
        };
        let mut state = State {
            settings,
            auth,
            data,
            selected_date: Date::default(),
            pending: Default::default(),
            online: true,
            loading: false,
            last_refresh: None,
            agent_server: AgentServerView::Off,
        };
        state.selected_date = state.calendar().date_of(deps.clock.now());
        Arc::new(Self {
            deps,
            files,
            state: Mutex::new(state),
            writes: tokio::sync::Mutex::new(()),
            login: Mutex::new(LoginControl::default()),
            local_ids: AtomicU64::new(1),
            agent_seen: Mutex::default(),
        })
    }

    pub fn bootstrap(&self) -> BootstrapView {
        BootstrapView {
            app: self.app_view(),
            catalog: self.catalog_view(),
        }
    }

    pub fn app_view(&self) -> AppView {
        let state = self.state();
        view::app_view(
            &state,
            &view::Clocks {
                now: self.deps.clock.now(),
                hour12: state.settings.clock.hour12(self.deps.hour12),
                system_zone: &self.deps.system_zone,
            },
        )
    }

    pub fn catalog_view(&self) -> Catalog {
        view::catalog_view(&self.state())
    }

    pub fn settings(&self) -> Settings {
        self.state().settings.clone()
    }

    pub fn is_signed_in(&self) -> bool {
        self.state().api().is_some()
    }

    // --- Signing in and out ---------------------------------------------------------------

    /// Runs a complete sign-in: opens the browser and waits for the redirect.
    pub async fn login(&self, settings: InstanceSettings) -> Result<(), CommandError> {
        let instance = Instance::new(&settings).map_err(user_error)?;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        if let Some(previous) = self.login_control().cancel.replace(cancel_tx) {
            let _ = previous.send(());
        }
        self.state().auth = AuthPhase::SigningIn {
            instance: settings.clone(),
            redirect: None,
        };
        self.emit_app();

        let result = self.run_login(LoginRequest::new(instance), cancel_rx).await;
        *self.login_control() = LoginControl::default();
        match result {
            Ok(session) => {
                let account = session.instance().account();
                {
                    let mut state = self.state();
                    state.settings.instance = settings;
                    state.auth = AuthPhase::SignedIn {
                        api: Api::new(Arc::new(session)),
                    };
                    state.data = self.files.load_cache(&account).unwrap_or_default();
                    self.files.save_settings(&state.settings);
                }
                self.emit_app();
                // A failed first load is reported by `refresh` itself; the sign-in worked.
                let _ = self.refresh(true).await;
                Ok(())
            }
            Err(AuthError::Cancelled) => {
                self.state().auth = AuthPhase::SignedOut { error: None };
                self.emit_app();
                Ok(())
            }
            Err(error) => {
                log::warn!("sign-in failed: {error}");
                self.state().auth = AuthPhase::SignedOut {
                    error: Some(error.to_string()),
                };
                self.emit_app();
                Err(user_error(error))
            }
        }
    }

    async fn run_login(
        &self,
        request: LoginRequest,
        cancel: oneshot::Receiver<()>,
    ) -> Result<Session, AuthError> {
        let attempt = async {
            // Listening first gives the port to ask about; the system picks a free one.
            let listener = LoopbackListener::bind().await?;
            let redirect = request.choose_redirect(listener.port()).await?;
            if let AuthPhase::SigningIn {
                redirect: chosen, ..
            } = &mut self.state().auth
            {
                *chosen = Some(redirect);
            }
            self.emit_app();
            let authorize_url = request.authorize_url(redirect);
            let code = match redirect {
                Redirect::Loopback { .. } => {
                    self.open_browser(authorize_url.as_str())?;
                    listener.wait_for_code(request.state()).await?
                }
                Redirect::DeepLink => {
                    drop(listener);
                    let (link_tx, link_rx) = oneshot::channel();
                    self.login_control().redirect = Some(link_tx);
                    self.open_browser(authorize_url.as_str())?;
                    let link = link_rx.await.map_err(|_| AuthError::Cancelled)?;
                    let link = Url::parse(&link)
                        .map_err(|error| AuthError::Protocol(error.to_string()))?;
                    request.code_from_redirect(&link)?
                }
            };
            Ok::<_, AuthError>((redirect, code))
        };
        let (redirect, code) = tokio::select! {
            attempt = tokio::time::timeout(LOGIN_TIMEOUT, attempt) => {
                attempt.map_err(|_| AuthError::TimedOut)??
            }
            _ = cancel => return Err(AuthError::Cancelled),
        };
        request
            .finish(
                self.deps.http.clone(),
                redirect,
                &code,
                self.deps.tokens.clone(),
                self.deps.clock.clone(),
            )
            .await
    }

    fn open_browser(&self, url: &str) -> Result<(), AuthError> {
        self.deps
            .opener
            .open(url)
            .map_err(|error| AuthError::Protocol(format!("couldn't open the browser: {error}")))
    }

    /// Hands a `solidtime://oauth/callback?…` deep link to the waiting sign-in.
    pub fn deliver_redirect(&self, url: &str) {
        match self.login_control().redirect.take() {
            Some(waiting) => {
                let _ = waiting.send(url.to_owned());
            }
            None => log::info!("ignoring a sign-in redirect nobody is waiting for"),
        }
    }

    pub fn cancel_login(&self) {
        if let Some(cancel) = self.login_control().cancel.take() {
            let _ = cancel.send(());
        }
    }

    pub async fn logout(&self) -> Result<(), CommandError> {
        let _writes = self.writes.lock().await;
        let api = self.state().api();
        if let Some(api) = api
            && let Err(error) = api.session().sign_out().await
        {
            log::warn!("could not remove the saved session: {error}");
        }
        {
            let mut state = self.state();
            state.auth = AuthPhase::SignedOut { error: None };
            state.data = Data::default();
            state.pending.clear();
            // Whoever signs in next picks their own organization.
            state.settings.active_membership_id = None;
            self.files.save_settings(&state.settings);
        }
        self.files.clear_cache();
        self.emit_app();
        self.emit_catalog();
        Ok(())
    }

    // --- Loading ----------------------------------------------------------------------------

    /// Reloads profile, timer, the selected week and (when stale or `force`) the catalog.
    pub async fn refresh(&self, force: bool) -> Result<(), CommandError> {
        let Some(api) = self.state().api() else {
            return Ok(());
        };
        let _writes = self.writes.lock().await;
        self.state().loading = true;
        self.emit_app();
        let result = self.load_everything(&api, force).await;
        {
            let mut state = self.state();
            state.loading = false;
            if result.is_ok() {
                state.online = true;
                state.last_refresh = Some(self.deps.clock.now());
            }
        }
        match result {
            Ok(()) => {
                self.emit_app();
                self.emit_catalog();
                self.persist();
                Ok(())
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    async fn load_everything(&self, api: &Api, force: bool) -> Result<(), ApiError> {
        let needs_profile = force || {
            let state = self.state();
            state.data.me.is_none() || state.data.memberships.is_empty()
        };
        if needs_profile {
            let (me, memberships) = tokio::try_join!(api.me(), api.memberships())?;
            let mut state = self.state();
            state.data.me = Some(me);
            state.data.memberships = memberships;
            // Remember the only organization, and forget one that's no longer available.
            let active = state
                .active_membership()
                .map(|membership| membership.id.clone());
            if active != state.settings.active_membership_id {
                state.settings.active_membership_id = active;
                self.files.save_settings(&state.settings);
            }
            // The profile may have changed the time zone, and with it "today".
            state.selected_date = state.calendar().date_of(self.deps.clock.now());
        }
        self.load_active_timer(api).await?;
        self.load_week(api, None).await?;
        let max_age = if force {
            SignedDuration::ZERO
        } else {
            CATALOG_TTL
        };
        self.load_catalog(api, max_age).await.map(|_| ())
    }

    async fn load_active_timer(&self, api: &Api) -> Result<bool, ApiError> {
        let active = api
            .active_time_entry()
            .await?
            .map(|(organization_id, entry)| ActiveTimer {
                organization_id,
                entry,
            });
        let mut state = self.state();
        let changed = state.data.active_timer != active;
        state.data.active_timer = active;
        Ok(changed)
    }

    /// Loads a week (default: the selected one) of the active membership.
    async fn load_week(&self, api: &Api, week: Option<Week>) -> Result<(), ApiError> {
        let (membership, week, bounds, current_week) = {
            let state = self.state();
            let Some(membership) = state.active_membership().cloned() else {
                return Ok(());
            };
            let calendar = state.calendar();
            let week = week.unwrap_or_else(|| state.selected_week());
            let current_week = calendar.week_of(calendar.date_of(self.deps.clock.now()));
            (membership, week, calendar.week_bounds(week), current_week)
        };
        let entries = api
            .time_entries(&membership.organization.id, &membership.id, bounds)
            .await?;

        let mut state = self.state();
        if week == current_week {
            let stored = state
                .data
                .recents
                .get(&membership.id)
                .cloned()
                .unwrap_or_default();
            state.data.recents.insert(
                membership.id.clone(),
                recents::merge_entries(&stored, &entries),
            );
        }
        let weeks = state.data.weeks.entry(membership.id).or_default();
        weeks.insert(week.first_day, entries);
        let span = jiff::Span::new().weeks(CACHED_WEEKS_AROUND);
        let (oldest, newest) = (
            current_week.first_day.saturating_sub(span),
            current_week.first_day.saturating_add(span),
        );
        weeks.retain(|first_day, _| (oldest..=newest).contains(first_day));
        Ok(())
    }

    /// Reloads projects, tasks and clients unless they're younger than `max_age`; says whether
    /// anything changed.
    async fn load_catalog(&self, api: &Api, max_age: SignedDuration) -> Result<bool, ApiError> {
        let organization_id = {
            let state = self.state();
            let Some(membership) = state.active_membership() else {
                return Ok(false);
            };
            let organization_id = membership.organization.id.clone();
            let fresh = state
                .data
                .catalogs
                .get(&organization_id)
                .is_some_and(|catalog| {
                    self.deps.clock.now().duration_since(catalog.fetched_at) < max_age
                });
            if fresh {
                return Ok(false);
            }
            organization_id
        };
        let (projects, tasks, clients, settings) = tokio::try_join!(
            api.projects(&organization_id),
            api.tasks(&organization_id),
            api.clients(&organization_id),
            api.organization_settings(&organization_id),
        )?;
        let mut state = self.state();
        let changed = state.data.catalogs.get(&organization_id).is_none_or(|old| {
            old.projects != projects
                || old.tasks != tasks
                || old.clients != clients
                || old.breaks_enabled != settings.breaks_enabled
        });
        state.data.catalogs.insert(
            organization_id,
            CatalogData {
                projects,
                tasks,
                clients,
                breaks_enabled: settings.breaks_enabled,
                fetched_at: self.deps.clock.now(),
            },
        );
        Ok(changed)
    }

    /// Called when the entry form opens: picks up projects and tasks created elsewhere a
    /// moment ago. Doesn't wait for (or block) saving; the picker updates when they arrive.
    pub async fn form_opened(&self) {
        let Some(api) = self.state().api() else {
            return;
        };
        match self.load_catalog(&api, CATALOG_TTL_FOR_FORM).await {
            Ok(true) => {
                self.emit_catalog();
                // Whether breaks are on is part of the app view.
                self.emit_app();
                self.persist();
            }
            Ok(false) => {}
            Err(error) => {
                // Shown as the offline banner or the sign-in screen when that's the cause.
                let _ = self.fail(error);
            }
        }
    }

    /// Called when the panel opens: back to today, and fresh data if it's getting old.
    pub async fn panel_opened(&self) {
        let stale = {
            let mut state = self.state();
            state.selected_date = state.calendar().date_of(self.deps.clock.now());
            state
                .last_refresh
                .is_none_or(|at| self.deps.clock.now().duration_since(at) > REFRESH_ON_OPEN_AFTER)
        };
        self.emit_app();
        if stale {
            let _ = self.refresh(false).await;
        }
    }

    pub async fn select_date(&self, date: &str) -> Result<(), CommandError> {
        let date = parse_date(date)?;
        let week_loaded = {
            let mut state = self.state();
            state.selected_date = date;
            let week = state.selected_week();
            state.week_entries(week).is_some()
        };
        self.emit_app();
        if week_loaded {
            return Ok(());
        }
        let Some(api) = self.state().api() else {
            return Ok(());
        };
        let _writes = self.writes.lock().await;
        match self.load_week(&api, None).await {
            Ok(()) => {
                self.emit_app();
                self.persist();
                Ok(())
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    pub async fn select_org(&self, membership_id: &str) -> Result<(), CommandError> {
        {
            let mut state = self.state();
            if !state
                .data
                .memberships
                .iter()
                .any(|membership| membership.id == membership_id)
            {
                return Err(CommandError::new(
                    "That organization is no longer available",
                ));
            }
            state.settings.active_membership_id = Some(membership_id.to_owned());
            self.files.save_settings(&state.settings);
        }
        self.emit_app();
        self.emit_catalog();
        self.refresh(false).await
    }

    // --- The entry form ---------------------------------------------------------------------

    pub fn prepare_form(&self, args: &FormArgs) -> Result<FormInit, CommandError> {
        let date = parse_date(&args.date)?;
        let state = self.state();
        let calendar = state.calendar();
        let wall_clock = |instant: Timestamp| {
            instant
                .to_zoned(calendar.time_zone.clone())
                .strftime("%H:%M")
                .to_string()
        };
        if !args.group_ids.is_empty() {
            let entries = args
                .group_ids
                .iter()
                .map(|id| {
                    find_entry(&state, id)
                        .map(|(_, entry)| entry)
                        .ok_or_else(|| user_error(ApiError::NotFound))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let first = entries
                .iter()
                .min_by_key(|entry| entry.start)
                .expect("the group has entries");
            let now = self.deps.clock.now();
            return Ok(FormInit {
                entry_id: None,
                group_ids: args.group_ids.clone(),
                date: calendar.date_of(first.start).to_string(),
                start_time: wall_clock(first.start),
                // Their total, for the form to mention; a group's times aren't edited.
                duration: format_hm(
                    entries
                        .iter()
                        .map(|entry| entry.duration_seconds(now))
                        .sum(),
                ),
                project_id: first.project_id.clone(),
                task_id: first.task_id.clone(),
                description: first.description.clone(),
                billable: first.billable,
                running: entries.iter().any(TimeEntry::is_running),
                kind: first.kind,
            });
        }
        match &args.entry_id {
            Some(id) => {
                let entry = find_entry(&state, id)
                    .map(|(_, entry)| entry)
                    .ok_or_else(|| user_error(ApiError::NotFound))?;
                Ok(FormInit {
                    entry_id: Some(entry.id.clone()),
                    group_ids: Vec::new(),
                    date: calendar.date_of(entry.start).to_string(),
                    start_time: wall_clock(entry.start),
                    duration: if entry.is_running() {
                        String::new()
                    } else {
                        format_hm(entry.duration_seconds(self.deps.clock.now()))
                    },
                    project_id: entry.project_id.clone(),
                    task_id: entry.task_id.clone(),
                    description: entry.description.clone(),
                    billable: entry.billable,
                    running: entry.is_running(),
                    kind: entry.kind,
                })
            }
            None => {
                let entries = state
                    .week_entries(calendar.week_of(date))
                    .cloned()
                    .unwrap_or_default();
                let start = stacking::default_start(
                    &calendar,
                    date,
                    &entries,
                    workday_start(&state.settings),
                );
                Ok(FormInit {
                    entry_id: None,
                    group_ids: Vec::new(),
                    date: date.to_string(),
                    start_time: wall_clock(start),
                    duration: String::new(),
                    project_id: None,
                    task_id: None,
                    description: String::new(),
                    billable: false,
                    running: false,
                    kind: EntryKind::Work,
                })
            }
        }
    }

    pub async fn save_entry(&self, draft: EntryDraft) -> Result<(), CommandError> {
        let _writes = self.writes.lock().await;
        let api = self.api()?;
        let membership = self.active_membership()?;
        let calendar = self.state().calendar();
        let now = self.deps.clock.now();
        let draft = without_work_fields_on_breaks(draft);
        if draft.task_id.is_some() && draft.project_id.is_none() {
            return Err(CommandError::new("Choose the project of the task"));
        }
        if !draft.group_ids.is_empty() {
            self.save_group(&api, &draft).await?;
            self.notice(
                NoticeKind::Info,
                &format!("Saved {} time entries", draft.group_ids.len()),
            );
            self.record_recent(&membership, &draft);
            self.emit_catalog();
            self.persist();
            return Ok(());
        }
        let date = parse_date(&draft.date)?;
        let start_time = parse_time(&draft.start_time)?;
        match &draft.id {
            None if draft.start_timer => {
                // A start the person set wins; otherwise a duration means "for that long now".
                let start = if draft.start_changed {
                    let start = calendar.at(date, start_time);
                    if start > now {
                        return Err(CommandError::new("A timer can't start in the future"));
                    }
                    start
                } else {
                    let elapsed = match draft.duration.trim() {
                        "" => None,
                        typed => Some(parse_minutes(typed).map_err(user_error)?),
                    };
                    stacking::timer_start(now, elapsed)
                };
                // Started in the past, it takes over from the running timer where it begins.
                let running_since = self
                    .state()
                    .data
                    .active_timer
                    .as_ref()
                    .map(|timer| timer.entry.start);
                if let Some(since) = running_since
                    && start < since
                {
                    let hour12 = self.state().settings.clock.hour12(self.deps.hour12);
                    return Err(CommandError::new(format!(
                        "A timer has been running since {}. Start the new one after that, or stop it first.",
                        view::time_label(since, &calendar, hour12)
                    )));
                }
                self.stop_running(&api, start).await?;
                let timer = new_entry(&draft, start, None);
                self.create(&api, &membership, timer).await?;
                self.notice(NoticeKind::Info, started_message(draft.kind));
            }
            None => {
                let minutes = parse_minutes(&draft.duration).map_err(user_error)?;
                let start = calendar.at(date, start_time);
                let interval =
                    stacking::interval_on(&calendar, date, start, minutes).map_err(user_error)?;
                let entry = new_entry(&draft, interval.start, Some(interval.end));
                self.create(&api, &membership, entry).await?;
                self.notice(
                    NoticeKind::Info,
                    &format!("Added {}", describe_minutes(i64::from(minutes))),
                );
            }
            Some(id) => {
                let (organization_id, original) = {
                    let state = self.state();
                    find_entry(&state, id).ok_or_else(|| user_error(ApiError::NotFound))?
                };
                let start = calendar.at(date, start_time);
                let end = if original.is_running() {
                    if start > now {
                        return Err(CommandError::new(
                            "A running timer can't start in the future",
                        ));
                    }
                    None
                } else {
                    let minutes = parse_minutes(&draft.duration).map_err(user_error)?;
                    Some(
                        stacking::interval_on(&calendar, date, start, minutes)
                            .map_err(user_error)?
                            .end,
                    )
                };
                let edited = TimeEntry {
                    id: original.id.clone(),
                    ..new_entry(&draft, start, end)
                };
                let changes = changes_between(&original, &edited);
                self.update(&api, &organization_id, original, edited, &changes)
                    .await?;
                self.notice(NoticeKind::Info, "Time entry saved");
            }
        }
        self.record_recent(&membership, &draft);
        self.emit_catalog();
        self.persist();
        Ok(())
    }

    /// Saves the form's project, task, notes, billable and kind to every entry of a group,
    /// keeping their times. If one fails, the ones already saved are put back.
    async fn save_group(&self, api: &Api, draft: &EntryDraft) -> Result<(), CommandError> {
        let edits = {
            let state = self.state();
            draft
                .group_ids
                .iter()
                .map(|id| {
                    let (organization_id, original) =
                        find_entry(&state, id).ok_or_else(|| user_error(ApiError::NotFound))?;
                    let edited = TimeEntry {
                        id: original.id.clone(),
                        ..new_entry(draft, original.start, original.end)
                    };
                    Ok((organization_id, original, edited))
                })
                .collect::<Result<Vec<_>, CommandError>>()?
        };
        {
            let mut state = self.state();
            for (organization_id, _, edited) in &edits {
                place(&mut state, organization_id, edited.clone());
                state.pending.insert(edited.id.clone());
            }
        }
        self.emit_app();
        for (index, (organization_id, original, edited)) in edits.iter().enumerate() {
            let changes = changes_between(original, edited);
            match api
                .update_time_entry(organization_id, &edited.id, &changes)
                .await
            {
                Ok(updated) => {
                    let mut state = self.state();
                    state.pending.remove(&edited.id);
                    place(&mut state, organization_id, updated);
                }
                Err(error) => {
                    for (organization_id, original, edited) in edits[..index].iter().rev() {
                        let undo = changes_between(edited, original);
                        if let Err(undo_error) = api
                            .update_time_entry(organization_id, &original.id, &undo)
                            .await
                        {
                            log::warn!(
                                "could not undo the change to {}: {undo_error}",
                                original.id
                            );
                        }
                    }
                    {
                        let mut state = self.state();
                        for (organization_id, original, _) in &edits {
                            state.pending.remove(&original.id);
                            place(&mut state, organization_id, original.clone());
                        }
                    }
                    return Err(self.fail(error));
                }
            }
        }
        self.emit_app();
        Ok(())
    }

    fn record_recent(&self, membership: &Membership, draft: &EntryDraft) {
        if let Some(project_id) = draft.project_id.clone() {
            let mut state = self.state();
            let pairs = state.data.recents.entry(membership.id.clone()).or_default();
            recents::record(
                pairs,
                WorkPair {
                    project_id,
                    task_id: draft.task_id.clone(),
                },
            );
        }
    }

    /// Deletes entries (one, or a group of continued ones). Stops at the first failure and
    /// shows the entries not deleted again.
    pub async fn delete_entries(&self, ids: &[String]) -> Result<(), CommandError> {
        let _writes = self.writes.lock().await;
        let api = self.api()?;
        let found = {
            let mut state = self.state();
            let found = ids
                .iter()
                .map(|id| find_entry(&state, id).ok_or_else(|| user_error(ApiError::NotFound)))
                .collect::<Result<Vec<_>, _>>()?;
            for (_, entry) in &found {
                unplace(&mut state, &entry.id);
                if state
                    .data
                    .active_timer
                    .as_ref()
                    .is_some_and(|timer| timer.entry.id == entry.id)
                {
                    state.data.active_timer = None;
                }
            }
            found
        };
        self.emit_app();
        for (index, (organization_id, entry)) in found.iter().enumerate() {
            match api.delete_time_entry(organization_id, &entry.id).await {
                Ok(()) | Err(ApiError::NotFound) => {}
                Err(error) => {
                    {
                        let mut state = self.state();
                        for (organization_id, entry) in &found[index..] {
                            place(&mut state, organization_id, entry.clone());
                        }
                    }
                    self.persist();
                    return Err(self.fail(error));
                }
            }
        }
        self.emit_app();
        self.persist();
        let message = match found.len() {
            1 => "Time entry deleted".to_owned(),
            count => format!("Deleted {count} time entries"),
        };
        self.notice(NoticeKind::Info, &message);
        Ok(())
    }

    /// ±1h (or any delta) on an entry; later entries of the day move along. See
    /// [`cascade::plan_adjust`].
    pub async fn adjust_entry(&self, args: &AdjustArgs) -> Result<(), CommandError> {
        let _writes = self.writes.lock().await;
        let api = self.api()?;
        let now = self.deps.clock.now();
        let (organization_id, calendar, entries, week) = {
            let state = self.state();
            let (organization_id, target) =
                find_entry(&state, &args.id).ok_or_else(|| user_error(ApiError::NotFound))?;
            let calendar = state.calendar();
            let week = calendar.week_of(calendar.date_of(target.start));
            let entries = state
                .week_entries(week)
                .cloned()
                .filter(|entries| entries.iter().any(|entry| entry.id == args.id))
                .unwrap_or_else(|| vec![target]);
            (organization_id, calendar, entries, week)
        };
        let delta = SignedDuration::from_mins(args.delta_minutes);
        let plan =
            cascade::plan_adjust(&calendar, &entries, &args.id, delta, now).map_err(user_error)?;

        {
            let mut state = self.state();
            for put in &plan {
                if let Some(entry) = entries.iter().find(|entry| entry.id == put.id) {
                    let moved = TimeEntry {
                        start: put.start,
                        end: put.end,
                        ..entry.clone()
                    };
                    place(&mut state, &organization_id, moved);
                    state.pending.insert(put.id.clone());
                }
            }
        }
        self.emit_app();

        for (index, put) in plan.iter().enumerate() {
            let changes = TimeEntryChanges::times(put.start, put.end);
            match api
                .update_time_entry(&organization_id, &put.id, &changes)
                .await
            {
                Ok(updated) => {
                    let mut state = self.state();
                    state.pending.remove(&put.id);
                    place(&mut state, &organization_id, updated);
                }
                Err(error) => {
                    // Put back what already went through, newest first, then show the truth.
                    for done in plan[..index].iter().rev() {
                        let undo = done.inverse();
                        let changes = TimeEntryChanges::times(undo.start, undo.end);
                        if let Err(undo_error) = api
                            .update_time_entry(&organization_id, &undo.id, &changes)
                            .await
                        {
                            log::warn!("could not undo the change to {}: {undo_error}", undo.id);
                        }
                    }
                    {
                        let mut state = self.state();
                        for put in &plan {
                            state.pending.remove(&put.id);
                        }
                    }
                    if let Err(reload_error) = self.load_week(&api, Some(week)).await {
                        log::warn!("could not reload the week: {reload_error}");
                    }
                    return Err(self.fail(error));
                }
            }
        }
        self.emit_app();
        self.persist();
        self.notice(
            NoticeKind::Info,
            &adjust_message(args.delta_minutes, plan.len() - 1),
        );
        Ok(())
    }

    // --- The timer --------------------------------------------------------------------------

    /// Starts a timer like an existing entry (continuing it), like the last used project, or
    /// like the work before the running break. Whatever runs is stopped first.
    pub async fn start_timer(&self, source: &TimerSource) -> Result<(), CommandError> {
        let _writes = self.writes.lock().await;
        let api = self.api()?;
        let membership = self.active_membership()?;
        let now = self.deps.clock.now();
        let like = {
            let state = self.state();
            match source {
                TimerSource::Entry { id } => {
                    let (_, entry) =
                        find_entry(&state, id).ok_or_else(|| user_error(ApiError::NotFound))?;
                    Some(entry)
                }
                TimerSource::Resume => state.last_work_entry(now).cloned(),
                TimerSource::LastUsed => {
                    let last = state
                        .data
                        .recents
                        .get(&membership.id)
                        .and_then(|pairs| pairs.first())
                        .cloned();
                    let billable = last
                        .as_ref()
                        .and_then(|pair| state.catalog()?.project(&pair.project_id))
                        .is_some_and(|project| project.is_billable);
                    Some(TimeEntry {
                        id: String::new(),
                        start: now,
                        end: None,
                        project_id: last.as_ref().map(|pair| pair.project_id.clone()),
                        task_id: last.and_then(|pair| pair.task_id),
                        description: String::new(),
                        billable,
                        kind: EntryKind::Work,
                    })
                }
            }
        };
        let Some(like) = like else {
            // Nothing to go back to after the break: just end it.
            if self.stop_running(&api, now).await?.is_some() {
                self.persist();
                self.notice(NoticeKind::Info, "Break ended");
            }
            return Ok(());
        };
        self.stop_running(&api, now).await?;
        let timer = TimeEntry {
            id: String::new(),
            start: now,
            end: None,
            ..like
        };
        let kind = timer.kind;
        self.create(&api, &membership, timer).await?;
        self.persist();
        self.notice(NoticeKind::Info, started_message(kind));
        Ok(())
    }

    /// Stops the work timer and starts a break at the same instant, so the two touch exactly.
    pub async fn start_break(&self) -> Result<(), CommandError> {
        let _writes = self.writes.lock().await;
        let api = self.api()?;
        let membership = self.active_membership()?;
        {
            let state = self.state();
            if !state.breaks_enabled() {
                return Err(CommandError::new("This organization doesn't track breaks"));
            }
            if state.on_break() {
                return Ok(());
            }
        }
        let now = self.deps.clock.now();
        self.stop_running(&api, now).await?;
        let pause = TimeEntry {
            id: String::new(),
            start: now,
            end: None,
            project_id: None,
            task_id: None,
            description: String::new(),
            billable: false,
            kind: EntryKind::Break,
        };
        self.create(&api, &membership, pause).await?;
        self.persist();
        self.notice(NoticeKind::Info, started_message(EntryKind::Break));
        Ok(())
    }

    pub async fn stop_timer(&self) -> Result<(), CommandError> {
        let _writes = self.writes.lock().await;
        let api = self.api()?;
        if let Some(stopped) = self.stop_running(&api, self.deps.clock.now()).await? {
            self.persist();
            let message = if stopped.is_break() {
                "Break ended"
            } else {
                "Timer stopped"
            };
            self.notice(NoticeKind::Info, message);
        }
        Ok(())
    }

    /// ⌥⌘S: back to work after a break, stop the running timer, or start one on the most
    /// recent project/task.
    pub async fn toggle_timer(&self) -> Result<(), CommandError> {
        let (running, on_break) = {
            let state = self.state();
            (state.data.active_timer.is_some(), state.on_break())
        };
        if on_break {
            self.start_timer(&TimerSource::Resume).await
        } else if running {
            self.stop_timer().await
        } else {
            self.start_timer(&TimerSource::LastUsed).await
        }
    }

    /// Stops the running timer at `at`, wherever it runs. Returns the stopped entry.
    async fn stop_running(
        &self,
        api: &Api,
        at: Timestamp,
    ) -> Result<Option<TimeEntry>, CommandError> {
        let Some(timer) = self.state().data.active_timer.clone() else {
            return Ok(None);
        };
        let end = at.max(timer.entry.start);
        let stopped = TimeEntry {
            end: Some(end),
            ..timer.entry.clone()
        };
        place(&mut self.state(), &timer.organization_id, stopped);
        self.emit_app();
        let changes = TimeEntryChanges::times(timer.entry.start, Some(end));
        match api
            .update_time_entry(&timer.organization_id, &timer.entry.id, &changes)
            .await
        {
            Ok(updated) => {
                place(&mut self.state(), &timer.organization_id, updated.clone());
                self.emit_app();
                Ok(Some(updated))
            }
            Err(ApiError::NotFound) => {
                // Deleted elsewhere in the meantime.
                unplace(&mut self.state(), &timer.entry.id);
                self.emit_app();
                Ok(None)
            }
            Err(error) => {
                place(&mut self.state(), &timer.organization_id, timer.entry);
                Err(self.fail(error))
            }
        }
    }

    // --- Shared write paths -----------------------------------------------------------------

    /// Creates `entry` (its `id` is ignored) in the membership's organization.
    async fn create(
        &self,
        api: &Api,
        membership: &Membership,
        entry: TimeEntry,
    ) -> Result<TimeEntry, CommandError> {
        let organization_id = &membership.organization.id;
        let body = NewTimeEntry {
            billable: entry.billable,
            project_id: entry.project_id.clone(),
            task_id: entry.task_id.clone(),
            description: Some(entry.description.clone()).filter(|text| !text.is_empty()),
            kind: entry.kind,
            ..NewTimeEntry::new(&membership.id, entry.start, entry.end)
        };
        let local_id = format!("local-{}", self.local_ids.fetch_add(1, Ordering::Relaxed));
        let local = TimeEntry {
            id: local_id.clone(),
            ..entry
        };
        {
            let mut state = self.state();
            place(&mut state, organization_id, local);
            state.pending.insert(local_id.clone());
        }
        self.emit_app();
        let result = api.create_time_entry(organization_id, &body).await;
        {
            let mut state = self.state();
            unplace(&mut state, &local_id);
            state.pending.remove(&local_id);
            if let Ok(created) = &result {
                place(&mut state, organization_id, created.clone());
            }
        }
        match result {
            Ok(created) => {
                self.emit_app();
                Ok(created)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    async fn update(
        &self,
        api: &Api,
        organization_id: &str,
        original: TimeEntry,
        edited: TimeEntry,
        changes: &TimeEntryChanges,
    ) -> Result<TimeEntry, CommandError> {
        let id = original.id.clone();
        {
            let mut state = self.state();
            place(&mut state, organization_id, edited);
            state.pending.insert(id.clone());
        }
        self.emit_app();
        let result = api.update_time_entry(organization_id, &id, changes).await;
        {
            let mut state = self.state();
            state.pending.remove(&id);
            let confirmed = result.as_ref().ok().cloned().unwrap_or(original);
            place(&mut state, organization_id, confirmed);
        }
        match result {
            Ok(updated) => {
                self.emit_app();
                Ok(updated)
            }
            Err(error) => Err(self.fail(error)),
        }
    }

    // --- Settings ---------------------------------------------------------------------------

    pub fn update_settings(&self, patch: &SettingsPatch) -> Result<(), CommandError> {
        {
            let mut state = self.state();
            if let Some(workday_start) = &patch.workday_start {
                parse_time(workday_start)?;
                state.settings.workday_start = workday_start.clone();
            }
            if let Some(theme) = patch.theme {
                state.settings.theme = theme;
            }
            if let Some(clock) = patch.clock {
                state.settings.clock = clock;
            }
            if let Some(menu_bar_text) = patch.menu_bar_text {
                state.settings.menu_bar_text = menu_bar_text;
            }
            if let Some(single_keys) = patch.single_key_shortcuts {
                state.settings.single_key_shortcuts = single_keys;
            }
            if let Some(offered) = patch.launch_at_login_offered {
                state.settings.launch_at_login_offered = offered;
            }
            self.files.save_settings(&state.settings);
        }
        self.emit_app();
        Ok(())
    }

    // --- AI agents ------------------------------------------------------------------------

    pub fn agent_access(&self) -> AgentAccess {
        self.state().settings.agents.clone()
    }

    /// Turns agents' access on or off. The first time, a free port and a key are picked.
    pub fn set_agent_access(&self, enabled: bool) -> Result<(), CommandError> {
        {
            let mut state = self.state();
            let agents = &mut state.settings.agents;
            if enabled {
                if agents.port.is_none() {
                    agents.port = Some(free_port()?);
                }
                if agents.key.is_none() {
                    agents.key = Some(new_agent_key());
                }
            }
            agents.enabled = enabled;
            self.files.save_settings(&state.settings);
        }
        self.emit_app();
        Ok(())
    }

    /// A new key; agents with the old one are turned away from now on.
    pub fn reset_agent_key(&self) {
        {
            let mut state = self.state();
            state.settings.agents.key = Some(new_agent_key());
            self.files.save_settings(&state.settings);
        }
        self.emit_app();
        self.notice(
            NoticeKind::Info,
            "New key. Agents set up with the old one need the new one.",
        );
    }

    /// Another free port, for when another app took the server's.
    pub fn move_agent_server(&self) -> Result<(), CommandError> {
        {
            let mut state = self.state();
            state.settings.agents.port = Some(free_port()?);
            self.files.save_settings(&state.settings);
        }
        self.emit_app();
        Ok(())
    }

    /// What Settings shows about the MCP server (the server reports it).
    pub fn set_agent_server(&self, server: AgentServerView) {
        {
            let mut state = self.state();
            if state.agent_server == server {
                return;
            }
            state.agent_server = server;
        }
        self.emit_app();
    }

    /// Records a shortcut the shell has already registered successfully.
    pub fn save_shortcut(&self, args: &ShortcutArgs) {
        {
            let mut state = self.state();
            state
                .settings
                .shortcuts
                .insert(args.action, args.accelerator.clone());
            self.files.save_settings(&state.settings);
        }
        self.emit_app();
    }

    /// The solidtime web app, for things Solidbar doesn't do.
    pub fn open_in_browser(&self) -> Result<(), CommandError> {
        let url = {
            let state = self.state();
            Instance::new(&state.settings.instance)
                .map_err(user_error)?
                .web_url("time")
        };
        self.deps
            .opener
            .open(url.as_str())
            .map_err(CommandError::new)
    }

    // --- Background work --------------------------------------------------------------------

    /// Re-renders time-dependent text: running durations, totals, the menu bar.
    pub fn tick(&self) {
        self.emit_app();
    }

    /// Picks up timers started or stopped elsewhere.
    pub async fn poll_timer(&self) {
        let Some(api) = self.state().api() else {
            return;
        };
        let _writes = self.writes.lock().await;
        match self.load_active_timer(&api).await {
            Ok(true) => {
                if let Err(error) = self.load_week(&api, None).await {
                    log::warn!("could not reload the week: {error}");
                }
                self.state().online = true;
                self.emit_app();
                self.persist();
            }
            Ok(false) => {
                let was_offline = !std::mem::replace(&mut self.state().online, true);
                if was_offline {
                    self.emit_app();
                }
            }
            Err(error) => {
                self.fail(error);
            }
        }
    }

    /// Runs forever: once a minute, at the turn of the minute, polls the timer and refreshes
    /// the menu bar. After the Mac wakes from sleep, reloads everything.
    pub async fn run_periodic(self: Arc<Self>) {
        let mut last = self.deps.clock.now();
        loop {
            let into_minute = Timestamp::now().as_second().rem_euclid(60);
            tokio::time::sleep(Duration::from_secs((60 - into_minute) as u64)).await;
            let now = self.deps.clock.now();
            let woke_up = now.duration_since(last) > SignedDuration::from_mins(3);
            last = now;
            if woke_up {
                let _ = self.refresh(false).await;
            } else {
                self.poll_timer().await;
            }
            self.tick();
        }
    }

    // --- Helpers ----------------------------------------------------------------------------

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .expect("no panics while holding the state lock")
    }

    fn login_control(&self) -> MutexGuard<'_, LoginControl> {
        self.login
            .lock()
            .expect("no panics while holding the login lock")
    }

    fn api(&self) -> Result<Api, CommandError> {
        self.state()
            .api()
            .ok_or_else(|| user_error(AuthError::SignedOut))
    }

    fn active_membership(&self) -> Result<Membership, CommandError> {
        self.state()
            .active_membership()
            .cloned()
            .ok_or_else(|| CommandError::new("Your solidtime account has no organization yet"))
    }

    fn emit_app(&self) {
        let view = self.app_view();
        self.deps.sink.app_changed(&view);
    }

    fn emit_catalog(&self) {
        let catalog = self.catalog_view();
        self.deps.sink.catalog_changed(&catalog);
    }

    fn notice(&self, kind: NoticeKind, text: &str) {
        self.deps.sink.notice(&Notice {
            kind,
            text: text.to_owned(),
        });
    }

    fn persist(&self) {
        let (account, data) = {
            let state = self.state();
            let Some(api) = state.api() else {
                return;
            };
            (api.session().instance().account(), state.data.clone())
        };
        self.files.save_cache(&account, &data);
    }

    /// Turns an API failure into the message for the person, updating the state it implies
    /// (offline, signed out). The UI shows the returned message.
    fn fail(&self, error: ApiError) -> CommandError {
        {
            let mut state = self.state();
            match &error {
                ApiError::Auth(AuthError::SessionExpired | AuthError::SignedOut) => {
                    state.auth = AuthPhase::SignedOut {
                        error: Some(error.to_string()),
                    };
                    state.data = Data::default();
                    state.pending.clear();
                }
                error if error.is_offline() => state.online = false,
                _ => {}
            }
        }
        self.emit_app();
        CommandError::new(error.to_string())
    }
}

/// Finds an entry of the active membership, or the running timer, with its organization.
fn find_entry(state: &State, id: &str) -> Option<(String, TimeEntry)> {
    if let Some(timer) = &state.data.active_timer
        && timer.entry.id == id
    {
        return Some((timer.organization_id.clone(), timer.entry.clone()));
    }
    let (_, entry) = state.find_entry(id)?;
    let organization_id = state.active_membership()?.organization.id.clone();
    Some((organization_id, entry.clone()))
}

/// Puts an entry where it belongs: the loaded week it starts in, and the timer slot if running.
fn place(state: &mut State, organization_id: &str, entry: TimeEntry) {
    unplace(state, &entry.id);
    if entry.is_running() {
        state.data.active_timer = Some(ActiveTimer {
            organization_id: organization_id.to_owned(),
            entry: entry.clone(),
        });
    } else if state
        .data
        .active_timer
        .as_ref()
        .is_some_and(|timer| timer.entry.id == entry.id)
    {
        state.data.active_timer = None;
    }
    let calendar = state.calendar();
    let week = calendar.week_of(calendar.date_of(entry.start));
    let Some(membership_id) = state
        .membership_of_org(organization_id)
        .map(|membership| membership.id.clone())
    else {
        return;
    };
    if let Some(entries) = state
        .data
        .weeks
        .get_mut(&membership_id)
        .and_then(|weeks| weeks.get_mut(&week.first_day))
    {
        entries.push(entry);
    }
}

fn unplace(state: &mut State, id: &str) -> Option<TimeEntry> {
    state
        .data
        .weeks
        .values_mut()
        .flat_map(|weeks| weeks.values_mut())
        .find_map(|entries| {
            let index = entries.iter().position(|entry| entry.id == id)?;
            Some(entries.remove(index))
        })
}

/// An entry with the form's fields; the id is filled in by solidtime.
fn new_entry(draft: &EntryDraft, start: Timestamp, end: Option<Timestamp>) -> TimeEntry {
    TimeEntry {
        id: String::new(),
        start,
        end,
        project_id: draft.project_id.clone(),
        task_id: draft.task_id.clone(),
        description: draft.description.trim().to_owned(),
        billable: draft.billable,
        kind: draft.kind,
    }
}

/// solidtime rejects breaks with a project, a task or the billable flag.
fn without_work_fields_on_breaks(draft: EntryDraft) -> EntryDraft {
    match draft.kind {
        EntryKind::Work => draft,
        EntryKind::Break => EntryDraft {
            project_id: None,
            task_id: None,
            billable: false,
            ..draft
        },
    }
}

/// The `PUT` that turns `from` into `to`: times always, the rest as solidtime expects it.
fn changes_between(from: &TimeEntry, to: &TimeEntry) -> TimeEntryChanges {
    TimeEntryChanges {
        billable: Some(to.billable),
        project_id: Some(to.project_id.clone()),
        task_id: Some(to.task_id.clone()),
        description: Some(Some(to.description.clone()).filter(|text| !text.is_empty())),
        kind: (to.kind != from.kind).then_some(to.kind),
        ..TimeEntryChanges::times(to.start, to.end)
    }
}

fn started_message(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Work => "Timer started",
        EntryKind::Break => "Break started",
    }
}

fn workday_start(settings: &Settings) -> Time {
    parse_time(&settings.workday_start).unwrap_or(Time::constant(9, 0, 0, 0))
}

fn parse_date(text: &str) -> Result<Date, CommandError> {
    text.parse()
        .map_err(|_| CommandError::new(format!("Not a date: {text}")))
}

fn parse_time(text: &str) -> Result<Time, CommandError> {
    let (hours, minutes) = text
        .trim()
        .split_once(':')
        .ok_or_else(|| CommandError::new("Enter the start time as HH:MM"))?;
    let hours: i8 = hours
        .parse()
        .map_err(|_| CommandError::new("Enter the start time as HH:MM"))?;
    let minutes: i8 = minutes
        .parse()
        .map_err(|_| CommandError::new("Enter the start time as HH:MM"))?;
    Time::new(hours, minutes, 0, 0).map_err(|_| CommandError::new("Enter the start time as HH:MM"))
}

/// A port nothing listens on right now, on the loopback interface.
fn free_port() -> Result<u16, CommandError> {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| CommandError::new(format!("No free port for the MCP server: {error}")))
}

/// 32 random bytes, base64url-encoded.
fn new_agent_key() -> String {
    use base64::Engine as _;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the operating system provides randomness");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn user_error(error: impl std::fmt::Display) -> CommandError {
    CommandError::new(error.to_string())
}

fn describe_minutes(minutes: i64) -> String {
    let (hours, rest) = (minutes / 60, minutes % 60);
    match (hours, rest) {
        (1, 0) => "1 hour".to_owned(),
        (hours, 0) => format!("{hours} hours"),
        (0, minutes) => format!("{minutes} minutes"),
        _ => format_hm(minutes * 60),
    }
}

fn adjust_message(delta_minutes: i64, moved: usize) -> String {
    let amount = describe_minutes(delta_minutes.abs());
    let verb = if delta_minutes > 0 {
        "Added"
    } else {
        "Removed"
    };
    match moved {
        0 => format!("{verb} {amount}"),
        1 => format!("{verb} {amount}; moved 1 later entry"),
        moved => format!("{verb} {amount}; moved {moved} later entries"),
    }
}

#[cfg(test)]
mod tests;
