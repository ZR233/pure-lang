mod backend;
mod head_tail_buffer;
pub(super) mod process_manager;
mod shell;

pub use backend::{
    CommandBackend, CommandOutputSizes, CommandOutputTarget, CommandSpawnRequest,
    LocalCommandBackend, command_output_model_path,
};
