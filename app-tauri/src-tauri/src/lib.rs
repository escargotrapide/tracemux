//! wanlogger Tauri 2 desktop shell.
//!
//! Responsibilities:
//! - Spawn the `wanlogger serve` sidecar (bundled as `binaries/wanlogger`).
//! - Load the SolidJS UI (dev: vite at 127.0.0.1:5173, prod: bundled).
//!
//! Per AGENTS.md, the server is the single source of truth ? this
//! shell never persists log data itself.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wanlogger=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|_app| {
            // TODO: spawn `wanlogger serve` sidecar via tauri_plugin_shell
            // once the bundled binary is wired up. The UI already falls
            // back to wss://127.0.0.1:7443/ws when run standalone.
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("E-UI-0100: tauri runtime failed to start");
}
