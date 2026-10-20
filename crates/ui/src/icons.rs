//! Small inline SVG icons. They are decorative: the buttons carry the accessible names.

use leptos::prelude::*;

fn icon(path: &'static str) -> impl IntoView {
    view! {
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
            <path d=path />
        </svg>
    }
}

pub fn chevron_left() -> impl IntoView {
    icon("M10 3 5 8l5 5")
}

pub fn chevron_right() -> impl IntoView {
    icon("m6 3 5 5-5 5")
}

/// The ⌃⌄ of a macOS pop-up button.
pub fn chevron_up_down() -> impl IntoView {
    icon("m5 6 3-3 3 3M5 10l3 3 3-3")
}

pub fn check() -> impl IntoView {
    icon("m3.5 8.5 3 3 6-7")
}

pub fn gear() -> impl IntoView {
    icon(
        "M13.27 6.84 L15.13 7.01 L15.13 8.99 L13.27 9.16 L12.55 10.91 L13.74 12.35 L12.35 13.74 L10.91 12.55 L9.16 13.27 L8.99 15.13 L7.01 15.13 L6.84 13.27 L5.09 12.55 L3.65 13.74 L2.26 12.35 L3.45 10.91 L2.73 9.16 L0.87 8.99 L0.87 7.01 L2.73 6.84 L3.45 5.09 L2.26 3.65 L3.65 2.26 L5.09 3.45 L6.84 2.73 L7.01 0.87 L8.99 0.87 L9.16 2.73 L10.91 3.45 L12.35 2.26 L13.74 3.65 L12.55 5.09Z M10.2 8a2.2 2.2 0 1 1-4.4 0 2.2 2.2 0 0 1 4.4 0Z",
    )
}

pub fn pin() -> impl IntoView {
    icon("M6 2h4l-.5 4 2.5 2.5H4L6.5 6 6 2ZM8 8.5V14")
}

pub fn play() -> impl IntoView {
    view! {
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
            <path class="fill" d="M5 3.5v9l7-4.5-7-4.5Z" />
        </svg>
    }
}

pub fn stop() -> impl IntoView {
    view! {
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
            <rect class="fill" x="4" y="4" width="8" height="8" rx="1.5" />
        </svg>
    }
}

pub fn plus() -> impl IntoView {
    icon("M8 3v10M3 8h10")
}

pub fn minus() -> impl IntoView {
    icon("M3 8h10")
}

/// The × of a window's close button.
pub fn cross() -> impl IntoView {
    icon("m4 4 8 8m0-8-8 8")
}

pub fn chevron_down() -> impl IntoView {
    icon("m3 6 5 5 5-5")
}

/// A cup, for breaks.
pub fn cup() -> impl IntoView {
    icon(
        "M3 6h8v4a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3V6Zm8 1h1a1.5 1.5 0 0 1 0 3h-1M5.5 2.5v1.5M8.5 2.5v1.5",
    )
}
