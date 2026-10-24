//! Keyboard handling. Shortcuts match `KeyboardEvent.code` (the physical key), so they work
//! the same on every keyboard layout, e.g. ⌥1 on a Hungarian layout.
//!
//! The rule: in the list, where nothing is typed, a single key is enough (N, T, S, B, E, R, and
//! ⌘N/⌘R too, out of macOS habit); where you type, shortcuts need a modifier; the shortcuts that
//! work from any app need ⌥⌘. Single keys can be turned off in Settings.

use leptos::prelude::*;
use solidbar_shared::ipc::{PanelHide, Refresh, RefreshArgs};
use wasm_bindgen::JsCast;
use web_sys::KeyboardEvent;

use crate::store::{Focus, Screen, Store, Toward};

/// The modifiers and physical key of a key press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keys {
    pub meta: bool,
    pub alt: bool,
    pub control: bool,
    pub shift: bool,
    pub code: String,
}

impl Keys {
    pub fn of(event: &KeyboardEvent) -> Self {
        Self {
            meta: event.meta_key(),
            alt: event.alt_key(),
            control: event.ctrl_key(),
            shift: event.shift_key(),
            code: event.code(),
        }
    }

    /// Exactly the combination `spec`, e.g. `Meta+KeyN` or `ArrowLeft`.
    pub fn is(&self, spec: &str) -> bool {
        let mut parts: Vec<&str> = spec.split('+').collect();
        let code = parts.pop().unwrap_or_default();
        let has = |name: &[&str]| parts.iter().any(|part| name.contains(part));
        self.code == code
            && self.meta == has(&["Meta", "Super"])
            && self.alt == has(&["Alt"])
            && self.control == has(&["Control"])
            && self.shift == has(&["Shift"])
    }

    /// The digit of `Digit1`…`Digit9`.
    pub fn digit(&self) -> Option<u32> {
        self.code.strip_prefix("Digit")?.parse().ok()
    }

    /// The combination as a global shortcut accelerator (`Alt+Super+KeyT`), if usable: it
    /// needs ⌘, ⌥ or ⌃ and a non-modifier key.
    pub fn accelerator(&self) -> Option<String> {
        let modifier_key = ["Meta", "Alt", "Control", "Shift", "OS", "Fn", "CapsLock"]
            .iter()
            .any(|name| self.code.starts_with(name));
        if modifier_key || !(self.meta || self.alt || self.control) {
            return None;
        }
        let mut parts = Vec::new();
        for (held, name) in [
            (self.control, "Control"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.meta, "Super"),
        ] {
            if held {
                parts.push(name);
            }
        }
        parts.push(&self.code);
        Some(parts.join("+"))
    }
}

/// `Alt+Super+KeyT` → `⌥⌘T`, in the macOS modifier order.
pub fn display(accelerator: &str) -> String {
    let mut parts: Vec<&str> = accelerator.split('+').collect();
    let code = parts.pop().unwrap_or_default();
    let mut text = String::new();
    for (names, symbol) in [
        (&["Control"][..], "⌃"),
        (&["Alt"][..], "⌥"),
        (&["Shift"][..], "⇧"),
        (&["Super", "Meta"][..], "⌘"),
    ] {
        if parts.iter().any(|part| names.contains(part)) {
            text.push_str(symbol);
        }
    }
    let key = code
        .strip_prefix("Key")
        .or_else(|| code.strip_prefix("Digit"))
        .map(str::to_owned)
        .unwrap_or_else(|| {
            match code {
                "Comma" => ",",
                "Period" => ".",
                "Equal" => "=",
                "Minus" => "−",
                "Space" => "Space",
                "Enter" => "↩",
                "Escape" => "⎋",
                "Backspace" => "⌫",
                "ArrowLeft" => "←",
                "ArrowRight" => "→",
                "ArrowUp" => "↑",
                "ArrowDown" => "↓",
                other => other,
            }
            .to_owned()
        });
    text.push_str(&key);
    text
}

/// Whether keys typed now go into a text field (or a select), so single-key shortcuts must not
/// fire.
pub fn typing() -> bool {
    let Some(element) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
    else {
        return false;
    };
    match element.tag_name().as_str() {
        "TEXTAREA" | "SELECT" => true,
        "INPUT" => element
            .dyn_ref::<web_sys::HtmlInputElement>()
            .is_some_and(|input| {
                !matches!(
                    input.type_().as_str(),
                    "checkbox" | "radio" | "button" | "submit"
                )
            }),
        _ => element
            .dyn_ref::<web_sys::HtmlElement>()
            .is_some_and(|element| element.is_content_editable()),
    }
}

pub fn focus_element(id: &str) -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(id))
        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok())
        .is_some_and(|element| element.focus().is_ok())
}

/// Focuses the entry form's field that had the focus before the panel went away, or else the
/// project and task.
pub fn refocus_form() {
    let active = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
        .filter(|element| matches!(element.closest(".entry-form"), Ok(Some(_))))
        .and_then(|element| element.dyn_into::<web_sys::HtmlElement>().ok());
    match active {
        Some(element) => {
            let _ = element.focus();
        }
        None => {
            focus_element("work-button");
        }
    }
}

