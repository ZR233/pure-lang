use pl_protocol::MessageContent;
use pl_protocol::SkillActivation;
use pl_trace::TraceAttachment;

use crate::instruction::InstructionSnapshot;

use super::TurnBudget;

/// 单轮核心编译请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRequest {
    pub turn_id: Option<String>,
    pub prompt: String,
    pub user_content: MessageContent,
    pub workspace_instructions: Option<String>,
    pub instruction_snapshot: Option<InstructionSnapshot>,
    pub budget: TurnBudget,
    pub materialized_attachments: Vec<crate::MaterializedAttachment>,
    pub trace_attachments: Vec<TraceAttachment>,
    pub skill_activations: Vec<SkillActivation>,
    pub skill_invocation_instruction: Option<String>,
}

impl TurnRequest {
    pub fn new(prompt: impl Into<String>) -> Self {
        let prompt = prompt.into();
        Self {
            turn_id: None,
            user_content: MessageContent::Text(prompt.clone()),
            prompt,
            workspace_instructions: None,
            instruction_snapshot: None,
            budget: TurnBudget::default(),
            materialized_attachments: Vec::new(),
            trace_attachments: Vec::new(),
            skill_activations: Vec::new(),
            skill_invocation_instruction: None,
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

    pub fn with_skill_activations(mut self, activations: Vec<SkillActivation>) -> Self {
        self.skill_activations = activations;
        self
    }

    /// 设置仅对本 Turn 生效的 Skill 用户指令。
    ///
    /// 该指令进入本 Turn 的 instruction bundle，但不会写入 Thread transcript，
    /// 因而不会在后续 Turn 中累积。
    pub fn with_skill_invocation_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.skill_invocation_instruction = Some(instruction.into());
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
