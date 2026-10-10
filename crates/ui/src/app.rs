//! The root component: wiring to the backend, focus management, live regions.

use leptos::prelude::*;
use leptos::task::spawn_local;
use solidbar_shared::ipc::{
    AppChanged, Bootstrap, CatalogChanged, GetUpdate, NoticeShown, PanelShown, UpdateChanged,
};
use solidbar_shared::settings::Theme;
use solidbar_shared::view::{AuthView, NoticeKind};

use crate::ipc::{call, on};
use crate::keymap::{self, focus_element};
use crate::login::Login;
use crate::org_chooser::OrgChooser;
use crate::shell::Shell;
use crate::store::{Focus, Screen, Store};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Starting,
    SignedOut,
    ChooseOrg,
    SignedIn,
}

#[component]
pub fn App() -> impl IntoView {
    let store = Store::new();
    provide_context(store);

    on::<AppChanged>(move |view| store.app.set(Some(view)));
    on::<CatalogChanged>(move |catalog| store.catalog.set(catalog));
    on::<NoticeShown>(move |notice| store.notify(notice));
    on::<PanelShown>(move |reason| store.panel_shown(reason));
    on::<UpdateChanged>(move |update| store.update.set(update));
    spawn_local(async move {
        match call::<Bootstrap>(&()).await {
            Ok(bootstrap) => {
                store.catalog.set(bootstrap.catalog);
                store.app.set(Some(bootstrap.app));
                store.focus.set(Some(Focus::Entry { date: None }));
            }
            Err(error) => store.error(&error.message),
        }
        if let Ok(update) = call::<GetUpdate>(&()).await {
            store.update.set(update);
        }
    });
    keymap::install(store);

    let theme = Memo::new(move |_| store.with_app(|app| app.settings.theme));
    Effect::new(move |_| {
        let value = match theme.get() {
            Some(Theme::Dark) => "dark",
            Some(Theme::Light) => "light",
            _ => "system",
        };
        if let Some(root) = document().document_element() {
            let _ = root.set_attribute("data-theme", value);
        }
    });

    // After every update, put focus where it was asked to go, or back on the selected entry
    // when the focused row was re-rendered away.
    Effect::new(move |_| {
        store.app.track();
        store.screen.track();
        let target = store.focus.get();
        request_animation_frame(move || apply_focus(store, target));
    });

    let mode = Memo::new(move |_| {
        store
            .with_app(|app| match app.auth {
                AuthView::SignedIn { .. } if app.choose_org => Mode::ChooseOrg,
                AuthView::SignedIn { .. } => Mode::SignedIn,
                _ => Mode::SignedOut,
            })
            .unwrap_or(Mode::Starting)
    });

    view! {
        <LiveRegions />
        {move || match mode.get() {
            Mode::Starting => view! { <p class="splash" role="status">"Starting…"</p> }.into_any(),
            Mode::SignedOut => view! { <Login /> }.into_any(),
            Mode::ChooseOrg => view! { <OrgChooser /> }.into_any(),
            Mode::SignedIn => view! { <Shell /> }.into_any(),
        }}
        <Toast />
    }
}

fn apply_focus(store: Store, target: Option<Focus>) {
    if store.screen.get_untracked() != Screen::List {
        return;
    }
    let Some(target) = target else {
        if focus_lost() {
            focus_selected_entry(store);
        }
        return;
    };
    let selected = store
        .app
        .with_untracked(|app| app.as_ref().map(|app| app.selected_date.clone()));
    let done = match &target {
        // Wait until the backend has switched to the requested day.
        Focus::Day(date) | Focus::Entry { date: Some(date) } if selected.as_ref() != Some(date) => {
            false
        }
        Focus::Day(date) => focus_element(&format!("tab-{date}")),
        Focus::Entry { .. } => focus_selected_entry(store),
        Focus::Element(id) => focus_element(id),
    };
    if done {
        store.focus.set(None);
    }
}

/// Focuses the selected row, else the day's first row, else the day's tab.
fn focus_selected_entry(store: Store) -> bool {
    let Some(day) = store.selected_day() else {
        return false;
    };
    let rows = store.rows();
    let row = store
        .selected_row
        .get_untracked()
        .filter(|id| rows.iter().any(|row| &row.dom_id() == id))
        .or_else(|| rows.first().map(|row| row.dom_id()));
    match row {
        Some(id) => {
            store.selected_row.set(Some(id.clone()));
            focus_element(&id)
        }
        None => focus_element(&format!("tab-{}", day.date)),
    }
}

fn focus_lost() -> bool {
    document()
        .active_element()
        .is_none_or(|element| element.tag_name() == "BODY")
}

/// Screen reader announcements: confirmations politely, errors right away.
#[component]
fn LiveRegions() -> impl IntoView {
    let store = expect_context::<Store>();
    view! {
        <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">
            {move || store.status.get()}
        </div>
        <div class="sr-only" role="alert" aria-atomic="true">
            {move || store.alert.get()}
        </div>
    }
}

/// The visible counterpart of the live regions.
#[component]
fn Toast() -> impl IntoView {
    let store = expect_context::<Store>();
    move || {
        store.toast.get().map(|notice| {
            let error = notice.kind == NoticeKind::Error;
            view! {
                <div class="toast" class:error=error aria-hidden="true">
                    {notice.text}
                </div>
            }
        })
    }
}
