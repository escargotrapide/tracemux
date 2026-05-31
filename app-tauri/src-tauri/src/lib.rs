//! TraceMux Tauri 2 desktop shell.
//!
//! Responsibilities:
//! - Spawn the `tracemux serve` sidecar (bundled as `binaries/tracemux`).
//! - Load the SolidJS UI (dev: vite at 127.0.0.1:5173, prod: bundled).
//!
//! Per AGENTS.md, the server is the single source of truth -- this
//! shell never persists log data itself.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::Mutex;

use tauri::{Manager, WindowEvent};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

struct SidecarState(Mutex<Option<CommandChild>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tracemux=debug".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            if sidecar_enabled() {
                let bind = std::env::var("TRACEMUX_TAURI_BIND")
                    .unwrap_or_else(|_| "127.0.0.1:9000".to_string());
                let (mut events, child) = app
                    .shell()
                    .sidecar("binaries/tracemux")?
                    .args(["serve", "--bind", bind.as_str(), "--no-auth"])
                    .spawn()?;

                tracing::info!(%bind, "tauri: tracemux sidecar started");
                app.manage(SidecarState(Mutex::new(Some(child))));

                tauri::async_runtime::spawn(async move {
                    while let Some(event) = events.recv().await {
                        tracing::debug!(?event, "tauri: sidecar event");
                    }
                    tracing::info!("tauri: sidecar event stream closed");
                });
            } else {
                tracing::info!("tauri: sidecar disabled by TRACEMUX_TAURI_SIDECAR=0");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                stop_sidecar(window);
            }
        })
        .run(tauri::generate_context!())
        .expect("E-UI-0100: tauri runtime failed to start");
}

fn sidecar_enabled() -> bool {
    !matches!(
        std::env::var("TRACEMUX_TAURI_SIDECAR").as_deref(),
        Ok("0" | "false" | "False")
    )
}

fn stop_sidecar<R: tauri::Runtime, M: Manager<R>>(manager: &M) {
    let Some(state) = manager.try_state::<SidecarState>() else {
        return;
    };
    let Ok(mut guard) = state.0.lock() else {
        tracing::warn!("tauri: sidecar state lock poisoned");
        return;
    };
    if let Some(child) = guard.take() {
        if let Err(err) = child.kill() {
            tracing::warn!(error = %err, "tauri: failed to stop sidecar");
        } else {
            tracing::info!("tauri: sidecar stopped");
        }
    }
}
