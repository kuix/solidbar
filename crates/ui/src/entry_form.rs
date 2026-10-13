//! The entry form (new and edit), with the project/task picker.

use leptos::prelude::*;
use leptos::task::spawn_local;
use solidbar_shared::clock::Time;
use solidbar_shared::duration::parse_minutes;
use solidbar_shared::ipc::{EntryDraft, SaveEntry};
use solidbar_shared::picker;
use solidbar_shared::view::{EntryKind, FormInit, WorkOption};
use wasm_bindgen::JsCast;
use web_sys::{KeyboardEvent, MouseEvent};

use crate::icons;
use crate::ipc::call;
use crate::keymap::{Keys, focus_element};
use crate::store::{FORM_DELETE, HOLD_TO_DELETE, Store};
use crate::time_field::TimeField;

/// The selected project and task, or a break.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Work {
    kind: EntryKind,
    project_id: Option<String>,
    task_id: Option<String>,
}

impl Work {
    fn is(&self, option: &WorkOption) -> bool {
        self.kind == option.kind
            && self.project_id == option.project_id
            && self.task_id == option.task_id
    }
}

#[component]
pub fn EntryForm(init: FormInit) -> impl IntoView {
    let store = expect_context::<Store>();
    let group_ids = init.group_ids.clone();
    let in_group = !group_ids.is_empty();
    let editing = init.entry_id.is_some() || in_group;
    let running = init.running;
    let day_label = store
        .with_app(|app| {
            app.week
                .as_ref()?
                .days
                .iter()
                .find(|day| day.date == init.date)
                .map(|day| day.label.clone())
        })
        .flatten()
        .unwrap_or_else(|| init.date.clone());
    let title = match (in_group, editing, running) {
        (true, _, _) => format!("Edit {} entries", group_ids.len()),
        (false, true, true) => "Edit running timer".to_owned(),
        (false, true, false) => "Edit time entry".to_owned(),
        (false, false, _) => "New time entry".to_owned(),
    };

    let work = RwSignal::new(Work {
        kind: init.kind,
        project_id: init.project_id.clone(),
        task_id: init.task_id.clone(),
    });
    let is_break = Memo::new(move |_| work.with(|work| work.kind == EntryKind::Break));
    let description = RwSignal::new(init.description.clone());
    let duration = RwSignal::new(init.duration.clone());
    let start_time = RwSignal::new(init.start_time.clone());
    // Changed by the person, the start is where a timer starts (otherwise: now).
    let start_changed = {
        let initial = init.start_time.clone();
        Memo::new(move |_| start_time.with(|time| !time.is_empty() && *time != initial))
    };
    let hour12 = Memo::new(move |_| store.with_app(|app| app.hour12).unwrap_or(false));
    let timer_label = move || match Time::parse(&start_time.get()) {
        Some(time) if start_changed.get() => {
            format!("Start timer from {}", time.display(hour12.get()))
        }
        _ => "Start timer".to_owned(),
    };
    let billable = RwSignal::new(init.billable);
    let error = RwSignal::new(None::<String>);
    let saving = RwSignal::new(false);
    let attempted = RwSignal::new(false);
    let recents = Memo::new(move |_| store.catalog.with(|catalog| catalog.recents.clone()));

    let duration_problem = Memo::new(move |_| {
        let text = duration.get();
        if running || in_group || (text.trim().is_empty() && !attempted.get()) {
            return None;
        }
        parse_minutes(&text)
            .err()
            .map(|problem| problem.to_string())
    });

    let pick = Callback::new(move |option: WorkOption| {
        work.set(Work {
            kind: option.kind,
            project_id: option.project_id,
            task_id: option.task_id,
        });
        // Breaks are never billable.
        if option.kind == EntryKind::Break {
            billable.set(false);
        } else if !editing {
            billable.set(option.billable);
        }
    });

    let entry_id = init.entry_id.clone();
    let date = init.date.clone();
    let draft_group_ids = group_ids.clone();
    let submit = Callback::new(move |start_timer: bool| {
        if saving.get_untracked() {
            return;
        }
        attempted.set(true);
        let start_timer = start_timer && !editing;
        let duration_text = duration.get_untracked();
        // A group keeps the times of its entries.
        let duration_optional =
            running || in_group || (start_timer && duration_text.trim().is_empty());
        if !duration_optional && let Err(problem) = parse_minutes(&duration_text) {
            error.set(Some(problem.to_string()));
            focus_element("duration");
            return;
        }
        if !in_group && start_time.with_untracked(String::is_empty) {
            error.set(Some("Enter when the entry starts".to_owned()));
            focus_element("start-time");
            return;
        }
        let Work {
            kind,
            project_id,
            task_id,
        } = work.get_untracked();
        let draft = EntryDraft {
            id: entry_id.clone(),
            group_ids: draft_group_ids.clone(),
            kind,
            date: date.clone(),
            project_id,
            task_id,
            description: description.get_untracked(),
            billable: billable.get_untracked(),
            duration: duration_text,
            start_time: start_time.get_untracked(),
            start_timer,
            start_changed: start_changed.get_untracked(),
        };
        saving.set(true);
        error.set(None);
        spawn_local(async move {
            match call::<SaveEntry>(&draft).await {
                Ok(()) => store.close_form(),
                Err(problem) => {
                    error.set(Some(problem.message));
                    saving.set(false);
                }
            }
        });
    });

    let on_keydown = move |event: KeyboardEvent| {
        if event.default_prevented() || event.is_composing() {
            return;
        }
        let keys = Keys::of(&event);
        let target = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok());
        let target_tag = target
            .as_ref()
            .map(|element| element.tag_name())
            .unwrap_or_default();
        let target_id = target
            .as_ref()
            .map(|element| element.id())
            .unwrap_or_default();
        let handled = if keys.alt && !keys.meta && !keys.control {
            match keys.digit() {
                Some(hours @ 1..=8) if !running && !in_group => {
                    duration.set(format!("{hours}:00"));
                    true
                }
                _ => false,
            }
        } else if keys.meta && !keys.alt && !keys.control && keys.digit().is_some() {
            let index = keys.digit().unwrap_or_default() as usize;
            match recents.get_untracked().get(index.wrapping_sub(1)) {
                Some(option) => {
                    pick.run(option.clone());
                    true
                }
                None => false,
            }
        } else if keys.is("Meta+Enter") {
            submit.run(true);
            true
        } else if keys.is("Enter") && (target_tag != "BUTTON" || target_id == "work-button") {
            submit.run(false);
            true
        } else {
            false
        };
        if handled {
            event.prevent_default();
        }
    };

    // On the window rather than the form: tabbing past the last text field (without macOS
    // Keyboard navigation) leaves focus on the page, and the shortcuts must still work.
    let keys = window_event_listener(leptos::ev::keydown, on_keydown);
    on_cleanup(move || keys.remove());

    request_animation_frame(|| {
        focus_element("work-button");
    });

    let delete_ids: Vec<String> = if in_group {
        group_ids.clone()
    } else {
        init.entry_id.clone().into_iter().collect()
    };
    let group_note = in_group.then(|| {
        format!(
            "Changes apply to all {} entries ({} in total); their times stay as they are.",
            group_ids.len(),
            init.duration
        )
    });
    view! {
        <form
            class="entry-form"
            aria-labelledby="form-title"
            on:submit=|event| event.prevent_default()
        >
            // Only the fields scroll; the buttons stay at the bottom of the panel.
            <div class="form-body" on:scroll=|_| place_work_popup()>
                <h2 id="form-title">
                    {title} <span class="form-day">{format!(" · {day_label}")}</span>
                </h2>
                <Show when=move || recents.with(|recents| !recents.is_empty())>
                    <div class="recents" role="group" aria-label="Recent projects">
                        <For
                            each=move || recents.get().into_iter().enumerate()
                            key=|(index, option)| (*index, option.clone())
                            let:item
                        >
                            <RecentButton number=item.0 + 1 option=item.1 pick />
                        </For>
                    </div>
                </Show>
                {group_note.map(|note| view! { <p class="group-note">{note}</p> })}
                <WorkPicker work pick />
                <label for="notes">"Notes"</label>
                <textarea
                    id="notes"
                    rows="2"
                    aria-describedby="notes-help"
                    prop:value=description
                    on:input=move |event| description.set(event_target_value(&event))
                ></textarea>
                <p id="notes-help" class="help">"⇧↩ for a new line"</p>
                <Show when=move || !running && !in_group>
                    <fieldset class="duration-field">
                        <legend>"Duration"</legend>
                        <div class="chips">
                            {(1..=8u32)
                                .map(|hours| {
                                    view! {
                                        <button
                                            type="button"
                                            class="chip"
                                            aria-label=format!("{hours} hour{}", if hours == 1 { "" } else { "s" })
                                            aria-keyshortcuts=format!("Alt+{hours}")
                                            aria-pressed=move || {
                                                (parse_minutes(&duration.get()) == Ok(hours * 60)).to_string()
                                            }
                                            on:click=move |_| duration.set(format!("{hours}:00"))
                                        >
                                            {format!("{hours}h")}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                        <input
                            id="duration"
                            type="text"
                            inputmode="decimal"
                            autocomplete="off"
                            placeholder="1:30, 1.5 or 90m"
                            aria-label="Duration"
                            aria-describedby="duration-help"
                            aria-invalid=move || duration_problem.get().is_some().to_string()
                            aria-errormessage="duration-error"
                            prop:value=duration
                            on:input=move |event| duration.set(event_target_value(&event))
                        />
                        <p id="duration-help" class="help">"⌥1–⌥8 for whole hours"</p>
                        {move || {
                            duration_problem
                                .get()
                                .map(|problem| view! { <p id="duration-error" class="field-error">{problem}</p> })
                        }}
                    </fieldset>
                </Show>
                <div class="form-row">
                    <Show when=move || !in_group>
                        <label for="start-time">"Start"</label>
                        <TimeField id="start-time" value=start_time />
                    </Show>
                    <label class="switch" class:disabled=is_break>
                        <input
                            type="checkbox"
                            role="switch"
                            aria-describedby=move || is_break.get().then_some("billable-help")
                            prop:checked=billable
                            prop:disabled=is_break
                            on:change=move |event| billable.set(event_target_checked(&event))
                        />
                        "Billable"
                    </label>
                    <Show when=move || is_break.get()>
                        <span id="billable-help" class="help">"Breaks aren't billable"</span>
                    </Show>
                </div>
            </div>
            <div class="form-footer">
                {move || error.get().map(|error| view! { <p class="error" role="alert">{error}</p> })}
                <div class="actions">
                    <button
                        type="button"
                        class="primary"
                        aria-keyshortcuts="Enter"
                        prop:disabled=saving
                        on:click=move |_| submit.run(false)
                    >
                        "Save"
                        <kbd aria-hidden="true">"↩"</kbd>
                    </button>
                    <Show when=move || !editing>
                        <button
                            type="button"
                            aria-keyshortcuts="Meta+Enter"
                            prop:disabled=saving
                            on:click=move |_| submit.run(true)
                        >
                            {timer_label}
                            <kbd aria-hidden="true">"⌘↩"</kbd>
                        </button>
                    </Show>
                    <button type="button" aria-keyshortcuts="Escape" on:click=move |_| store.close_form()>
                        "Cancel"
                        <kbd aria-hidden="true">"⎋"</kbd>
                    </button>
                    {(!delete_ids.is_empty()).then(|| view! { <HoldToDelete ids=delete_ids.clone() /> })}
                </div>
            </div>
        </form>
    }
}

#[component]
fn RecentButton(number: usize, option: WorkOption, pick: Callback<WorkOption>) -> impl IntoView {
    let color = option
        .color
        .clone()
        .unwrap_or_else(|| "var(--no-project)".to_owned());
    let label = option.label.clone();
    let title = format!("{label} (⌘{number})");
    view! {
        <button
            type="button"
            class="recent"
            aria-keyshortcuts=format!("Meta+{number}")
            title=title
            on:click=move |_| pick.run(option.clone())
        >
            <kbd aria-hidden="true">{format!("⌘{number}")}</kbd>
            <span class="dot" style=format!("background:{color}") aria-hidden="true"></span>
            <span class="recent-label">{label}</span>
        </button>
    }
}

/// Puts the open project list under its button. The list is `position: fixed` so the
/// scrolling form body doesn't clip it; it follows the button when the body scrolls.
fn place_work_popup() {
    let element = |id| {
        document()
            .get_element_by_id(id)
            .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
    };
    let (Some(button), Some(popup)) = (element("work-button"), element("work-popup")) else {
        return;
    };
    if popup.has_attribute("hidden") {
        return;
    }
    let rect = button.get_bounding_client_rect();
    let height = window()
        .inner_height()
        .ok()
        .and_then(|height| height.as_f64())
        .unwrap_or(520.0);
    // What's left under the button, less the search field and the popup's padding.
    let list_height = (height - rect.bottom() - 64.0).clamp(80.0, 220.0);
    let style = popup.style();
    for (property, value) in [
        ("top", rect.bottom() + 4.0),
        ("left", rect.left()),
        ("width", rect.width()),
        ("--list-height", list_height),
    ] {
        let _ = style.set_property(property, &format!("{value}px"));
    }
}

/// The project and task as a pop-up button, like a `<select>`, whose list has a search field
/// (an APG editable combobox; `aria-activedescendant` points at the highlighted option).
/// Space, ↓ or a click opens it; typing on the closed button opens it and starts searching, so
/// ⌘N, "rand", ↩ works. ↩ on the closed button saves the form, as everywhere else in it.
#[component]
fn WorkPicker(work: RwSignal<Work>, pick: Callback<WorkOption>) -> impl IntoView {
    let store = expect_context::<Store>();
    let options = Memo::new(move |_| store.catalog.with(|catalog| catalog.options.clone()));
    let query = RwSignal::new(String::new());
    let open = RwSignal::new(false);
    let active = RwSignal::new(0usize);

    let is_selected = move |option: &WorkOption| work.with_untracked(|work| work.is(option));
    let selected = Memo::new(move |_| {
        work.track();
        options
            .with(|options| options.iter().find(|option| is_selected(option)).cloned())
            .unwrap_or_else(|| {
                let Work {
                    kind, project_id, ..
                } = work.get_untracked();
                WorkOption {
                    label: match (kind, &project_id) {
                        (EntryKind::Break, _) => "Break",
                        (EntryKind::Work, Some(_)) => "Unknown project",
                        (EntryKind::Work, None) => "No project",
                    }
                    .to_owned(),
                    kind,
                    project_id,
                    task_id: None,
                    task_name: None,
                    client_name: None,
                    color: None,
                    billable: false,
                }
            })
    });
    let listing = Memo::new(move |_| {
        let query = query.get();
        options.with(|options| picker::list(&query, options))
    });

    let open_with = move |text: String| {
        query.set(text.clone());
        // Without a search, start on the current choice; with one, on the best match.
        let start = listing.with_untracked(|listing| {
            let current = text
                .is_empty()
                .then(|| listing.options.iter().position(is_selected));
            current.flatten().or(listing.best).unwrap_or(0)
        });
        active.set(start);
        open.set(true);
        // Show the list and focus its search field right away rather than after the next
        // render, so the keys typed after the first one land in it.
        if let Some(popup) = document().get_element_by_id("work-popup") {
            let _ = popup.remove_attribute("hidden");
        }
        place_work_popup();
        if let Some(search) = document()
            .get_element_by_id("work-search")
            .and_then(|element| element.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            search.set_value(&text);
            let _ = search.focus();
        }
    };
    let close = move |refocus: bool| {
        open.set(false);
        query.set(String::new());
        if refocus {
            focus_element("work-button");
        }
    };
    let choose = move |option: WorkOption| {
        pick.run(option);
        close(true);
    };

    // A recent pair picked with ⌘1–⌘5 while the list is open closes it.
    Effect::new(move |previous: Option<()>| {
        work.track();
        if previous.is_some() && open.get_untracked() {
            close(true);
        }
    });

    // Keep the highlighted option visible while moving through a long list, with the client
    // over it when it's the client's first.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let id = format!("work-option-{}", active.get());
        request_animation_frame(move || {
            if let Some(element) = document().get_element_by_id(&id) {
                let options = web_sys::ScrollIntoViewOptions::new();
                options.set_block(web_sys::ScrollLogicalPosition::Nearest);
                if let Some(heading) = element
                    .previous_element_sibling()
                    .filter(|sibling| sibling.class_list().contains("group-heading"))
                {
                    heading.scroll_into_view_with_scroll_into_view_options(&options);
                }
                element.scroll_into_view_with_scroll_into_view_options(&options);
            }
        });
    });

    let on_button_keydown = move |event: KeyboardEvent| {
        if event.is_composing() {
            return;
        }
        let keys = Keys::of(&event);
        let key = event.key();
        let plain = !keys.meta && !keys.control && !keys.alt;
        let handled = if keys.is("Space")
            || keys.is("ArrowDown")
            || keys.is("ArrowUp")
            || keys.is("Alt+ArrowDown")
        {
            open_with(String::new());
            true
        } else if plain && key.chars().count() == 1 {
            open_with(key);
            true
        } else {
            false
        };
        if handled {
            event.prevent_default();
            event.stop_propagation();
        }
    };

    let on_search_keydown = move |event: KeyboardEvent| {
        if event.is_composing() {
            return;
        }
        let keys = Keys::of(&event);
        let count = listing.with_untracked(|listing| listing.options.len());
        let handled = if keys.is("ArrowDown") {
            active.update(|index| *index = (*index + 1).min(count.saturating_sub(1)));
            true
        } else if keys.is("ArrowUp") {
            active.update(|index| *index = index.saturating_sub(1));
            true
        } else if keys.is("Enter") {
            if let Some(option) = listing
                .with_untracked(|listing| listing.options.get(active.get_untracked()).cloned())
            {
                choose(option);
            }
            true
        } else if keys.is("Escape") {
            close(true);
            true
        } else {
            false
        };
        if handled {
            event.prevent_default();
            event.stop_propagation();
        }
    };

    view! {
        <div class="work-picker">
            <span id="work-label" class="label">"Project and task"</span>
            <button
                type="button"
                id="work-button"
                class="select-button"
                aria-haspopup="listbox"
                aria-expanded=move || open.get().to_string()
                aria-labelledby="work-label work-value"
                on:mousedown=move |event| {
                    // Keep focus in the search field, so the click closes the list.
                    if open.get_untracked() {
                        event.prevent_default();
                    }
                }
                on:click=move |_| {
                    if open.get_untracked() {
                        close(true);
                    } else {
                        open_with(String::new());
                    }
                }
                on:keydown=on_button_keydown
            >
                {move || {
                    let option = selected.get();
                    let marker = option_marker(&option);
                    view! {
                        {marker}
                        <span id="work-value" class="value">
                            {option.label}
                            {option.client_name.map(|client| view! { <span class="client">{format!(" · {client}")}</span> })}
                        </span>
                    }
                }}
                <span class="chevron">{icons::chevron_up_down()}</span>
            </button>
            <div id="work-popup" class="work-popup" hidden=move || !open.get()>
                <input
                    id="work-search"
                    type="text"
                    role="combobox"
                    aria-label="Search projects and tasks"
                    aria-autocomplete="list"
                    aria-expanded=move || open.get().to_string()
                    aria-controls="work-listbox"
                    aria-activedescendant=move || {
                        (open.get() && listing.with(|listing| !listing.options.is_empty()))
                            .then(|| format!("work-option-{}", active.get()))
                    }
                    placeholder="Search projects, tasks and clients"
                    autocomplete="off"
                    spellcheck="false"
                    prop:value=query
                    on:input=move |event| {
                        query.set(event_target_value(&event));
                        active.set(listing.with_untracked(|listing| listing.best.unwrap_or(0)));
                    }
                    on:blur=move |_| {
                        if open.get_untracked() {
                            close(false);
                        }
                    }
                    on:keydown=on_search_keydown
                />
                <ul id="work-listbox" role="listbox" aria-label="Projects and tasks" class="listbox">
                    <For
                        each=move || listing.get().rows.into_iter().enumerate()
                        key=|(index, row)| (*index, row.clone())
                        let:item
                    >
                        {
                            // Clicks on the list keep the focus in the search field.
                            let keep_focus = |event: MouseEvent| event.prevent_default();
                            match item.1 {
                                picker::Row::Client(client) => {
                                    // Each option names its client, so the heading is only seen.
                                    view! {
                                        <li
                                            class="group-heading"
                                            role="presentation"
                                            aria-hidden="true"
                                            on:mousedown=keep_focus
                                        >
                                            {client.unwrap_or_else(|| "No client".to_owned())}
                                        </li>
                                    }
                                        .into_any()
                                }
                                picker::Row::Divider => {
                                    view! {
                                        <li
                                            class="divider"
                                            role="presentation"
                                            aria-hidden="true"
                                            on:mousedown=keep_focus
                                        ></li>
                                    }
                                        .into_any()
                                }
                                picker::Row::Option { position, option, task } => {
                                    // A task shows only its name, under its project.
                                    let marker = (!task).then(|| option_marker(&option));
                                    let name = match (&option.task_name, task) {
                                        (Some(name), true) => name.clone(),
                                        _ => option.label.clone(),
                                    };
                                    let spoken = match &option.client_name {
                                        Some(client) => format!("{} · {client}", option.label),
                                        None => option.label.clone(),
                                    };
                                    let current = is_selected(&option);
                                    view! {
                                        <li
                                            role="option"
                                            id=format!("work-option-{position}")
                                            class:task=task
                                            class:active=move || active.get() == position
                                            aria-selected=move || (active.get() == position).to_string()
                                            aria-label=spoken
                                            on:mousedown=keep_focus
                                            on:click=move |_| choose(option.clone())
                                        >
                                            {marker}
                                            <span class="option-label">{name}</span>
                                            <span class="current" aria-hidden="true">
                                                {current.then(icons::check)}
                                            </span>
                                        </li>
                                    }
                                        .into_any()
                                }
                            }
                        }
                    </For>
                    <Show when=move || listing.with(|listing| listing.options.is_empty())>
                        <li class="empty" role="presentation">"No matching projects"</li>
                    </Show>
                </ul>
                <p class="sr-only" aria-live="polite">
                    {move || {
                        if open.get() {
                            // A project listed for its tasks isn't a result.
                            match listing.with(|listing| listing.matched) {
                                0 => "No matching projects".to_owned(),
                                1 => "1 result".to_owned(),
                                count => format!("{count} results"),
                            }
                        } else {
                            String::new()
                        }
                    }}
                </p>
            </div>
        </div>
    }
}

/// The project's color dot, or a cup for "Break".
fn option_marker(option: &WorkOption) -> AnyView {
    if option.kind == EntryKind::Break {
        return view! { <span class="kind-icon">{icons::cup()}</span> }.into_any();
    }
    let color = option
        .color
        .clone()
        .unwrap_or_else(|| "var(--no-project)".to_owned());
    view! { <span class="dot" style=format!("background:{color}") aria-hidden="true"></span> }
        .into_any()
}

/// Deletes once held down, with the pointer or with Space or ↩, for [`HOLD_TO_DELETE`]: a stray
/// click can't delete, and nothing has to be confirmed. Assistive technology activates buttons
/// with a click it can't hold, so a click that no press started asks "Delete?" instead.
#[component]
fn HoldToDelete(ids: Vec<String>) -> impl IntoView {
    let store = expect_context::<Store>();
    let count = ids.len();
    let holding = Memo::new(move |_| store.is_holding(FORM_DELETE));
    let hinting = Memo::new(move |_| {
        store
            .hold_hint
            .with(|hint| hint.as_deref() == Some(FORM_DELETE))
    });
    let confirming = RwSignal::new(false);
    // Set by a press, so the click that ends it isn't taken for an assistive technology's.
    let pressed = StoredValue::new(false);

    let start = {
        let ids = ids.clone();
        move || {
            pressed.set_value(true);
            store.hold_delete(FORM_DELETE.to_owned(), ids.clone());
        }
    };
    let release = move || {
        store.release_delete();
        // The click that follows a press arrives before this runs.
        set_timeout(move || pressed.set_value(false), std::time::Duration::ZERO);
    };
    let activates =
        |event: &KeyboardEvent| matches!(event.code().as_str(), "Space" | "Enter" | "NumpadEnter");
    let on_pointerdown = {
        let start = start.clone();
        move |event: leptos::ev::PointerEvent| {
            if event.button() == 0 {
                start();
            }
        }
    };
    let on_keydown = move |event: KeyboardEvent| {
        if activates(&event) {
            event.prevent_default();
            if !event.repeat() {
                start();
            }
        }
    };
    let on_keyup = move |event: KeyboardEvent| {
        if activates(&event) {
            event.prevent_default();
            release();
        }
    };
    let on_click = move |_| {
        if !pressed.get_value() {
            confirming.set(true);
            request_animation_frame(|| {
                focus_element("confirm-form-delete");
            });
        }
    };
    let delete = {
        let ids = ids.clone();
        move |_| store.delete_now(ids.clone())
    };
    let keep = move |_| {
        confirming.set(false);
        request_animation_frame(|| {
            focus_element("form-delete");
        });
    };
    let (help, question) = match count {
        1 => (
            "Hold for 2 seconds, or hold Space, to delete this entry".to_owned(),
            "Delete this entry?".to_owned(),
        ),
        count => (
            format!("Hold for 2 seconds, or hold Space, to delete these {count} entries"),
            format!("Delete these {count} entries?"),
        ),
    };

    view! {
        <div class="delete-area">
        <Show when=move || holding.get() || hinting.get()>
            <span class="hold-hint" aria-hidden="true">
                {move || if holding.get() { "Keep holding…" } else { "Hold to delete" }}
            </span>
        </Show>
        <Show
            when=move || !confirming.get()
            fallback=move || {
                view! {
                    <div class="confirm-delete form-confirm" role="group" aria-label=question.clone()>
                        <span>{question.clone()}</span>
                        <button type="button" class="danger" id="confirm-form-delete" on:click=delete.clone()>
                            "Delete"
                        </button>
                        <button type="button" on:click=keep>"Keep"</button>
                    </div>
                }
            }
        >
            <button
                type="button"
                id="form-delete"
                class="danger hold-button"
                class:holding=holding
                style=format!("--hold: {}ms", HOLD_TO_DELETE.as_millis())
                aria-describedby="form-delete-help"
                on:pointerdown=on_pointerdown.clone()
                on:pointerup=move |_| release()
                on:pointerleave=move |_| release()
                on:pointercancel=move |_| release()
                on:keydown=on_keydown.clone()
                on:keyup=on_keyup
                on:blur=move |_| store.cancel_hold()
                on:click=on_click
            >
                <span class="hold-label">"Delete"</span>
                <span class="hold-fill" aria-hidden="true">
                    <span class="hold-label">"Delete"</span>
                </span>
            </button>
        </Show>
        <span id="form-delete-help" class="sr-only">{help}</span>
        </div>
    }
}
