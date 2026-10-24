//! The signed-in panel: header, week strip, the selected day and the footer.

use leptos::prelude::*;
use solidbar_shared::group::{EntryGroup, group_entries};
use solidbar_shared::ipc::{
    AutostartArgs, InstallUpdate, MembershipArgs, PanelSetPinned, PinArgs, SelectOrg, SetAutostart,
    StartBreak, StopTimer, UpdateSettings, WindowDock,
};
use solidbar_shared::settings::SettingsPatch;
use solidbar_shared::update::UpdateState;
use solidbar_shared::view::{DayView, EntryKind, EntryView, WeekView};
use web_sys::MouseEvent;

use crate::entry_form::EntryForm;
use crate::format::{
    day_title, format_hm, group_label, installing, interval_label, project_title, spoken_duration,
    task_title, time_range, work_title,
};
use crate::icons;
use crate::rows::Row;
use crate::settings::Settings;
use crate::store::{Screen, Store, Toward};

#[component]
pub fn Shell() -> impl IntoView {
    let store = expect_context::<Store>();
    let week = Memo::new(move |_| store.with_app(|app| app.week.clone()).flatten());
    let selected_date = Memo::new(move |_| {
        store
            .with_app(|app| app.selected_date.clone())
            .unwrap_or_default()
    });
    let day = Memo::new(move |_| {
        let date = selected_date.get();
        week.get()?.days.into_iter().find(|day| day.date == date)
    });
    let screen = Memo::new(move |_| store.screen.get());

    view! {
        <div class="shell">
            <Header week />
            <Banners />
            // Only the list is about the days: the form and Settings take the week strip's
            // place too, and the form's heading names its day.
            {move || match screen.get() {
                Screen::List => {
                    view! {
                        <WeekStrip week selected_date />
                        <div
                            class="day-panel"
                            id="day-panel"
                            role="tabpanel"
                            aria-labelledby=move || format!("tab-{}", selected_date.get())
                        >
                            <DayList day />
                        </div>
                    }
                        .into_any()
                }
                Screen::Form => {
                    let form = store.form.get_untracked().map(|init| view! { <EntryForm init /> });
                    view! { <div class="form-panel">{form}</div> }.into_any()
                }
                Screen::Settings => view! { <div class="settings-panel"><Settings /></div> }.into_any(),
            }}
            <Show when=move || screen.get() == Screen::List>
                <Footer week />
            </Show>
        </div>
    }
}

#[component]
fn Header(week: Memo<Option<WeekView>>) -> impl IntoView {
    let store = expect_context::<Store>();
    let label = Memo::new(move |_| week.get().map(|week| week.label).unwrap_or_default());

    let toggle_pin = move |_| {
        let pinned = !store.pinned.get_untracked();
        store.pinned.set(pinned);
        store.run::<PanelSetPinned>(PinArgs { pinned });
    };
    // In the window, in the pin's place: the panel under the menu bar icon again.
    let dock = move |_| store.run::<WindowDock>(());

    let screen = Memo::new(move |_| store.screen.get());
    let week_nav = move || {
        view! {
            <div class="week-nav">
                <button
                    type="button"
                    class="icon-button"
                    aria-label="Previous week"
                    aria-keyshortcuts="Alt+ArrowLeft"
                    title="Previous week (⌥←)"
                    on:click=move |_| store.shift_days(-7, Toward::Entries)
                >
                    {icons::chevron_left()}
                </button>
                <h1 class="week-label">{label}</h1>
                <button
                    type="button"
                    class="icon-button"
                    aria-label="Next week"
                    aria-keyshortcuts="Alt+ArrowRight"
                    title="Next week (⌥→)"
                    on:click=move |_| store.shift_days(7, Toward::Entries)
                >
                    {icons::chevron_right()}
                </button>
            </div>
            <button
                type="button"
                class="today-button"
                aria-keyshortcuts=move || store.single_keys().then_some("T")
                title=move || store.titled("Go to today", "T")
                on:click=move |_| store.go_to_today(Toward::Entries)
            >
                "Today"
            </button>
        }
    };

    view! {
        // Dragged by anything in it but its buttons, the panel comes away from the menu bar,
        // and the window buttons take the header's left end.
        <header class="header" class:windowed=store.windowed data-tauri-drag-region="deep">
            // Weeks and today are only about the list. The form's entry goes on one day, which
            // its header changes; Settings has no days at all.
            {move || match screen.get() {
                Screen::List => week_nav().into_any(),
                Screen::Form => view! { <FormDayNav /> }.into_any(),
                Screen::Settings => view! { <SettingsNav /> }.into_any(),
            }}
            {if store.windowed {
                view! {
                    <button
                        type="button"
                        class="icon-button"
                        aria-label="Dock to menu bar"
                        title="Back under the menu bar icon"
                        on:click=dock
                    >
                        {icons::dock()}
                    </button>
                }
                    .into_any()
            } else {
                view! {
                    <button
                        type="button"
                        class="icon-button"
                        aria-label="Keep open"
                        title="Keep the panel open"
                        aria-pressed=move || store.pinned.get().to_string()
                        on:click=toggle_pin
                    >
                        {icons::pin()}
                    </button>
                }
                    .into_any()
            }}
            <button
                type="button"
                class="icon-button"
                aria-label="Settings"
                aria-keyshortcuts="Meta+Comma"
                title="Settings (⌘,)"
                aria-pressed=move || (store.screen.get() == Screen::Settings).to_string()
                on:click=move |_| store.toggle_settings()
            >
                {icons::gear()}
            </button>
        </header>
    }
}

