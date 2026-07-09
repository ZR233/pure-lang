use pl_protocol::MessageContent;
use pl_trace::TraceAttachment;

use crate::instruction::InstructionSnapshot;

use super::TurnBudget;

/// 编译请求的执行模式。
///
/// `Plan` 产出规划与解释，也可以在已注册工具边界内做只读探索；
/// `Auto` 允许模型生成更主动的编译步骤和子任务。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    Plan,
    #[default]
    Auto,
}

impl CompileMode {
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Plan => include_str!("../../prompts/plan.md"),
            Self::Auto => include_str!("../../prompts/auto.md"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Auto => "auto",
        }
    }

    pub fn from_label(label: &str) -> Self {
        match label {
            "plan" => Self::Plan,
            "auto" => Self::Auto,
            _ => Self::Auto,
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
