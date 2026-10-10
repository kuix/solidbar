//! Preferences, shown inside the panel.

use leptos::prelude::*;
use leptos::task::spawn_local;
use solidbar_shared::clock::ClockFormat;
use solidbar_shared::ipc::{
    AuthLogout, AutostartArgs, GetAutostart, InstallUpdate, MembershipArgs, OpenInBrowser, Quit,
    ResumeShortcuts, SelectOrg, SetAutostart, SetShortcut, ShortcutArgs, SuspendShortcuts,
    UpdateSettings,
};
use solidbar_shared::settings::{SettingsPatch, Theme};
use solidbar_shared::shortcuts::GlobalAction;
use solidbar_shared::update::UpdateState;
use solidbar_shared::view::AuthView;
use web_sys::KeyboardEvent;

use crate::format::update_status;
use crate::ipc::call;
use crate::keymap::{Keys, display, focus_element};
use crate::store::{Focus, Screen, Store};
use crate::time_field::TimeField;

#[component]
pub fn Settings() -> impl IntoView {
    let store = expect_context::<Store>();
    let settings = Memo::new(move |_| {
        store
            .with_app(|app| app.settings.clone())
            .unwrap_or_default()
    });
    let account = Memo::new(move |_| {
        store
            .with_app(|app| match &app.auth {
                AuthView::SignedIn {
                    user_name,
                    instance_url,
                } => Some((user_name.clone(), instance_url.clone())),
                _ => None,
            })
            .flatten()
    });
    let orgs = Memo::new(move |_| store.with_app(|app| app.orgs.clone()).unwrap_or_default());
    let active = Memo::new(move |_| {
        store
            .with_app(|app| app.active_membership_id.clone())
            .flatten()
    });
    let autostart = RwSignal::new(false);
    spawn_local(async move {
        if let Ok(enabled) = call::<GetAutostart>(&()).await {
            autostart.set(enabled);
        }
    });
    // ⌘O asks for the organization; otherwise start at the top.
    let target = match store.focus.get_untracked() {
        Some(Focus::Element(id)) => {
            store.focus.set(None);
            Some(id)
        }
        _ => None,
    };
    request_animation_frame(move || {
        if !target.is_some_and(|id| focus_element(&id)) {
            focus_element("settings-back");
        }
    });

    let update = move |patch: SettingsPatch| store.run::<UpdateSettings>(patch);
    let set_autostart = move |enabled: bool| {
        spawn_local(async move {
            match call::<SetAutostart>(&AutostartArgs { enabled }).await {
                Ok(()) => autostart.set(enabled),
                Err(error) => store.error(&error.message),
            }
        });
    };
    let back = move |_| {
        store.screen.set(Screen::List);
        store.focus.set(Some(Focus::Entry { date: None }));
    };
    let theme_choice = move |theme: Theme, label: &'static str| {
        view! {
            <label class="choice">
                <input
                    type="radio"
                    name="theme"
                    prop:checked=move || settings.get().theme == theme
                    on:change=move |_| update(SettingsPatch { theme: Some(theme), ..SettingsPatch::default() })
                />
                {label}
            </label>
        }
    };

    let clock_choice = move |clock: ClockFormat, label: &'static str| {
        view! {
            <label class="choice">
                <input
                    type="radio"
                    name="clock"
                    prop:checked=move || settings.get().clock == clock
                    on:change=move |_| update(SettingsPatch { clock: Some(clock), ..SettingsPatch::default() })
                />
                {label}
            </label>
        }
    };
    let workday_start = RwSignal::new(settings.get_untracked().workday_start);
    let save_workday_start = move |workday_start: String| {
        update(SettingsPatch {
            workday_start: Some(workday_start),
            ..SettingsPatch::default()
        })
    };

    let open_solidtime = move |_| store.run::<OpenInBrowser>(());
    let sign_out = move |_| store.run::<AuthLogout>(());
    let quit = move |_| store.run::<Quit>(());

    view! {
        <section class="settings" aria-labelledby="settings-title">
            <div class="settings-header">
                <button
                    type="button"
                    id="settings-back"
                    class="link"
                    aria-keyshortcuts="Escape"
                    on:click=back
                >
                    "‹ Back"
                </button>
                <h2 id="settings-title">"Settings"</h2>
            </div>

            <h3>"Account"</h3>
            {move || {
                account
                    .get()
                    .map(|(name, url)| {
                        view! { <p class="account">{format!("Signed in as {name} on {url}")}</p> }
                    })
            }}
            <div class="button-row">
                <button type="button" on:click=open_solidtime>
                    "Open solidtime"
                </button>
                <button type="button" on:click=sign_out>
                    "Sign out"
                </button>
            </div>

            <Show when=move || orgs.with(|orgs| orgs.len() > 1)>
                <label for="settings-org">"Organization for new entries"</label>
                <select
                    id="settings-org"
                    aria-keyshortcuts="Meta+O"
                    on:change=move |event| {
                        store.run::<SelectOrg>(MembershipArgs {
                            membership_id: event_target_value(&event),
                        })
                    }
                >
                    <For each=move || orgs.get() key=|org| org.membership_id.clone() let:org>
                        <option
                            value=org.membership_id.clone()
                            selected=move || active.get().as_ref() == Some(&org.membership_id)
                        >
                            {org.name.clone()}
                        </option>
                    </For>
                </select>
            </Show>

            <fieldset>
                <legend>"Appearance"</legend>
                {theme_choice(Theme::System, "Match macOS")}
                {theme_choice(Theme::Dark, "Dark")}
                {theme_choice(Theme::Light, "Light")}
            </fieldset>

            <fieldset>
                <legend>"Time format"</legend>
                {clock_choice(ClockFormat::System, "Match macOS")}
                {clock_choice(ClockFormat::TwentyFourHour, "24-hour (14:30)")}
                {clock_choice(ClockFormat::TwelveHour, "12-hour (2:30 PM)")}
            </fieldset>

            <label class="switch">
                <input
                    type="checkbox"
                    role="switch"
                    prop:checked=move || settings.get().menu_bar_text
                    on:change=move |event| {
                        update(SettingsPatch {
                            menu_bar_text: Some(event_target_checked(&event)),
                            ..SettingsPatch::default()
                        })
                    }
                />
                "Show the timer or today's total in the menu bar"
            </label>

            <label class="switch">
                <input
                    type="checkbox"
                    role="switch"
                    prop:checked=autostart
                    on:change=move |event| set_autostart(event_target_checked(&event))
                />
                "Launch at login"
            </label>

            <div class="form-row">
                <label for="workday-start">"New days start at"</label>
                <TimeField
                    id="workday-start"
                    value=workday_start
                    described_by="workday-help"
                    on_commit=save_workday_start
                />
            </div>
            <p id="workday-help" class="help">
                "Where the first entry of an empty day begins; later entries continue from the last one."
            </p>

            <h3>"Keyboard shortcuts"</h3>
            <p class="help">"These work from any app."</p>
            {GlobalAction::ALL
                .into_iter()
                .map(|action| view! { <ShortcutRecorder action /> })
                .collect_view()}

            <Updates />

            <div class="button-row">
                <button type="button" class="danger" on:click=quit>
                    "Quit Solidbar"
                </button>
            </div>
        </section>
    }
}

