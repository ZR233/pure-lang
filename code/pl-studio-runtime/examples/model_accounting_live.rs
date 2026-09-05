//! Opt-in live acceptance using existing Studio connection settings without modifying them.
//! Run with PURE_STUDIO_WIRE_CAPTURE_DIR and an optional report path argument.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use pl_core::tool::{
    GlobalToolInheritance, StaticToolDefinition, ToolGroupId, ToolInstallGroup, ToolManager,
    ToolName, ToolPolicy, ToolResult,
};
use pl_core::{
    AgentSession, CoreRuntimeProfile, ProviderId, ReasoningEffort, ResolvedModelRoute,
    TraceRecorder, TurnEngineBuilder, TurnOptions, TurnRequest,
};
use pl_model::completion::{
    CompletionFailure, CompletionRequest, CompletionResponse, ReasoningConfig,
};
use pl_model::model::{ModelInfo, ModelProtocolOptions, ResponsesMaxTokensField};
use pl_model::provider::{ProviderConnectionMode, ProviderEndpoint};
use pl_model::runtime::{ModelInvocationContext, ModelRuntime, ModelSession};
use pl_protocol::{
    AgentRoleId, InferenceAccounting, Message, MessageContent, MessageRole, PricingMode,
    UsageStatus,
};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct ExistingConfig {
    models: ExistingModels,
}
#[derive(Deserialize)]
struct ExistingModels {
    providers: BTreeMap<String, ExistingConnection>,
}
#[derive(Deserialize)]
struct ExistingConnection {
    preset: Option<String>,
    base_url: String,
    bearer_token_env: Option<String>,
    pricing_mode: Option<PricingMode>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    provider: String,
    endpoint: String,
    model: String,
    scenario: String,
    status: String,
    response_id: Option<String>,
    accounting: Option<InferenceAccounting>,
    detail: Option<String>,
    inferences: Vec<pl_protocol::InferenceBillingRecord>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DoubleInput {
    value: i64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let tools_only = std::env::args().any(|argument| argument == "--tools");
    let compatible_only = std::env::args().any(|argument| argument == "--compatible");
    let native_only = compatible_only || std::env::args().any(|argument| argument == "--native");
    let report_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/model-accounting-live/report.json"));
    let config_path = PathBuf::from(std::env::var_os("HOME").context("HOME is unavailable")?)
        .join(".pure/config.toml");
    let config: ExistingConfig = toml::from_str(&tokio::fs::read_to_string(config_path).await?)?;
    let registry = pl_core::builtin_provider_catalog();
    let mut observations = Vec::new();
    for (provider_id, connection) in config.models.providers {
        let Some(preset) = registry
            .presets
            .iter()
            .find(|preset| Some(preset.id.as_str()) == connection.preset.as_deref())
        else {
            continue;
        };
        let credential = credential(
            &provider_id,
            connection
                .bearer_token_env
                .as_deref()
                .or(preset.credential_env.as_deref()),
        )
        .await;
        let mut provider = preset.provider.clone();
        provider.base_url = connection.base_url.clone();
        provider.bearer_token = credential;
        provider.pricing_mode = connection.pricing_mode.unwrap_or(provider.pricing_mode);
        if provider.resolved_bearer_token().is_none() {
            observations.push(observation(
                &provider_id,
                &connection.base_url,
                "",
                "connection",
                "missingCredential",
                None,
                Some("No available environment or stored credential".into()),
            ));
            persist(&report_path, &observations).await?;
            continue;
        }
        let models = provider.effective_models()?;
        if native_only {
            if provider_id == "openai" {
                let endpoint = ProviderEndpoint::compatible(
                    "OpenAI API compatible",
                    provider.base_url.clone(),
                );
                let mut endpoint = endpoint;
                endpoint.bearer_token = provider.resolved_bearer_token();
                let mut model = ModelInfo::compatible("gpt-5.6-sol");
                model
                    .binding
                    .set_transport(pl_model::model::ModelTransportProfile::responses_http());
                let result = tool_task(&provider_id, endpoint, model, PricingMode::Disabled).await;
                observations.push(tool_observation(
                    &provider_id,
                    &connection.base_url,
                    "gpt-5.6-sol",
                    "compatible-responses-tool-roundtrip",
                    result,
                ));
            } else if provider_id == "deepseek" && compatible_only {
                let mut endpoint =
                    ProviderEndpoint::compatible("compatible Chat", provider.base_url.clone());
                endpoint.bearer_token = provider.resolved_bearer_token();
                let result = tool_task(
                    &provider_id,
                    endpoint,
                    ModelInfo::compatible("deepseek-v4-flash"),
                    PricingMode::Disabled,
                )
                .await;
                observations.push(tool_observation(
                    &provider_id,
                    &connection.base_url,
                    "deepseek-v4-flash",
                    "compatible-chat-tool-roundtrip",
                    result,
                ));
            } else if provider_id == "deepseek" {
                let mut model = models
                    .iter()
                    .find(|model| model.slug == "deepseek-v4-flash")
                    .context("DeepSeek Flash")?
                    .clone();
                constrain_output(&mut model);
                let runtime = ModelRuntime::new_with_provider_id(
                    &provider_id,
                    provider.to_endpoint()?,
                    model,
                )?
                .with_pricing_mode(provider.pricing_mode);
                match native_search_task(&runtime).await {
                    Ok(responses) => {
                        for (index, response) in responses.into_iter().enumerate() {
                            observations.push(from_result(
                                &provider_id,
                                &connection.base_url,
                                "deepseek-v4-flash",
                                &format!("native-search-{}", index + 1),
                                Ok(response),
                            ));
                        }
                    }
                    Err(error) => observations.push(observation(
                        &provider_id,
                        &connection.base_url,
                        "deepseek-v4-flash",
                        "native-search",
                        "failed",
                        None,
                        Some(error.to_string()),
                    )),
                }
            }
            persist(&report_path, &observations).await?;
            continue;
        }

        if !tools_only {
            for mut model in models.clone() {
                let slug = model.slug.clone();
                constrain_output(&mut model);
                let runtime = ModelRuntime::new_with_provider_id(
                    &provider_id,
                    provider.to_endpoint()?,
                    model.clone(),
                )?
                .with_pricing_mode(provider.pricing_mode);
                let result = invoke(
                    &runtime,
                    &model,
                    "Reply with exactly OK.",
                    ModelSession::default(),
                )
                .await;
                observations.push(from_result(
                    &provider_id,
                    &connection.base_url,
                    &slug,
                    "smoke",
                    result,
                ));
                persist(&report_path, &observations).await?;
            }
        }
        let mut model = models
            .iter()
            .find(|model| model.slug == preset.suggested_model)
            .or(models.first())
            .context("empty configured catalog")?
            .clone();
        if provider_id == "openai" {
            model = models
                .iter()
                .find(|model| model.slug == "gpt-6-astra")
                .unwrap_or(&model)
                .clone();
        }
        constrain_output(&mut model);
        // Use full HTTP replay for cache evidence; WS transport is exercised by the normal smoke.
        model.binding.transport.default_connection_mode = ProviderConnectionMode::Http;
        let runtime = ModelRuntime::new_with_provider_id(
            &provider_id,
            provider.to_endpoint()?,
            model.clone(),
        )?
        .with_pricing_mode(provider.pricing_mode);
        let prompt = format!("{}\nReply with exactly OK.", "Public cache acceptance reference: stable prefixes can be reused across identical model requests. ".repeat(240));
        if !tools_only {
            for attempt in 1..=3 {
                let result = invoke(&runtime, &model, &prompt, ModelSession::default()).await;
                let hit = result
                    .as_ref()
                    .ok()
                    .and_then(|response| response.accounting.usage.cache_read_tokens)
                    .is_some_and(|read| read > 0);
                let mut record = from_result(
                    &provider_id,
                    &connection.base_url,
                    &model.slug,
                    &format!("cache-{attempt}"),
                    result,
                );
                if record.status == "passed" {
                    record.status = if hit { "cacheHit" } else { "cacheNotObserved" }.into();
                }
                observations.push(record);
                persist(&report_path, &observations).await?;
                if hit {
                    break;
                }
            }
        }
        let result = tool_task(
            &provider_id,
            provider.to_endpoint()?,
            model.clone(),
            provider.pricing_mode,
        )
        .await;
        observations.push(tool_observation(
            &provider_id,
            &connection.base_url,
            &model.slug,
            "tool-roundtrip",
            result,
        ));
        persist(&report_path, &observations).await?;
    }
    if !native_only
        && !observations
            .iter()
            .any(|row| row.provider.starts_with("mimo"))
    {
        observations.push(observation(
            "mimo",
            "https://api.xiaomimimo.com/v1",
            "mimo-v2.5-pro",
            "connection",
            "notConfigured",
            None,
            Some("MiMo is not configured; no call was made".into()),
        ));
    }
    persist(&report_path, &observations).await?;
    if observations.iter().any(|row| {
        matches!(
            row.status.as_str(),
            "failed" | "missingCredential" | "usageIncomplete" | "notConfigured"
        ) || (row.scenario == "cache-3" && row.status == "cacheNotObserved")
    }) {
        anyhow::bail!(
            "live acceptance has incomplete scenarios; see {}",
            report_path.display()
        );
    }
    Ok(())
}

async fn credential(provider: &str, environment: Option<&str>) -> Option<String> {
    let provider = provider.to_owned();
    let stored = tokio::task::spawn_blocking(move || {
        keyring::Entry::new("pure-studio", &format!("provider:{provider}"))
            .and_then(|entry| entry.get_password())
    })
    .await
    .ok()
    .and_then(Result::ok);
    stored.filter(|value| !value.trim().is_empty()).or_else(|| {
        environment
            .and_then(|key| std::env::var(key).ok())
            .filter(|value| !value.trim().is_empty())
    })
}

fn constrain_output(model: &mut ModelInfo) {
    if let ModelProtocolOptions::Responses(options) = &mut model.binding.request.protocol {
        options.max_tokens_field = ResponsesMaxTokensField::MaxOutputTokens;
    }
}

async fn invoke(
    runtime: &ModelRuntime,
    model: &ModelInfo,
    prompt: &str,
    session: ModelSession,
) -> std::result::Result<CompletionResponse, CompletionFailure> {
    let (tx, _) = tokio::sync::broadcast::channel(256);
    let cancellation = tokio_util::sync::CancellationToken::new();
    let reasoning = model.default_effort().map(|effort| ReasoningConfig {
        effort: Some(effort),
        summary: None,
    });
    let request = CompletionRequest::builder()
        .instructions("This is a brief API acceptance check. Follow the user exactly.")
        .messages(vec![Message {
            presentation: Default::default(),
            role: MessageRole::User,
            content: MessageContent::text(prompt),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: Default::default(),
        }])
        .reasoning(reasoning)
        .max_tokens(1024)
        .build();
    let future = runtime.complete(
        request,
        ModelInvocationContext::new(session)
            .with_events(tx)
            .with_cancellation(Some(cancellation.clone())),
    );
    tokio::pin!(future);
    match tokio::time::timeout(Duration::from_secs(60), &mut future).await {
        Ok(result) => result,
        Err(_) => {
            cancellation.cancel();
            future.await
        }
    }
}

async fn tool_task(
    provider_id: &str,
    endpoint: ProviderEndpoint,
    model: ModelInfo,
    pricing_mode: PricingMode,
) -> Result<pl_protocol::TurnBillingRecord> {
    let effort = model.default_effort().map(ReasoningEffort::new);
    let route = ResolvedModelRoute {
        pricing_mode,
        role: AgentRoleId::new("live-acceptance")?,
        provider_id: ProviderId::new(provider_id)?,
        endpoint,
        model,
        effort,
    };
    let manager = ToolManager::new();
    let tools = manager.agent_tool_set("live-check", GlobalToolInheritance::Isolated);
    let observed = Arc::new(Mutex::new(None));
    let output = observed.clone();
    let tool = pl_core::tool::static_tool::<DoubleInput>(StaticToolDefinition::new(
        ToolName::bare("verify_double")?,
        "Double an integer. Use this to verify the requested result.",
    ))
    .policy(ToolPolicy::read_only())
    .build(move |input, _| {
        let output = output.clone();
        async move {
            *output.lock().expect("acceptance observation") = Some(input.value);
            Ok(ToolResult::success((input.value * 2).to_string()))
        }
    });
    tools.install(ToolInstallGroup::direct(
        ToolGroupId::new("live-math"),
        vec![tool],
    ))?;
    let engine = TurnEngineBuilder::from_route(&route)?
        .with_agent_tool_set(tools)
        .with_runtime_profile(CoreRuntimeProfile::minimal())
        .build();
    let (tx, _) = tokio::sync::broadcast::channel(512);
    let mut recorder = TraceRecorder::new(format!("live-{provider_id}"), tx, 0);
    let mut session = AgentSession::new();
    let cancellation = tokio_util::sync::CancellationToken::new();
    let operation = engine.run_turn_with_trace(&mut session,
        TurnRequest::new("You must first call verify_double with value 21, then reply with its result. Do not calculate it yourself.")
            .with_budget(pl_core::turn::TurnBudget::new(Duration::from_secs(60))),
        &mut recorder, TurnOptions::default().with_cancellation(cancellation.clone()));
    tokio::pin!(operation);
    let result = match tokio::time::timeout(Duration::from_secs(75), &mut operation).await {
        Ok(result) => result?,
        Err(_) => {
            cancellation.cancel();
            operation.await?
        }
    };
    anyhow::ensure!(
        result.is_completed(),
        "tool task did not complete: {:?}",
        result.outcome
    );
    anyhow::ensure!(
        *observed.lock().expect("acceptance observation") == Some(21),
        "the provider did not execute the required tool"
    );
    anyhow::ensure!(
        result.content.contains("42"),
        "the provider did not use the actual tool result"
    );
    Ok(result.billing)
}

async fn native_search_task(runtime: &ModelRuntime) -> Result<Vec<CompletionResponse>> {
    use pl_protocol::{
        HostedWebSearchOptions, ModelContextItem, ResponsesContextItemKind, ToolSpec,
    };
    let message = |text: &str| Message {
        presentation: Default::default(),
        role: MessageRole::User,
        content: MessageContent::text(text),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    };
    let question = message(
        "Use web search to find the official DeepSeek API pricing page. Answer in one short sentence.",
    );
    let first = native_search_call(
        runtime,
        CompletionRequest::builder()
            .messages(vec![question.clone()])
            .tools(vec![ToolSpec::WebSearch {
                options: HostedWebSearchOptions::DeepSeek,
            }])
            .max_tokens(4096)
            .build(),
    )
    .await?;
    anyhow::ensure!(
        first
            .responses_context_items
            .iter()
            .any(|item| item.kind == ResponsesContextItemKind::WebSearchCall),
        "native hosted search was not executed"
    );
    let mut input = vec![ModelContextItem::from(question)];
    input.extend(
        first
            .responses_context_items
            .iter()
            .cloned()
            .map(|item| ModelContextItem::Responses { item }),
    );
    input.push(ModelContextItem::from(Message {
        presentation: Default::default(),
        role: MessageRole::Assistant,
        content: MessageContent::text(first.content.clone().unwrap_or_default()),
        reasoning_content: first.reasoning_content.clone(),
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    }));
    input.push(ModelContextItem::from(message("Restate the previous finding briefly using the existing search results. Do not search again.")));
    let second = native_search_call(
        runtime,
        CompletionRequest::builder()
            .input(input)
            .max_tokens(2048)
            .build(),
    )
    .await?;
    anyhow::ensure!(
        second
            .content
            .as_ref()
            .is_some_and(|content| !content.trim().is_empty()),
        "native search context replay produced no answer"
    );
    Ok(vec![first, second])
}

async fn native_search_call(
    runtime: &ModelRuntime,
    request: CompletionRequest,
) -> Result<CompletionResponse> {
    let pl_model::provider::ProviderClient::DeepSeek(client) = runtime.provider() else {
        anyhow::bail!("native DeepSeek client is unavailable");
    };
    let (tx, _) = tokio::sync::broadcast::channel(512);
    let cancellation = tokio_util::sync::CancellationToken::new();
    let operation = client.complete(
        pl_model::provider::deepseek::DeepSeekCompletion {
            request,
            options: Default::default(),
        },
        ModelInvocationContext::new(Default::default())
            .with_events(tx)
            .with_cancellation(Some(cancellation.clone())),
    );
    tokio::pin!(operation);
    match tokio::time::timeout(Duration::from_secs(60), &mut operation).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            cancellation.cancel();
            Ok(operation.await?)
        }
    }
}

