#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use pl_core::StudioRuntime;
use tokio::sync::Mutex;

mod approvals;
mod commands;
mod dto;
mod events;
mod mappers;
mod state;
mod user_input;

use state::AppState;

fn main() {
    let studio = tauri::async_runtime::block_on(StudioRuntime::default_app())
        .expect("failed to initialize Pure Studio runtime");
    let state = AppState {
        studio,
        approvals: Arc::new(Mutex::new(HashMap::new())),
        user_inputs: Arc::new(Mutex::new(HashMap::new())),
        active_turns: Arc::new(Mutex::new(HashMap::new())),
    };
    let setup_state = state.clone();
    let shutdown_state = state.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(move |app| {
            events::start_mcp_health_tasks(app.handle().clone(), setup_state.clone());
            events::start_lsp_health_tasks(app.handle().clone(), setup_state.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap_studio,
            commands::open_project,
            commands::select_project,
            commands::archive_project,
            commands::create_session,
            commands::delete_session,
            commands::select_session,
            commands::set_session_mode,
            commands::run_prompt,
            commands::implement_plan,
            commands::dismiss_plan,
            commands::stop_prompt,
            commands::load_session_timeline,
            commands::approve_tool,
            commands::deny_tool,
            commands::answer_user_input,
            commands::load_config,
            commands::load_provider_usages,
            commands::save_config,
            commands::save_provider_settings,
            commands::save_instructions_settings,
            commands::save_permission_mode,
            commands::save_mcp_settings,
            commands::list_discovered_skills,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Pure Studio");
    app.run(move |_app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            let studio = shutdown_state.studio.clone();
            tauri::async_runtime::block_on(async move {
                let _ = tokio::time::timeout(Duration::from_secs(3), studio.shutdown()).await;
            });
        }
    });
}
