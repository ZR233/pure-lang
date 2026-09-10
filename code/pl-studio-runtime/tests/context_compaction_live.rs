use pl_core::{
    context::ContextContent,
    model::{Model, ModelRequest},
    thread::{StepInput, ThreadHandle},
};
use pl_model::{
    model::{ModelInfo, default_models, openai_default_model_slugs},
    provider::ProviderEndpoint,
    runtime::{ModelRuntime, ThreadCompactionOptions, ThreadCompactionStrategy, ThreadModel},
};

#[tokio::test]
async fn openai_responses_compacts_context_live() {
    let mut endpoint = ProviderEndpoint::openai(std::env::var("OPENAI_BASE_URL").ok());
    endpoint.bearer_token = Some(
        std::env::var("OPENAI_API_KEY").expect("explicit live acceptance requires OPENAI_API_KEY"),
    );
    endpoint.service_capabilities.remote_compaction = true;
    let slug = std::env::var("OPENAI_MODEL")
        .unwrap_or_else(|_| openai_default_model_slugs()[0].to_owned());
    let model = default_models()
        .into_iter()
        .find(|model| model.slug == slug)
        .unwrap_or_else(|| ModelInfo::compatible(&slug));
    let factory = ThreadModel::new(ModelRuntime::new(endpoint, model).unwrap(), None);
    let thread = ThreadHandle::start(
        "compaction-live".into(),
        factory.open_session().await.unwrap(),
    )
    .unwrap();
    thread
        .step(StepInput {
            turn_id: "first".into(),
            attempt_id: "first-call".into(),
            content: vec![ContextContent::Text {
                text: "项目代号 alpha，用户偏好回答要简短。只回复 ok。".into(),
            }],
            cancellation: Default::default(),
        })
        .await
        .unwrap();
    let snapshot = thread.snapshot();
    let compacted = factory
        .compact(
            ModelRequest {
                tool_call_mode: pl_core::model::ToolCallMode::Parallel,
                solo_tool_ids: Vec::new().into(),
                progress: None,
                thread_id: "compaction-live".into(),
                turn_id: "compaction".into(),
                attempt_id: "compaction-call".into(),
                context: snapshot.context.clone(),
                tools: snapshot.discovered_tools.clone(),
                committed_private_context: snapshot.private_context.clone(),
                resources: None,
                cancellation: Default::default(),
            },
            ThreadCompactionOptions {
                strategy: ThreadCompactionStrategy::PreferNative,
                instructions: "保留用户偏好和项目事实。".into(),
                requirement: "总结完整上下文。".into(),
                summary_prefix: "此前对话摘要".into(),
                max_output_tokens: Some(256),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        compacted.implementation,
        ThreadCompactionStrategy::PreferNative
    );
    assert!(compacted.replacement.records.iter().any(|record| record.content.iter().any(|content| matches!(content, ContextContent::Opaque { payload } if payload.format() == "pl.model.compaction"))));
    thread.replace_context(compacted.replacement).await.unwrap();
    let response = thread
        .step(StepInput {
            turn_id: "second".into(),
            attempt_id: "second-call".into(),
            content: vec![ContextContent::Text {
                text: "项目代号是什么？只回答代号。".into(),
            }],
            cancellation: Default::default(),
        })
        .await
        .unwrap();
    assert!(response.content.iter().any(|content| matches!(content, ContextContent::Text { text } if text.to_lowercase().contains("alpha"))));
    let replay = pl_core::thread::journal::replay(&thread.journal().await.unwrap()).unwrap();
    assert_eq!(replay.context, thread.snapshot().context);
    thread.close().await.unwrap();
}
