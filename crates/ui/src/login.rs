//! Signing in: which solidtime instance and which OAuth client. How the browser comes back is
//! worked out from what the client allows.

use leptos::prelude::*;
use leptos::task::spawn_local;
use solidbar_shared::instance::{
    DEEP_LINK_REDIRECT, InstanceSettings, LOOPBACK_REDIRECT, Redirect,
};
use solidbar_shared::ipc::{AuthCancel, AuthLogin, Quit};
use solidbar_shared::view::AuthView;

use crate::ipc::call;
use crate::keymap::focus_element;
use crate::store::Store;

#[component]
pub fn Login() -> impl IntoView {
    let store = expect_context::<Store>();
    let auth = Memo::new(move |_| store.with_app(|app| app.auth.clone()));
    let initial = match auth.get_untracked() {
        Some(AuthView::SignedOut { instance, .. } | AuthView::SigningIn { instance, .. }) => {
            instance
        }
        _ => InstanceSettings::cloud(),
    };

    let url = RwSignal::new(initial.url.clone());
    let client_id = RwSignal::new(initial.client_id.clone());
    let local_error = RwSignal::new(None::<String>);

    let signing_in = Memo::new(move |_| matches!(auth.get(), Some(AuthView::SigningIn { .. })));
    let redirect = Memo::new(move |_| match auth.get() {
        Some(AuthView::SigningIn { redirect, .. }) => redirect,
        _ => None,
    });
    let error = Memo::new(move |_| {
        local_error.get().or_else(|| match auth.get() {
            Some(AuthView::SignedOut { error, .. }) => error,
            _ => None,
        })
    });
    request_animation_frame(|| {
        focus_element("instance-url");
    });

    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let settings = InstanceSettings {
            url: url.get_untracked(),
            client_id: client_id.get_untracked(),
        };
        local_error.set(None);
        spawn_local(async move {
            if let Err(error) = call::<AuthLogin>(&settings).await {
                local_error.set(Some(error.message));
            }
        });
    };

    let use_cloud = move |_| {
        let cloud = InstanceSettings::cloud();
        url.set(cloud.url);
        client_id.set(cloud.client_id);
    };

    let cancel = move |_| store.run::<AuthCancel>(());
    let quit = move |_| store.run::<Quit>(());

    view! {
        <main class="login" aria-labelledby="login-title">
            <h1 id="login-title">"Sign in to solidtime"</h1>
            <p class="lead">"Solidbar opens your browser to sign in to your solidtime account."</p>
            <form on:submit=submit>
                <label for="instance-url">"solidtime address"</label>
                <input
                    id="instance-url"
                    type="url"
                    required
                    autocomplete="url"
                    spellcheck="false"
                    prop:value=url
                    on:input=move |event| url.set(event_target_value(&event))
                    prop:disabled=signing_in
                />
                <label for="client-id">"OAuth client ID"</label>
                <input
                    id="client-id"
                    type="text"
                    required
                    autocomplete="off"
                    spellcheck="false"
                    aria-describedby="client-help"
                    prop:value=client_id
                    on:input=move |event| client_id.set(event_target_value(&event))
                    prop:disabled=signing_in
                />
                <p id="client-help" class="help">
                    "For your own solidtime, the client must allow " <code>{LOOPBACK_REDIRECT}</code>
                    " or " <code>{DEEP_LINK_REDIRECT}</code> " as redirect URI. Solidbar uses whichever it allows."
                </p>
                <button type="button" class="link" on:click=use_cloud prop:disabled=signing_in>
                    "Use app.solidtime.io"
                </button>
                {move || error.get().map(|error| view! { <p class="error" role="alert">{error}</p> })}
                <Show
                    when=move || signing_in.get()
                    fallback=|| view! { <button type="submit" class="primary">"Sign in with browser"</button> }
                >
                    <p class="waiting" role="status">
                        {move || match redirect.get() {
                            None => "Checking the OAuth client…",
                            Some(_) => "Waiting for you to approve Solidbar in the browser…",
                        }}
                    </p>
                    <Show when=move || redirect.get() == Some(Redirect::DeepLink)>
                        <p class="help">
                            "The browser returns through a solidtime:// link. If the official solidtime app opens instead, Solidbar can't finish: ask for a client that allows "
                            <code>{LOOPBACK_REDIRECT}</code> "."
                        </p>
                    </Show>
                    <button type="button" on:click=cancel>"Cancel"</button>
                </Show>
            </form>
            <footer class="login-footer">
                <button type="button" class="link" on:click=quit>
                    "Quit Solidbar"
                </button>
            </footer>
        </main>
    }
}
