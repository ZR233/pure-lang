use pl_protocol::MessageContent;
use pl_trace::TraceAttachment;

use crate::instruction::InstructionSnapshot;

use super::TurnBudget;

/// 编译请求的执行模式。
///
/// `Simple` 由 executor 直接对话和执行；`Task` 由 planner 负责规划、
/// 协调实施、合并和审查闭环。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    #[default]
    Simple,
    Task,
}

impl CompileMode {
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Simple => include_str!("../../prompts/simple.md"),
            Self::Task => include_str!("../../prompts/task.md"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Simple => "simple",
            Self::Task => "task",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "task" => Self::Task,
            "simple" => Self::Simple,
            _ => Self::Simple,
        }
    }
}

/// 单轮核心编译请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRequest {
    pub turn_id: Option<String>,
    pub prompt: String,
    pub user_content: MessageContent,
    pub mode: CompileMode,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<InstructionSnapshot>,
    pub budget: TurnBudget,
    pub materialized_attachments: Vec<crate::MaterializedAttachment>,
    pub trace_attachments: Vec<TraceAttachment>,
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>, mode: CompileMode) -> Self {
        let prompt = prompt.into();
        Self {
            turn_id: None,
            user_content: MessageContent::Text(prompt.clone()),
            prompt,
            mode,
            workspace_instructions: None,
            instruction_snapshot: None,
            budget: TurnBudget::default(),
            materialized_attachments: Vec::new(),
            trace_attachments: Vec::new(),
        }
    }

    pub fn with_user_content(mut self, content: MessageContent) -> Self {
        self.user_content = content;
        self
    }

    pub fn with_turn_id(mut self, turn_id: impl Into<String>) -> Self {
        self.turn_id = Some(turn_id.into());
        self
    }

    pub fn with_materialized_attachments(
        mut self,
        attachments: Vec<crate::MaterializedAttachment>,
    ) -> Self {
        self.materialized_attachments = attachments;
        self
    }

    pub fn with_trace_attachments(mut self, attachments: Vec<TraceAttachment>) -> Self {
        self.trace_attachments = attachments;
        self
    }

    pub fn with_workspace_instructions(mut self, instructions: String) -> Self {
        self.workspace_instructions = Some(instructions);
        self
    }

    pub fn with_instruction_snapshot(mut self, snapshot: InstructionSnapshot) -> Self {
        self.instruction_snapshot = Some(snapshot);
        self
    }

    pub fn with_budget(mut self, budget: TurnBudget) -> Self {
        self.budget = budget;
        self
    }
}
