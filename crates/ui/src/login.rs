//! Signing in: which solidtime instance, which OAuth client, and how the browser comes back.

use leptos::prelude::*;
use leptos::task::spawn_local;
use solidbar_shared::instance::{DEFAULT_LOOPBACK_PORT, InstanceSettings, RedirectMode};
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
        Some(AuthView::SignedOut { instance, .. } | AuthView::SigningIn { instance }) => instance,
        _ => InstanceSettings::cloud(),
    };

    let url = RwSignal::new(initial.url.clone());
    let client_id = RwSignal::new(initial.client_id.clone());
    let loopback = RwSignal::new(matches!(initial.redirect, RedirectMode::Loopback { .. }));
    let port = RwSignal::new(match initial.redirect {
        RedirectMode::Loopback { port } => port.to_string(),
        RedirectMode::DeepLink => DEFAULT_LOOPBACK_PORT.to_string(),
    });
    let local_error = RwSignal::new(None::<String>);

    let signing_in = Memo::new(move |_| matches!(auth.get(), Some(AuthView::SigningIn { .. })));
    let error = Memo::new(move |_| {
        local_error.get().or_else(|| match auth.get() {
            Some(AuthView::SignedOut { error, .. }) => error,
            _ => None,
        })
    });
    let redirect_uri = Memo::new(move |_| {
        let port = port.get().trim().parse().unwrap_or(DEFAULT_LOOPBACK_PORT);
        RedirectMode::Loopback { port }.redirect_uri()
    });

    request_animation_frame(|| {
        focus_element("instance-url");
    });

    let submit = move |event: leptos::ev::SubmitEvent| {
        event.prevent_default();
        let redirect = if loopback.get_untracked() {
            match port.get_untracked().trim().parse() {
                Ok(port) => RedirectMode::Loopback { port },
                Err(_) => {
                    local_error.set(Some("Choose a port between 1024 and 65535".to_owned()));
                    return;
                }
            }
        } else {
            RedirectMode::DeepLink
        };
        let settings = InstanceSettings {
            url: url.get_untracked(),
            client_id: client_id.get_untracked(),
            redirect,
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
        loopback.set(false);
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
                    prop:value=client_id
                    on:input=move |event| client_id.set(event_target_value(&event))
                    prop:disabled=signing_in
                />
                <fieldset prop:disabled=signing_in>
                    <legend>"How the browser returns to Solidbar"</legend>
                    <label class="choice">
                        <input
                            type="radio"
                            name="redirect"
                            prop:checked=move || !loopback.get()
                            on:change=move |_| loopback.set(false)
                        />
                        "solidtime:// link (app.solidtime.io)"
                    </label>
                    <label class="choice">
                        <input
                            type="radio"
                            name="redirect"
                            prop:checked=loopback
                            on:change=move |_| loopback.set(true)
                        />
                        "Local address (self-hosted)"
                    </label>
                </fieldset>
                <Show
                    when=move || loopback.get()
                    fallback=|| {
                        view! {
                            <p class="help">
                                "Works when Solidbar is installed in Applications. If the official solidtime desktop app is installed too, the browser may hand the sign-in to that app instead."
                            </p>
                        }
                    }
                >
                    <label for="loopback-port">"Port"</label>
                    <input
                        id="loopback-port"
                        type="number"
                        min="1024"
                        max="65535"
                        aria-describedby="redirect-help"
                        prop:value=port
                        on:input=move |event| port.set(event_target_value(&event))
                    />
                    <p id="redirect-help" class="help">
                        "The OAuth client must allow this redirect URI: " <code>{redirect_uri}</code>
                    </p>
                </Show>
                <button type="button" class="link" on:click=use_cloud prop:disabled=signing_in>
                    "Use app.solidtime.io"
                </button>
                {move || error.get().map(|error| view! { <p class="error" role="alert">{error}</p> })}
                <Show
                    when=move || signing_in.get()
                    fallback=|| view! { <button type="submit" class="primary">"Sign in with browser"</button> }
                >
                    <p class="waiting" role="status">"Waiting for you to approve Solidbar in the browser…"</p>
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