fn from_result(
    provider: &str,
    endpoint: &str,
    model: &str,
    scenario: &str,
    result: std::result::Result<CompletionResponse, CompletionFailure>,
) -> Observation {
    match result {
        Ok(response) => {
            let status = if response.accounting.usage.status() == UsageStatus::Reported
                && response.accounting.usage.cache_read_tokens.is_some()
                && !response.accounting.has_unpriced_usage()
            {
                "passed"
            } else {
                "usageIncomplete"
            };
            let mut row = observation(
                provider,
                endpoint,
                model,
                scenario,
                status,
                Some(response.accounting),
                None,
            );
            row.response_id = response.response_id;
            row
        }
        Err(error) => observation(
            provider,
            endpoint,
            model,
            scenario,
            "failed",
            Some((*error.accounting).clone()),
            Some(error.to_string()),
        ),
    }
}

fn observation(
    provider: &str,
    endpoint: &str,
    model: &str,
    scenario: &str,
    status: &str,
    accounting: Option<InferenceAccounting>,
    detail: Option<String>,
) -> Observation {
    Observation {
        provider: provider.into(),
        endpoint: endpoint.into(),
        model: model.into(),
        scenario: scenario.into(),
        status: status.into(),
        response_id: None,
        inferences: Vec::new(),
        accounting,
        detail,
    }
}

fn tool_observation(
    provider: &str,
    endpoint: &str,
    model: &str,
    scenario: &str,
    result: Result<pl_protocol::TurnBillingRecord>,
) -> Observation {
    match result {
        Ok(billing) => {
            let mut row = observation(provider, endpoint, model, scenario, "passed", None, None);
            row.inferences = billing.inferences;
            row
        }
        Err(error) => observation(
            provider,
            endpoint,
            model,
            scenario,
            "failed",
            None,
            Some(error.to_string()),
        ),
    }
}

async fn persist(path: &Path, observations: &[Observation]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, serde_json::to_vec_pretty(observations)?).await?;
    if let Some(row) = observations.last() {
        println!(
            "{} {} {}: {}",
            row.provider, row.model, row.scenario, row.status
        );
    }
    Ok(())
}
