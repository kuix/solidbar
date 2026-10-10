mod app;
mod entry_form;
mod format;
mod icons;
mod ipc;
mod keymap;
mod login;
mod org_chooser;
mod settings;
mod shell;
mod store;
mod time_field;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
