//! Workspace tool construction, instruction discovery, and atomic file writes.

mod assignment;
mod atomic_file;
pub use assignment::{AgentWorkspace, WorkspaceBoundary, WorkspaceMutability};
mod path_policy;
pub mod path_safety;
pub use path_policy::{PathAccess, ToolPathPolicy};

mod instructions;

pub use atomic_file::{WriteMode, write_file_atomically, write_file_with_mode};

pub use instructions::{
    WorkspaceInstructionDocument, WorkspaceInstructions, load_workspace_instruction_documents,
    resolve_workspace_root,
};

mod capabilities;
pub use capabilities::ToolCapabilityConfig;

mod runtime;
pub use runtime::{ToolWorkspace, WorkspaceWriteGuard};
