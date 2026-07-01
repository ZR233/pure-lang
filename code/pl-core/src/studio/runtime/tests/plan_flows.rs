use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn proposed_plan_tag_does_not_create_pending_confirmation_interaction() {
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<proposed_plan>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"1. Inspect\\\\n2. Implement\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"</proposed_plan><final>Ready</final>\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2,\"total_tokens\":3}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-runtime-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-runtime-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_chat_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Plan test", CompileMode::Plan)
        .await
        .unwrap();
    let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
    let interaction_emitter = emitter(interaction_events.clone());
    let interaction_callback = runtime
        .interactions()
        .callback(session.id.clone(), interaction_emitter.clone());

    let outcome = runtime
        .run_prompt(RunPromptRequest {
            session_id: session.id.clone(),
            turn_id: "turn-plan-test".to_string(),
            prompt: "make a plan".to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(outcome.result.status, TurnResultStatus::Completed);
    assert!(outcome.result.content.contains("Ready"));
    let plan_item = outcome
        .trace_events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        });
    assert!(plan_item.is_none());
    assert!(outcome.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(TraceTextChannel::Final)
                && item.content.contains("Ready")
    )));
    let studio_events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    assert!(!studio_events.iter().any(|envelope| {
        matches!(
            &envelope.kind,
            StudioEventKind::PlanLifecycleChanged { event }
                if event.state == PlanLifecycleState::PendingConfirmation
        )
    }));
    assert!(interaction_events.lock().await.is_empty());
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

#[tokio::test]
async fn plan_exit_tool_creates_pending_confirmation_interaction() {
    let tool_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"plan_exit\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"call_id\":\"call_1\",\"delta\":\"{\\\"content\\\":\\\"# Plan\\\\n\\\\n- Inspect\\\\n- Implement\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"plan_exit\",\"arguments\":\"{\\\"content\\\":\\\"# Plan\\\\n\\\\n- Inspect\\\\n- Implement\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let final_sse = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_2\",\"delta\":\"Plan submitted.\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_2\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"Plan submitted.\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_2\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_sequence(vec![tool_sse, final_sse]).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-runtime-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-runtime-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Plan exit test", CompileMode::Plan)
        .await
        .unwrap();
    let interaction_events = std::sync::Arc::new(Mutex::new(Vec::new()));
    let interaction_emitter = emitter(interaction_events.clone());
    let interaction_callback = runtime
        .interactions()
        .callback(session.id.clone(), interaction_emitter.clone());

    let outcome = runtime
        .run_prompt(RunPromptRequest {
            session_id: session.id.clone(),
            turn_id: "turn-plan-exit-test".to_string(),
            prompt: "make a plan".to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(outcome.result.status, TurnResultStatus::Completed);
    assert_eq!(outcome.result.content, "Plan submitted.");
    let plan_item = outcome
        .trace_events
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .expect("completed plan item");
    assert_eq!(plan_item.content, "# Plan\n\n- Inspect\n- Implement");

    let interaction = store
        .read_interaction(&plan_confirmation_id(&plan_item.item_id))
        .await
        .unwrap()
        .expect("plan confirmation interaction");
    assert_eq!(interaction.kind, InteractionKind::PlanConfirmation);
    assert_eq!(interaction.status, InteractionStatus::Pending);
    assert_eq!(
        interaction.payload,
        InteractionPayload::PlanConfirmation {
            plan_id: plan_item.item_id.clone(),
            content: "# Plan\n\n- Inspect\n- Implement".to_string(),
        }
    );
    let studio_events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    assert!(studio_events.iter().any(|envelope| {
        matches!(
            &envelope.kind,
            StudioEventKind::PlanLifecycleChanged { event }
                if event.plan_id == plan_item.item_id
                    && event.state == PlanLifecycleState::PendingConfirmation
        )
    }));
    assert_eq!(interaction_events.lock().await.len(), 1);
    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}