/// The panel-wide shortcuts. Components handle their own keys first (e.g. the combobox's
/// arrows) and call `preventDefault` when they do.
pub fn install(store: Store) {
    // The listener lives as long as the page.
    let _ = window_event_listener(leptos::ev::keydown, move |event| {
        if event.default_prevented() || event.is_composing() {
            return;
        }
        let keys = Keys::of(&event);
        // Signed in, with an organization to work in.
        let ready = store
            .with_app(|app| {
                matches!(app.auth, solidbar_shared::view::AuthView::SignedIn { .. })
                    && !app.choose_org
            })
            .unwrap_or(false);
        if !ready {
            // Escape puts the panel away; a window stays, as windows do.
            if keys.is("Escape") && !store.windowed {
                store.run::<PanelHide>(());
                event.prevent_default();
            }
            return;
        }
        let handled = match store.screen.get_untracked() {
            Screen::List => list_keys(store, &keys),
            Screen::Form => {
                // The form handles its own keys; Escape that nothing else consumed cancels.
                if keys.is("Escape") {
                    store.close_form();
                    true
                } else {
                    false
                }
            }
            Screen::Settings => {
                if keys.is("Meta+KeyO") {
                    focus_element("settings-org")
                } else if keys.is("Escape") || keys.is("Meta+Comma") {
                    store.close_settings();
                    true
                } else {
                    false
                }
            }
        };
        if handled {
            event.prevent_default();
        }
    });
}

fn list_keys(store: Store, keys: &Keys) -> bool {
    let single_keys = store.single_keys();
    let single = |code: &str| single_keys && keys.is(code);
    if store.confirm_delete.get_untracked().is_some() {
        if keys.is("Enter") {
            store.confirm_delete();
            return true;
        }
        if keys.is("Escape") {
            store.cancel_delete();
            return true;
        }
        return false;
    }
    if keys.is("Meta+KeyN") {
        store.open_new_form();
    } else if keys.is("Meta+KeyR") {
        store.run::<Refresh>(RefreshArgs { force: true });
    } else if keys.is("Meta+KeyO") {
        // The organization is chosen in Settings.
        store
            .focus
            .set(Some(Focus::Element("settings-org".to_owned())));
        store.screen.set(Screen::Settings);
    } else if keys.is("Meta+Comma") {
        store.screen.set(Screen::Settings);
    } else if keys.is("Escape") && !store.windowed {
        store.run::<PanelHide>(());
    } else if typing() {
        return false;
    } else if single("KeyN") {
        store.open_new_form();
    } else if single("KeyR") {
        store.run::<Refresh>(RefreshArgs { force: true });
    } else if keys.is("ArrowLeft") || keys.is("ArrowRight") {
        let right = keys.is("ArrowRight");
        let in_week_strip = focus_in_week_strip();
        // → opens a group of continued entries and ← closes it; otherwise they change the day.
        if in_week_strip || !store.disclose(right) {
            let toward = if in_week_strip {
                Toward::Day
            } else {
                Toward::Entries
            };
            store.shift_days(if right { 1 } else { -1 }, toward);
        }
    } else if keys.is("Alt+ArrowLeft") || keys.is("Alt+ArrowRight") {
        store.shift_days(
            if keys.is("Alt+ArrowLeft") { -7 } else { 7 },
            Toward::Entries,
        );
    } else if single("KeyT") {
        store.go_to_today(Toward::Entries);
    } else if keys.is("ArrowUp") || keys.is("ArrowDown") {
        move_selection(store, if keys.is("ArrowUp") { -1 } else { 1 });
    } else if single("KeyS") {
        return store.online() && store.timer_key();
    } else if single("KeyB") {
        return store.online() && store.break_key();
    } else if let Some(row) = store.selected() {
        let editable = store.online() && !row.pending();
        if keys.is("Enter") || single("KeyE") {
            store.edit(&row);
        } else if (keys.is("Meta+Equal") || keys.is("Meta+Minus")) && editable {
            store.adjust(&row, if keys.is("Meta+Equal") { 60 } else { -60 });
        } else if (keys.is("Backspace") || keys.is("Delete")) && editable {
            store.ask_delete(row.dom_id(), row.ids());
        } else {
            return false;
        }
    } else {
        return false;
    }
    true
}

fn focus_in_week_strip() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
        .is_some_and(|element| element.get_attribute("role").as_deref() == Some("tab"))
}

/// ↑/↓ through the rows of the selected day.
fn move_selection(store: Store, step: isize) {
    let rows = store.rows();
    if rows.is_empty() {
        return;
    }
    let current = store
        .selected_row
        .get_untracked()
        .and_then(|id| rows.iter().position(|row| row.dom_id() == id));
    let next = match current {
        Some(index) => (index as isize + step).clamp(0, rows.len() as isize - 1) as usize,
        None if step > 0 => 0,
        None => rows.len() - 1,
    };
    let id = rows[next].dom_id();
    store.selected_row.set(Some(id.clone()));
    focus_element(&id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(meta: bool, alt: bool, code: &str) -> Keys {
        Keys {
            meta,
            alt,
            control: false,
            shift: false,
            code: code.to_owned(),
        }
    }

    #[test]
    fn matches_exact_combinations() {
        assert!(keys(true, false, "KeyN").is("Meta+KeyN"));
        assert!(!keys(true, true, "KeyN").is("Meta+KeyN"));
        assert!(keys(false, false, "ArrowLeft").is("ArrowLeft"));
        assert!(!keys(false, true, "ArrowLeft").is("ArrowLeft"));
    }

    #[test]
    fn builds_accelerators_only_with_a_command_modifier() {
        assert_eq!(
            keys(true, true, "KeyT").accelerator().as_deref(),
            Some("Alt+Super+KeyT")
        );
        assert_eq!(keys(false, false, "KeyT").accelerator(), None);
        assert_eq!(keys(true, false, "MetaLeft").accelerator(), None);
    }

    #[test]
    fn displays_mac_symbols() {
        assert_eq!(display("Alt+Super+KeyT"), "⌥⌘T");
        assert_eq!(display("Control+Shift+Digit1"), "⌃⇧1");
        assert_eq!(display("Super+Comma"), "⌘,");
    }
}
