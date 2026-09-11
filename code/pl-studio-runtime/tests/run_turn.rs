//! Real provider adapter + Studio assembly + core execution roundtrip.
#[path = "support/actor.rs"]
mod actor;
#[path = "support/engine.rs"]
mod fixture;
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::{TurnInput, TurnOutcome},
    tool::{
        ToolOutput,
        opaque::{CallContext, Registration, Tool, ToolError},
    },
};
use pl_model::{
    completion::ToolSpec,
    runtime::{model_response_receipt, thread_tool_declaration},
};
use pl_studio_runtime::thread_assembler::StudioThreadAssembler;
use pretty_assertions::assert_eq;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
struct Counter(Arc<AtomicUsize>);
impl Tool for Counter {
    async fn execute(&self, input: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
        assert_eq!(input.format(), "application/json");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(input.content()).unwrap(),
            serde_json::json!({})
        );
        let value = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(ToolOutput::new(
            OpaquePayload::text(value.to_string()),
            vec![ContextContent::Text {
                text: value.to_string().into(),
            }],
        ))
    }
}

#[tokio::test]
async fn assembled_turn_executes_tools_once_preserves_full_history_and_retains_each_usage_receipt()
{
    let responses = vec![
        sse(
            serde_json::json!({"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"counter","arguments":"{}"}}]}),
            "tool_calls",
            10,
        ),
        sse(serde_json::json!({"content":"observed 1"}), "stop", 20),
    ];
    let (url, server) = fixture::serve_checked_sse_sequence(responses, |step, body| {
        let tools = body["tools"].as_array().unwrap();
        tools.len() == 1
            && tools[0]["function"]["name"] == "counter"
            && (step == 0
                || (body.to_string().contains("call-1")
                    && body["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|message| message["role"] == "tool" && message["content"] == "1")))
    })
    .await;
    let route = route(url, "fixture-model");
    let root = tempfile::tempdir().unwrap();
    let owner = StudioThreadAssembler::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut spec = actor::specification("roundtrip", route, root.path());
    spec.tools.push(
        Registration::new(
            "counter".into(),
            thread_tool_declaration(&ToolSpec::function(
                "counter",
                "Return a count",
                serde_json::json!({"type":"object","properties":{}}),
            ))
            .unwrap(),
            Counter(calls.clone()),
        )
        .unwrap(),
    );
    let thread = owner.assemble(spec).await.unwrap();
    let result = thread.run_turn(input("first")).await.unwrap();
    assert_eq!(result.outcome, TurnOutcome::Completed);
    assert_eq!(result.model_steps, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        result.last_output.content[0],
        ContextContent::Text {
            text: "observed 1".into()
        }
    );
    let snapshot = thread.snapshot();
    let usage = snapshot
        .attempts
        .iter()
        .map(|attempt| match &attempt.outcome {
            pl_core::thread::AttemptOutcome::Committed(output) => {
                model_response_receipt(output)
                    .unwrap()
                    .unwrap()
                    .response
                    .accounting
                    .usage
                    .input_tokens
            }
            outcome => panic!("unexpected attempt outcome {outcome:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(usage, vec![Some(10), Some(20)]);
    assert_eq!(snapshot.deliveries.len(), 1);
    let replay = pl_core::thread::journal::replay(&thread.journal().await.unwrap()).unwrap();
    assert_eq!(replay.context, snapshot.context);
    owner.close("roundtrip").await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn route_replacement_changes_frozen_model_parameters_without_rewriting_prior_receipts() {
    let (url, server) = fixture::serve_checked_sse_sequence(
        vec![
            sse(serde_json::json!({"content":"first"}), "stop", 7),
            sse(serde_json::json!({"content":"second"}), "stop", 9),
        ],
        |index, body| body["model"] == if index == 0 { "old-model" } else { "new-model" },
    )
    .await;
    let root = tempfile::tempdir().unwrap();
    let owner = StudioThreadAssembler::default();
    let thread = owner
        .assemble(actor::specification(
            "refresh",
            route(url.clone(), "old-model"),
            root.path(),
        ))
        .await
        .unwrap();
    let old = thread.run_turn(input("first")).await.unwrap();
    owner
        .replace_model("refresh", &route(url, "new-model"), vec![], None)
        .await
        .unwrap();
    let new = thread.run_turn(input("second")).await.unwrap();
    assert_eq!(
        model_response_receipt(&old.last_output)
            .unwrap()
            .unwrap()
            .binding
            .requested_model,
        "old-model"
    );
    assert_eq!(
        model_response_receipt(&new.last_output)
            .unwrap()
            .unwrap()
            .binding
            .requested_model,
        "new-model"
    );
    assert!(
        thread.snapshot().attempts[1]
            .input
            .records
            .iter()
            .any(|record| record.turn_id.as_deref() == Some("first"))
    );
    owner.close("refresh").await.unwrap();
    server.await.unwrap();
}

fn route(url: String, model: &str) -> pl_model::config::ResolvedModelRoute {
    fixture::route(
        "fixture",
        pl_model::provider::ProviderEndpoint::deepseek(Some(url)),
        pl_model::model::ModelInfo::compatible(model),
        None,
    )
}
fn input(turn: &str) -> TurnInput {
    TurnInput {
        turn_id: turn.into(),
        attempt_prefix: format!("{turn}-attempt"),
        content: vec![ContextContent::Text {
            text: "Use the counter then answer".into(),
        }],
        max_model_steps: std::num::NonZeroU32::new(4).unwrap(),
        cancellation: Default::default(),
    }
}
fn sse(delta: serde_json::Value, finish: &str, input: u64) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({"choices":[{"delta":delta,"finish_reason":finish}],"usage":{"prompt_tokens":input,"completion_tokens":2,"total_tokens":input+2}})
    )
}

#[tokio::test]
async fn provider_tool_tasks_preserve_native_optimizations_and_account_for_the_complete_turn() {
    use pl_model::provider::{ProviderAdapterKind, ProviderConnectionMode, ProviderWireProtocol};
    let cases = [
        (
            "gpt-6-astra",
            ProviderAdapterKind::OpenAi,
            Some("low"),
            Some(0.0328),
        ),
        (
            "deepseek-flash",
            ProviderAdapterKind::DeepSeek,
            Some("high"),
            Some(0.005632),
        ),
        (
            "glm-5.3",
            ProviderAdapterKind::Zhipu,
            Some("low"),
            Some(0.0224),
        ),
        (
            "mimo-v2.5-pro",
            ProviderAdapterKind::MiMo,
            Some("enabled"),
            Some(0.00602),
        ),
        (
            "custom-model",
            ProviderAdapterKind::OpenAiCompatible,
            None,
            None,
        ),
    ];
    for (slug, adapter, effort, expected_cost) in cases {
        let mut model = pl_model::model::default_models()
            .into_iter()
            .find(|model| model.slug == slug)
            .unwrap_or_else(|| pl_model::model::ModelInfo::compatible(slug));
        model.binding.transport.default_connection_mode = ProviderConnectionMode::Http;
        let protocol = model.binding.transport.protocol;
        let responses = [false, true].into_iter().map(|final_answer| {
            let usage = match protocol {
                ProviderWireProtocol::Responses => serde_json::json!({"input_tokens":1000,"output_tokens":200,"total_tokens":1200,"input_tokens_details":{"cached_tokens":400,"cache_write_tokens":0}}),
                ProviderWireProtocol::ChatCompletions => serde_json::json!({"prompt_tokens":1000,"completion_tokens":200,"total_tokens":1200,"prompt_tokens_details":{"cached_tokens":400}}),
            };
            let mut events = match (protocol, final_answer) {
                (ProviderWireProtocol::Responses, false) => vec![
                    serde_json::json!({"type":"response.output_item.added","item":{"id":"tool-item","type":"function_call","call_id":"native-call","name":"double"}}),
                    serde_json::json!({"type":"response.function_call_arguments.delta","item_id":"tool-item","delta":"{\"value\":2}"}),
                    serde_json::json!({"type":"response.output_item.done","item":{"id":"tool-item","type":"function_call","call_id":"native-call","name":"double","arguments":"{\"value\":2}"}}),
                ],
                (ProviderWireProtocol::Responses, true) => vec![serde_json::json!({"type":"response.output_text.delta","item_id":"answer","delta":"4"})],
                (ProviderWireProtocol::ChatCompletions, false) => vec![
                    serde_json::json!({"choices":[{"delta":{"reasoning_content":"calculate double","tool_calls":[{"index":0,"id":"native-call","type":"function","function":{"name":"double","arguments":"{\"value\":2}"}}]},"finish_reason":null}]}),
                    serde_json::json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
                ],
                (ProviderWireProtocol::ChatCompletions, true) => vec![serde_json::json!({"choices":[{"delta":{"content":"4"},"finish_reason":"stop"}]})],
            };
            events.push(match protocol {
                ProviderWireProtocol::Responses => serde_json::json!({"type":"response.completed","response":{"id":if final_answer {"final"} else {"tool"},"usage":usage}}),
                ProviderWireProtocol::ChatCompletions => serde_json::json!({"choices":[],"usage":usage}),
            });
            events.into_iter().map(|event| format!("data: {event}\n\n")).collect::<String>() + "data: [DONE]\n\n"
        }).collect();
        let (url, server) = fixture::serve_checked_sse_sequence(responses, move |step, body| {
            if adapter == ProviderAdapterKind::Zhipu
                && (body["tool_stream"] != true || body["thinking"]["clear_thinking"] != false)
            {
                return false;
            }
            if adapter == ProviderAdapterKind::MiMo && body["thinking"]["type"] != "enabled" {
                return false;
            }
            if step == 0 {
                return body["tools"].as_array().is_some_and(|tools| {
                    tools.iter().any(|tool| {
                        tool["name"] == "double" || tool["function"]["name"] == "double"
                    })
                });
            }
            match protocol {
                ProviderWireProtocol::Responses => body["input"].as_array().is_some_and(|items| {
                    items.iter().any(|item| {
                        item["type"] == "function_call_output"
                            && item["call_id"] == "native-call"
                            && item["output"] == "4"
                    })
                }),
                ProviderWireProtocol::ChatCompletions => {
                    body["messages"].as_array().is_some_and(|messages| {
                        messages.iter().any(|message| {
                            message["role"] == "tool"
                                && message["tool_call_id"] == "native-call"
                                && message["content"] == "4"
                        }) && messages.iter().any(|message| {
                            message["role"] == "assistant"
                                && message["reasoning_content"] == "calculate double"
                        })
                    })
                }
            }
        })
        .await;
        let endpoint = pl_model::provider::ProviderEndpoint::compatible("task fixture", url)
            .with_adapter(adapter);
        let mut route = fixture::route("fixture", endpoint, model, effort);
        if expected_cost.is_none() {
            route.pricing_mode = pl_protocol::PricingMode::Disabled;
        }
        use pl_core::model::Model;
        let runtime = pl_model::runtime::ModelRuntime::from_route(&route)
            .unwrap()
            .with_clock(Arc::new(TariffClock));
        let model = pl_model::runtime::ThreadModel::new(runtime, route.reasoning_config());
        let thread = pl_core::thread::ThreadHandle::start(
            format!("native-{slug}"),
            model.open_session().await.unwrap(),
        )
        .unwrap();
        thread.register_tools(vec![Registration::new("double".into(), thread_tool_declaration(&ToolSpec::function("double", "Double an integer", serde_json::json!({"type":"object","properties":{"value":{"type":"integer"}},"required":["value"]}))).unwrap(), Double).unwrap()]).await.unwrap();
        let result = thread.run_turn(input("native")).await.unwrap();
        server.await.expect("strict provider fixture");
        assert_eq!(result.outcome, TurnOutcome::Completed);
        assert_eq!(
            result.last_output.content[0],
            ContextContent::Text { text: "4".into() }
        );
        let snapshot = thread.snapshot();
        let receipts = snapshot
            .attempts
            .iter()
            .map(|attempt| match &attempt.outcome {
                pl_core::thread::AttemptOutcome::Committed(output) => {
                    model_response_receipt(output).unwrap().unwrap()
                }
                outcome => panic!("unexpected native attempt: {outcome:?}"),
            })
            .collect::<Vec<_>>();
        let tokens = receipts
            .iter()
            .map(|receipt| receipt.response.accounting.usage.totals().total_tokens)
            .sum::<u64>();
        assert_eq!(tokens, 2400, "usage includes both model steps");
        let costs = receipts
            .iter()
            .flat_map(|receipt| receipt.response.accounting.estimated_costs())
            .collect::<Vec<_>>();
        match expected_cost {
            Some(expected) => assert!(
                (costs.iter().map(|cost| cost.amount).sum::<f64>() - expected).abs() < 1e-12,
                "{slug}: {costs:?}"
            ),
            None => assert!(costs.is_empty()),
        }
        thread.close().await.unwrap();
    }
}

#[derive(Debug)]
struct Double;
impl Tool for Double {
    async fn execute(&self, input: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
        #[derive(serde::Deserialize)]
        struct Input {
            value: i64,
        }
        let value: Input = serde_json::from_str(input.content()).map_err(ToolError::new)?;
        let output = (value.value * 2).to_string();
        Ok(ToolOutput::new(
            OpaquePayload::text(output.clone()),
            vec![ContextContent::Text {
                text: output.into(),
            }],
        ))
    }
}
#[derive(Debug)]
struct TariffClock;
impl pl_model::runtime::InferenceClock for TariffClock {
    fn unix_seconds(&self) -> pl_protocol::Result<i64> {
        Ok(1_788_483_600)
    }
}
