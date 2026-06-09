use std::collections::HashMap;
use std::sync::Arc;

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

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap_studio,
            commands::open_project,
            commands::select_project,
            commands::create_session,
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
            commands::save_config,
            commands::save_provider_settings,
            commands::save_permission_mode,
            commands::save_mcp_settings,
            commands::list_discovered_skills,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Pure Studio");
}
