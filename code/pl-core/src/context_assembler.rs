use pl_protocol::{Message, ModelContextItem, PinnedContextSection, PureError};

use crate::working_set::{MAX_PINNED_CONTEXT_BYTES, MAX_PINNED_SECTION_BYTES};

/// provider 无关的 inference 上下文组装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledModelContext {
    pub instructions: String,
    pub prelude_messages: Vec<Message>,
    pub history: Vec<ModelContextItem>,
}

/// 每次 inference 从 canonical session 重建 instructions、pinned context 与历史。
#[derive(Debug, Default)]
pub struct ContextAssembler;

impl ContextAssembler {
    pub fn assemble(
        base_instructions: &str,
        prelude_messages: &[Message],
        items: &[ModelContextItem],
    ) -> Result<AssembledModelContext, PureError> {
        let mut sections = items
            .iter()
            .filter_map(ModelContextItem::as_pinned_context)
            .cloned()
            .collect::<Vec<_>>();
        sections.sort_by(|left, right| left.id.cmp(&right.id));
        validate_sections(&sections)?;

        let instructions = if sections.is_empty() {
            base_instructions.to_string()
        } else {
            format!(
                "{}\n\n{}",
                base_instructions.trim_end(),
                render_sections(&sections)
            )
        };
        let history = items
            .iter()
            .filter(|item| !item.is_pinned_context())
            .cloned()
            .collect();
        Ok(AssembledModelContext {
            instructions,
            prelude_messages: prelude_messages.to_vec(),
            history,
        })
    }
}

fn validate_sections(sections: &[PinnedContextSection]) -> Result<(), PureError> {
    let mut total = 0;
    for section in sections {
        if section.content.len() > MAX_PINNED_SECTION_BYTES {
            return Err(PureError::ConfigError(format!(
                "pinned context section `{}` exceeds {MAX_PINNED_SECTION_BYTES} bytes",
                section.id
            )));
        }
        total += section.content.len();
    }
    if total > MAX_PINNED_CONTEXT_BYTES {
        return Err(PureError::ConfigError(format!(
            "pinned context exceeds {MAX_PINNED_CONTEXT_BYTES} bytes"
        )));
    }
    Ok(())
}

fn render_sections(sections: &[PinnedContextSection]) -> String {
    let mut rendered = String::from(
        "# Canonical working context\nThe following sections are current runtime state. Keep using them after compaction and do not restart completed work.",
    );
    for section in sections {
        rendered.push_str(&format!(
            "\n\n## {} [{} rev={} hash={}]\n{}",
            section.title, section.id, section.revision, section.content_hash, section.content
        ));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::working_set::context_section;

    #[test]
    fn pinned_sections_are_injected_but_not_sent_as_history_items() {
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

        assert!(assembled.instructions.contains("base=abc"));
        assert_eq!(assembled.history, vec![ModelContextItem::from(user)]);
    }

    #[test]
    fn three_history_replacements_cannot_remove_canonical_progress() {
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
        for summary in [
            "summary without todo",
            "summary without constraints",
            "summary reset",
        ] {
            session.replace_compactable_items(vec![ModelContextItem::from(
                crate::assistant_text_message(summary),
            )]);
        }

        let assembled = ContextAssembler::assemble("system", &[], session.items()).unwrap();

        assert!(
            assembled
                .instructions
                .contains("review files; submit final review")
        );
        assert!(
            assembled
                .instructions
                .contains(crate::CURRENT_TODO_SECTION_ID)
        );
        assert_eq!(assembled.history.len(), 1);
    }
}
