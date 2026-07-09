use std::sync::{Arc, Mutex};

use super::*;
use pretty_assertions::assert_eq;

#[derive(Clone)]
struct RecordingHostedRuntime {
    provider: pl_model::ProviderInfo,
    completions: Arc<Mutex<Vec<HostedTurnCompletion>>>,
    events: Arc<Mutex<Vec<pl_trace::AgentEvent>>>,
}

impl RecordingHostedRuntime {
    fn new(provider: pl_model::ProviderInfo) -> Self {
        Self {
            provider,
            completions: Arc::new(Mutex::new(Vec::new())),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl HostedAgentRuntime for RecordingHostedRuntime {
    type Error = pl_protocol::PureError;

    fn prepare_turn(
        &self,
        request: HostedTurnRequest,
    ) -> impl std::future::Future<Output = std::result::Result<HostedTurnPreparation, Self::Error>> + Send
    {
        let provider = self.provider.clone();
        async move {
            let kernel = AgentKernel::builder(PureCoreBuilder::from_provider_info(provider)?)
                .with_profile(CoreAgentProfile::host_provided(std::env::temp_dir()))
                .build()
                .await;
            Ok(HostedTurnPreparation::new(
                request,
                kernel,
                Vec::new(),
                TurnRequest::new("hello".to_string(), CompileMode::Auto),
                TurnOptions::default(),
            ))
        }
    }

    fn handle_event(
        &self,
        event: pl_trace::AgentEvent,
    ) -> impl std::future::Future<Output = std::result::Result<(), Self::Error>> + Send {
        let events = self.events.clone();
        async move {
            events.lock().unwrap().push(event);
            Ok(())
        }
    }

    fn complete_turn(
        &self,
        completion: HostedTurnCompletion,
    ) -> impl std::future::Future<Output = std::result::Result<(), Self::Error>> + Send {
        let completions = self.completions.clone();
        async move {
            completions.lock().unwrap().push(completion);
            Ok(())
        }
    }
}

#[tokio::test]
async fn hosted_agent_runner_executes_prepared_turn_and_reports_completion() {
    let sse_body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"hosted ok\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"phase\":\"final_answer\",\"content\":[{\"type\":\"output_text\",\"text\":\"hosted ok\"}]}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n",
        "data: [DONE]\n\n"
    )
    .to_string();
    let (base_url, handle) = serve_sse_once(sse_body).await;
    let mut provider = pl_model::ProviderInfo::openai(Some(base_url));
    provider.bearer_token = Some("test-token".to_string());
    provider.default_model = "local-responses".to_string();
    let runtime = RecordingHostedRuntime::new(provider);

    HostedAgentRunner::new(runtime.clone())
        .run(HostedTurnRequest::new("hosted-session", "hosted-turn"))
        .await
        .unwrap();
    handle.await.unwrap();

    let completions = runtime.completions.lock().unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].turn_id(), "hosted-turn");
    assert_eq!(completions[0].result().content, "hosted ok");
    assert_eq!(completions[0].session().messages().len(), 2);
    assert!(!completions[0].trace_events().is_empty());
    assert!(
        runtime
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| matches!(event, pl_trace::AgentEvent::Done))
    );
}
