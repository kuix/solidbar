//! The UI's shared state and the actions components trigger.

use std::time::Duration;

use jiff::civil::Date;
use leptos::prelude::*;
use leptos::task::spawn_local;
use solidbar_shared::ipc::{
    CheckForUpdate, Command, DateArgs, EntryArgs, FormArgs, PrepareForm, SelectDate, ShowReason,
    StartTimer, StopTimer, TimerSource,
};
use solidbar_shared::update::{UpdateState, UpdateView};
use solidbar_shared::view::{AppView, Catalog, DayView, EntryView, FormInit, Notice, NoticeKind};

use crate::ipc::call;

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
    /// The selected (or first) entry, once `date` is selected; falls back to the day's tab.
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

#[derive(Clone, Copy)]
pub struct Store {
    pub app: RwSignal<Option<AppView>>,
    pub catalog: RwSignal<Catalog>,
    pub screen: RwSignal<Screen>,
    pub form: RwSignal<Option<FormInit>>,
    /// The entry row that holds the roving focus.
    pub selected_entry: RwSignal<Option<String>>,
    /// The entry waiting for "Delete? ⏎ / ⎋".
    pub confirm_delete: RwSignal<Option<String>>,
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
            selected_entry: RwSignal::new(None),
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

    pub fn entry(&self, id: &str) -> Option<EntryView> {
        self.selected_day()?
            .entries
            .into_iter()
            .find(|entry| entry.id == id)
    }

    pub fn select_date(self, date: Date, toward: Toward) {
        self.selected_entry.set(None);
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
            self.open_form(date.to_string(), None);
        }
    }

    pub fn open_edit_form(self, id: String) {
        if let Some(date) = self.selected_date() {
            self.selected_entry.set(Some(id.clone()));
            self.open_form(date.to_string(), Some(id));
        }
    }

    fn open_form(self, date: String, entry_id: Option<String>) {
        spawn_local(async move {
            match call::<PrepareForm>(&FormArgs { date, entry_id }).await {
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

    /// Starts a timer from the entry, or stops it when it is the running one.
    pub fn toggle_entry_timer(self, entry: &EntryView) {
        if entry.running {
            self.run::<StopTimer>(());
        } else {
            self.run::<StartTimer>(TimerSource::Entry {
                id: entry.id.clone(),
            });
        }
    }

    pub fn ask_delete(self, id: String) {
        self.confirm_delete.set(Some(id.clone()));
        self.focus
            .set(Some(Focus::Element(format!("confirm-delete-{id}"))));
    }

    pub fn confirm_delete(self) {
        if let Some(id) = self.confirm_delete.get_untracked() {
            self.confirm_delete.set(None);
            self.selected_entry.set(None);
            self.focus.set(Some(Focus::Entry { date: None }));
            self.run::<solidbar_shared::ipc::DeleteEntry>(EntryArgs { id });
        }
    }

    pub fn cancel_delete(self) {
        if let Some(id) = self.confirm_delete.get_untracked() {
            self.confirm_delete.set(None);
            self.focus.set(Some(Focus::Element(format!("entry-{id}"))));
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
                    self.open_form(today.to_string(), None);
                }
            }
            ShowReason::Toggle => {
                if self.screen.get_untracked() == Screen::Form {
                    self.close_form();
                }
                self.selected_entry.set(None);
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
