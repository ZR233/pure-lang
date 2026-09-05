//! The routed facade holds concrete provider clients, without an erased mega-interface.
use super::{InvocationRunner, ModelInvocationContext};
use crate::completion::{
    CompletionFailure, CompletionRequest, CompletionResponse, ModelCompactionRequest,
    ModelCompactionResponse,
};
use crate::model::{ModelCapabilities, ModelInfo};
use crate::provider::{ProviderClient, ProviderEndpoint, ProviderWireProtocol};
use pl_protocol::{PricingMode, Result};

/// One resolved model route. Native capabilities remain available through `provider()`.
#[derive(Debug, Clone)]
pub struct ModelRuntime {
    client: ProviderClient,
}

impl ModelRuntime {
    /// Binds a concrete adapter and one model.
    /// # Errors
    /// Returns invalid model or endpoint configuration.
    pub fn new(endpoint: ProviderEndpoint, model: ModelInfo) -> Result<Self> {
        Self::new_with_provider_id(endpoint.name.clone(), endpoint, model)
    }

    /// Binds a route with its stable provider instance identity.
    /// # Errors
    /// Returns invalid model or endpoint configuration.
    pub fn new_with_provider_id(
        id: impl Into<String>,
        endpoint: ProviderEndpoint,
        model: ModelInfo,
    ) -> Result<Self> {
        Ok(Self {
            client: ProviderClient::new(id.into(), endpoint, model)?,
        })
    }

    /// Explicit native access, preserving the concrete vendor types.
    pub fn provider(&self) -> &ProviderClient {
        &self.client
    }
    pub fn model(&self) -> &ModelInfo {
        self.client.runner().model()
    }
    pub fn endpoint(&self) -> &ProviderEndpoint {
        self.client.runner().endpoint()
    }
    pub fn provider_instance_id(&self) -> &str {
        self.client.runner().provider_instance_id()
    }
    pub fn effective_model_capabilities(&self) -> ModelCapabilities {
        self.client.runner().effective_model_capabilities()
    }
    pub fn connection_fingerprint(&self) -> u64 {
        self.client.runner().connection_fingerprint()
    }

    /// Freezes this provider's monetary accounting choice for subsequent invocations.
    pub fn with_pricing_mode(mut self, mode: PricingMode) -> Self {
        self.client.runner_mut().pricing_mode = mode;
        self
    }

    /// Uses an explicit clock for reproducible tariff selection and replay simulations.
    pub fn with_clock(mut self, clock: std::sync::Arc<dyn super::InferenceClock>) -> Self {
        self.client.runner_mut().clock = clock;
        self
    }

    /// Executes a provider-neutral request.
    /// # Errors
    /// Returns a typed failure with any usage observed before termination.
    pub async fn complete(
        &self,
        request: CompletionRequest,
        context: ModelInvocationContext,
    ) -> std::result::Result<CompletionResponse, CompletionFailure> {
        self.client.runner().complete(request, context).await
    }

    /// Returns a real remote compaction capability only when the endpoint declares support.
    pub fn compaction(&self) -> Option<RemoteCompaction<'_>> {
        let runner = self.client.runner();
        (runner.endpoint().service_capabilities.remote_compaction
            && runner.model().binding.transport.protocol == ProviderWireProtocol::Responses)
            .then_some(RemoteCompaction { runner })
    }
}

/// An available remote compaction operation; unsupported adapters never implement it.
#[derive(Debug, Clone, Copy)]
pub struct RemoteCompaction<'a> {
    runner: &'a InvocationRunner,
}

