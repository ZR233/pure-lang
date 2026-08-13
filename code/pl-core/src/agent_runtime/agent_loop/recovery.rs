use std::collections::{BTreeSet, HashSet};

use pl_protocol::{
    ConversationRecoveryMode, ConversationRecoveryRecord, ConversationRecoveryTurnRange,
    InteractionStatus, MessageContent, MessageRole, ModelContextItem, PromptPrefixChangedReason,
    ThreadNotification, ToolCallHistoryMetadata, ToolResultMetadata,
};

use super::super::host::ThreadProjectionCommit;
use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentLifecycleState, AgentRuntimeHost, AgentRuntimeResult, ConversationRecoveryPreview,
    ConversationRecoveryRequest, ConversationRecoveryResult, ConversationRecoveryTarget,
    DurableCommitFacts, ThreadContextMutation, ThreadMutation,
};
use super::AgentLoop;
use super::commit::{CommitPublication, PendingCommit};
use crate::thread_event::{ThreadNotificationFact, project_thread_facts};
use crate::{
    CONVERSATION_RECOVERY_SECTION_ID, CURRENT_TODO_SECTION_ID, canonical_content_hash,
    canonical_json_hash, context_section,
};

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) fn preview_conversation_recovery(
        &self,
        target: ConversationRecoveryTarget,
    ) -> AgentRuntimeResult<ConversationRecoveryPreview> {
        self.validate_recovery_gate()?;
        preview_for_state(&self.state, target)
    }

    pub(super) async fn recover_conversation(
        &mut self,
        request: ConversationRecoveryRequest,
    ) -> AgentRuntimeResult<ConversationRecoveryResult> {
        if request.recovery_id.trim().is_empty() {
            return Err(AgentRuntimeError::InvalidInput(
                "conversation recovery id must not be empty".to_string(),
            ));
        }
        if let Some(record) = self
            .state
            .session
            .session
            .conversation_recovery()
            .last_recovery
            .as_ref()
            && record.recovery_id == request.recovery_id
        {
            return Ok(result_from_record(record));
        }

        self.validate_recovery_gate()?;
        let actual = preview_for_state(&self.state, request.preview.target.clone())?;
        if actual != request.preview {
            return Err(AgentRuntimeError::RevisionConflict {
                expected: Some(request.preview.expected_runtime_revision),
                actual: Some(self.state.snapshot.revision),
            });
        }

        let retained_items = self.state.session.session.items()
            [..usize_from_u64(actual.retained_item_count)?]
            .to_vec();
        let now = unix_timestamp();
        let mut next = self.state.clone();
        next.snapshot.revision = actual.expected_runtime_revision.saturating_add(1);
        next.snapshot.updated_at = now;
        next.snapshot.progress = None;
        next.session.session.replace_items(retained_items.clone());
        let _ = next
            .session
            .session
            .remove_pinned_context(CURRENT_TODO_SECTION_ID);
        next.session.session.mark_context_recovered(now);

        let current_thread = self
            .runtime
            .thread_events
            .snapshot(next.snapshot.identity.id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let projection = current_thread
            .runtime
            .clone()
            .map(
                |mut runtime| -> AgentRuntimeResult<ThreadProjectionCommit> {
                    runtime.todo = None;
                    runtime.progress = None;
                    runtime.updated_at = now;
                    runtime.usage.prefix_changed_reason =
                        Some(PromptPrefixChangedReason::ContextRecovered);
                    runtime.usage.prompt_generation = next
                        .session
                        .session
                        .prompt_metadata()
                        .slots
                        .values()
                        .map(|slot| slot.generation)
                        .max()
                        .or(runtime.usage.prompt_generation);
                    runtime.usage.updated_at = now;
                    let projected = project_thread_facts(
                        next.snapshot.identity.id.as_str(),
                        &current_thread,
                        vec![ThreadNotificationFact::durable(
                            now,
                            ThreadNotification::ThreadRuntimeUpdated {
                                runtime: Box::new(runtime),
                            },
                        )],
                    );
                    Ok(ThreadProjectionCommit {
                        snapshot: self
                            .runtime
                            .thread_events
                            .project(next.snapshot.identity.id.as_str(), &projected.notifications)
                            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
                        notifications: projected.notifications,
                    })
                },
            )
            .transpose()?;
        next.session.thread_revision = projection
            .as_ref()
            .map_or(current_thread.revision, |projection| {
                projection.snapshot.revision
            });

        let mut recovery = next.session.session.conversation_recovery().clone();
        recovery.revision = actual.facts.recovery_revision;
        if !actual.target.turn_ids.is_empty() {
            recovery
                .rolled_back_turn_ranges
                .push(ConversationRecoveryTurnRange {
                    turn_ids: actual.target.turn_ids.clone(),
                });
        }
        let record = ConversationRecoveryRecord {
            recovery_id: request.recovery_id.clone(),
            revision: actual.facts.recovery_revision,
            mode: actual.target.mode,
            target_turn_ids: actual.target.turn_ids.clone(),
            before_transcript_hash: actual.facts.before_transcript_hash.clone(),
            after_transcript_hash: actual.facts.after_transcript_hash.clone(),
            removed_input_count: actual.removed_input_count,
            removed_item_count: actual.removed_item_count,
            runtime_revision: next.snapshot.revision,
            thread_revision: next.session.thread_revision,
            recovered_at: now,
        };
        recovery.last_recovery = Some(record.clone());
        let _ = next.session.session.replace_conversation_recovery(recovery);
        next.session.session.upsert_pinned_context(
            context_section(
                CONVERSATION_RECOVERY_SECTION_ID,
                actual.facts.recovery_revision,
                "Conversation Recovery",
                recovery_context(&record),
            )
            .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?,
        );

        let committed_notifications = projection
            .as_ref()
            .map_or_else(Vec::new, |projection| projection.notifications.clone());
        let thread_id = next.snapshot.identity.id.clone();
        let durable_facts = DurableCommitFacts::from_state(
            &next,
            Vec::new(),
            Vec::new(),
            projection,
            Some(ThreadContextMutation::Replace {
                items: retained_items,
            }),
        );
        self.commit_and_publish(
            PendingCommit::new(
                next,
                durable_facts,
                ThreadMutation::ReplaceThread {
                    thread_id: thread_id.clone(),
                },
            )
            .publish(
                CommitPublication::new(Some(thread_id), None)
                    .store_directory_snapshot()
                    .with_thread_notifications(committed_notifications),
            ),
        )
        .await?;
        Ok(result_from_record(&record))
    }

    fn validate_recovery_gate(&self) -> AgentRuntimeResult<()> {
        let agent_id = self.state.snapshot.identity.id.clone();
        if self.state.snapshot.lifecycle != AgentLifecycleState::Active {
            return Err(AgentRuntimeError::NotActive(
                agent_id,
                self.state.snapshot.lifecycle,
            ));
        }
        if self.active.is_some()
            || self.state.snapshot.active_turn_id.is_some()
            || self.state.active_input.is_some()
            || !self.state.pending_inputs.is_empty()
            || self.state.snapshot.pending_inputs != 0
        {
            return Err(AgentRuntimeError::InvalidInput(
                "conversation recovery requires an idle Thread without active or pending input"
                    .to_string(),
            ));
        }
        let snapshot = self
            .runtime
            .thread_events
            .snapshot(self.state.snapshot.identity.id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        if snapshot
            .interactions
            .iter()
            .any(|interaction| interaction.status == InteractionStatus::Pending)
        {
            return Err(AgentRuntimeError::InvalidInput(
                "conversation recovery requires no pending interaction".to_string(),
            ));
        }
        Ok(())
    }
}

fn preview_for_state(
    state: &super::super::ThreadActorState,
    target: ConversationRecoveryTarget,
) -> AgentRuntimeResult<ConversationRecoveryPreview> {
    validate_target(&target)?;
    let items = state.session.session.items();
    let retained = match target.mode {
        ConversationRecoveryMode::RewindTail => rewind_cutoff(items, &target.input_hashes)?,
        ConversationRecoveryMode::RebuildThread => 0,
    };
    validate_closed_tool_history(&items[..retained])?;
    let before_transcript_hash = transcript_hash(items)?;
    let after_transcript_hash = transcript_hash(&items[..retained])?;
    let removed_item_count = count_u64(items.len().saturating_sub(retained))?;
    if removed_item_count == 0 {
        return Err(AgentRuntimeError::InvalidInput(
            "conversation recovery must remove at least one transcript item".to_string(),
        ));
    }
    let removed_input_count = count_u64(target.input_hashes.len())?;
    Ok(ConversationRecoveryPreview {
        target,
        expected_runtime_revision: state.snapshot.revision,
        expected_thread_revision: state.session.thread_revision,
        facts: super::super::ConversationRecoveryFacts {
            recovery_revision: state
                .session
                .session
                .conversation_recovery()
                .revision
                .saturating_add(1),
            before_transcript_hash,
            after_transcript_hash,
        },
        retained_item_count: count_u64(retained)?,
        removed_item_count,
        removed_input_count,
    })
}

fn validate_target(target: &ConversationRecoveryTarget) -> AgentRuntimeResult<()> {
    if target.turn_ids.is_empty() || target.turn_ids.len() > 8 {
        return Err(AgentRuntimeError::InvalidInput(
            "conversation recovery requires a continuous suffix of 1 to 8 Turns".to_string(),
        ));
    }
    let unique = target.turn_ids.iter().collect::<BTreeSet<_>>();
    if unique.len() != target.turn_ids.len() {
        return Err(AgentRuntimeError::InvalidInput(
            "conversation recovery Turn ids must be unique".to_string(),
        ));
    }
    if matches!(target.mode, ConversationRecoveryMode::RewindTail) && target.input_hashes.is_empty()
    {
        return Err(AgentRuntimeError::InvalidInput(
            "rewindTail requires consumed mailbox input hashes".to_string(),
        ));
    }
    if target
        .input_hashes
        .iter()
        .any(|hash| !hash.starts_with("sha256:"))
    {
        return Err(AgentRuntimeError::InvalidInput(
            "conversation recovery input hashes must use sha256".to_string(),
        ));
    }
    Ok(())
}

fn rewind_cutoff(items: &[ModelContextItem], input_hashes: &[String]) -> AgentRuntimeResult<usize> {
    let mut expected = input_hashes.len();
    let mut cutoff = None;
    for (index, item) in items.iter().enumerate().rev() {
        let Some(message) = item.as_message() else {
            continue;
        };
        if message.role != MessageRole::User {
            continue;
        }
        if expected == 0 {
            break;
        }
        let MessageContent::Text(text) = &message.content else {
            return Err(AgentRuntimeError::InvalidInput(
                "rewindTail cannot match a multipart user message".to_string(),
            ));
        };
        let actual = canonical_content_hash(text.as_bytes());
        if actual != input_hashes[expected - 1] {
            return Err(AgentRuntimeError::InvalidInput(
                "rewindTail input hashes do not match the canonical transcript suffix".to_string(),
            ));
        }
        expected -= 1;
        cutoff = Some(index);
        if expected == 0 {
            break;
        }
    }
    if expected != 0 {
        return Err(AgentRuntimeError::InvalidInput(
            "rewindTail input hashes are missing from the canonical transcript suffix".to_string(),
        ));
    }
    cutoff.ok_or_else(|| {
        AgentRuntimeError::InvalidInput(
            "rewindTail did not resolve a safe user-message boundary".to_string(),
        )
    })
}

fn validate_closed_tool_history(items: &[ModelContextItem]) -> AgentRuntimeResult<()> {
    let mut pending = HashSet::<String>::new();
    for item in items {
        let Some(message) = item.as_message() else {
            if !pending.is_empty() {
                return Err(invalid_tool_boundary());
            }
            continue;
        };
        match message.role {
            MessageRole::Assistant if message.metadata.contains_key("tool_calls") => {
                if !pending.is_empty() {
                    return Err(invalid_tool_boundary());
                }
                let metadata = ToolCallHistoryMetadata::from_metadata(&message.metadata)
                    .ok_or_else(invalid_tool_boundary)?;
                let calls = serde_json::from_str::<serde_json::Value>(&metadata.tool_calls_json)
                    .map_err(|_| invalid_tool_boundary())?;
                let calls = calls.as_array().ok_or_else(invalid_tool_boundary)?;
                for call in calls {
                    let id = call
                        .get("id")
                        .or_else(|| call.get("call_id"))
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty())
                        .ok_or_else(invalid_tool_boundary)?;
                    if !pending.insert(id.to_string()) {
                        return Err(invalid_tool_boundary());
                    }
                }
            }
            MessageRole::Tool => {
                let metadata = ToolResultMetadata::from_metadata(&message.metadata)
                    .map_err(|_| invalid_tool_boundary())?;
                if !pending.remove(&metadata.tool_call_id) {
                    return Err(invalid_tool_boundary());
                }
            }
            MessageRole::System | MessageRole::User | MessageRole::Assistant => {
                if !pending.is_empty() {
                    return Err(invalid_tool_boundary());
                }
            }
        }
    }
    if pending.is_empty() {
        Ok(())
    } else {
        Err(invalid_tool_boundary())
    }
}