/// The open form's day, with ‹ › (⌘[ ⌘]) to move its entry to the day before or after. A
/// running timer and a group keep their days.
#[component]
fn FormDayNav() -> impl IntoView {
    let store = expect_context::<Store>();
    let movable = Memo::new(move |_| store.form_day_movable());
    let title = move || {
        store
            .form_date
            .get()
            .map(|date| day_title(date, store.today()))
            .unwrap_or_default()
    };
    view! {
        <div class="week-nav day-nav">
            <button
                type="button"
                class="icon-button"
                aria-label="Previous day"
                aria-keyshortcuts="Meta+BracketLeft"
                title="Previous day (⌘[)"
                prop:disabled=move || !movable.get()
                on:click=move |_| store.shift_form_day(-1)
            >
                {icons::chevron_left()}
            </button>
            // Announced when it changes, as the day the entry will be saved on.
            <h1 id="form-day" class="week-label" aria-live="polite">{title}</h1>
            <button
                type="button"
                class="icon-button"
                aria-label="Next day"
                aria-keyshortcuts="Meta+BracketRight"
                title="Next day (⌘])"
                prop:disabled=move || !movable.get()
                on:click=move |_| store.shift_form_day(1)
            >
                {icons::chevron_right()}
            </button>
        </div>
    }
}

/// Settings' title, with ‹ Back (Escape) to the list.
#[component]
fn SettingsNav() -> impl IntoView {
    let store = expect_context::<Store>();
    view! {
        <div class="week-nav settings-nav">
            <button
                type="button"
                id="settings-back"
                class="link"
                aria-keyshortcuts="Escape"
                on:click=move |_| store.close_settings()
            >
                "‹ Back"
            </button>
            <h1 id="settings-title" class="week-label">"Settings"</h1>
        </div>
    }
}