impl RemoteCompaction<'_> {
    /// Compacts provider context using the declared native protocol.
    /// # Errors
    /// Returns transport or compaction protocol failures.
    pub async fn complete(
        &self,
        request: ModelCompactionRequest,
        context: ModelInvocationContext,
    ) -> std::result::Result<ModelCompactionResponse, CompletionFailure> {
        super::compaction::compact_context(self.runner, request, context).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::default_models;
    use crate::provider::ProviderClient;
    use crate::runtime::test_support::{serve_sse_checked, serve_sse_once};
    use pl_protocol::{PricingOutcome, UsageStatus};
    use pretty_assertions::assert_eq;
    use std::sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    };

    #[derive(Debug)]
    struct Clock(AtomicI64);
    impl crate::runtime::InferenceClock for Clock {
        fn unix_seconds(&self) -> Result<i64> {
            Ok(self.0.load(Ordering::SeqCst))
        }
    }

    fn model(slug: &str) -> ModelInfo {
        let mut model = default_models()
            .into_iter()
            .find(|model| model.slug == slug)
            .expect("published model");
        model.binding.transport.default_connection_mode =
            crate::provider::ProviderConnectionMode::Http;
        model
    }

    fn invocation() -> ModelInvocationContext {
        let (tx, _) = tokio::sync::broadcast::channel(32);
        ModelInvocationContext::new(Default::default()).with_events(tx)
    }

    fn response_sse(protocol: ProviderWireProtocol, usage: serde_json::Value) -> String {
        let events = match protocol {
            ProviderWireProtocol::Responses => vec![
                serde_json::json!({"type":"response.output_text.delta","item_id":"answer","delta":"done"}),
                serde_json::json!({"type":"response.completed","response":{"id":"result","usage":usage}}),
            ],
            ProviderWireProtocol::ChatCompletions => vec![
                serde_json::json!({"choices":[{"delta":{"content":"done"},"finish_reason":null}]}),
                serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
                serde_json::json!({"choices":[],"usage":usage}),
            ],
        };
        events
            .into_iter()
            .map(|event| format!("data: {event}\n\n"))
            .collect::<String>()
            + "data: [DONE]\n\n"
    }

    #[tokio::test]
    async fn completed_requests_freeze_official_tariffs_and_consume_final_usage() {
        // Full inference scenarios: selection happens before the fake provider advances wall time.
        let cases = [
            (
                "deepseek-v4-flash",
                1_788_483_599,
                1000,
                200,
                400,
                0,
                0.00182,
            ),
            (
                "deepseek-v4-flash",
                1_788_483_600,
                1000,
                200,
                400,
                0,
                0.00364,
            ),
            (
                "deepseek-v4-flash",
                1_788_494_400,
                1000,
                200,
                400,
                0,
                0.00182,
            ),
            (
                "deepseek-v4-flash-vision-exp",
                1_788_501_600,
                1000,
                200,
                400,
                0,
                0.00364,
            ),
            (
                "deepseek-v4-flash",
                1_788_516_000,
                1000,
                200,
                400,
                0,
                0.00182,
            ),
            ("deepseek-v4-pro", 1_788_570_000, 1000, 200, 400, 0, 0.00546),
            (
                "gpt-6-astra",
                1_788_483_600,
                272000,
                1000,
                100000,
                20000,
                1.92,
            ),
            (
                "gpt-6-astra",
                1_788_483_600,
                272001,
                1000,
                100000,
                20000,
                3.81502,
            ),
            ("glm-4.7", 1_788_483_600, 10000, 199, 4000, 0, 0.015192),
            ("glm-4.7", 1_788_483_600, 10000, 200, 4000, 0, 0.0232),
            ("glm-4.7", 1_788_483_600, 32000, 199, 4000, 0, 0.118384),
            ("mimo-v2.5", 1_788_483_600, 1000, 200, 400, 0, 0.001008),
            ("glm-4.7-flash", 1_788_483_600, 1000, 200, 400, 0, 0.0),
        ];
        for (slug, started, input, output, read, write, expected) in cases {
            let model = model(slug);
            let protocol = model.binding.transport.protocol;
            let usage = match protocol {
                ProviderWireProtocol::Responses => {
                    serde_json::json!({"input_tokens":input,"output_tokens":output,"total_tokens":input+output,"input_tokens_details":{"cached_tokens":read,"cache_write_tokens":write},"output_tokens_details":{"reasoning_tokens":100}})
                }
                ProviderWireProtocol::ChatCompletions => {
                    serde_json::json!({"prompt_tokens":input,"completion_tokens":output,"total_tokens":input+output,"prompt_tokens_details":{"cached_tokens":read},"completion_tokens_details":{"reasoning_tokens":100}})
                }
            };
            let clock = Arc::new(Clock(AtomicI64::new(started)));
            let advancing_clock = clock.clone();
            let (url, server) = serve_sse_checked(response_sse(protocol, usage), move |_| {
                advancing_clock.0.store(1_788_570_000, Ordering::SeqCst);
                true
            })
            .await;
            let runtime = ModelRuntime::new(ProviderEndpoint::compatible("fixture", url), model)
                .unwrap()
                .with_clock(clock);
            let result = runtime
                .complete(
                    CompletionRequest::builder()
                        .instructions("Finish this task with done")
                        .build(),
                    invocation(),
                )
                .await
                .expect(slug);
            server.await.unwrap();
            assert_eq!(result.content.as_deref(), Some("done"), "{slug}");
            assert_eq!(
                result.accounting.usage.status(),
                UsageStatus::Reported,
                "{slug}"
            );
            assert_eq!(result.accounting.request_started_at, Some(started));
            assert_eq!(
                result.accounting.usage.totals().total_tokens,
                input + output
            );
            let PricingOutcome::Estimated { cost, .. } = result.accounting.pricing else {
                panic!("{slug} was not priced");
            };
            assert!(
                (cost.amount - expected).abs() < 1e-12,
                "{slug}: {} != {expected}",
                cost.amount
            );
        }
    }

    #[tokio::test]
    async fn native_openai_cache_control_runs_through_the_same_accounted_completion() {
        use crate::provider::openai::{
            CacheMode, OpenAiCompletion, OpenAiCompletionOptions, PromptCacheOptions,
        };
        let usage = serde_json::json!({"input_tokens":100,"output_tokens":10,"total_tokens":110,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0}});
        let (url, server) = serve_sse_checked(
            response_sse(ProviderWireProtocol::Responses, usage),
            |request| request.body["prompt_cache_options"]["mode"] == "explicit",
        )
        .await;
        let runtime =
            ModelRuntime::new(ProviderEndpoint::openai(Some(url)), model("gpt-6-astra")).unwrap();
        let ProviderClient::OpenAi(client) = runtime.provider() else {
            panic!("native OpenAI client was erased");
        };
        let result = client
            .complete(
                OpenAiCompletion {
                    request: CompletionRequest::builder()
                        .instructions("Finish this task")
                        .build(),
                    options: OpenAiCompletionOptions {
                        prompt_cache_options: Some(PromptCacheOptions {
                            mode: CacheMode::Explicit,
                            ttl: None,
                        }),
                    },
                },
                invocation(),
            )
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(result.content.as_deref(), Some("done"));
        assert!(matches!(
            result.accounting.pricing,
            PricingOutcome::Estimated { .. }
        ));
    }

    #[tokio::test]
    async fn failed_completion_and_disabled_pricing_retain_reported_usage() {
        let usage = serde_json::json!({"input_tokens":1000,"output_tokens":200,"total_tokens":1200,"input_tokens_details":{"cached_tokens":400}});
        let terminal = serde_json::json!({"type":"response.incomplete","response":{"usage":usage,"incomplete_details":{"reason":"max_output_tokens"}}});
        let (url, server) = serve_sse_once(format!("data: {terminal}\n\n")).await;
        let runtime = ModelRuntime::new(
            ProviderEndpoint::deepseek(Some(url)),
            model("deepseek-v4-flash"),
        )
        .unwrap()
        .with_pricing_mode(PricingMode::Disabled);
        let error = runtime
            .complete(
                CompletionRequest::builder()
                    .instructions("Complete task")
                    .build(),
                invocation(),
            )
            .await
            .unwrap_err();
        server.await.unwrap();
        assert_eq!(error.accounting.usage.status(), UsageStatus::Reported);
        assert_eq!(error.accounting.usage.input_tokens, Some(1000));
        assert_eq!(error.accounting.pricing, PricingOutcome::Disabled);
    }
}