fn invalid_tool_boundary() -> AgentRuntimeError {
    AgentRuntimeError::InvalidInput(
        "conversation recovery would retain orphaned tool call/output history".to_string(),
    )
}

fn transcript_hash(items: &[ModelContextItem]) -> AgentRuntimeResult<String> {
    serde_json::to_value(items)
        .map(|value| canonical_json_hash(&value))
        .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))
}

fn count_u64(value: usize) -> AgentRuntimeResult<u64> {
    u64::try_from(value).map_err(|_| {
        AgentRuntimeError::InvalidInput("conversation recovery count overflow".to_string())
    })
}

fn usize_from_u64(value: u64) -> AgentRuntimeResult<usize> {
    usize::try_from(value).map_err(|_| {
        AgentRuntimeError::InvalidInput("conversation recovery item offset overflow".to_string())
    })
}

fn recovery_context(record: &ConversationRecoveryRecord) -> String {
    format!(
        "对话上下文已恢复（mode={:?}, revision={}）。被回退对话不再是有效模型上下文。Task、WorkUnit、文件、Git commit、工具副作用和其他外部状态均未回滚；继续前必须读取 canonical Task 状态并检查当前工作区，以它们作为事实源。",
        record.mode, record.revision
    )
}

fn result_from_record(record: &ConversationRecoveryRecord) -> ConversationRecoveryResult {
    ConversationRecoveryResult {
        recovery_id: record.recovery_id.clone(),
        mode: record.mode,
        facts: super::super::ConversationRecoveryFacts {
            recovery_revision: record.revision,
            before_transcript_hash: record.before_transcript_hash.clone(),
            after_transcript_hash: record.after_transcript_hash.clone(),
        },
        runtime_revision: record.runtime_revision,
        thread_revision: record.thread_revision,
        removed_item_count: record.removed_item_count,
        removed_input_count: record.removed_input_count,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{Message, MessageContent, ToolCallKind};

    use super::*;

    fn message(role: MessageRole, text: &str) -> ModelContextItem {
        Message {
            role,
            content: MessageContent::Text(text.to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        }
        .into()
    }

    fn assistant_tool_call(id: &str) -> ModelContextItem {
        let mut metadata = HashMap::new();
        ToolCallHistoryMetadata::new(
            serde_json::json!([{"id": id, "type": "function"}]).to_string(),
        )
        .insert_into(&mut metadata);
        Message {
            role: MessageRole::Assistant,
            content: MessageContent::Text(String::new()),
            reasoning_content: None,
            metadata,
        }
        .into()
    }

    fn tool_result(id: &str) -> ModelContextItem {
        let mut metadata = HashMap::new();
        ToolResultMetadata::new(
            id.to_string(),
            Some(id.to_string()),
            "shell".to_string(),
            ToolCallKind::Function,
            "{}".to_string(),
        )
        .insert_into(&mut metadata);
        Message {
            role: MessageRole::Tool,
            content: MessageContent::Text("done".to_string()),
            reasoning_content: None,
            metadata,
        }
        .into()
    }

    #[test]
    fn rewind_tail_matches_all_selected_leading_inputs() {
        let items = vec![
            message(MessageRole::User, "first"),
            message(MessageRole::Assistant, "done"),
            message(MessageRole::User, "second"),
            message(MessageRole::User, "coalesced"),
            message(MessageRole::Assistant, "failed"),
        ];
        let hashes = vec![
            canonical_content_hash(b"second"),
            canonical_content_hash(b"coalesced"),
        ];

        assert_eq!(rewind_cutoff(&items, &hashes).unwrap(), 2);
    }

    #[test]
    fn rewind_tail_rejects_non_matching_suffix() {
        let items = vec![message(MessageRole::User, "actual")];
        let error = rewind_cutoff(&items, &[canonical_content_hash(b"other")]).unwrap_err();
        assert!(error.to_string().contains("do not match"));
    }

    #[test]
    fn retained_history_accepts_a_closed_tool_call_pair() {
        let items = vec![assistant_tool_call("call-1"), tool_result("call-1")];

        validate_closed_tool_history(&items).unwrap();
    }

    #[test]
    fn retained_history_rejects_an_orphaned_tool_call() {
        let items = vec![assistant_tool_call("call-1")];

        let error = validate_closed_tool_history(&items).unwrap_err();

        assert!(error.to_string().contains("orphaned tool call"));
    }

    #[test]
    fn recovery_target_rejects_more_than_eight_turns() {
        let target = ConversationRecoveryTarget {
            mode: ConversationRecoveryMode::RebuildThread,
            turn_ids: (0..9).map(|index| format!("turn-{index}")).collect(),
            input_hashes: Vec::new(),
        };

        let error = validate_target(&target).unwrap_err();

        assert!(error.to_string().contains("1 to 8 Turns"));
    }

    #[test]
    fn rebuild_thread_requires_turn_audit_but_not_input_hashes() {
        let target = ConversationRecoveryTarget {
            mode: ConversationRecoveryMode::RebuildThread,
            turn_ids: vec!["turn-compacted".to_string()],
            input_hashes: Vec::new(),
        };

        validate_target(&target).unwrap();
    }
}
