pub mod entities;
mod event_runtime;
mod ids;
mod interaction_runtime;
mod mappers;
mod paths;
mod records;
mod runtime;
mod store;
mod store_support;

pub use event_runtime::StudioEventRuntime;
pub use interaction_runtime::{
    InteractionEmitter, InteractionEmitterFuture, InteractionRuntime, resolution_matches_kind,
};
pub use records::{
    AgentSnapshotRecord, AgentTimelineEventRecord, AttachmentRecord, MaterializedAttachment,
    PlanImplementationHandoffStart, ProjectRecord, SessionHandoffKind, SessionHandoffRecord,
    SessionHandoffStatus, SessionRecord, SessionRuntimeRecord, SessionSkillRecord,
    SessionVisibility, StudioPromptOutcome,
};
pub use runtime::{RunPromptRequest, StudioRuntime};
pub use store::{StudioStore, studio_attachment};

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use pl_protocol::{
        AgentRuntimeDelta, AgentStatus, ContentPart, ImageSource, InteractionKind,
        InteractionPayload, InteractionRequest, InteractionResolution, InteractionScope,
        InteractionStatus, Message, MessageContent, MessageRole, PlanConfirmationResolution,
        PlanLifecycleState, RuntimeCostAmount, SkillActivation, StudioEventEnvelope,
        StudioEventKind, StudioMessage, StudioMessageRole, StudioMessageStatus, StudioPart,
        StudioPartStatus, StudioPartType, StudioTextChannel, TokenUsageSnapshot,
    };
    use pl_trace::{
        TraceEvent, TraceEventKind, TracePart, TracePartKind, TracePartStatus, TraceTextChannel,
    };
    use pretty_assertions::assert_eq;

    use crate::{CompileMode, SessionVisibility};
    use crate::{InstructionBlock, InstructionSnapshot, InstructionSource, InstructionSourceKind};

    use super::*;

    #[tokio::test]
    async fn project_crud_orders_by_recent_open() {
        let store = StudioStore::open_memory().await.unwrap();

        let first = store.upsert_project("C:/work/alpha").await.unwrap();
        let second = store.upsert_project("C:/work/beta").await.unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
        store.mark_project_opened(&first.id).await.unwrap();

        let projects = store.list_projects().await.unwrap();

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].id, first.id);
        assert_eq!(projects[1].id, second.id);
    }

    #[tokio::test]
    async fn archive_project_hides_project_and_clears_studio_history() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();
        let message = Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        };
        store.append_message(&session.id, &message).await.unwrap();
        store
            .append_studio_event(StudioEventEnvelope {
                event_id: "studio-event-1".to_string(),
                project_id: Some(project.id.clone()),
                session_id: Some(session.id.clone()),
                turn_id: Some("turn-1".to_string()),
                sequence: 0,
                created_at: 1,
                kind: StudioEventKind::MessageUpdated {
                    message: Box::new(StudioMessage {
                        message_id: "turn-1:user".to_string(),
                        session_id: session.id.clone(),
                        turn_id: "turn-1".to_string(),
                        role: StudioMessageRole::User,
                        status: StudioMessageStatus::Completed,
                        created_at: 1,
                        updated_at: 1,
                        completed_at: Some(1),
                        error: None,
                        metadata: serde_json::json!({}),
                    }),
                },
            })
            .await
            .unwrap();
        store
            .append_studio_event(StudioEventEnvelope {
                event_id: "studio-event-2".to_string(),
                project_id: Some(project.id.clone()),
                session_id: Some(session.id.clone()),
                turn_id: Some("turn-1".to_string()),
                sequence: 0,
                created_at: 1,
                kind: StudioEventKind::MessagePartUpdated {
                    part: Box::new(StudioPart {
                        part_id: "turn-1:user-text".to_string(),
                        message_id: "turn-1:user".to_string(),
                        session_id: session.id.clone(),
                        turn_id: "turn-1".to_string(),
                        part_type: StudioPartType::Text,
                        order: 1,
                        status: StudioPartStatus::Completed,
                        created_at: 1,
                        updated_at: 1,
                        completed_at: Some(1),
                        error: None,
                        text_channel: Some(StudioTextChannel::User),
                        text: "hello".to_string(),
                        attachments: Vec::new(),
                        tool: None,
                        agent: None,
                        inference: None,
                        plan: None,
                        file: None,
                        usage: None,
                        synthetic: false,
                        ignored: false,
                    }),
                },
            })
            .await
            .unwrap();
        store
            .create_turn(
                &session.id,
                "turn-1",
                pl_protocol::StudioTurnStatus::Queued,
                1,
            )
            .await
            .unwrap();
        store
            .upsert_agent_snapshot(AgentSnapshotRecord {
                id: "agent-1".to_string(),
                session_id: session.id.clone(),
                path: "/root/research".to_string(),
                parent_path: None,
                role: "executor".to_string(),
                task: "research".to_string(),
                status: AgentStatus::Running,
                summary: None,
                depth: 1,
                error: None,
                reason: None,
                budget_limit_kind: None,
                budget_usage: None,
                runtime_usage: None,
                updated_at: 10,
            })
            .await
            .unwrap();
        store
            .record_agent_event(AgentTimelineEventRecord {
                event_id: "event-1".to_string(),
                session_id: session.id.clone(),
                sequence: 0,
                kind: "agentStatus".to_string(),
                agent_id: Some("agent-1".to_string()),
                path: Some("/root/research".to_string()),
                parent_path: None,
                payload_json: "{}".to_string(),
                created_at: 1,
            })
            .await
            .unwrap();
        let runtime_delta = AgentRuntimeDelta {
            inference_id: "root-1".to_string(),
            agent_id: "agent-root".to_string(),
            path: "/root".to_string(),
            parent_path: None,
            role: "root".to_string(),
            model: "model".to_string(),
            context_window: Some(128_000),
            usage: TokenUsageSnapshot {
                prompt_tokens: 10,
                completion_tokens: 2,
                total_tokens: 12,
                cached_prompt_tokens: 0,
            },
            estimated_costs: Vec::new(),
            has_unpriced_usage: true,
            updated_at: 20,
        };
        store
            .record_agent_runtime_delta(&session.id, &runtime_delta)
            .await
            .unwrap();

        let archived = store.archive_project(&project.id).await.unwrap().unwrap();
        let hidden_projects = store.list_projects().await.unwrap();
        let sessions = store.list_sessions(&project.id).await.unwrap();
        let messages = store.load_messages(&session.id).await.unwrap();
        let studio_events = store
            .load_studio_events(&session.id, None, None)
            .await
            .unwrap();
        let studio_messages = store.load_studio_messages(&session.id).await.unwrap();
        let message_parts = store.load_message_parts(&session.id).await.unwrap();
        let turn = store
            .set_turn_status("turn-1", pl_protocol::StudioTurnStatus::Completed, None, 2)
            .await
            .unwrap();
        let agents = store.list_agents(&session.id).await.unwrap();
        let agent_events = store.list_agent_events(&session.id).await.unwrap();
        let runtime = store.load_session_runtime(&session.id).await.unwrap();
        let skills = store.list_session_skills(&session.id).await.unwrap();
        let reopened = store.upsert_project("C:/work/alpha").await.unwrap();
        let visible_projects = store.list_projects().await.unwrap();
        let reopened_sessions = store.list_sessions(&project.id).await.unwrap();

        assert_eq!(archived.id, project.id);
        assert_eq!(hidden_projects, Vec::<ProjectRecord>::new());
        assert_eq!(sessions, Vec::<SessionRecord>::new());
        assert_eq!(messages, Vec::<Message>::new());
        assert_eq!(studio_events, Vec::<StudioEventEnvelope>::new());
        assert_eq!(studio_messages, Vec::new());
        assert_eq!(message_parts, Vec::new());
        assert_eq!(turn, None);
        assert_eq!(agents, Vec::<AgentSnapshotRecord>::new());
        assert_eq!(agent_events, Vec::<AgentTimelineEventRecord>::new());
        assert_eq!(runtime, None);
        assert_eq!(skills, Vec::<SessionSkillRecord>::new());
        assert_eq!(reopened.id, project.id);
        assert_eq!(visible_projects[0].id, project.id);
        assert_eq!(reopened_sessions, Vec::<SessionRecord>::new());
    }

    #[tokio::test]
    async fn append_studio_event_projects_message_part_snapshots() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Conversation", CompileMode::Auto)
            .await
            .unwrap();
        let message = StudioMessage {
            message_id: "turn-1:assistant".to_string(),
            session_id: session.id.clone(),
            turn_id: "turn-1".to_string(),
            role: StudioMessageRole::Assistant,
            status: StudioMessageStatus::Streaming,
            created_at: 10,
            updated_at: 10,
            completed_at: None,
            error: None,
            metadata: serde_json::json!({}),
        };
        let message_event = store
            .append_studio_event(StudioEventEnvelope {
                event_id: "studio-event-1".to_string(),
                project_id: Some(project.id.clone()),
                session_id: Some(session.id.clone()),
                turn_id: Some("turn-1".to_string()),
                sequence: 0,
                created_at: 10,
                kind: StudioEventKind::MessageUpdated {
                    message: Box::new(message),
                },
            })
            .await
            .unwrap();
        let part = StudioPart {
            part_id: "turn-1:assistant-final".to_string(),
            message_id: "turn-1:assistant".to_string(),
            session_id: session.id.clone(),
            turn_id: "turn-1".to_string(),
            part_type: StudioPartType::Text,
            order: 999,
            status: StudioPartStatus::Completed,
            created_at: 10,
            updated_at: 11,
            completed_at: Some(11),
            error: None,
            text_channel: Some(StudioTextChannel::Final),
            text: "hello".to_string(),
            attachments: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            plan: None,
            file: None,
            usage: None,
            synthetic: false,
            ignored: false,
        };
        let part_event = store
            .append_studio_event(StudioEventEnvelope {
                event_id: "studio-event-2".to_string(),
                project_id: Some(project.id),
                session_id: Some(session.id.clone()),
                turn_id: Some("turn-1".to_string()),
                sequence: 0,
                created_at: 11,
                kind: StudioEventKind::MessagePartUpdated {
                    part: Box::new(part),
                },
            })
            .await
            .unwrap();

        let StudioEventKind::MessageUpdated { message } = &message_event.kind else {
            panic!("expected message snapshot");
        };
        assert_eq!(message_event.sequence, 0);
        assert_eq!(message.message_id, "turn-1:assistant");
        let StudioEventKind::MessagePartUpdated { part } = &part_event.kind else {
            panic!("expected part snapshot");
        };
        assert_eq!(part_event.sequence, 1);
        assert_eq!(part.order, 1);

        let stored_events = store
            .load_studio_events(&session.id, None, None)
            .await
            .unwrap();
        let StudioEventKind::MessagePartUpdated { part } = &stored_events[1].kind else {
            panic!("expected stored part snapshot");
        };
        assert_eq!(stored_events[1].sequence, 1);
        assert_eq!(part.order, 1);
        assert_eq!(part.text, "hello");

        let messages = store.load_studio_messages(&session.id).await.unwrap();
        let parts = store.load_message_parts(&session.id).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].part.order, 1);
    }

    #[tokio::test]
    async fn core_trace_user_snapshot_does_not_duplicate_canonical_user_part() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Conversation", CompileMode::Auto)
            .await
            .unwrap();
        let runtime = StudioEventRuntime::new(store.clone());
        let message = StudioMessage {
            message_id: "turn-1:user".to_string(),
            session_id: session.id.clone(),
            turn_id: "turn-1".to_string(),
            role: StudioMessageRole::User,
            status: StudioMessageStatus::Completed,
            created_at: 10,
            updated_at: 10,
            completed_at: Some(10),
            error: None,
            metadata: serde_json::json!({}),
        };
        let part = StudioPart {
            part_id: "turn-1:user-text".to_string(),
            message_id: "turn-1:user".to_string(),
            session_id: session.id.clone(),
            turn_id: "turn-1".to_string(),
            part_type: StudioPartType::Text,
            order: 0,
            status: StudioPartStatus::Completed,
            created_at: 10,
            updated_at: 10,
            completed_at: Some(10),
            error: None,
            text_channel: Some(StudioTextChannel::User),
            text: "hello".to_string(),
            attachments: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            plan: None,
            file: None,
            usage: None,
            synthetic: false,
            ignored: false,
        };
        runtime
            .emit(
                Some(project.id.clone()),
                Some(session.id.clone()),
                Some("turn-1".to_string()),
                StudioEventKind::MessageUpdated {
                    message: Box::new(message),
                },
            )
            .await
            .unwrap();
        runtime
            .emit(
                Some(project.id),
                Some(session.id.clone()),
                Some("turn-1".to_string()),
                StudioEventKind::MessagePartUpdated {
                    part: Box::new(part),
                },
            )
            .await
            .unwrap();

        let trace_item = TracePart {
            turn_id: "turn-1".to_string(),
            item_id: "turn-1-user".to_string(),
            started_sequence: 0,
            kind: TracePartKind::Text,
            status: TracePartStatus::Completed,
            created_at: 11,
            updated_at: 11,
            text_channel: Some(TraceTextChannel::User),
            content: "hello".to_string(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        };
        let emitted = runtime
            .emit_agent_event(
                &session.id,
                pl_trace::AgentEvent::TracePartCompleted { item: trace_item },
            )
            .await
            .unwrap();
        assert_eq!(emitted, None);

        let parts = store.load_message_parts(&session.id).await.unwrap();
        let events = store
            .load_studio_events(&session.id, None, None)
            .await
            .unwrap();
        let user_part_events = events
            .iter()
            .filter_map(|event| match &event.kind {
                StudioEventKind::MessagePartUpdated { part }
                    if part.message_id == "turn-1:user" =>
                {
                    Some(part.part_id.as_str())
                }
                StudioEventKind::MessageUpdated { .. }
                | StudioEventKind::MessageRemoved { .. }
                | StudioEventKind::MessagePartUpdated { .. }
                | StudioEventKind::MessagePartRemoved { .. }
                | StudioEventKind::MessagePartDelta { .. }
                | StudioEventKind::TurnChanged { .. }
                | StudioEventKind::InteractionChanged { .. }
                | StudioEventKind::PlanLifecycleChanged { .. }
                | StudioEventKind::SessionRuntimeChanged { .. }
                | StudioEventKind::AgentChanged { .. }
                | StudioEventKind::AgentTimelineChanged { .. }
                | StudioEventKind::SkillActivated { .. }
                | StudioEventKind::SessionHandoffChanged { .. }
                | StudioEventKind::SessionListChanged { .. }
                | StudioEventKind::McpHealthChanged { .. }
                | StudioEventKind::LspHealthChanged { .. }
                | StudioEventKind::Stale { .. } => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].part.part_id, "turn-1:user-text");
        assert_eq!(parts[0].part.message_id, "turn-1:user");
        assert_eq!(user_part_events, vec!["turn-1:user-text"]);
    }

    #[tokio::test]
    async fn session_skills_persist_from_skill_activation_trace_events_and_dedupe() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Skills", CompileMode::Auto)
            .await
            .unwrap();
        let first = SkillActivation {
            name: "skill-creator".to_string(),
            source: "user".to_string(),
            path: "C:/skills/skill-creator".to_string(),
            turn_id: "turn-1".to_string(),
            tool_call_id: "call-1".to_string(),
            activated_at: 10,
        };
        let repeated = SkillActivation {
            name: "Skill-Creator".to_string(),
            source: "user".to_string(),
            path: "C:/skills/skill-creator".to_string(),
            turn_id: "turn-2".to_string(),
            tool_call_id: "call-2".to_string(),
            activated_at: 20,
        };

        store
            .append_turn_records(
                &session.id,
                &[
                    TraceEvent {
                        session_id: session.id.clone(),
                        sequence: 0,
                        timestamp: 10,
                        kind: TraceEventKind::SkillActivated { activation: first },
                    },
                    TraceEvent {
                        session_id: session.id.clone(),
                        sequence: 1,
                        timestamp: 20,
                        kind: TraceEventKind::SkillActivated {
                            activation: repeated,
                        },
                    },
                ],
                &[],
            )
            .await
            .unwrap();

        let skills = store.list_session_skills(&session.id).await.unwrap();
        let names = store.list_session_skill_names(&session.id).await.unwrap();

        assert_eq!(names, vec!["Skill-Creator".to_string()]);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].first_turn_id, "turn-1");
        assert_eq!(skills[0].last_turn_id, "turn-2");
        assert_eq!(skills[0].last_tool_call_id, "call-2");
        assert_eq!(skills[0].activated_at, 10);
        assert_eq!(skills[0].updated_at, 20);
    }

    #[tokio::test]
    async fn session_crud_and_message_restore() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();
        let message = Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        };

        store.append_message(&session.id, &message).await.unwrap();
        let restored = store.load_core_session(&session.id).await.unwrap();

        assert_eq!(restored.len(), 1);
        assert_eq!(restored.messages()[0].role, MessageRole::User);
        match &restored.messages()[0].content {
            MessageContent::Text(text) => assert_eq!(text, "hello"),
            MessageContent::MultiPart(_) => panic!("expected text message"),
        }
    }

    #[tokio::test]
    async fn message_storage_round_trips_image_attachment_parts() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Vision", CompileMode::Auto)
            .await
            .unwrap();
        let message = Message {
            role: MessageRole::User,
            content: MessageContent::MultiPart(vec![
                ContentPart::Text {
                    text: "what is this?".to_string(),
                },
                ContentPart::Image {
                    source: ImageSource::Attachment {
                        attachment_id: "attachment-1".to_string(),
                    },
                    media_type: "image/png".to_string(),
                    filename: Some("image.png".to_string()),
                },
            ]),
            reasoning_content: None,
            metadata: HashMap::new(),
        };

        store.append_message(&session.id, &message).await.unwrap();

        assert_eq!(
            store.load_messages(&session.id).await.unwrap(),
            vec![message]
        );
    }

    #[tokio::test]
    async fn archive_session_hides_it_from_session_list() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();
        let message = Message {
            role: MessageRole::User,
            content: MessageContent::Text("hello".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        };

        store.append_message(&session.id, &message).await.unwrap();
        let archived = store.archive_session(&session.id).await.unwrap().unwrap();
        let sessions = store.list_sessions(&project.id).await.unwrap();
        let restored = store.load_messages(&session.id).await.unwrap();

        assert_eq!(archived.id, session.id);
        assert_eq!(sessions, Vec::<SessionRecord>::new());
        assert_eq!(restored, vec![message]);
    }

    #[tokio::test]
    async fn plan_implementation_handoff_creates_child_and_reuses_target() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let origin = store
            .create_session(&project.id, "Plan work", CompileMode::Plan)
            .await
            .unwrap();
        let interaction = InteractionRequest {
            interaction_id: "plan-confirmation-plan-1".to_string(),
            kind: InteractionKind::PlanConfirmation,
            status: InteractionStatus::Pending,
            scope: InteractionScope {
                session_id: origin.id.clone(),
                turn_id: "turn-1".to_string(),
                item_id: Some("plan-1".to_string()),
                tool_id: None,
                agent_path: None,
            },
            payload: InteractionPayload::PlanConfirmation {
                plan_id: "plan-1".to_string(),
                content: "1. Inspect\n2. Implement".to_string(),
            },
            created_at: 10,
            updated_at: 10,
            resolved_at: None,
            resolution: None,
        };
        store.upsert_interaction(&interaction).await.unwrap();

        let resolution = InteractionResolution::PlanConfirmation {
            decision: PlanConfirmationResolution::ImplementFreshContext,
            content: None,
            reason: None,
        };
        let first = store
            .start_plan_implementation_handoff(&interaction.interaction_id, resolution.clone())
            .await
            .unwrap();
        let second = store
            .start_plan_implementation_handoff(&interaction.interaction_id, resolution)
            .await
            .unwrap();
        let listed = store.list_sessions(&project.id).await.unwrap();
        let restored_origin = store.read_session(&origin.id).await.unwrap().unwrap();

        assert!(first.should_start_run);
        assert!(!second.should_start_run);
        assert_eq!(first.target_session.id, second.target_session.id);
        assert_eq!(listed, vec![restored_origin.clone()]);
        assert_eq!(restored_origin.visibility, SessionVisibility::Active);
        assert_eq!(
            first.target_session.parent_session_id,
            Some(restored_origin.id.clone())
        );
        assert_eq!(
            second.target_session.parent_session_id,
            Some(restored_origin.id)
        );
        assert_eq!(first.interaction.status, InteractionStatus::Resolved);
        assert_eq!(first.plan_content, "1. Inspect\n2. Implement");
        let states = first
            .plan_lifecycle_events
            .iter()
            .map(|event| event.state)
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            vec![
                PlanLifecycleState::Accepted,
                PlanLifecycleState::Implementing,
            ]
        );
        assert_eq!(second.plan_lifecycle_events, Vec::new());
    }

    #[tokio::test]
    async fn replace_session_messages_rewrites_history() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();
        let first = Message {
            role: MessageRole::User,
            content: MessageContent::Text("first".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        };
        let second = Message {
            role: MessageRole::User,
            content: MessageContent::Text("second".to_string()),
            reasoning_content: None,
            metadata: HashMap::new(),
        };

        store.append_message(&session.id, &first).await.unwrap();
        store
            .replace_session_messages(&session.id, std::slice::from_ref(&second))
            .await
            .unwrap();
        let restored = store.load_messages(&session.id).await.unwrap();

        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0], second);
    }

    #[tokio::test]
    async fn set_session_mode_persists_mode_label() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Plan work", CompileMode::Auto)
            .await
            .unwrap();

        store
            .set_session_mode(&session.id, CompileMode::Plan)
            .await
            .unwrap();
        let updated = store.read_session(&session.id).await.unwrap().unwrap();

        assert_eq!(updated.mode, "plan");
    }

    #[tokio::test]
    async fn instruction_snapshot_round_trips_with_session() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();
        let snapshot = InstructionSnapshot {
            base: InstructionBlock {
                source: InstructionSource {
                    kind: InstructionSourceKind::BuiltInBase,
                    label: "base".to_string(),
                    path: None,
                },
                content: "base".to_string(),
            },
            developer: vec![InstructionBlock {
                source: InstructionSource {
                    kind: InstructionSourceKind::Mode,
                    label: "mode".to_string(),
                    path: None,
                },
                content: "developer".to_string(),
            }],
            user: vec![InstructionBlock {
                source: InstructionSource {
                    kind: InstructionSourceKind::ProjectDoc,
                    label: "AGENTS.md".to_string(),
                    path: Some("C:/work/alpha/AGENTS.md".to_string()),
                },
                content: "project".to_string(),
            }],
        };

        assert_eq!(session.instruction_snapshot, None);
        let saved = store
            .save_instruction_snapshot(&session.id, &snapshot)
            .await
            .unwrap()
            .unwrap();
        let read = store.read_session(&session.id).await.unwrap().unwrap();
        let listed = store.list_sessions(&project.id).await.unwrap();

        assert_eq!(saved.instruction_snapshot, Some(snapshot.clone()));
        assert_eq!(read.instruction_snapshot, Some(snapshot.clone()));
        assert_eq!(listed[0].instruction_snapshot, Some(snapshot));
    }

    #[tokio::test]
    async fn agent_trace_events_are_append_only_and_agents_are_snapshots() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Agent work", CompileMode::Auto)
            .await
            .unwrap();

        let base_snapshot = AgentSnapshotRecord {
            id: "agent-1".to_string(),
            session_id: session.id.clone(),
            path: "/root/research".to_string(),
            parent_path: None,
            role: "executor".to_string(),
            task: "research".to_string(),
            status: AgentStatus::Running,
            summary: None,
            depth: 1,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            runtime_usage: None,
            updated_at: 10,
        };
        store
            .upsert_agent_snapshot(base_snapshot.clone())
            .await
            .unwrap();
        store
            .upsert_agent_snapshot(AgentSnapshotRecord {
                status: AgentStatus::Completed,
                summary: Some("done".to_string()),
                updated_at: 20,
                ..base_snapshot.clone()
            })
            .await
            .unwrap();

        for sequence in [1, 2, 3] {
            store
                .record_agent_event(AgentTimelineEventRecord {
                    event_id: format!("event-{sequence}"),
                    session_id: session.id.clone(),
                    sequence,
                    kind: "agentStatus".to_string(),
                    agent_id: Some("agent-1".to_string()),
                    path: Some("/root/research".to_string()),
                    parent_path: None,
                    payload_json: format!(r#"{{"sequence":{sequence}}}"#),
                    created_at: sequence,
                })
                .await
                .unwrap();
        }

        let agents = store.list_agents(&session.id).await.unwrap();
        let events = store.list_agent_events(&session.id).await.unwrap();

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].status, AgentStatus::Completed);
        assert_eq!(agents[0].summary.as_deref(), Some("done"));
        assert_eq!(events.len(), 3);
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_id.as_str())
                .collect::<Vec<_>>(),
            vec!["event-1", "event-2", "event-3"],
        );
    }

    #[tokio::test]
    async fn session_runtime_snapshot_accumulates_usage_and_cost() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();

        store
            .upsert_agent_snapshot(AgentSnapshotRecord {
                id: "agent-1".to_string(),
                session_id: session.id.clone(),
                path: "/root/research".to_string(),
                parent_path: Some("/root".to_string()),
                role: "executor".to_string(),
                task: "research".to_string(),
                status: AgentStatus::Completed,
                summary: Some("done".to_string()),
                depth: 1,
                error: None,
                reason: None,
                budget_limit_kind: None,
                budget_usage: None,
                runtime_usage: None,
                updated_at: 5,
            })
            .await
            .unwrap();

        let root_usage = TokenUsageSnapshot {
            prompt_tokens: 100_000,
            completion_tokens: 10_000,
            total_tokens: 110_000,
            cached_prompt_tokens: 40_000,
        };
        let root_delta = AgentRuntimeDelta {
            inference_id: "root-1".to_string(),
            agent_id: "agent-root".to_string(),
            path: "/root".to_string(),
            parent_path: None,
            role: "root".to_string(),
            model: "priced-model".to_string(),
            context_window: Some(1_000_000),
            usage: root_usage.clone(),
            estimated_costs: vec![RuntimeCostAmount {
                currency: "CNY".to_string(),
                amount: 0.0808,
            }],
            has_unpriced_usage: false,
            updated_at: 10,
        };
        let second_root_delta = AgentRuntimeDelta {
            inference_id: "root-2".to_string(),
            updated_at: 20,
            ..root_delta.clone()
        };
        let subagent_delta = AgentRuntimeDelta {
            inference_id: "agent-1-inference".to_string(),
            agent_id: "agent-1".to_string(),
            path: "/root/research".to_string(),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            model: "usd-model".to_string(),
            context_window: Some(400_000),
            usage: TokenUsageSnapshot {
                prompt_tokens: 50_000,
                completion_tokens: 5_000,
                total_tokens: 55_000,
                cached_prompt_tokens: 0,
            },
            estimated_costs: vec![RuntimeCostAmount {
                currency: "USD".to_string(),
                amount: 0.06,
            }],
            has_unpriced_usage: false,
            updated_at: 30,
        };
        let unpriced_delta = AgentRuntimeDelta {
            inference_id: "agent-1-unpriced".to_string(),
            agent_id: "agent-1".to_string(),
            path: "/root/research".to_string(),
            parent_path: Some("/root".to_string()),
            role: "executor".to_string(),
            model: "unpriced-model".to_string(),
            context_window: Some(400_000),
            usage: TokenUsageSnapshot {
                prompt_tokens: 10_000,
                completion_tokens: 1_000,
                total_tokens: 11_000,
                cached_prompt_tokens: 0,
            },
            estimated_costs: Vec::new(),
            has_unpriced_usage: true,
            updated_at: 40,
        };

        assert!(
            store
                .record_agent_runtime_delta(&session.id, &root_delta)
                .await
                .unwrap()
        );
        assert!(
            !store
                .record_agent_runtime_delta(&session.id, &root_delta)
                .await
                .unwrap()
        );
        assert!(
            store
                .record_agent_runtime_delta(&session.id, &second_root_delta)
                .await
                .unwrap()
        );
        assert!(
            store
                .record_agent_runtime_delta(&session.id, &subagent_delta)
                .await
                .unwrap()
        );
        assert!(
            store
                .record_agent_runtime_delta(&session.id, &unpriced_delta)
                .await
                .unwrap()
        );

        let runtime = store
            .load_session_runtime(&session.id)
            .await
            .unwrap()
            .unwrap();
        let agents = store.list_agents(&session.id).await.unwrap();

        assert_eq!(runtime.model, "unpriced-model");
        assert_eq!(runtime.context_window, Some(400_000));
        assert_eq!(runtime.latest_context_tokens, 10_000);
        assert_eq!(runtime.prompt_tokens, 260_000);
        assert_eq!(runtime.completion_tokens, 26_000);
        assert_eq!(runtime.cached_prompt_tokens, 80_000);
        assert_eq!(runtime.total_tokens, 286_000);
        assert_eq!(runtime.currency, None);
        assert_eq!(runtime.estimated_cost, None);
        assert_eq!(
            runtime
                .estimated_costs
                .iter()
                .map(|cost| cost.currency.as_str())
                .collect::<Vec<_>>(),
            vec!["CNY", "USD"],
        );
        assert!(
            runtime.estimated_costs[0].amount.is_finite()
                && (runtime.estimated_costs[0].amount - 0.1616).abs() < 0.000_001
        );
        assert!(
            runtime.estimated_costs[1].amount.is_finite()
                && (runtime.estimated_costs[1].amount - 0.06).abs() < 0.000_001
        );
        assert!(runtime.has_unpriced_usage);
        assert_eq!(agents.len(), 1);
        assert_eq!(
            agents[0].runtime_usage.as_ref().map(|usage| (
                usage.model.as_str(),
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens,
                usage.has_unpriced_usage,
            )),
            Some(("unpriced-model", 60_000, 6_000, 66_000, true)),
        );
    }

    #[tokio::test]
    async fn settings_round_trip() {
        let store = StudioStore::open_memory().await.unwrap();

        store
            .save_setting("activeProject", "project-1")
            .await
            .unwrap();
        let value = store.load_setting("activeProject").await.unwrap();

        assert_eq!(value.as_deref(), Some("project-1"));
    }
}