/// Things worth knowing right now: a timer in another organization, being offline, and a
/// time zone that differs from the Mac's.
#[component]
fn Banners() -> impl IntoView {
    let store = expect_context::<Store>();
    let elsewhere = Memo::new(move |_| {
        store
            .with_app(|app| app.timer.clone().filter(|timer| !timer.in_active_org))
            .flatten()
    });
    let on_break = Memo::new(move |_| {
        store
            .with_app(|app| {
                app.timer
                    .clone()
                    .filter(|timer| timer.in_active_org && timer.kind == EntryKind::Break)
            })
            .flatten()
    });
    let offline = Memo::new(move |_| store.with_app(|app| !app.online).unwrap_or(false));
    let hint = Memo::new(move |_| store.with_app(|app| app.time_zone_hint.clone()).flatten());
    let offer_autostart = Memo::new(move |_| {
        store
            .with_app(|app| !app.settings.launch_at_login_offered)
            .unwrap_or(false)
    });
    let launch_at_login = move |_| store.run::<SetAutostart>(AutostartArgs { enabled: true });
    let not_now = move |_| {
        store.run::<UpdateSettings>(SettingsPatch {
            launch_at_login_offered: Some(true),
            ..SettingsPatch::default()
        })
    };
    // Settings has its own "Updates" section.
    let update = Memo::new(move |_| {
        if store.screen.get() == Screen::Settings {
            return UpdateState::Idle;
        }
        store.update.with(|update| update.state.clone())
    });

    view! {
        {move || {
            elsewhere
                .get()
                .map(|timer| {
                    let org = timer.org_name.clone().unwrap_or_else(|| "another organization".to_owned());
                    let membership_id = timer.membership_id.clone();
                    let switch = move |_| {
                        if let Some(membership_id) = membership_id.clone() {
                            store.run::<SelectOrg>(MembershipArgs { membership_id });
                        }
                    };
                    let stop = move |_| store.run::<StopTimer>(());
                    let (what, stop_label) = match timer.kind {
                        EntryKind::Work => ("Timer running in ", "Stop"),
                        EntryKind::Break => ("On a break in ", "End break"),
                    };
                    view! {
                        <div class="banner timer-banner" role="region" aria-label="Running timer">
                            <span>{what} <strong>{org}</strong></span>
                            <button type="button" on:click=stop>{stop_label}</button>
                            <button type="button" on:click=switch>"Switch"</button>
                        </div>
                    }
                })
        }}
        {move || {
            on_break
                .get()
                .map(|timer| {
                    view! {
                        <div class="banner break-banner" role="region" aria-label="Break">
                            <span class="kind-icon">{icons::cup()}</span>
                            <span>"On a break"</span>
                            {timer
                                .resume_label
                                .map(|label| {
                                    view! {
                                        <button
                                            type="button"
                                            aria-keyshortcuts=move || store.single_keys().then_some("S")
                                            title=move || store.titled("Back to work", "S")
                                            on:click=move |_| store.resume()
                                        >
                                            {icons::play()}
                                            {format!("Resume “{label}”")}
                                        </button>
                                    }
                                })}
                        </div>
                    }
                })
        }}
        <Show when=move || offline.get()>
            <div class="banner offline" role="status">
                "Offline — showing saved data. Changes are paused."
            </div>
        </Show>
        {move || hint.get().map(|hint| view! { <p class="banner hint">{hint}</p> })}
        {move || match update.get() {
            UpdateState::Available { version } => {
                let install = move |_| store.run::<InstallUpdate>(());
                Some(
                    view! {
                        <div class="banner update" role="region" aria-label="Update">
                            <span>{format!("Solidbar {version} is available.")}</span>
                            <button type="button" on:click=install>"Restart to update"</button>
                        </div>
                    }
                        .into_any(),
                )
            }
            UpdateState::Installing { version, percent } => {
                Some(
                    view! {
                        <div class="banner update" role="region" aria-label="Update">
                            <span>{installing(&version, percent)}</span>
                        </div>
                    }
                        .into_any(),
                )
            }
            _ => None,
        }}
        <Show when=move || offer_autostart.get()>
            <div class="banner offer" role="region" aria-label="Launch at login">
                <span>"Start Solidbar automatically when you log in?"</span>
                <div class="button-row">
                    <button type="button" on:click=launch_at_login>"Launch at login"</button>
                    <button type="button" class="link" on:click=not_now>"Not now"</button>
                </div>
            </div>
        </Show>
    }
}

#[component]
fn WeekStrip(week: Memo<Option<WeekView>>, selected_date: Memo<String>) -> impl IntoView {
    let store = expect_context::<Store>();
    let days = Memo::new(move |_| week.get().map(|week| week.days).unwrap_or_default());
    view! {
        <div class="week-strip" role="tablist" aria-label="Days of the week">
            <For
                each=move || days.get()
                key=|day| (day.date.clone(), day.total_seconds, day.is_today)
                let:day
            >
                <DayTab day selected_date store />
            </For>
        </div>
    }
}

#[component]
fn DayTab(day: DayView, selected_date: Memo<String>, store: Store) -> impl IntoView {
    let date = day.date.clone();
    let is_selected = {
        let date = date.clone();
        Memo::new(move |_| selected_date.get() == date)
    };
    let mut label = format!("{}, {}", day.label, spoken_duration(day.total_seconds));
    if day.is_today {
        label.push_str(", today");
    }
    let total = if day.total_seconds > 0 {
        format_hm(day.total_seconds)
    } else {
        "–".to_owned()
    };
    let parsed = date.parse().ok();
    view! {
        <button
            type="button"
            role="tab"
            id=format!("tab-{date}")
            class="day-tab"
            class:today=day.is_today
            aria-selected=move || is_selected.get().to_string()
            aria-controls="day-panel"
            aria-label=label
            tabindex=move || if is_selected.get() { "0" } else { "-1" }
            on:click=move |_| {
                if let Some(date) = parsed {
                    store.select_date(date, Toward::Day);
                }
            }
        >
            <span class="weekday">{day.weekday_short.clone()}</span>
            <span class="day-total">{total}</span>
        </button>
    }
}

