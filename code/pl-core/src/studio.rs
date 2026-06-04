pub mod entities;
mod ids;
mod mappers;
mod paths;
mod records;
mod runtime;
mod store;
mod store_support;

pub use records::{
    AgentSnapshotRecord, AgentTimelineEventRecord, ProjectRecord, SessionRecord,
    SessionRuntimeRecord, StudioPromptOutcome, TimelineEventRecord, ToolApprovalRecord,
};
pub use runtime::StudioRuntime;
pub use store::StudioStore;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use pl_protocol::{
        AgentRuntimeDelta, AgentStatus, Message, MessageContent, MessageRole, RuntimeCostAmount,
        TokenUsageSnapshot,
    };
    use pretty_assertions::assert_eq;

    use crate::CompileMode;

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
    async fn records_tool_approval() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();

        store
            .record_tool_approval(ToolApprovalRecord {
                session_id: session.id,
                tool_call_id: "call-1".to_string(),
                tool_name: "bash".to_string(),
                arguments_json: "{}".to_string(),
                working_directory: None,
                decision: "approved".to_string(),
                reason: None,
            })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn agent_timeline_events_are_append_only_and_agents_are_snapshots() {
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