/// Shows a global shortcut; click it, then press the new combination (⎋ cancels).
#[component]
fn ShortcutRecorder(action: GlobalAction) -> impl IntoView {
    let store = expect_context::<Store>();
    let current = Memo::new(move |_| {
        store
            .with_app(|app| app.settings.shortcut(action).map(str::to_owned))
            .flatten()
    });
    let recording = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    let label_id = format!("shortcut-{action:?}");
    let key_id = format!("shortcut-key-{action:?}");
    // Read as "Show or hide Solidbar ⌥⌘T".
    let labelled_by = format!("{label_id} {key_id}");

    let stop = move || {
        if recording.get_untracked() {
            recording.set(false);
            store.run::<ResumeShortcuts>(());
        }
    };
    let save = move |accelerator: Option<String>| {
        spawn_local(async move {
            let result = call::<SetShortcut>(&ShortcutArgs {
                action,
                accelerator,
            })
            .await;
            problem.set(result.err().map(|error| error.message));
            let _ = call::<ResumeShortcuts>(&()).await;
        });
    };
    let on_keydown = move |event: KeyboardEvent| {
        if !recording.get_untracked() {
            return;
        }
        let keys = Keys::of(&event);
        if keys.is("Tab") || keys.is("Shift+Tab") {
            stop();
            return;
        }
        event.prevent_default();
        event.stop_propagation();
        if keys.is("Escape") {
            stop();
            return;
        }
        // Wait until a modifier and a key are held together.
        if let Some(accelerator) = keys.accelerator() {
            recording.set(false);
            save(Some(accelerator));
        }
    };

    view! {
        <div class="shortcut-row">
            <span id=label_id.clone()>{action.label()}</span>
            <button
                type="button"
                class="shortcut-key"
                id=key_id
                aria-labelledby=labelled_by
                aria-pressed=move || recording.get().to_string()
                on:click=move |_| {
                    problem.set(None);
                    recording.set(true);
                    store.run::<SuspendShortcuts>(());
                }
                on:keydown=on_keydown
                on:blur=move |_| stop()
            >
                {move || {
                    if recording.get() {
                        "Type a shortcut…".to_owned()
                    } else {
                        current.get().map(|accelerator| display(&accelerator)).unwrap_or_else(|| "Off".to_owned())
                    }
                }}
            </button>
            <button
                type="button"
                class="link"
                aria-label=format!("Turn off “{}”", action.label())
                on:click=move |_| save(None)
            >
                "Clear"
            </button>
            {move || problem.get().map(|problem| view! { <p class="field-error" role="alert">{problem}</p> })}
        </div>
    }
}

#[component]
fn Updates() -> impl IntoView {
    let store = expect_context::<Store>();
    let state = Memo::new(move |_| store.update.with(|update| update.state.clone()));
    let available = Memo::new(move |_| matches!(state.get(), UpdateState::Available { .. }));
    // `aria-disabled` rather than `disabled`, so focus stays on the button while it works.
    let busy = Memo::new(move |_| {
        matches!(
            state.get(),
            UpdateState::Checking | UpdateState::Installing { .. }
        )
    });
    let activate = move |_| {
        if busy.get_untracked() {
            return;
        }
        if available.get_untracked() {
            store.run::<InstallUpdate>(());
        } else {
            store.check_for_update();
        }
    };

    view! {
        <h3>"Updates"</h3>
        <p id="update-status" class="help">
            {move || store.update.with(update_status)}
        </p>
        <div class="button-row">
            <button
                type="button"
                id="update-button"
                aria-describedby="update-status"
                aria-disabled=move || busy.get().then_some("true")
                on:click=activate
            >
                {move || if available.get() { "Install and restart" } else { "Check for updates" }}
            </button>
        </div>
    }
}
