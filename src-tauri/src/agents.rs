//! AI agents: the MCP server's place in the app, `solidbar mcp` (the stdio bridge), and
//! copying the setup to the clipboard. The server itself is in `solidbar_core::mcp`.

use std::path::PathBuf;
use std::sync::Arc;

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;
use solidbar_core::engine::Engine;
use solidbar_core::mcp::{AgentServer, bridge};
use solidbar_shared::ipc::CommandError;
use tauri::{AppHandle, Manager};

/// The bridge runs without Tauri, so it names the app (as in tauri.conf.json) itself.
const BUNDLE_ID: &str = "co.kuix.solidbar";

/// Starts the server if Settings has it on.
pub fn setup(app: &AppHandle, engine: Arc<Engine>) {
    let bridge = std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let server = AgentServer::new(engine, bridge);
    app.manage(server.clone());
    tauri::async_runtime::spawn(async move { server.sync().await });
}

/// `solidbar mcp`: relays MCP between stdio and the running app until stdin closes, for
/// agents like Claude Desktop that start their servers themselves. Returns the exit code.
pub fn run_bridge() -> i32 {
    let Some(home) = std::env::var_os("HOME") else {
        eprintln!("solidbar mcp: HOME isn't set, so Solidbar's settings can't be found");
        return 1;
    };
    // Where Tauri keeps the app's data (`app_data_dir`).
    let data_dir = PathBuf::from(home)
        .join("Library/Application Support")
        .join(BUNDLE_ID);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a Tokio runtime");
    let relayed = runtime.block_on(bridge::run(
        &data_dir,
        tokio::io::stdin(),
        tokio::io::stdout(),
        launch_solidbar,
    ));
    match relayed {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("solidbar mcp: {error}");
            1
        }
    }
}

/// Starts Solidbar in the background, without taking the focus.
fn launch_solidbar() {
    if let Err(error) = std::process::Command::new("/usr/bin/open")
        .args(["-g", "-b", BUNDLE_ID])
        .spawn()
    {
        eprintln!("solidbar mcp: couldn't start Solidbar: {error}");
    }
}

pub fn copy(text: &str) -> Result<(), CommandError> {
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: `NSPasteboardTypeString` is an immutable AppKit constant.
    let string_type = unsafe { NSPasteboardTypeString };
    if pasteboard.setString_forType(&NSString::from_str(text), string_type) {
        Ok(())
    } else {
        Err(CommandError::new("Couldn't copy that"))
    }
}
