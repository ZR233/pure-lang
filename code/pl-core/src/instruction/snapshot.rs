use std::collections::{BTreeMap, HashMap};

use pl_protocol::{Message, MessageContent, MessageRole, Result};

use super::{
    InstructionAssembler, InstructionAssemblyRequest, InstructionBlock, InstructionBundle,
    InstructionSnapshot, InstructionSource, InstructionSourceKind,
};

impl InstructionSnapshot {
    /// 构造由宿主产品层提供的完整 base system prompt 快照。
    ///
    /// 这用于 mai-team 等宿主已经完成 profile 拼装的场景，避免宿主直接依赖
    /// `InstructionBlock` 和 `InstructionSource` 的内部结构。
    pub fn profile_base_override(label: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            base: InstructionBlock {
                source: InstructionSource::new(InstructionSourceKind::ProfileBaseOverride, label),
                content: content.into(),
            },
            developer: Vec::new(),
            user: Vec::new(),
        }
    }

    pub fn with_turn_overlay(
        &self,
        request: InstructionAssemblyRequest<'_>,
    ) -> Result<InstructionSnapshot> {
        let overlay = InstructionAssembler::assemble(request)?;
        let mut snapshot = self.without_turn_overlay();
        snapshot.developer.splice(
            0..0,
            overlay
                .developer
                .into_iter()
                .filter(|block| block.source.kind.is_turn_overlay()),
        );
        Ok(snapshot)
    }

    pub fn to_bundle(&self) -> InstructionBundle {
        let mut prelude_messages = Vec::new();
        let mut prefix_section_hashes = BTreeMap::from([(
            "base".to_string(),
            crate::canonical_content_hash(self.base.content.as_bytes()),
        )]);
        push_instruction_group(
            &mut prelude_messages,
            &mut prefix_section_hashes,
            "globalDeveloper",
            MessageRole::System,
            "# Global Developer Instructions",
            select_blocks(
                &self.developer,
                &[
                    InstructionSourceKind::Platform,
                    InstructionSourceKind::ConfigDeveloper,
                    InstructionSourceKind::ProfileDeveloper,
                ],
            ),
        );
        push_instruction_group(
            &mut prelude_messages,
            &mut prefix_section_hashes,
            "globalUser",
            MessageRole::User,
            "# Global User Context",
            select_blocks(
                &self.user,
                &[
                    InstructionSourceKind::ConfigUser,
                    InstructionSourceKind::ProfileUser,
                ],
            ),
        );
        push_instruction_group(
            &mut prelude_messages,
            &mut prefix_section_hashes,
            "modeRole",
            MessageRole::System,
            "# Mode and Role Instructions",
            select_blocks(
                &self.developer,
                &[
                    InstructionSourceKind::ExecutionProfile,
                    InstructionSourceKind::SubagentConstraint,
                    InstructionSourceKind::SubagentForce,
                ],
            ),
        );
        push_instruction_group(
            &mut prelude_messages,
            &mut prefix_section_hashes,
            "skills",
            MessageRole::System,
            "# Skill Instructions",
            select_blocks(&self.developer, &[InstructionSourceKind::Skills]),
        );
        push_instruction_group(
            &mut prelude_messages,
            &mut prefix_section_hashes,
            "workspace",
            MessageRole::User,
            "# Workspace Context",
            select_blocks(
                &self.user,
                &[
                    InstructionSourceKind::ProjectDoc,
                    InstructionSourceKind::WorkspaceFallback,
                ],
            ),
        );
        push_instruction_group(
            &mut prelude_messages,
            &mut prefix_section_hashes,
            "turnSkills",
            MessageRole::User,
            "# Turn Skill Instructions",
            select_blocks(
                &self.user,
                &[
                    InstructionSourceKind::SkillSuggestions,
                    InstructionSourceKind::SkillInvocation,
                ],
            ),
        );
        InstructionBundle {
            instructions: self.base.content.clone(),
            prelude_messages,
            prefix_section_hashes,
        }
    }

    pub fn with_subagent_force(mut self, content: &str) -> Self {
        self.push_developer(
            InstructionSource::new(
                InstructionSourceKind::SubagentForce,
                "subagent force dispatch",
            ),
            content,
        );
        self
    }

    pub fn with_subagent_constraint(mut self, content: &str) -> Self {
        self.push_developer(
            InstructionSource::new(
                InstructionSourceKind::SubagentConstraint,
                "subagent dispatch constraint",
            ),
            content,
        );
        self
    }

    fn without_turn_overlay(&self) -> Self {
        Self {
            base: self.base.clone(),
            developer: self
                .developer
                .iter()
                .filter(|block| !block.source.kind.is_turn_overlay())
                .cloned()
                .collect(),
            user: self
                .user
                .iter()
                .filter(|block| !block.source.kind.is_turn_overlay())
                .cloned()
                .collect(),
        }
    }
}

fn format_blocks(title: &str, blocks: &[InstructionBlock]) -> String {
    let mut output = String::from(title);
    for block in blocks {
        output.push_str("\n\n## ");
        output.push_str(&block.source.label);
        if let Some(path) = &block.source.path {
            output.push_str("\nSource: ");
            output.push_str(path);
        }
        output.push_str("\n\n");
        output.push_str(&block.content);
    }
    output
}

fn select_blocks(
    blocks: &[InstructionBlock],
    kinds: &[InstructionSourceKind],
) -> Vec<InstructionBlock> {
    blocks
        .iter()
        .filter(|block| kinds.contains(&block.source.kind))
        .cloned()
        .collect()
}

fn push_instruction_group(
    messages: &mut Vec<Message>,
    hashes: &mut BTreeMap<String, String>,
    id: &str,
    role: MessageRole,
    title: &str,
    blocks: Vec<InstructionBlock>,
) {
    if blocks.is_empty() {
        return;
    }
    let content = format_blocks(title, &blocks);
    hashes.insert(
        id.to_string(),
        crate::canonical_content_hash(content.as_bytes()),
    );
    messages.push(text_message(role, content));
}

fn text_message(role: MessageRole, content: String) -> Message {
    Message {
        role,
        content: MessageContent::text(content),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    }
}
