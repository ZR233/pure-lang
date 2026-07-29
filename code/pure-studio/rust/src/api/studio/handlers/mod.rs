pub mod events;
pub mod lifecycle;
pub mod prompt;
pub mod providers;
pub mod recovery;
pub mod session;
pub mod settings;
mod snapshot;
pub mod updater;

pub use events::{BridgeSessionStreamFrame, subscribe_product_events, subscribe_session_events};
pub use lifecycle::{
    archive_project, bootstrap_studio, initialize_runtime, open_project, select_project,
    shutdown_runtime, start_runtime,
};
pub use prompt::{resolve_interaction, stop_prompt, submit_prompt};
pub use providers::{list_discovered_skills, load_provider_usages};
pub use recovery::{
    cleanup_project, cleanup_recovery_issue, preview_project_cleanup,
    preview_recovery_issue_cleanup,
};
pub use session::{archive_session, create_session, set_model_role, set_session_mode};
pub use settings::{
    load_provider_catalog, load_web_search_settings, save_general_settings,
    save_instructions_settings, save_mcp_settings, save_provider_settings,
    save_runtime_permission_mode, save_skills_settings, save_web_search_settings,
};
pub use updater::{check_studio_update, install_studio_update};
