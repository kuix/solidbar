//! Shown after signing in to several organizations, before anything else: Solidbar works in
//! one of them at a time. Settings (⌘O) changes it later.

use leptos::prelude::*;
use solidbar_shared::ipc::{AuthLogout, MembershipArgs, Quit, SelectOrg};
use web_sys::KeyboardEvent;

use crate::keymap::{Keys, focus_element};
use crate::store::{Focus, Store};

#[component]
pub fn OrgChooser() -> impl IntoView {
    let store = expect_context::<Store>();
    let orgs = Memo::new(move |_| store.with_app(|app| app.orgs.clone()).unwrap_or_default());
    request_animation_frame(|| {
        focus_element("org-option-0");
    });

    let choose = move |membership_id: String| {
        store.focus.set(Some(Focus::Entry { date: None }));
        store.run::<SelectOrg>(MembershipArgs { membership_id });
    };
    let sign_out = move |_| store.run::<AuthLogout>(());
    let quit = move |_| store.run::<Quit>(());
    // ↑/↓ move between the organizations, like a menu.
    let on_keydown = move |event: KeyboardEvent| {
        let keys = Keys::of(&event);
        let step: isize = if keys.is("ArrowDown") {
            1
        } else if keys.is("ArrowUp") {
            -1
        } else {
            return;
        };
        let count = orgs.with_untracked(Vec::len) as isize;
        let current = document()
            .active_element()
            .and_then(|element| {
                element
                    .id()
                    .strip_prefix("org-option-")?
                    .parse::<isize>()
                    .ok()
            })
            .unwrap_or(-1);
        focus_element(&format!(
            "org-option-{}",
            (current + step).clamp(0, count - 1)
        ));
        event.prevent_default();
    };

    view! {
        <main class="org-chooser" aria-labelledby="org-chooser-title">
            <h1 id="org-chooser-title">"Choose an organization"</h1>
            <p id="org-chooser-lead" class="lead">
                "Solidbar shows and adds time entries in one organization. You can switch later in Settings (⌘O)."
            </p>
            <ul class="org-list" aria-labelledby="org-chooser-title" aria-describedby="org-chooser-lead" on:keydown=on_keydown>
                <For
                    each=move || orgs.get().into_iter().enumerate()
                    key=|(index, org)| (*index, org.membership_id.clone())
                    let:item
                >
                    {
                        let (index, org) = item;
                        let membership_id = org.membership_id.clone();
                        view! {
                            <li>
                                <button
                                    type="button"
                                    id=format!("org-option-{index}")
                                    class="org-option"
                                    on:click=move |_| choose(membership_id.clone())
                                >
                                    {org.name.clone()}
                                </button>
                            </li>
                        }
                    }
                </For>
            </ul>
            <footer class="login-footer">
                <button type="button" class="link" on:click=sign_out>
                    "Sign out"
                </button>
                <button type="button" class="link" on:click=quit>
                    "Quit Solidbar"
                </button>
            </footer>
        </main>
    }
}
