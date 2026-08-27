use std::collections::HashMap;

use pl_protocol::{
    Message, MessageContent, MessageRole, ModelContextItem, ModelContextSnapshot, PureError,
};

/// provider 无关的 inference 上下文组装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledModelContext {
    pub instructions: String,
    pub prelude_messages: Vec<Message>,
    pub history: Vec<ModelContextItem>,
    pub working_context_tail: Option<Message>,
}

/// 冻结单个 Turn 的模型可见 working context 及其 transcript 锚点。
///
/// Turn 内新增的 assistant/tool history 必须出现在该锚点之后，确保模型输入
/// 保持 append-only。上下文压缩替换 transcript 后，调用方需要显式 rebase。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnContextSnapshot {
    working_context: ModelContextSnapshot,
    history_anchor: usize,
}

impl TurnContextSnapshot {
    /// 在当前 transcript 尾部建立 Turn 级 working-context 锚点。
    pub fn capture(items: &[ModelContextItem], working_context: ModelContextSnapshot) -> Self {
        Self {
            working_context,
            history_anchor: items.len(),
        }
    }

    /// 返回该 Turn 冻结的模型可见 working context。
    pub fn model_context(&self) -> &ModelContextSnapshot {
        &self.working_context
    }

    /// transcript 被压缩或替换后，把锚点移动到新的 canonical 尾部。
    pub fn rebase(&mut self, items: &[ModelContextItem]) {
        self.history_anchor = items.len();
    }
}

/// 每次 inference 从 canonical session 重建固定前缀、append-only transcript
/// 与唯一的 working-context tail。
#[derive(Debug, Default)]
pub struct ContextAssembler;

impl ContextAssembler {
    pub fn assemble(
        base_instructions: &str,
        prelude_messages: &[Message],
        items: &[ModelContextItem],
        working_context: &ModelContextSnapshot,
    ) -> Result<AssembledModelContext, PureError> {
        let snapshot = TurnContextSnapshot::capture(items, working_context.clone());
        Self::assemble_turn(base_instructions, prelude_messages, items, &snapshot)
    }

    /// 使用冻结的 Turn 快照组装 append-only 模型输入。
    ///
    /// # Errors
    ///
    /// transcript 已被替换而调用方尚未 rebase 时返回配置错误。
    pub fn assemble_turn(
        base_instructions: &str,
        prelude_messages: &[Message],
        items: &[ModelContextItem],
        snapshot: &TurnContextSnapshot,
    ) -> Result<AssembledModelContext, PureError> {
        if snapshot.history_anchor > items.len() {
            return Err(PureError::ConfigError(format!(
                "turn context anchor {} exceeds transcript length {}; rebase after replacement",
                snapshot.history_anchor,
                items.len()
            )));
        }
        let instructions = base_instructions.trim_end().to_string();
        let working_context_tail = render_working_context_message(snapshot.model_context());
        let mut history =
            Vec::with_capacity(items.len() + usize::from(working_context_tail.is_some()));
        history.extend_from_slice(&items[..snapshot.history_anchor]);
        if let Some(tail) = working_context_tail.clone() {
            history.push(ModelContextItem::from(tail));
        }
        history.extend_from_slice(&items[snapshot.history_anchor..]);
        Ok(AssembledModelContext {
            instructions,
            prelude_messages: prelude_messages.to_vec(),
            history,
            working_context_tail,
        })
    }
}

