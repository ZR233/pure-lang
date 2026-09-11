//! Stable declarations for the opaque Thread tool implementations.
use crate::{session_note::SessionNoteToolKind, task_control::TaskControlKind};
use pl_protocol::ToolSpec;

/// Tool-owned schema and descriptions, without provider encoding or mutable task state.
#[derive(Debug, Clone, Copy)]
pub enum ThreadBuiltin {
    ViewImage,
    CreateDirectory,
    DeletePath,
    CopyPath,
    MovePath,
    ListTasks,
    Sleep,
    WriteFile,
    StatPath,
    Exec,
    WriteStdin,
    File(crate::workspace_file::WorkspaceFileToolKind),
    Complete,
    AskUser,
    Todo,
    Discover,
    Task(TaskControlKind),
    Note(SessionNoteToolKind),
}

impl ThreadBuiltin {
    /// Returns the deterministic declaration for this implementation's actual input contract.
    pub fn declaration(self) -> ToolSpec {
        match self {
            Self::CreateDirectory => schema::<crate::file::PathInput>(
                "create_directory",
                "Create a directory inside the workspace.",
            ),
            Self::DeletePath => schema::<crate::file::DeletePathInput>(
                "delete_path",
                "Delete a file or directory using an explicit file, emptyDirectory, or recursiveDirectory mode.",
            ),
            Self::CopyPath => schema::<crate::file::CopyMoveInput>(
                "copy_path",
                "Copy a workspace file with explicit destination collision behavior.",
            ),
            Self::MovePath => schema::<crate::file::CopyMoveInput>(
                "move_path",
                "Move a workspace file or directory with explicit destination collision behavior.",
            ),
            Self::ListTasks => crate::task_control::ListTasksTool::declaration(),
            Self::Sleep => crate::session::SleepTool::declaration(),
            Self::ViewImage => schema::<crate::image::ViewImageInput>(
                crate::image::TOOL_VIEW_IMAGE,
                "Read a workspace image using this prepared model's image capability. Original bytes and the model-visible image are retained for history replay.",
            ),
            Self::StatPath => schema::<crate::file::PathInput>(
                "stat_path",
                "Return metadata for a workspace path, or exists: false when the path is absent.",
            ),
            Self::WriteFile => schema::<crate::file::WriteFileInput>(
                "write_file",
                "Write exact UTF-8 text to a workspace file using create, overwrite, or append mode.",
            ),
            Self::File(kind) => {
                ToolSpec::function(kind.name(), kind.description(), kind.input_schema())
            }
            Self::WriteStdin => schema::<crate::exec::WriteStdinInput>(
                "write_stdin",
                "Write nonempty input to an active exec task in this Thread. This does not start another command or wait for completion.",
            ),
            Self::Exec => schema::<crate::exec::ExecInput>(
                "exec",
                "Run a shell command in the configured workspace. Completion includes process exit and output drain. Long commands return a task receipt; use wait or get_tool_task to observe completion. Full output is archived as a resource.",
            ),
            Self::Complete => schema::<crate::complete::CompleteInput>(
                "complete",
                "Finish this Turn with a concise summary and supporting evidence.",
            ),
            Self::AskUser => schema::<crate::ask_user::AskUserInput>(
                "request_user_input",
                "Ask structured questions and suspend until the user answers or cancels.",
            ),
            Self::Todo => schema::<crate::todo::TodoListInput>(
                crate::todo::TOOL_UPDATE_TODO_LIST,
                "Replace the current task checklist; maintain at most one in-progress item.",
            ),
            Self::Discover => schema::<crate::discovery::SearchInput>(
                "discover_tools",
                "Find tool IDs by name and reveal matching deferred declarations for the next model step.",
            ),
            Self::Task(TaskControlKind::Wait) => schema::<crate::task_control::WaitTasksInput>(
                "wait",
                "Call wait ALONE in a model response. For a tool use its exact receipt taskId: {\"taskIds\":[\"task:call-id\"]}. For child notifications use {\"taskIds\":[],\"timeoutMs\":300000}, then match childId and turn terminal before read_agent_submissions({\"target\":\"child-id\"}). Never put agentId or callId in taskIds. Returns readiness; completed background results arrive with the next model step.",
            ),
            Self::Task(TaskControlKind::Query) => schema::<crate::task_control::QueryTaskInput>(
                "get_tool_task",
                "Read task status without consuming events. Use resultCursor to page through the immutable complete payload after completion.",
            ),
            Self::Task(TaskControlKind::Cancel) => schema::<crate::task_control::CancelTaskInput>(
                "cancel_tool_task",
                "Request cancellation of one task. The acknowledgement is not proof of process exit; use wait to observe its terminal result.",
            ),
            Self::Note(kind) => {
                ToolSpec::function(kind.name(), kind.description(), kind.input_schema())
            }
        }
    }
}

fn schema<T: schemars::JsonSchema>(name: &str, description: &str) -> ToolSpec {
    ToolSpec::function(name, description, schemars::schema_for!(T).to_value())
}
