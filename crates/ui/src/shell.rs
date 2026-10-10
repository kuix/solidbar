//! The signed-in panel: header, week strip, the selected day and the footer.

use leptos::prelude::*;
use solidbar_shared::ipc::{
    AdjustArgs, AdjustEntry, AutostartArgs, InstallUpdate, MembershipArgs, PanelSetPinned, PinArgs,
    SelectOrg, SetAutostart, StopTimer, UpdateSettings,
};
use solidbar_shared::settings::SettingsPatch;
use solidbar_shared::update::UpdateState;
use solidbar_shared::view::{DayView, EntryView, WeekView};

use crate::entry_form::EntryForm;
use crate::format::{entry_label, format_hm, installing, spoken_duration, work_title};
use crate::icons;
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
            <WeekStrip week selected_date />
            <div
                class="day-panel"
                class:with-form=move || screen.get() == Screen::Form
                id="day-panel"
                role="tabpanel"
                aria-labelledby=move || format!("tab-{}", selected_date.get())
            >
                {move || match screen.get() {
                    Screen::List => view! { <DayList day /> }.into_any(),
                    Screen::Form => {
                        store
                            .form
                            .get_untracked()
                            .map(|init| view! { <EntryForm init /> }.into_any())
                            .unwrap_or_else(|| ().into_any())
                    }
                    Screen::Settings => view! { <Settings /> }.into_any(),
                }}
            </div>
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

    view! {
        <header class="header">
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
                aria-keyshortcuts="T"
                title="Go to today (T)"
                on:click=move |_| store.go_to_today(Toward::Entries)
            >
                "Today"
            </button>
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
            <button
                type="button"
                class="icon-button"
                aria-label="Settings"
                aria-keyshortcuts="Meta+Comma"
                title="Settings (⌘,)"
                on:click=move |_| store.screen.set(Screen::Settings)
            >
                {icons::gear()}
            </button>
        </header>
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
                    view! {
                        <div class="banner timer-banner" role="region" aria-label="Running timer">
                            <span>"Timer running in " <strong>{org}</strong></span>
                            <button type="button" on:click=stop>"Stop"</button>
                            <button type="button" on:click=switch>"Switch"</button>
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
    let entries = Memo::new(move |_| day.get().map(|day| day.entries).unwrap_or_default());
    view! {
        {move || match day.get() {
            None => view! { <p class="empty" role="status">"Loading…"</p> }.into_any(),
            Some(day) => {
                let empty = day.entries.is_empty();
                let label = day.label.clone();
                view! {
                    <div class="day-heading">
                        <h2 id="day-heading">{day.label.clone()}</h2>
                        <span class="day-heading-total" aria-label=spoken_duration(day.total_seconds)>
                            {format_hm(day.total_seconds)}
                        </span>
                    </div>
                    <Show
                        when=move || !empty
                        fallback=move || {
                            view! {
                                <p class="empty">
                                    {format!("No time on {label}. ")}
                                    <button type="button" class="link" on:click=move |_| store.open_new_form()>
                                        "Add time"
                                    </button>
                                    " (⌘N)"
                                </p>
                            }
                        }
                    >
                        <ul class="entries" aria-labelledby="day-heading" aria-busy=move || loading.get().to_string()>
                            <For each=move || entries.get() key=|entry| entry.clone() let:entry>
                                <EntryRow entry />
                            </For>
                        </ul>
                    </Show>
                }
                    .into_any()
            }
        }}
    }
}

#[component]
fn EntryRow(entry: EntryView) -> impl IntoView {
    let store = expect_context::<Store>();
    let id = entry.id.clone();
    let is_selected = {
        let id = id.clone();
        Memo::new(move |_| store.selected_entry.get().as_ref() == Some(&id))
    };
    let confirming = {
        let id = id.clone();
        Memo::new(move |_| store.confirm_delete.get().as_ref() == Some(&id))
    };
    let editable = {
        let pending = entry.pending;
        Memo::new(move |_| store.with_app(|app| app.online).unwrap_or(false) && !pending)
    };
    let adjust = {
        let id = id.clone();
        move |delta_minutes: i64| {
            store.selected_entry.set(Some(id.clone()));
            store.run::<AdjustEntry>(AdjustArgs {
                id: id.clone(),
                delta_minutes,
            })
        }
    };
    let adjust_down = adjust.clone();
    let toggle = {
        let entry = entry.clone();
        move |_| store.toggle_entry_timer(&entry)
    };
    let edit = {
        let id = id.clone();
        move |_| store.open_edit_form(id.clone())
    };
    let focus = {
        let id = id.clone();
        move |_| store.selected_entry.set(Some(id.clone()))
    };
    let delete = {
        let id = id.clone();
        move |_| store.ask_delete(id.clone())
    };
    let color = entry
        .color
        .clone()
        .unwrap_or_else(|| "var(--no-project)".to_owned());
    let times = format!(
        "{}–{}",
        entry.start_label,
        entry.end_label.clone().unwrap_or_else(|| "now".to_owned())
    );

    view! {
        <li class="entry" class:running=entry.running class:pending=entry.pending>
            <button
                type="button"
                class="entry-main"
                id=format!("entry-{id}")
                tabindex=move || if is_selected.get() { "0" } else { "-1" }
                aria-label=entry_label(&entry)
                aria-keyshortcuts="Enter S Meta+Equal Meta+Minus Backspace"
                on:focus=focus
                on:click=edit
            >
                <span class="dot" style=format!("background:{color}") aria-hidden="true"></span>
                <span class="titles">
                    <span class="work">
                        {work_title(&entry)}
                        {entry
                            .client_name
                            .clone()
                            .map(|client| view! { <span class="client">{format!(" · {client}")}</span> })}
                    </span>
                    <span class="notes">
                        {if entry.description.is_empty() { times.clone() } else { entry.description.clone() }}
                    </span>
                </span>
                <span class="duration">{format_hm(entry.duration_seconds)}</span>
            </button>
            <div class="row-actions">
                <button
                    type="button"
                    tabindex="-1"
                    class="icon-button"
                    aria-label="Remove 1 hour"
                    title="Remove 1 hour (⌘−)"
                    prop:disabled=move || !editable.get()
                    on:click=move |_| adjust_down(-60)
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
                    on:click=move |_| adjust(60)
                >
                    {icons::plus()}
                </button>
                <button
                    type="button"
                    tabindex="-1"
                    class="icon-button timer-button"
                    aria-label=if entry.running { "Stop timer" } else { "Start a timer like this entry" }
                    title=if entry.running { "Stop timer (S)" } else { "Start timer (S)" }
                    prop:disabled=move || !editable.get()
                    on:click=toggle
                >
                    {if entry.running { icons::stop().into_any() } else { icons::play().into_any() }}
                </button>
            </div>
            <Show when=move || confirming.get()>
                <div class="confirm-delete" role="group" aria-label="Delete this entry?">
                    <span>"Delete this entry?"</span>
                    <button
                        type="button"
                        class="danger"
                        id=format!("confirm-delete-{id}")
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
                "Delete entry"
            </button>
        </li>
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
                aria-keyshortcuts="Meta+N"
                on:click=move |_| store.open_new_form()
            >
                {icons::plus()}
                "New entry"
                <kbd aria-hidden="true">"⌘N"</kbd>
            </button>
            <span class="week-total" aria-label=move || format!("This week: {}", spoken_duration(total.get()))>
                "Week " <strong>{move || format_hm(total.get())}</strong>
            </span>
        </footer>
    }
}
