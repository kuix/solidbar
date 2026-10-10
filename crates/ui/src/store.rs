//! The UI's shared state and the actions components trigger.

use std::collections::HashSet;
use std::time::Duration;

use jiff::civil::Date;
use leptos::prelude::*;
use leptos::task::spawn_local;
use solidbar_shared::group::{EntryGroup, group_entries};
use solidbar_shared::ipc::{
    AdjustArgs, AdjustEntry, CheckForUpdate, Command, DateArgs, DeleteEntries, EntriesArgs,
    FormArgs, PrepareForm, SelectDate, ShowReason, StartBreak, StartTimer, StopTimer, TimerSource,
};
use solidbar_shared::update::{UpdateState, UpdateView};
use solidbar_shared::view::{AppView, Catalog, DayView, EntryKind, FormInit, Notice, NoticeKind};

use crate::ipc::call;
use crate::rows::{Row, group_dom_id, visible_rows};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    List,
    Form,
    Settings,
}

/// Where keyboard focus goes once the view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Focus {
    /// The tab of this day in the week strip, once that day is selected.
    Day(String),
    /// The selected (or first) row, once `date` is selected; falls back to the day's tab.
    Entry { date: Option<String> },
    /// An element by id.
    Element(String),
}

/// Where focus should land after switching days.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Day,
    Entries,
}