#[component]
fn DayList(day: Memo<Option<DayView>>) -> impl IntoView {
    let store = expect_context::<Store>();
    let loading = Memo::new(move |_| store.with_app(|app| app.loading).unwrap_or(false));
    let groups = Memo::new(move |_| {
        day.with(|day| {
            day.as_ref()
                .map(|day| group_entries(&day.entries))
                .unwrap_or_default()
        })
    });
    view! {
        {move || match day.get() {
            None => {
                view! {
                    <div class="empty-day">
                        <p class="empty" role="status">"Loading…"</p>
                    </div>
                }
                    .into_any()
            }
            Some(day) => {
                let empty = day.entries.is_empty();
                let label = day.label.clone();
                let total_label = match day.break_seconds {
                    0 => spoken_duration(day.total_seconds),
                    breaks => {
                        format!(
                            "{} worked, {} of breaks",
                            spoken_duration(day.total_seconds),
                            spoken_duration(breaks),
                        )
                    }
                };
                view! {
                    <div class="day-heading">
                        <h2 id="day-heading">{day.label.clone()}</h2>
                        <span class="day-heading-total">
                            <span class="sr-only">{total_label}</span>
                            {(day.break_seconds > 0)
                                .then(|| {
                                    view! {
                                        <span class="break-total" aria-hidden="true">
                                            {icons::cup()}
                                            {format_hm(day.break_seconds)}
                                        </span>
                                    }
                                })}
                            <span aria-hidden="true">{format_hm(day.total_seconds)}</span>
                        </span>
                    </div>
                    <Show
                        when=move || !empty
                        fallback=move || {
                            view! {
                                <div class="empty-day">
                                    <p class="empty">
                                        {format!("No time on {label}. ")}
                                        <button type="button" class="link" on:click=move |_| store.open_new_form()>
                                            "Add time"
                                        </button>
                                        {move || if store.single_keys() { " (N)" } else { " (⌘N)" }}
                                    </p>
                                </div>
                            }
                        }
                    >
                        <ul class="entries" aria-labelledby="day-heading" aria-busy=move || loading.get().to_string()>
                            <For each=move || groups.get() key=|group| group.clone() let:group>
                                <GroupRow group />
                            </For>
                        </ul>
                    </Show>
                }
                    .into_any()
            }
        }}
    }
}

