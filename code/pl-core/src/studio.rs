pub mod entities;
mod ids;
mod mappers;
mod paths;
mod records;
mod runtime;
mod store;
mod store_support;

pub use records::{
    AgentEventRecord, ProjectRecord, SessionRecord, SessionRuntimeRecord, StudioPromptOutcome,
    ToolApprovalRecord, TraceEventRecord,
};
pub use runtime::StudioRuntime;
pub use store::StudioStore;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use pl_protocol::{Message, MessageContent, MessageRole};
    use pretty_assertions::assert_eq;

    use crate::{CompileMode, TurnResult};

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
    async fn session_runtime_snapshot_accumulates_usage_and_cost() {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store.upsert_project("C:/work/alpha").await.unwrap();
        let session = store
            .create_session(&project.id, "Build app", CompileMode::Auto)
            .await
            .unwrap();
        let mut model = pl_model::ModelInfo::fallback("priced-model");
        model.context_window = Some(1_000_000);
        model.currency = Some("CNY".to_string());
        model.input_price_per_mtok = Some(8.0);
        model.output_price_per_mtok = Some(32.0);
        model.cache_read_price_per_mtok = Some(2.0);
        let result = TurnResult {
            content: "ok".to_string(),
            reasoning_content: None,
            model: "priced-model".to_string(),
            usage: pl_model::TokenUsage {
                prompt_tokens: 100_000,
                completion_tokens: 10_000,
                total_tokens: 110_000,
                cached_prompt_tokens: 40_000,
            },
            mode: CompileMode::Auto,
            session_message_count: 2,
            status: crate::turn::TurnResultStatus::Completed,
            trace_events: Vec::new(),
        };

        store
            .upsert_session_runtime(&session.id, &result, Some(&model))
            .await
            .unwrap();
        store
            .upsert_session_runtime(&session.id, &result, Some(&model))
            .await
            .unwrap();

        let runtime = store
            .load_session_runtime(&session.id)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(runtime.model, "priced-model");
        assert_eq!(runtime.context_window, Some(1_000_000));
        assert_eq!(runtime.latest_context_tokens, 100_000);
        assert_eq!(runtime.prompt_tokens, 200_000);
        assert_eq!(runtime.completion_tokens, 20_000);
        assert_eq!(runtime.cached_prompt_tokens, 80_000);
        assert_eq!(runtime.currency.as_deref(), Some("CNY"));
        assert!(
            runtime
                .estimated_cost
                .is_some_and(|cost| (cost - 1.76).abs() < 0.000_001)
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
