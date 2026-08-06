use pl_protocol::{Message, ModelContextItem, PureError};

/// provider 无关的 inference 上下文组装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledModelContext {
    pub instructions: String,
    pub prelude_messages: Vec<Message>,
    pub history: Vec<ModelContextItem>,
}

/// 每次 inference 从 canonical session 重建固定 instructions 与 append-only 历史。
#[derive(Debug, Default)]
pub struct ContextAssembler;

impl ContextAssembler {
    pub fn assemble(
        base_instructions: &str,
        prelude_messages: &[Message],
        items: &[ModelContextItem],
    ) -> Result<AssembledModelContext, PureError> {
        let instructions = base_instructions.trim_end().to_string();
        let history = items
            .iter()
            .filter(|item| !item.is_pinned_context() && !item.is_session_note())
            .cloned()
            .collect();
        Ok(AssembledModelContext {
            instructions,
            prelude_messages: prelude_messages.to_vec(),
            history,
        })
    }
}

#[cfg(test)]
mod tests {
    use pl_protocol::SessionNote;
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::working_set::context_section;

    #[test]
    fn pinned_sections_do_not_change_the_fixed_instruction_prefix() {
        let pinned = context_section("mai.review_manifest", 1, "Review", "base=abc").unwrap();
        let user = crate::user_text_message("review");
        let assembled = ContextAssembler::assemble(
            "system",
            &[],
            &[
                ModelContextItem::from(user.clone()),
                ModelContextItem::PinnedContext { section: pinned },
            ],
        )
        .unwrap();

        assert_eq!(assembled.instructions, "system");
        assert_eq!(assembled.history, vec![ModelContextItem::from(user)]);
    }

    #[test]
    fn context_patch_remains_in_append_only_history() {
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
        let provider = pl_model::ProviderInfo::deepseek(None);
        crate::prepare_prompt_context(
            &mut session,
            crate::PromptCacheInput {
                scope: "simple:root",
                turn_id: "turn-1",
                provider: &provider,
                model: "deepseek-v4-flash",
                instructions: "system",
                prelude_messages: &[],
                fixed_prefix_section_hashes: Default::default(),
                tools: &[],
                tool_choice: "auto",
                parallel_tool_calls: false,
                reasoning: None,
                output_schema: None,
                service_tier: None,
                compacted: false,
                prompt_cache_policy: pl_model::EffectivePromptCachePolicy::ImplicitPrefix,
                updated_at: 1,
            },
        )
        .unwrap();

        let assembled = ContextAssembler::assemble("system", &[], session.items()).unwrap();

        assert_eq!(assembled.instructions, "system");
        assert_eq!(assembled.history.len(), 1);
        assert!(assembled.history[0].as_context_patch().is_some());
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
        let assembled =
            ContextAssembler::assemble("system", &[], &[ModelContextItem::SessionNote { note }])
                .unwrap();

        assert!(assembled.history.is_empty());
        assert_eq!(assembled.instructions, "system");
        assert!(!assembled.instructions.contains(secret));
    }
}
