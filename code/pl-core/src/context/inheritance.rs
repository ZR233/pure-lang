//! Context-only inheritance. No executor, mutable model session or application state is copied.
use super::{ContextError, ContextRecord, ContextSnapshot, ContextSource};
use std::{collections::BTreeSet, num::NonZeroUsize};

/// Host-selected conversation history window.
#[derive(Debug, Clone, Copy)]
pub enum HistoryInheritance {
    Empty,
    All,
    LastUserTurns(NonZeroUsize),
}

/// Instruction ownership is an explicit host decision, independent from conversation selection.
#[derive(Debug, Clone, Copy)]
pub enum InstructionInheritance {
    Exclude,
    Keep,
}

/// Context selection applied before assembling a new Thread with fresh physical resources.
#[derive(Debug, Clone, Copy)]
pub struct ContextInheritance {
    pub history: HistoryInheritance,
    pub instructions: InstructionInheritance,
}

impl ContextSnapshot {
    /// Selects immutable records without interpreting producer payloads or manufacturing missing results.
    /// A partially completed trailing tool batch is omitted in its entirety.
    ///
    /// # Errors
    /// Rejects malformed source relationships or an invalid resulting context.
    pub fn inherit(
        &self,
        selection: ContextInheritance,
    ) -> Result<Vec<ContextRecord>, ContextError> {
        self.pending_calls()?;
        let mut outstanding = BTreeSet::new();
        let mut complete_end = 0;
        for (index, record) in self.records.iter().enumerate() {
            outstanding.extend(record.tool_calls.iter().map(|call| call.call_id.as_str()));
            if let ContextSource::ToolResult { call_id, .. } = &record.source {
                outstanding.remove(call_id.as_str());
            }
            if outstanding.is_empty() {
                complete_end = index + 1;
            }
        }
        let complete = &self.records[..complete_end];
        let start = match selection.history {
            HistoryInheritance::Empty => complete.len(),
            HistoryInheritance::All => 0,
            HistoryInheritance::LastUserTurns(count) => complete
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, record)| record.source == ContextSource::User)
                .take(count.get())
                .last()
                .map_or(complete.len(), |(index, _)| index),
        };
        let records = complete
            .iter()
            .enumerate()
            .filter(|(index, record)| match record.source {
                ContextSource::Instruction => {
                    matches!(selection.instructions, InstructionInheritance::Keep)
                }
                ContextSource::User
                | ContextSource::Assistant
                | ContextSource::Runtime { .. }
                | ContextSource::ToolResult { .. } => *index >= start,
            })
            .map(|(_, record)| record.clone())
            .collect::<Vec<_>>();
        ContextSnapshot {
            revision: 0,
            records: records.clone().into(),
        }
        .validate_complete()?;
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        context::{ContextContent, OpaquePayload},
        model::ModelToolCall,
    };
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn text(id: &str, source: ContextSource) -> ContextRecord {
        ContextRecord {
            id: id.into(),
            turn_id: Some("parent-turn".into()),
            source,
            content: vec![ContextContent::Text {
                text: Arc::from(id),
            }],
            tool_calls: Vec::new(),
        }
    }

    #[test]
    fn inheritance_keeps_exact_closed_history_and_omits_the_whole_unfinished_batch() {
        let first = text("first-user", ContextSource::User);
        let second = text("second-user", ContextSource::User);
        let mut batch = text("pending-batch", ContextSource::Assistant);
        batch.tool_calls = ["one", "two"]
            .into_iter()
            .map(|id| ModelToolCall {
                call_id: id.into(),
                tool_id: "tool".into(),
                arguments: OpaquePayload::text("not JSON\r\n"),
            })
            .collect();
        let partial = text(
            "one-result",
            ContextSource::ToolResult {
                call_id: "one".into(),
                tool_id: "tool".into(),
            },
        );
        let instructions = text("parent-instructions", ContextSource::Instruction);
        let context = ContextSnapshot {
            revision: 7,
            records: vec![
                instructions.clone(),
                first.clone(),
                second.clone(),
                batch,
                partial,
            ]
            .into(),
        };
        assert_eq!(
            context
                .inherit(ContextInheritance {
                    history: HistoryInheritance::All,
                    instructions: InstructionInheritance::Exclude
                })
                .unwrap(),
            vec![first, second.clone()]
        );
        assert_eq!(
            context
                .inherit(ContextInheritance {
                    history: HistoryInheritance::LastUserTurns(NonZeroUsize::new(1).unwrap()),
                    instructions: InstructionInheritance::Keep
                })
                .unwrap(),
            vec![instructions, second]
        );
        assert_eq!(context.records.len(), 5);
    }
}
