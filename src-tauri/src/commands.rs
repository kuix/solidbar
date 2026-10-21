//! The commands the UI invokes; see `solidbar_shared::ipc` for the contract. Each takes its
//! arguments as one value named `args`.

use std::sync::Arc;

use solidbar_core::engine::Engine;
use solidbar_core::mcp::AgentServer;
use solidbar_shared::instance::InstanceSettings;
use solidbar_shared::ipc::{
    AdjustArgs, AgentAccessArgs, AutostartArgs, BootstrapView, CommandError, DateArgs, EntriesArgs,
    EntryDraft, FormArgs, MembershipArgs, PinArgs, RefreshArgs, ShortcutArgs, TextArgs,
    TimerSource,
};
use solidbar_shared::settings::SettingsPatch;
use solidbar_shared::update::UpdateView;
use solidbar_shared::view::FormInit;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::autostart::Autostart;
use crate::{agents, panel, shortcuts, updates, window};

type EngineState<'a> = State<'a, Arc<Engine>>;
type Reply<T = ()> = Result<T, CommandError>;

#[tauri::command]
pub fn bootstrap(engine: EngineState<'_>) -> BootstrapView {
    engine.bootstrap()
}

#[tauri::command]
pub async fn auth_login(engine: EngineState<'_>, args: InstanceSettings) -> Reply {
    engine.login(args).await
}

#[tauri::command]
pub fn auth_cancel(engine: EngineState<'_>) {
    engine.cancel_login();
}

#[tauri::command]
pub async fn auth_logout(engine: EngineState<'_>) -> Reply {
    engine.logout().await
}

#[tauri::command]
pub async fn select_org(engine: EngineState<'_>, args: MembershipArgs) -> Reply {
    engine.select_org(&args.membership_id).await
}

#[tauri::command]
pub async fn select_date(engine: EngineState<'_>, args: DateArgs) -> Reply {
    engine.select_date(&args.date).await
}

#[tauri::command]
pub async fn refresh(engine: EngineState<'_>, args: RefreshArgs) -> Reply {
    engine.refresh(args.force).await
}

#[tauri::command]
pub fn prepare_form(engine: EngineState<'_>, args: FormArgs) -> Reply<FormInit> {
    let init = engine.prepare_form(&args)?;
    let engine = Arc::clone(&engine);
    tauri::async_runtime::spawn(async move { engine.form_opened().await });
    Ok(init)
}

#[tauri::command]
pub async fn save_entry(engine: EngineState<'_>, args: EntryDraft) -> Reply {
    engine.save_entry(args).await
}

#[tauri::command]
pub async fn delete_entries(engine: EngineState<'_>, args: EntriesArgs) -> Reply {
    engine.delete_entries(&args.ids).await
}

#[tauri::command]
pub async fn adjust_entry(engine: EngineState<'_>, args: AdjustArgs) -> Reply {
    engine.adjust_entry(&args).await
}

#[tauri::command]
pub async fn start_timer(engine: EngineState<'_>, args: TimerSource) -> Reply {
    engine.start_timer(&args).await
}

#[tauri::command]
pub async fn stop_timer(engine: EngineState<'_>) -> Reply {
    engine.stop_timer().await
}

#[tauri::command]
pub async fn start_break(engine: EngineState<'_>) -> Reply {
    engine.start_break().await
}

#[tauri::command]
pub fn update_settings(engine: EngineState<'_>, args: SettingsPatch) -> Reply {
    engine.update_settings(&args)
}

#[tauri::command]
pub fn set_shortcut(app: AppHandle, engine: EngineState<'_>, args: ShortcutArgs) -> Reply {
    shortcuts::change(&app, args.action, args.accelerator.as_deref()).map_err(CommandError::new)?;
    engine.save_shortcut(&args);
    Ok(())
}

/// While the shortcut recorder listens, the current shortcuts must not fire.
#[tauri::command]
pub fn shortcuts_suspend(app: AppHandle) {
    shortcuts::suspend(&app);
}

#[tauri::command]
pub fn shortcuts_resume(app: AppHandle) {
    shortcuts::resume(&app);
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, engine: EngineState<'_>, args: AutostartArgs) -> Reply {
    let autostart = app
        .try_state::<Autostart>()
        .ok_or_else(|| CommandError::new("Launch at Login isn't available"))?;
    autostart
        .set_enabled(args.enabled)
        .map_err(|error| CommandError::new(format!("Couldn't change Launch at Login: {error}")))?;
    engine.update_settings(&SettingsPatch {
        launch_at_login_offered: Some(true),
        ..SettingsPatch::default()
    })
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> bool {
    app.try_state::<Autostart>()
        .is_some_and(|autostart| autostart.is_enabled())
}

#[tauri::command]
pub fn panel_hide(app: AppHandle) {
    panel::hide(&app);
}

#[tauri::command]
pub fn window_dock(app: AppHandle) {
    window::dock(&app);
}

#[tauri::command]
pub fn panel_set_pinned(app: AppHandle, args: PinArgs) {
    panel::set_pinned(&app, args.pinned);
}

#[tauri::command]
pub fn open_in_browser(engine: EngineState<'_>) -> Reply {
    engine.open_in_browser()
}

/// THIRD_PARTY_LICENSES.html, which `cargo about` writes and the bundle carries as a resource.
#[tauri::command]
pub fn open_licenses(app: AppHandle) -> Reply {
    let cant_open = |error: String| {
        log::warn!("couldn't open the licenses: {error}");
        CommandError::new("Solidbar can't open its licenses.")
    };
    let path = app
        .path()
        .resolve("THIRD_PARTY_LICENSES.html", BaseDirectory::Resource)
        .map_err(|error| cant_open(error.to_string()))?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| cant_open(error.to_string()))
}

#[tauri::command]
pub async fn set_agent_access(
    engine: EngineState<'_>,
    agents: State<'_, Arc<AgentServer>>,
    args: AgentAccessArgs,
) -> Reply {
    engine.set_agent_access(args.enabled)?;
    agents.sync().await;
    Ok(())
}

#[tauri::command]
pub fn reset_agent_key(engine: EngineState<'_>) {
    engine.reset_agent_key();
}

#[tauri::command]
pub async fn move_agent_server(
    engine: EngineState<'_>,
    agents: State<'_, Arc<AgentServer>>,
) -> Reply {
    engine.move_agent_server()?;
    agents.sync().await;
    Ok(())
}

#[tauri::command]
pub fn copy_text(args: TextArgs) -> Reply {
    agents::copy(&args.text)
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn update_get(app: AppHandle) -> UpdateView {
    updates::view(&app)
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateView {
    updates::check(&app, true).await
}

#[tauri::command]
pub async fn update_install(app: AppHandle) -> Reply {
    updates::install(&app).await
}