fn render_working_context_message(context: &ModelContextSnapshot) -> Option<Message> {
    if context.sections.is_empty() {
        return None;
    }
    let mut rendered = String::from(
        "# Current working context\nThis is the current model-visible runtime context.",
    );
    for section in &context.sections {
        rendered.push_str(&format!(
            "\n\n## {} [{}]\n{}",
            section.title, section.id, section.content
        ));
    }
    Some(Message {
        role: MessageRole::System,
        content: MessageContent::text(rendered),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: HashMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use pl_model::{ToolCall, ToolCallKind};
    use pl_protocol::{SessionNote, ToolResultReceipt, ToolResultRecord};
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::working_set::context_section;

    #[test]
    fn pinned_sections_do_not_change_the_fixed_instruction_prefix() {
        let pinned = context_section("mai.review_manifest", 1, "Review", "base=abc").unwrap();
        let user = crate::user_text_message("review");
        let mut session = crate::AgentSession::from_messages(vec![user.clone()]);
        session.upsert_pinned_context(pinned);
        let assembled = ContextAssembler::assemble(
            "system",
            &[],
            session.items(),
            &session.working_context_snapshot(),
        )
        .unwrap();

        assert_eq!(assembled.instructions, "system");
        assert_eq!(assembled.history.len(), 2);
        assert_eq!(assembled.history[0], ModelContextItem::from(user));
        assert!(
            assembled.history[1]
                .as_message()
                .is_some_and(|message| message.role == MessageRole::System)
        );
    }

    #[test]
    fn repeated_assembly_materializes_exactly_one_working_context_tail() {
        let mut session = crate::AgentSession::new();
        session.upsert_pinned_context(
            context_section(
                crate::CURRENT_TODO_SECTION_ID,
                7,
                "Current Todo",
                "review files; submit final review",
            )
            .unwrap(),
        );
        let snapshot = session.working_context_snapshot();
        let assembled =
            ContextAssembler::assemble("system", &[], session.items(), &snapshot).unwrap();
        let second = ContextAssembler::assemble("system", &[], session.items(), &snapshot).unwrap();

        assert_eq!(assembled.instructions, "system");
        assert_eq!(assembled.history.len(), 1);
        assert_eq!(assembled, second);
        assert!(session.items().is_empty());
    }

    #[test]
    fn frozen_turn_context_stays_in_prefix_when_history_appends() {
        let mut session = crate::AgentSession::from_messages(vec![crate::user_text_message("run")]);
        session.upsert_pinned_context(
            context_section(
                "studio.task_executor_handoff",
                1,
                "Task handoff",
                "scope=pl-model",
            )
            .unwrap(),
        );
        let snapshot =
            TurnContextSnapshot::capture(session.items(), session.working_context_snapshot());
        let first =
            ContextAssembler::assemble_turn("system", &[], session.items(), &snapshot).unwrap();

        session.push_assistant_response("checking".to_string(), None);
        let second =
            ContextAssembler::assemble_turn("system", &[], session.items(), &snapshot).unwrap();

        assert!(second.history.starts_with(&first.history));
        assert_eq!(working_context_tail_count(&second.history), 1);
        assert_eq!(second.history.len(), first.history.len() + 1);
    }

    #[test]
    fn turn_context_requires_rebase_after_transcript_replacement() {
        let mut session = crate::AgentSession::from_messages(vec![
            crate::user_text_message("one"),
            crate::assistant_text_message("two"),
        ]);
        session.upsert_pinned_context(
            context_section(
                "studio.task_executor_handoff",
                1,
                "Task handoff",
                "scope=core",
            )
            .unwrap(),
        );
        let mut snapshot =
            TurnContextSnapshot::capture(session.items(), session.working_context_snapshot());
        session.replace_messages(vec![crate::user_text_message("summary")]);

        let error =
            ContextAssembler::assemble_turn("system", &[], session.items(), &snapshot).unwrap_err();
        assert!(error.to_string().contains("rebase after replacement"));

        snapshot.rebase(session.items());
        let assembled =
            ContextAssembler::assemble_turn("system", &[], session.items(), &snapshot).unwrap();
        assert_eq!(working_context_tail_count(&assembled.history), 1);
    }

    #[test]
    fn session_note_body_and_dynamic_metadata_do_not_change_instructions() {
        let secret = "do-not-send-note-body";
        let note = SessionNote {
            revision: 9,
            content: format!("first\n{secret}"),
            content_hash: crate::canonical_content_hash(format!("first\n{secret}").as_bytes()),
            updated_at: 1,
        };
        let mut session = crate::AgentSession::new();
        session.replace_session_note(note);
        let assembled = ContextAssembler::assemble(
            "system",
            &[],
            session.items(),
            &session.working_context_snapshot(),
        )
        .unwrap();

        assert!(assembled.history.is_empty());
        assert_eq!(assembled.instructions, "system");
        assert!(!assembled.instructions.contains(secret));
    }

    #[test]
    fn evidence_ledger_stays_durable_without_entering_model_context() {
        let mut session = crate::AgentSession::new();
        let working_set = crate::TurnWorkingSetHandle::default();

        for index in 0..100 {
            let call_id = format!("call-{index}");
            session.push_assistant_tool_calls(
                None,
                vec![ToolCall::function(
                    call_id.clone(),
                    "read_file",
                    serde_json::json!({"path": format!("file-{index}.rs")}),
                    call_id.clone(),
                )],
                None,
            );
            session.push_tool_result(
                ToolResultRecord {
                    item_id: call_id.clone(),
                    call_id: call_id.clone(),
                    name: "read_file".to_string(),
                    kind: ToolCallKind::Function,
                },
                format!("result-{index}"),
                format!(r#"{{"path":"file-{index}.rs"}}"#),
            );
            working_set
                .apply(crate::TurnWorkingSetChange::AppendEvidence(
                    ToolResultReceipt {
                        call_id,
                        tool_name: "read_file".to_string(),
                        arguments_hash: format!("sha256:args-{index}"),
                        result_hash: format!("sha256:result-{index}"),
                        total_bytes: 10,
                        visible_bytes: 10,
                        truncated: false,
                        artifacts: Vec::new(),
                        continuation: None,
                        reused_from_call_id: None,
                    },
                ))
                .unwrap();
            working_set.sync_session(&mut session).unwrap();

            let assembled = ContextAssembler::assemble(
                "system",
                &[],
                session.items(),
                &session.working_context_snapshot(),
            )
            .unwrap();
            assert_eq!(working_context_tail_count(&assembled.history), 0);
        }

        assert_eq!(session.items().len(), 200);
        assert!(session.items().as_chunks::<2>().0.iter().all(|pair| {
            pair[0]
                .as_message()
                .is_some_and(|message| message.role == MessageRole::Assistant)
                && pair[1]
                    .as_message()
                    .is_some_and(|message| message.role == MessageRole::Tool)
        }));
        let ledger = session
            .pinned_context_sections()
            .find(|section| section.id.as_str() == crate::EVIDENCE_LEDGER_SECTION_ID)
            .expect("evidence ledger");
        let ledger_json: serde_json::Value = serde_json::from_str(&ledger.content).unwrap();
        assert!(ledger.content.len() <= 16 * 1024);
        assert!(
            ledger_json["recent"]
                .as_array()
                .is_some_and(|recent| recent.len() <= 64)
        );
    }

    fn working_context_tail_count(items: &[ModelContextItem]) -> usize {
        items
            .iter()
            .filter_map(ModelContextItem::as_message)
            .filter(|message| {
                message.role == MessageRole::System
                    && message
                        .content
                        .text_value()
                        .starts_with("# Current working context")
            })
            .count()
    }
}