/// A row waiting for "Delete? ⏎ / ⎋".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingDelete {
    /// The row's element id, where the question shows.
    pub row: String,
    pub ids: Vec<String>,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub app: RwSignal<Option<AppView>>,
    pub catalog: RwSignal<Catalog>,
    pub screen: RwSignal<Screen>,
    pub form: RwSignal<Option<FormInit>>,
    /// The element id of the row that holds the roving focus (see [`Row::dom_id`]).
    pub selected_row: RwSignal<Option<String>>,
    /// Groups (by id) opened to show their entries.
    pub expanded: RwSignal<HashSet<String>>,
    pub confirm_delete: RwSignal<Option<PendingDelete>>,
    pub pinned: RwSignal<bool>,
    pub focus: RwSignal<Option<Focus>>,
    /// Polite live region text (confirmations).
    pub status: RwSignal<String>,
    /// Assertive live region text (errors).
    pub alert: RwSignal<String>,
    pub toast: RwSignal<Option<Notice>>,
    pub update: RwSignal<UpdateView>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            app: RwSignal::new(None),
            catalog: RwSignal::new(Catalog::default()),
            screen: RwSignal::new(Screen::List),
            form: RwSignal::new(None),
            selected_row: RwSignal::new(None),
            expanded: RwSignal::new(HashSet::new()),
            confirm_delete: RwSignal::new(None),
            pinned: RwSignal::new(false),
            focus: RwSignal::new(None),
            status: RwSignal::new(String::new()),
            alert: RwSignal::new(String::new()),
            toast: RwSignal::new(None),
            update: RwSignal::new(UpdateView::default()),
        }
    }

    /// Reads from the current app view without subscribing to unrelated changes when used
    /// inside a `Memo`.
    pub fn with_app<T>(&self, read: impl FnOnce(&AppView) -> T) -> Option<T> {
        self.app.with(|app| app.as_ref().map(read))
    }

    pub fn online(&self) -> bool {
        self.with_app(|app| app.online).unwrap_or(false)
    }

    pub fn breaks_enabled(&self) -> bool {
        self.app
            .with_untracked(|app| app.as_ref().is_some_and(|app| app.breaks_enabled))
    }

    /// A break runs in the organization on screen.
    pub fn on_break(&self) -> bool {
        self.app.with_untracked(|app| {
            app.as_ref()
                .and_then(|app| app.timer.as_ref())
                .is_some_and(|timer| timer.in_active_org && timer.kind == EntryKind::Break)
        })
    }

    /// Runs a command in the background, showing its error if it fails.
    pub fn run<C: Command + 'static>(self, args: C::Args) {
        spawn_local(async move {
            if let Err(error) = call::<C>(&args).await {
                self.error(&error.message);
            }
        });
    }

    pub fn notify(&self, notice: Notice) {
        let region = match notice.kind {
            NoticeKind::Info => self.status,
            NoticeKind::Error => self.alert,
        };
        announce(region, notice.text.clone());
        self.toast.set(Some(notice.clone()));
        let toast = self.toast;
        set_timeout(
            move || {
                if toast.get_untracked().as_ref() == Some(&notice) {
                    toast.set(None);
                }
            },
            Duration::from_secs(4),
        );
    }

    pub fn error(&self, text: &str) {
        self.notify(Notice {
            kind: NoticeKind::Error,
            text: text.to_owned(),
        });
    }

    // --- Days ---------------------------------------------------------------------------------

    pub fn selected_date(&self) -> Option<Date> {
        self.app
            .with_untracked(|app| app.as_ref()?.selected_date.parse().ok())
    }

    pub fn today(&self) -> Option<Date> {
        self.app
            .with_untracked(|app| app.as_ref()?.today.parse().ok())
    }

    pub fn selected_day(&self) -> Option<DayView> {
        self.app.with_untracked(|app| {
            let app = app.as_ref()?;
            app.week
                .as_ref()?
                .days
                .iter()
                .find(|day| day.date == app.selected_date)
                .cloned()
        })
    }

    /// The selected day's entries, grouped as the list shows them.
    pub fn groups(&self) -> Vec<EntryGroup> {
        self.selected_day()
            .map(|day| group_entries(&day.entries))
            .unwrap_or_default()
    }

    pub fn rows(&self) -> Vec<Row> {
        self.expanded
            .with_untracked(|expanded| visible_rows(&self.groups(), expanded))
    }

    /// The row holding the roving focus, if it's still on screen.
    pub fn selected(&self) -> Option<Row> {
        let id = self.selected_row.get_untracked()?;
        self.rows().into_iter().find(|row| row.dom_id() == id)
    }

    pub fn select_date(self, date: Date, toward: Toward) {
        self.selected_row.set(None);
        self.confirm_delete.set(None);
        let date_text = date.to_string();
        self.focus.set(Some(match toward {
            Toward::Day => Focus::Day(date_text),
            Toward::Entries => Focus::Entry {
                date: Some(date_text),
            },
        }));
        self.run::<SelectDate>(DateArgs {
            date: date.to_string(),
        });
    }

    pub fn shift_days(self, days: i64, toward: Toward) {
        if let Some(date) = self
            .selected_date()
            .and_then(|date| date.checked_add(jiff::Span::new().days(days)).ok())
        {
            self.select_date(date, toward);
        }
    }

    pub fn go_to_today(self, toward: Toward) {
        if let Some(today) = self.today() {
            self.select_date(today, toward);
        }
    }

    // --- Entries ------------------------------------------------------------------------------

    pub fn open_new_form(self) {
        if let Some(date) = self.selected_date() {
            self.open_form(date.to_string(), None, Vec::new());
        }
    }

    /// Opens the row in the form: one entry, or a group's shared fields.
    pub fn edit(self, row: &Row) {
        let Some(date) = self.selected_date() else {
            return;
        };
        self.selected_row.set(Some(row.dom_id()));
        let (entry_id, group_ids) = match row {
            Row::Group(group) if group.len() > 1 => (None, group.ids()),
            Row::Group(group) => (Some(group.id().to_owned()), Vec::new()),
            Row::Interval { entry, .. } => (Some(entry.id.clone()), Vec::new()),
        };
        self.open_form(date.to_string(), entry_id, group_ids);
    }

    fn open_form(self, date: String, entry_id: Option<String>, group_ids: Vec<String>) {
        spawn_local(async move {
            let args = FormArgs {
                date,
                entry_id,
                group_ids,
            };
            match call::<PrepareForm>(&args).await {
                Ok(init) => {
                    self.confirm_delete.set(None);
                    self.form.set(Some(init));
                    self.screen.set(Screen::Form);
                }
                Err(error) => self.error(&error.message),
            }
        });
    }

    pub fn close_form(self) {
        self.screen.set(Screen::List);
        self.form.set(None);
        self.focus.set(Some(Focus::Entry { date: None }));
    }

    /// The row's timer button: stops the running one, otherwise continues the row (a new entry
    /// like it). During a break, continuing work ends the break.
    pub fn toggle_row_timer(self, row: &Row) {
        if row.running() {
            self.run::<StopTimer>(());
        } else if row.is_break() {
            self.run::<StartBreak>(());
        } else {
            self.run::<StartTimer>(TimerSource::Entry {
                id: row.target().id.clone(),
            });
        }
    }

    /// S: like the selected row's timer button, except that during a break it goes back to
    /// work unless a work row is selected. Says whether it did anything.
    pub fn timer_key(self) -> bool {
        let row = self.selected().filter(|row| !row.pending());
        if self.on_break() {
            match row {
                Some(row) if !row.is_break() => self.toggle_row_timer(&row),
                _ => self.resume(),
            }
            return true;
        }
        match row {
            Some(row) => {
                self.toggle_row_timer(&row);
                true
            }
            None => false,
        }
    }

    pub fn resume(self) {
        self.run::<StartTimer>(TimerSource::Resume);
    }

    /// B: ends the break, or takes one (stopping the work timer).
    pub fn break_key(self) -> bool {
        if !self.breaks_enabled() {
            return false;
        }
        if self.on_break() {
            self.run::<StopTimer>(());
        } else {
            self.run::<StartBreak>(());
        }
        true
    }

    pub fn adjust(self, row: &Row, delta_minutes: i64) {
        self.selected_row.set(Some(row.dom_id()));
        self.run::<AdjustEntry>(AdjustArgs {
            id: row.target().id.clone(),
            delta_minutes,
        });
    }

    /// → opens a group with several entries, ← closes it (also from one of its entries). Says
    /// whether it did, so the arrow can change the day otherwise.
    pub fn disclose(self, open: bool) -> bool {
        let Some(row) = self.selected() else {
            return false;
        };
        let group = row.group().clone();
        let is_open = self
            .expanded
            .with_untracked(|expanded| expanded.contains(group.id()));
        match row {
            Row::Group(_) if group.len() > 1 && open != is_open => {
                self.set_expanded(group.id(), open);
                true
            }
            Row::Interval { .. } if !open => {
                self.set_expanded(group.id(), false);
                let id = group_dom_id(&group);
                self.selected_row.set(Some(id.clone()));
                self.focus.set(Some(Focus::Element(id)));
                true
            }
            _ => false,
        }
    }

    pub fn set_expanded(self, group_id: &str, open: bool) {
        self.expanded.update(|expanded| {
            if open {
                expanded.insert(group_id.to_owned());
            } else {
                expanded.remove(group_id);
            }
        });
    }

    pub fn ask_delete(self, row: String, ids: Vec<String>) {
        self.focus
            .set(Some(Focus::Element(format!("confirm-delete-{row}"))));
        self.confirm_delete.set(Some(PendingDelete { row, ids }));
    }

    pub fn confirm_delete(self) {
        if let Some(pending) = self.confirm_delete.get_untracked() {
            self.confirm_delete.set(None);
            self.selected_row.set(None);
            self.focus.set(Some(Focus::Entry { date: None }));
            self.run::<DeleteEntries>(EntriesArgs { ids: pending.ids });
        }
    }

    pub fn cancel_delete(self) {
        if let Some(pending) = self.confirm_delete.get_untracked() {
            self.confirm_delete.set(None);
            self.focus.set(Some(Focus::Element(pending.row)));
        }
    }

    pub fn panel_shown(self, reason: ShowReason) {
        self.confirm_delete.set(None);
        if self.with_app(|app| app.choose_org).unwrap_or(false) {
            // Nothing to show or add to before an organization is chosen.
            request_animation_frame(|| {
                crate::keymap::focus_element("org-option-0");
            });
            return;
        }
        match reason {
            ShowReason::NewEntry => {
                if let Some(today) = self.today() {
                    self.open_form(today.to_string(), None, Vec::new());
                }
            }
            ShowReason::Toggle => {
                if self.screen.get_untracked() == Screen::Form {
                    self.close_form();
                }
                self.selected_row.set(None);
                self.focus.set(Some(Focus::Entry { date: None }));
            }
        }
    }
}

impl Store {
    // --- Updates ------------------------------------------------------------------------------

    /// "Check for updates" in Settings: says what it found, unlike the background checks.
    pub fn check_for_update(self) {
        spawn_local(async move {
            match call::<CheckForUpdate>(&()).await {
                Ok(update) => {
                    match &update.state {
                        UpdateState::UpToDate => self.notify(Notice {
                            kind: NoticeKind::Info,
                            text: "Solidbar is up to date".to_owned(),
                        }),
                        UpdateState::Available { version } => self.notify(Notice {
                            kind: NoticeKind::Info,
                            text: format!("Solidbar {version} is available"),
                        }),
                        UpdateState::Failed { message } => self.error(message),
                        _ => {}
                    }
                    self.update.set(update);
                }
                Err(error) => self.error(&error.message),
            }
        });
    }
}

/// Sets a live region so screen readers announce `text`, even when it repeats.
fn announce(region: RwSignal<String>, text: String) {
    region.set(String::new());
    set_timeout(move || region.set(text), Duration::from_millis(50));
}
