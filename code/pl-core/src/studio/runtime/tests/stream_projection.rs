use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn tool_boundary_with_reused_provider_ids_creates_new_parts_after_tool() {
    let tool_sse = concat!(
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"thinking\",\"delta\":\"before tool\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"before \"}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"list_files\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"call_id\":\"call_1\",\"delta\":\"{\\\"path\\\":\\\".\\\"}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"list_files\",\"arguments\":\"{\\\"path\\\":\\\".\\\"}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let final_sse = concat!(
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"thinking\",\"delta\":\"after tool\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"after\"}\n\n",
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
    tokio::fs::write(workspace.join("alpha.txt"), "alpha")
        .await
        .unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Tool boundary test", CompileMode::Simple)
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
            turn_id: "turn-tool-boundary-test".to_string(),
            prompt: "list files and continue".to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(outcome.result.status, TurnResultStatus::Completed);
    assert_eq!(outcome.result.content, "after");

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let assistant_parts = parts
        .iter()
        .filter(|record| {
            record.part.message_id == "turn-tool-boundary-test:assistant" && !record.part.synthetic
        })
        .map(|record| &record.part)
        .collect::<Vec<_>>();
    let compact = assistant_parts
        .iter()
        .filter_map(|part| match part.part_type {
            pl_protocol::StudioPartType::Reasoning | pl_protocol::StudioPartType::Text => Some((
                part.part_id.as_str(),
                part.part_type,
                part.text.as_str(),
                part.order,
            )),
            pl_protocol::StudioPartType::Tool
            | pl_protocol::StudioPartType::Agent
            | pl_protocol::StudioPartType::Turn
            | pl_protocol::StudioPartType::Inference
            | pl_protocol::StudioPartType::Plan
            | pl_protocol::StudioPartType::File => None,
        })
        .collect::<Vec<_>>();
    let compact_identity = compact
        .iter()
        .map(|(part_id, part_type, text, _)| {
            assert!(part_id.starts_with("turn-tool-boundary-test:part-"));
            (*part_type, *text)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        compact_identity,
        vec![
            (pl_protocol::StudioPartType::Reasoning, "before tool",),
            (pl_protocol::StudioPartType::Text, "before ",),
            (pl_protocol::StudioPartType::Reasoning, "after tool",),
            (pl_protocol::StudioPartType::Text, "after",),
        ]
    );
    assert!(compact[0].3 < compact[1].3);
    assert!(compact[1].3 < compact[2].3);
    assert!(compact[2].3 < compact[3].3);
    let tool = assistant_parts
        .iter()
        .find(|part| part.part_type == pl_protocol::StudioPartType::Tool)
        .expect("tool part");
    assert!(tool.part_id.starts_with("turn-tool-boundary-test:part-"));
    assert_eq!(
        tool.tool
            .as_ref()
            .and_then(|tool| tool.provider_item_id.as_deref()),
        Some("fc_1")
    );
    assert!(tool.order > compact[1].3 && tool.order < compact[2].3);

    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}

#[tokio::test]
async fn late_responses_phase_reopens_text_block_as_new_part() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"default \"}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"commentary\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"commentary\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"commentary\",\"content\":[{\"type\":\"output_text\",\"text\":\"commentary\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("pure-late-phase-home-{unique}"));
    let workspace = std::env::temp_dir().join(format!("pure-late-phase-workspace-{unique}"));
    tokio::fs::create_dir_all(&workspace).await.unwrap();
    let config_store = ConfigStore::new(crate::config::ConfigPaths::from_home(&home));
    config_store.save(&test_config(base_url)).unwrap();
    let store = StudioStore::open_memory().await.unwrap();
    let runtime = StudioRuntime::new(store.clone(), config_store);
    let project = runtime.open_project(&workspace).await.unwrap();
    let session = store
        .create_session(&project.id, "Late phase test", CompileMode::Simple)
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
            turn_id: "turn-late-phase-test".to_string(),
            prompt: "stream late phase".to_string(),
            attachment_ids: Vec::new(),
            interaction_callback,
            interaction_emitter,
            options: TurnOptions::default(),
        })
        .await
        .unwrap();
    handle.await.unwrap();

    assert_eq!(outcome.result.status, TurnResultStatus::Completed);
    assert_eq!(outcome.result.content, "default ");

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let text_parts = parts
        .iter()
        .filter_map(|record| {
            (record.part.message_id == "turn-late-phase-test:assistant"
                && record.part.part_type == pl_protocol::StudioPartType::Text
                && !record.part.synthetic)
                .then_some(&record.part)
        })
        .collect::<Vec<_>>();

    assert_eq!(text_parts.len(), 2);
    assert_ne!(text_parts[0].part_id, text_parts[1].part_id);
    assert!(text_parts[0].order < text_parts[1].order);
    assert_eq!(text_parts[0].text_channel, Some(StudioTextChannel::Final));
    assert_eq!(text_parts[0].text, "default ");
    assert_eq!(
        text_parts[1].text_channel,
        Some(StudioTextChannel::Commentary)
    );
    assert_eq!(text_parts[1].text, "commentary");

    let _ = tokio::fs::remove_dir_all(home).await;
    let _ = tokio::fs::remove_dir_all(workspace).await;
}