/// A row of the day: one entry, or several continued ones (→ shows them one by one).
#[component]
fn GroupRow(group: EntryGroup) -> impl IntoView {
    let store = expect_context::<Store>();
    let row = Row::Group(group.clone());
    let dom_id = row.dom_id();
    let group_id = group.id().to_owned();
    let several = group.len() > 1;
    let expanded = {
        let group_id = group_id.clone();
        Memo::new(move |_| several && store.expanded.with(|expanded| expanded.contains(&group_id)))
    };
    let toggle_open = {
        let (group_id, dom_id) = (group_id.clone(), dom_id.clone());
        move |event: MouseEvent| {
            // Opens the group rather than the form.
            event.stop_propagation();
            event.prevent_default();
            store.selected_row.set(Some(dom_id.clone()));
            store.set_expanded(&group_id, !expanded.get_untracked());
        }
    };
    let entry = group.first().clone();
    let is_break = group.is_break();
    let color = entry
        .color
        .clone()
        .unwrap_or_else(|| "var(--no-project)".to_owned());
    let notes = if entry.description.is_empty() {
        time_range(group.start_label(), group.end_label())
    } else {
        entry.description.clone()
    };
    let intervals_id = format!("intervals-{group_id}");
    let count = group.len();
    let total = format_hm(group.duration_seconds());
    let label = group_label(&group);
    let work_tooltip = match &entry.client_name {
        Some(client) => format!("{} · {client}", work_title(&entry)),
        None => work_title(&entry),
    };
    let intervals = move || {
        group
            .entries
            .iter()
            .map(|entry| view! { <IntervalRow group=group.clone() entry=entry.clone() /> })
            .collect_view()
    };
    let disclosure = (several, Some(expanded), intervals_id.clone());

    view! {
        <li class="entry" class:break=is_break>
            <RowLine row label disclosure>
                {if is_break {
                    view! { <span class="kind-icon">{icons::cup()}</span> }.into_any()
                } else {
                    view! { <span class="dot" style=format!("background:{color}") aria-hidden="true"></span> }
                        .into_any()
                }}
                <span class="titles">
                    // The task rather than the client, which shows on hover.
                    <span class="work" title=work_tooltip>
                        {project_title(&entry)}
                        {task_title(&entry)
                            .map(|task| view! { <span class="task">{format!(" › {task}")}</span> })}
                    </span>
                    <span class="notes">{notes}</span>
                </span>
                {several
                    .then(|| {
                        view! {
                            <span class="count" aria-hidden="true" title="Show the entries (→)" on:click=toggle_open>
                                {move || {
                                    if expanded.get() {
                                        icons::chevron_down().into_any()
                                    } else {
                                        icons::chevron_right().into_any()
                                    }
                                }}
                                {count}
                            </span>
                        }
                    })}
                <span class="duration">{total}</span>
            </RowLine>
            <Show when=move || expanded.get()>
                <ul class="intervals" id=intervals_id.clone() aria-label="Entries">
                    {intervals.clone()}
                </ul>
            </Show>
        </li>
    }
}

/// One entry of an opened group.
#[component]
fn IntervalRow(group: EntryGroup, entry: EntryView) -> impl IntoView {
    let label = interval_label(&entry);
    let times = time_range(&entry.start_label, entry.end_label.as_deref());
    let duration = format_hm(entry.duration_seconds);
    let row = Row::Interval { group, entry };
    view! {
        <li class="interval">
            <RowLine row label disclosure=(false, None, String::new())>
                <span class="titles">
                    <span class="notes">{times}</span>
                </span>
                <span class="duration">{duration}</span>
            </RowLine>
        </li>
    }
}

