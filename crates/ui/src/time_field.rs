//! A time-of-day text field in the chosen clock format. WebKit's `<input type="time">` follows
//! the page language rather than the Mac, so it would always ask for AM/PM.

use leptos::prelude::*;
use solidbar_shared::clock::Time;
use web_sys::KeyboardEvent;

use crate::keymap::Keys;

/// `value` holds the time as `HH:MM`, or nothing while the text isn't a time. `on_commit` runs
/// with a valid time when the field loses focus. ↑/↓ move by 15 minutes, ⇧↑/⇧↓ by an hour.
#[component]
pub fn TimeField(
    id: &'static str,
    value: RwSignal<String>,
    #[prop(optional)] described_by: Option<&'static str>,
    #[prop(optional, into)] on_commit: Option<Callback<String>>,
) -> impl IntoView {
    let store = expect_context::<crate::store::Store>();
    let hour12 = Memo::new(move |_| store.with_app(|app| app.hour12).unwrap_or(false));
    let shown = move || {
        Time::parse(&value.get_untracked())
            .map(|time| time.display(hour12.get_untracked()))
            .unwrap_or_default()
    };
    let text = RwSignal::new(shown());
    let invalid = RwSignal::new(false);

    // Show the time again in the new format when the setting changes.
    Effect::new(move |previous: Option<()>| {
        hour12.track();
        if previous.is_some() {
            text.set(shown());
        }
    });

    // Show a time set from elsewhere, such as the start of the day the form moved to. While
    // typing, the time comes from the text, so it doesn't change what's typed.
    Effect::new(move |_| {
        let time = value.with(|value| Time::parse(value));
        if let Some(time) = time
            && text.with_untracked(|text| Time::parse(text)) != Some(time)
        {
            text.set(time.display(hour12.get_untracked()));
            invalid.set(false);
        }
    });

    let set_time = move |time: Time| {
        value.set(time.to_hm());
        text.set(time.display(hour12.get_untracked()));
        invalid.set(false);
    };
    let on_keydown = move |event: KeyboardEvent| {
        let keys = Keys::of(&event);
        let step = match () {
            _ if keys.is("ArrowUp") => 15,
            _ if keys.is("ArrowDown") => -15,
            _ if keys.is("Shift+ArrowUp") => 60,
            _ if keys.is("Shift+ArrowDown") => -60,
            _ => return,
        };
        if let Some(time) = Time::parse(&value.get_untracked()) {
            set_time(time.add_minutes(step));
        }
        event.prevent_default();
    };

    let error_id = format!("{id}-error");
    let described = move || {
        let ids: Vec<&str> = described_by
            .into_iter()
            .chain(invalid.get().then_some(error_id.as_str()))
            .collect();
        (!ids.is_empty()).then(|| ids.join(" "))
    };
    let example = move || if hour12.get() { "9:30 AM" } else { "09:30" };

    view! {
        <span class="time-field-wrap">
            <input
                id=id
                class="time-field"
                type="text"
                inputmode="text"
                autocomplete="off"
                spellcheck="false"
                placeholder=example
                aria-invalid=move || invalid.get().to_string()
                aria-describedby=described
                prop:value=text
                on:input=move |event| {
                    let typed = event_target_value(&event);
                    value.set(Time::parse(&typed).map(Time::to_hm).unwrap_or_default());
                    text.set(typed);
                    invalid.set(false);
                }
                on:blur=move |_| match Time::parse(&text.get_untracked()) {
                    Some(time) => {
                        set_time(time);
                        if let Some(commit) = on_commit {
                            commit.run(time.to_hm());
                        }
                    }
                    None => invalid.set(true),
                }
                on:keydown=on_keydown
            />
            <Show when=move || invalid.get()>
                <span id=format!("{id}-error") class="field-error">
                    {move || format!("Enter a time like {}", example())}
                </span>
            </Show>
        </span>
    }
}