/// A focusable row with its −/+ and timer buttons and the delete question. `disclosure` is
/// whether the row opens (several entries), whether it is open, and what it opens.
#[component]
fn RowLine(
    row: Row,
    label: String,
    disclosure: (bool, Option<Memo<bool>>, String),
    children: Children,
) -> impl IntoView {
    let store = expect_context::<Store>();
    let dom_id = row.dom_id();
    let (opens, open, controls) = disclosure;
    let is_selected = {
        let dom_id = dom_id.clone();
        Memo::new(move |_| store.selected_row.get().as_ref() == Some(&dom_id))
    };
    let confirming = {
        let dom_id = dom_id.clone();
        Memo::new(move |_| {
            store.confirm_delete.with(|pending| {
                pending
                    .as_ref()
                    .is_some_and(|pending| pending.row == dom_id)
            })
        })
    };
    let pending = row.pending();
    let editable =
        Memo::new(move |_| store.with_app(|app| app.online).unwrap_or(false) && !pending);
    let breaks_enabled =
        Memo::new(move |_| store.with_app(|app| app.breaks_enabled).unwrap_or(false));
    let (running, is_break) = (row.running(), row.is_break());
    let ids = row.ids();
    let count = ids.len();

    let edit = {
        let row = row.clone();
        move |_| store.edit(&row)
    };
    let focus = {
        let dom_id = dom_id.clone();
        move |_| store.selected_row.set(Some(dom_id.clone()))
    };
    let adjust_up = {
        let row = row.clone();
        move |_| store.adjust(&row, 60)
    };
    let adjust_down = {
        let row = row.clone();
        move |_| store.adjust(&row, -60)
    };
    let toggle = {
        let row = row.clone();
        move |_| store.toggle_row_timer(&row)
    };
    let delete = {
        let (dom_id, ids) = (dom_id.clone(), ids.clone());
        move |_| store.ask_delete(dom_id.clone(), ids.clone())
    };
    let take_break = move |_| store.run::<StartBreak>(());
    let (timer_label, timer_title, timer_key) = match (is_break, running) {
        (false, true) => ("Stop timer", "Stop timer", "S"),
        (false, false) => ("Continue: start a timer like this entry", "Continue", "S"),
        (true, true) => ("End break", "End break", "B"),
        (true, false) => ("Take a break", "Take a break", "B"),
    };
    let keyshortcuts = move || {
        let single_keys = store.single_keys();
        let mut keys = vec!["Enter"];
        if single_keys {
            keys.extend(["E", "S"]);
        }
        keys.extend(["Meta+Equal", "Meta+Minus", "Backspace"]);
        if opens {
            keys.extend(["ArrowRight", "ArrowLeft"]);
        }
        if single_keys && breaks_enabled.get() {
            keys.push("B");
        }
        keys.join(" ")
    };
    let question = match count {
        1 => "Delete this entry?".to_owned(),
        count => format!("Delete these {count} entries?"),
    };

    view! {
        <div
            class="entry-row"
            class:running=running
            class:pending=pending
        >
            <button
                type="button"
                class="entry-main"
                class:with-count=opens
                id=dom_id.clone()
                tabindex=move || if is_selected.get() { "0" } else { "-1" }
                aria-label=label
                aria-expanded=move || opens.then(|| open.is_some_and(|open| open.get()).to_string())
                aria-controls=move || open.is_some_and(|open| open.get()).then(|| controls.clone())
                aria-keyshortcuts=keyshortcuts
                on:focus=focus
                on:click=edit
            >
                {children()}
            </button>
            <div class="row-actions">
                <button
                    type="button"
                    tabindex="-1"
                    class="icon-button"
                    aria-label="Remove 1 hour"
                    title="Remove 1 hour (⌘−)"
                    prop:disabled=move || !editable.get()
                    on:click=adjust_down
                >
                    {icons::minus()}
                </button>
                <button
                    type="button"
                    tabindex="-1"
                    class="icon-button"
                    aria-label="Add 1 hour"
                    title="Add 1 hour (⌘=)"
                    prop:disabled=move || !editable.get()
                    on:click=adjust_up
                >
                    {icons::plus()}
                </button>
                <Show when=move || running && !is_break && breaks_enabled.get()>
                    <button
                        type="button"
                        tabindex="-1"
                        class="icon-button break-button"
                        aria-label="Take a break"
                        title=move || store.titled("Take a break", "B")
                        prop:disabled=move || !editable.get()
                        on:click=take_break
                    >
                        {icons::cup()}
                    </button>
                </Show>
                <Show when=move || !is_break || running || breaks_enabled.get()>
                    <button
                        type="button"
                        tabindex="-1"
                        class="icon-button timer-button"
                        aria-label=timer_label
                        title=move || store.titled(timer_title, timer_key)
                        prop:disabled=move || !editable.get()
                        on:click=toggle.clone()
                    >
                        {if running { icons::stop().into_any() } else { icons::play().into_any() }}
                    </button>
                </Show>
            </div>
        </div>
        <Show when=move || confirming.get()>
            <div class="confirm-delete" role="group" aria-label=question.clone()>
                <span>{question.clone()}</span>
                <button
                    type="button"
                    class="danger"
                    id=format!("confirm-delete-{dom_id}")
                    on:click=move |_| store.confirm_delete()
                >
                    "Delete ↩"
                </button>
                <button type="button" on:click=move |_| store.cancel_delete()>
                    "Cancel ⎋"
                </button>
            </div>
        </Show>
        <button type="button" class="sr-only" tabindex="-1" on:click=delete>
            {if count == 1 { "Delete entry" } else { "Delete entries" }}
        </button>
    }
}

#[component]
fn Footer(week: Memo<Option<WeekView>>) -> impl IntoView {
    let store = expect_context::<Store>();
    let total = Memo::new(move |_| week.get().map(|week| week.total_seconds).unwrap_or(0));
    view! {
        <footer class="footer">
            <button
                type="button"
                class="primary new-entry"
                aria-keyshortcuts=move || if store.single_keys() { "N Meta+N" } else { "Meta+N" }
                on:click=move |_| store.open_new_form()
            >
                {icons::plus()}
                "New entry"
                <kbd aria-hidden="true">{move || if store.single_keys() { "N" } else { "⌘N" }}</kbd>
            </button>
            <span class="week-total" aria-label=move || format!("This week: {}", spoken_duration(total.get()))>
                "Week " <strong>{move || format_hm(total.get())}</strong>
            </span>
        </footer>
    }
}
