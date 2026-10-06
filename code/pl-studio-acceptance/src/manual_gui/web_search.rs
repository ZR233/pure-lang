//! Real native GUI web-search + MCP isolation acceptance.
//!
//! One isolated home starts the real GUI/bridge/Studio twice (a `first` and a
//! `restart` lifecycle) against a loopback fixture that serves the OpenAI
//! standalone search API, the DeepSeek native search endpoint and a generic
//! Streamable-HTTP MCP server. A Driver probe opens a session, switches the
//! session model across the three fixture models, exercises each search path,
//! edits and persists the Web Search settings and probes the hot-add/remove of
//! the search tools. The coordinator proves the real wire traffic and the
//! per-session-model tool declarations from the fixture request log.
//!
//! The fixture never answers with a blanket success: every `/v1/responses`
//! request is recorded with the tools it actually declared, and a marker that
//! names a missing tool is recorded as a rejection. Fault runs keep the same
//! journeys but assert the fault is visible instead of the happy-path hits.

use super::context_replay_recovery::wait_driver;
use super::*;
use pl_model::provider::{
    HostedWebSearchDialect, ProviderAdapterKind, ProviderEndpoint, ProviderServiceCapabilities,
    StandaloneWebSearchDialect, WebSearchProviderCapabilities,
};
use pl_studio_runtime::config::{
    McpServerTransport, StudioMcpServerEntry as McpServerConfig, WebSearchContextSize,
    WebSearchLocation, WebSearchMode,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const OPENAI_PROVIDER_ID: &str = "gui-fixture-1-openai";
const DEEPSEEK_PROVIDER_ID: &str = "gui-fixture-2-deepseek";
const ZHIPU_PROVIDER_ID: &str = "gui-fixture-3-zhipu";
const OPENAI_MODEL: &str = "fixture-openai";
const DEEPSEEK_MODEL: &str = "fixture-deepseek";
const ZHIPU_MODEL: &str = "fixture-glm";

/// Provider credential environment variables the coordinator injects into the
/// isolated GUI process. The names are this scenario's own; the values match the
/// fixture's per-endpoint expected keys.
const OPENAI_KEY_ENV: &str = "ANYWORK_FIXTURE_WEB_SEARCH_OPENAI_KEY";
const DEEPSEEK_KEY_ENV: &str = "ANYWORK_FIXTURE_WEB_SEARCH_DEEPSEEK_KEY";
const WRONG_KEY_VALUE: &str = "fixture-wrong-credential";

/// Unique nonce the Driver embeds only in the restart MCP prompt. It lets the
/// coordinator locate that Turn's model request in the fixture log and prove the
/// restart really issued a fresh MCP discover/tools/call, instead of answering
/// from an earlier Turn's identical tool call id.
const RESTART_MCP_NONCE: &str = "ws-restart-mcp";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WebSearchFault {
    None,
    Unstructured,
    WrongSearchKey,
    WrongMcpKey,
    MissingSearchKey,
}

impl WebSearchFault {
    pub(super) fn parse(value: &str) -> Result<Self> {
        match value {
            "unstructured" => Ok(Self::Unstructured),
            "wrong-search-key" => Ok(Self::WrongSearchKey),
            "wrong-mcp-key" => Ok(Self::WrongMcpKey),
            "missing-search-key" => Ok(Self::MissingSearchKey),
            other => bail!("unknown web-search fault: {other}"),
        }
    }

    fn omits_search_credentials(self) -> bool {
        matches!(self, Self::MissingSearchKey)
    }

    fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Unstructured => "unstructured",
            Self::WrongSearchKey => "wrong-search-key",
            Self::WrongMcpKey => "wrong-mcp-key",
            Self::MissingSearchKey => "missing-search-key",
        }
    }
}

fn openai_capabilities() -> ProviderServiceCapabilities {
    ProviderServiceCapabilities {
        web_search: WebSearchProviderCapabilities {
            hosted_responses: false,
            hosted_dialect: HostedWebSearchDialect::OpenAiResponses,
            standalone: Some(StandaloneWebSearchDialect::OpenAiSearchApi),
        },
        ..ProviderServiceCapabilities::default()
    }
}

fn deepseek_capabilities() -> ProviderServiceCapabilities {
    ProviderServiceCapabilities {
        web_search: WebSearchProviderCapabilities {
            hosted_responses: false,
            hosted_dialect: HostedWebSearchDialect::DeepSeekResponses,
            // B 任务的锁定接口：DeepSeek 原生搜索使用 Anthropic Messages wire。
            standalone: Some(StandaloneWebSearchDialect::DeepSeekAnthropicMessages),
        },
        ..ProviderServiceCapabilities::default()
    }
}

fn build_provider(
    name: &str,
    adapter: ProviderAdapterKind,
    base_url: &str,
    credential_env: Option<&str>,
    model_slug: &str,
    capabilities: ProviderServiceCapabilities,
    model_web_search: bool,
) -> ProviderConfig {
    let mut endpoint = ProviderEndpoint::compatible(name, base_url);
    endpoint.adapter = adapter;
    endpoint.service_capabilities = capabilities;
    let mut model = ModelInfo::compatible(model_slug);
    model.display_name = format!("Fixture {name}");
    model.capabilities.web_search = model_web_search;
    model
        .binding
        .set_transport(ModelTransportProfile::responses_http());
    model.binding.request.api_model = None;
    let mut provider = ProviderConfig::from_explicit_models(endpoint, vec![model]);
    provider.bearer_token_env = credential_env.map(str::to_owned);
    provider
}

/// Writes the isolated config: three fixture session models (OpenAI / DeepSeek /
/// GLM style), a live OpenAI web-search config, the DeepSeek toggle and a user
/// Streamable-HTTP MCP server pointing at the fixture.
pub(super) fn write_config(home: &Path, ready: &FixtureReady, fault: WebSearchFault) -> Result<()> {
    let mut config = StudioConfig::default_config()?;
    let base_url = ready.base_url.clone();
    let search_env = (!fault.omits_search_credentials()).then_some(OPENAI_KEY_ENV);
    let deepseek_env = (!fault.omits_search_credentials()).then_some(DEEPSEEK_KEY_ENV);
    let providers = [
        (
            OPENAI_PROVIDER_ID,
            build_provider(
                "Fixture OpenAI",
                ProviderAdapterKind::OpenAiCompatible,
                &base_url,
                search_env,
                OPENAI_MODEL,
                openai_capabilities(),
                false,
            ),
        ),
        (
            DEEPSEEK_PROVIDER_ID,
            build_provider(
                "Fixture DeepSeek",
                ProviderAdapterKind::DeepSeek,
                &base_url,
                deepseek_env,
                DEEPSEEK_MODEL,
                deepseek_capabilities(),
                true,
            ),
        ),
        (
            ZHIPU_PROVIDER_ID,
            build_provider(
                "Fixture GLM",
                ProviderAdapterKind::Zhipu,
                &base_url,
                None,
                ZHIPU_MODEL,
                ProviderServiceCapabilities::default(),
                false,
            ),
        ),
    ];
    config.models.providers.clear();
    for (id, provider) in providers {
        config
            .models
            .providers
            .insert(ProviderId::new(id)?, provider);
    }
    let openai_id = ProviderId::new(OPENAI_PROVIDER_ID)?;
    for route in config
        .models
        .routes
        .values_mut()
        .chain(config.mode_model_routes.values_mut())
    {
        route.provider = openai_id.clone();
        route.model = OPENAI_MODEL.to_string();
        route.effort = None;
    }
    config.web_search.mode = WebSearchMode::Live;
    config.web_search.context_size = Some(WebSearchContextSize::Medium);
    config.web_search.allowed_domains = vec!["fixture.test".to_string()];
    config.web_search.location = Some(WebSearchLocation {
        country: Some("CN".to_string()),
        region: Some("Shanghai".to_string()),
        city: Some("Shanghai".to_string()),
        timezone: Some("Asia/Shanghai".to_string()),
    });
    config.deepseek_web_search.enabled = true;

    let host = ready
        .base_url
        .strip_suffix("/v1")
        .unwrap_or(&ready.base_url)
        .to_string();
    let mcp_key = match fault {
        WebSearchFault::WrongMcpKey => WRONG_KEY_VALUE,
        _ => pl_provider_fixture::MCP_KEY,
    };
    let mut headers = BTreeMap::new();
    headers.insert("Authorization".to_string(), format!("Bearer {mcp_key}"));
    config.mcp.servers.insert(
        pl_provider_fixture::MCP_SERVER_ID.to_string(),
        McpServerConfig {
            enabled: true,
            transport: McpServerTransport::StreamableHttp,
            url: Some(format!("{host}{}", pl_provider_fixture::MCP_PATH)),
            headers,
            ..McpServerConfig::default()
        },
    );

    config.skills.user_dir = home.join("skills").to_string_lossy().into_owned();
    config.validate()?;
    let file = home.join("config.toml");
    fs::write(&file, toml::to_string_pretty(&config)?)
        .with_context(|| format!("failed to create isolated config: {}", file.display()))
}

fn start_gui(
    workspace: &Path,
    home: &Path,
    log_path: &Path,
    fault: WebSearchFault,
) -> Result<OwnedProcess> {
    let log = File::create(log_path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace)
        .args(["xtask", "run-gui", "--driver"])
        .env("ANYWORK_HOME", home)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    let openai_value = match fault {
        WebSearchFault::WrongSearchKey => WRONG_KEY_VALUE,
        _ => pl_provider_fixture::OPENAI_SEARCH_KEY,
    };
    command
        .env(OPENAI_KEY_ENV, openai_value)
        .env(DEEPSEEK_KEY_ENV, pl_provider_fixture::DEEPSEEK_SEARCH_KEY);
    OwnedProcess::start(&mut command, false)
}

/// Declared tool names on one recorded model request, from the real `tools`
/// array the runtime sent (Responses `name` or chat-style `function.name`).
fn declared_tool_names(request: &Value) -> Vec<String> {
    request
        .pointer("/body/tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    tool.get("name")
                        .and_then(Value::as_str)
                        .or_else(|| tool.pointer("/function/name").and_then(Value::as_str))
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn category(request: &Value) -> Option<&str> {
    request
        .pointer("/diagnostic/category")
        .and_then(Value::as_str)
}

/// Whether the recorded model request carries [needle] anywhere in its body.
///
/// Used only with a scenario-owned nonce (never user content), to locate the
/// restart MCP Turn in the fixture log without depending on request ordering.
fn request_mentions(request: &Value, needle: &str) -> bool {
    request
        .get("body")
        .and_then(|body| serde_json::to_string(body).ok())
        .is_some_and(|body| body.contains(needle))
}

/// Rejection categories a fault run is *expected* to own.
///
/// Every other rejection stays a real failure even in a fault run, so a fault
/// run can never blanket-ignore an unrelated break. `Unstructured` keeps the
/// tool declared and only breaks the response shape, so it owns no rejections.
fn owned_reject_categories(fault: WebSearchFault) -> BTreeSet<&'static str> {
    match fault {
        WebSearchFault::None | WebSearchFault::Unstructured => BTreeSet::new(),
        WebSearchFault::WrongSearchKey => BTreeSet::from(["openai_search_auth_failed"]),
        WebSearchFault::WrongMcpKey => BTreeSet::from(["mcp_auth_failed", "mcp_tool_not_revealed"]),
        WebSearchFault::MissingSearchKey => BTreeSet::from([
            "search_tools_missing",
            "openai_search_tool_not_declared",
            "deepseek_search_tool_not_declared",
        ]),
    }
}

fn sanitized_requests(requests: &[Value]) -> Vec<Value> {
    requests
        .iter()
        .map(|request| {
            json!({
                "method": request.get("method").cloned().unwrap_or(Value::Null),
                "path": request.get("path").cloned().unwrap_or(Value::Null),
                "accepted": request.get("accepted").cloned().unwrap_or(Value::Null),
                // `category` is `Option<&str>`, which `json!` already maps to a
                // JSON string or null; cloning it into `Value` would not type.
                "category": category(request),
                "model": request.pointer("/body/model").cloned().unwrap_or(Value::Null),
                "mcpMethod": request.pointer("/body/method").cloned().unwrap_or(Value::Null),
                // Scenario-owned nonce: marks the restart MCP Turn's requests so
                // the fresh call is visible in the sanitized evidence.
                "restartMcpTurn": request_mentions(request, RESTART_MCP_NONCE),
                "declaredTools": declared_tool_names(request),
            })
        })
        .collect()
}

pub(super) fn run(
    context: &WebSearchScenario<'_>,
    mut fixture: OwnedProcess,
    ready: &FixtureReady,
    fault: WebSearchFault,
) -> Result<()> {
    let project = context.working.join("web-search-project");
    fs::create_dir(&project)?;
    // The fixture child spawns with the unrestricted `explorer` Profile, so it
    // needs no committed Git worktree; a plain `git init` matches the other
    // scenarios and keeps the temp repo a real repository for Git tools.
    ensure!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(&project)
            .status()?
            .success(),
        "failed to initialize isolated web-search project"
    );
    let mut logs = Vec::new();
    let mut lifecycles = Vec::new();
    let mut summaries: Vec<Value> = Vec::new();
    let journey = (|| -> Result<()> {
        for phase in ["first", "restart"] {
            let gui_log = context.working.join(format!("web-search-{phase}-gui.log"));
            let driver_log = context
                .working
                .join(format!("web-search-{phase}-driver.log"));
            logs.push((
                gui_log.clone(),
                context.output.join(format!("{phase}-gui.log")),
            ));
            logs.push((
                driver_log.clone(),
                context.output.join(format!("{phase}-driver.log")),
            ));
            let mut gui = start_gui(context.workspace, context.home, &gui_log, fault)?;
            let vm_url = wait_for_vm(&gui_log, &mut gui, &mut fixture, context.interrupt)?;
            let gui_pid = gui.child.id();
            let log = File::create(&driver_log)?;
            let allow_turn_failure = fault != WebSearchFault::None;
            let mut command = process::path_command("dart", &[]);
            command
                .current_dir(context.app_dir)
                .args(["run", "test_driver/web_search_journey.dart"])
                .arg(format!("--phase={phase}"))
                .arg(format!("--vm={vm_url}"))
                .arg(format!("--project={}", project.display()))
                .arg(format!("--output={}", context.output.display()))
                .arg(format!("--coord={}", context.working.display()))
                .arg(format!("--probe-mode={}", context.probe_mode))
                .arg(format!("--allow-turn-failure={allow_turn_failure}"))
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log))
                .env("ANYWORK_HOME", context.home);
            if let Some(providers) = context.providers {
                command.arg(format!("--provider-ids={providers}"));
            }
            if let Some(models) = context.models {
                command.arg(format!("--models={models}"));
            }
            let mut driver = OwnedProcess::start(&mut command, false)?;
            let driver_pid = driver.child.id();
            wait_driver(&mut driver, &mut gui, &mut fixture, context.interrupt)?;
            let summary: Value = serde_json::from_slice(&fs::read(
                context.output.join(format!("{phase}-summary.json")),
            )?)?;
            ensure!(
                summary["status"] == "complete" && summary["shutdown"] == "completed",
                "web-search {phase} did not finish a normal native shutdown"
            );
            drop(driver);
            drop(gui);
            lifecycles.push(json!({
                "phase": phase,
                "guiLauncherPid": gui_pid,
                "driverLauncherPid": driver_pid,
                "shutdown": "completed",
                "ownersReapedAt": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            }));
            fs::write(
                context.output.join("lifecycle.json"),
                serde_json::to_vec_pretty(&lifecycles)?,
            )?;
            summaries.push(summary);
        }
        let first = summaries.first().context("missing first summary")?;
        let restart = summaries.get(1).context("missing restart summary")?;
        ensure!(
            first["settingsFingerprint"].is_string()
                && first["settingsFingerprint"] == restart["settingsFingerprint"],
            "canonical web-search settings did not survive the restart lifecycle"
        );
        Ok(())
    })();
    for (source, destination) in &logs {
        if source.is_file() {
            write_sanitized_log(source, destination)?;
        }
    }
    let gui_errors = logs
        .iter()
        .filter(|(source, _)| {
            source
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with("gui.log"))
        })
        .map(|(source, _)| {
            if source.is_file() {
                count_gui_errors(source)
            } else {
                Ok(0)
            }
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .sum::<usize>();
    let summary: Option<Value> = fs::read(context.output.join("first-summary.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let fixture_exit = fixture.stop(context.requests_file);
    // Keep the fixture's raw shutdown fact, but do not let an expected fault
    // rejection be reported as an unexplained shutdown failure.
    let fixture_exit_success = fixture_exit
        .as_ref()
        .is_ok_and(std::process::ExitStatus::success);
    drop(fixture);
    write_fixture_log(context.fixture_log, &context.output.join("fixture.log"))?;
    let requests: Vec<Value> = if context.requests_file.is_file() {
        let raw = serde_json::from_slice::<Vec<Value>>(&fs::read(context.requests_file)?)?;
        fs::write(
            context.output.join("web-search-requests.json"),
            serde_json::to_vec_pretty(&sanitized_requests(&raw))?,
        )?;
        raw
    } else {
        Vec::new()
    };
    let accepted = requests
        .iter()
        .filter(|request| request.get("accepted").and_then(Value::as_bool) == Some(true))
        .count();
    let rejected = requests.len() - accepted;
    let rejected_categories: BTreeSet<String> = requests
        .iter()
        .filter(|request| request.get("accepted").and_then(Value::as_bool) == Some(false))
        .filter_map(|request| category(request).map(str::to_owned))
        .collect();
    let owned_rejects = owned_reject_categories(fault);
    let unexpected_rejects: BTreeSet<String> = rejected_categories
        .iter()
        .filter(|name| !owned_rejects.contains(name.as_str()))
        .cloned()
        .collect();
    let accepted_path = |path: &str| {
        requests.iter().any(|request| {
            request.get("accepted").and_then(Value::as_bool) == Some(true)
                && request.get("path").and_then(Value::as_str) == Some(path)
        })
    };
    let openai_search =
        accepted_path(pl_provider_fixture::OPENAI_SEARCH_PATH) || accepted_path("/alpha/search");
    let deepseek_search = accepted_path(pl_provider_fixture::DEEPSEEK_MESSAGES_PATH)
        || accepted_path("/anthropic/v1/messages");
    let mcp_methods: BTreeSet<String> = requests
        .iter()
        .filter(|request| {
            request.get("path").and_then(Value::as_str) == Some(pl_provider_fixture::MCP_PATH)
        })
        .filter_map(|request| {
            request
                .pointer("/body/method")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    let search_tool_models: BTreeSet<String> = requests
        .iter()
        .filter(|request| category(request) == Some("search_tools_present"))
        .filter_map(|request| {
            request
                .pointer("/body/model")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    let search_tools_absent = requests
        .iter()
        .filter(|request| category(request) == Some("search_tools_absent"))
        .count();
    let categories = |name: &str| {
        requests
            .iter()
            .filter(|request| category(request) == Some(name))
            .count()
    };
    let fault_observed = match fault {
        WebSearchFault::None => true,
        WebSearchFault::Unstructured => {
            categories("openai_search_unstructured_fault") > 0
                || categories("deepseek_messages_unstructured_fault") > 0
        }
        WebSearchFault::WrongSearchKey => categories("openai_search_auth_failed") > 0,
        WebSearchFault::WrongMcpKey => categories("mcp_auth_failed") > 0,
        WebSearchFault::MissingSearchKey => {
            categories("search_tools_missing") > 0
                || categories("openai_search_tool_not_declared") > 0
        }
    };
    // Fresh restart evidence: locate the restart MCP Turn by its scenario-owned
    // nonce, then require that this Turn really issued the MCP tool (directly or
    // after a fresh discover) and that a real `/mcp` `tools/call` followed it.
    let restart_mcp_positions: Vec<usize> = requests
        .iter()
        .enumerate()
        .filter(|(_, request)| request_mentions(request, RESTART_MCP_NONCE))
        .map(|(index, _)| index)
        .collect();
    let restart_fresh_issued = restart_mcp_positions
        .iter()
        .any(|index| category(&requests[*index]) == Some("mcp_tool_call_issued"));
    let restart_fresh_discovered = restart_mcp_positions
        .iter()
        .any(|index| category(&requests[*index]) == Some("mcp_discover_issued"));
    let restart_fresh_call = restart_mcp_positions.first().is_some_and(|first| {
        requests.iter().skip(first + 1).any(|request| {
            request.get("path").and_then(Value::as_str) == Some(pl_provider_fixture::MCP_PATH)
                && request.pointer("/body/method").and_then(Value::as_str) == Some("tools/call")
        })
    });
    // Child tool assembly: the Driver records the child Thread it opened and the
    // tools that child really called; the fixture log holds the tools each child
    // model request actually declared. Cross-check that the called rows are a
    // subset of the declared set instead of trusting a fabricated snapshot.
    let child_summary = summary
        .as_ref()
        .and_then(|summary| summary.get("child"))
        .cloned()
        .unwrap_or(Value::Null);
    let child_thread_id = child_summary.get("threadId").and_then(Value::as_str);
    let child_root_thread_id = child_summary.get("rootThreadId").and_then(Value::as_str);
    let child_declared: BTreeSet<String> = requests
        .iter()
        .filter(|request| category(request).is_some_and(|name| name.starts_with("child_search")))
        .flat_map(declared_tool_names)
        .collect();
    let child_tools: BTreeSet<String> = child_summary
        .get("toolNames")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let child_has_search_tools = ["web_search", "deepseek_web_search", "discover_tools"]
        .iter()
        .all(|name| child_tools.contains(*name));
    let child_has_mcp = child_tools.iter().any(|name| name.starts_with("mcp__"));
    let child_tools_declared = !child_tools.is_empty() && child_tools.is_subset(&child_declared);
    let spawn_requested = categories("spawn_child_requested");
    let spawn_wait = categories("spawn_child_wait");
    let auxiliary_title = categories("auxiliary_title_answered");
    let child_tools_present = categories("child_search_tools_present");
    let child_deepseek = categories("child_search_deepseek");
    let child_discover = categories("child_search_discover");
    let child_mcp_present = categories("child_search_mcp_present");
    let summaries_complete = summaries.len() == 2
        && summaries
            .iter()
            .all(|summary| summary["status"] == "complete");
    fs::write(
        context.output.join("collection.json"),
        serde_json::to_vec_pretty(&json!({
            "scenario": ready.scenario,
            "platform": std::env::consts::OS,
            "probeMode": context.probe_mode,
            "fault": fault.label(),
            "guiLifecycles": lifecycles.len(),
            "plannedGuiLifecycles": 2,
            "guiErrors": gui_errors,
            "acceptedRequests": accepted,
            "rejectedRequests": rejected,
            "fixtureExitSuccess": fixture_exit_success,
            "rejectedCategories": rejected_categories,
            "ownedRejectCategories": owned_rejects,
            "unexpectedRejects": unexpected_rejects,
            "restartFreshMcpIssued": restart_fresh_issued,
            "restartFreshMcpDiscovered": restart_fresh_discovered,
            "restartFreshMcpCall": restart_fresh_call,
            "openaiSearchExercised": openai_search,
            "deepseekSearchExercised": deepseek_search,
            "mcpMethods": mcp_methods,
            "searchToolModels": search_tool_models,
            "searchToolsAbsentRequests": search_tools_absent,
            "auxiliaryTitleRequests": auxiliary_title,
            "spawnChildRequests": spawn_requested,
            "childWaitRequests": spawn_wait,
            "childSearchToolsPresentRequests": child_tools_present,
            "childDeepseekRequests": child_deepseek,
            "childDiscoverRequests": child_discover,
            "childMcpPresentRequests": child_mcp_present,
            "childThreadId": child_thread_id,
            "childRootThreadId": child_root_thread_id,
            "childTools": child_tools,
            "childDeclaredTools": child_declared,
            "childToolsDeclared": child_tools_declared,
            "driverSummariesComplete": summaries_complete,
            "driverSettingsFingerprint": summary.as_ref().and_then(|s| s.get("settingsFingerprint").cloned()),
            "journey": if journey.is_ok() { "complete" } else { "failed" },
            "error": journey.as_ref().err().map(ToString::to_string),
            "humanVerdict": "pending",
        }))?,
    )?;
    journey?;
    ensure!(
        summaries_complete,
        "Driver did not complete both web-search lifecycles"
    );
    ensure!(
        gui_errors == 0,
        "GUI reported {gui_errors} unhandled errors"
    );
    if fault != WebSearchFault::None {
        // A fault run keeps the same journeys but only requires that the fault
        // was actually observed, not the happy-path endpoint hits. The fixture's
        // own non-zero shutdown is preserved as a raw fact; it is accepted only
        // when every rejection is a category this fault owns, so an unrelated
        // break still fails.
        ensure!(
            fault_observed,
            "fault {} was not observable in the fixture request log",
            fault.label()
        );
        ensure!(
            unexpected_rejects.is_empty(),
            "fault {} produced unexpected rejections: {unexpected_rejects:?} (owned: {owned_rejects:?})",
            fault.label()
        );
        ensure!(
            fixture_exit_success || rejected > 0,
            "fixture shutdown failed without a rejected request to explain it"
        );
        if fault == WebSearchFault::MissingSearchKey {
            // Missing search credentials must not take the other tools down with
            // them: the MCP path is independent and must still be callable.
            ensure!(
                requests.iter().any(|request| {
                    request.get("path").and_then(Value::as_str)
                        == Some(pl_provider_fixture::MCP_PATH)
                        && request.pointer("/body/method").and_then(Value::as_str)
                            == Some("tools/call")
                        && request.get("accepted").and_then(Value::as_bool) == Some(true)
                }),
                "missing search credentials also broke the independent MCP tool"
            );
        }
        println!(
            "Evidence: {} (fault {}, human verdict pending)",
            context.output.display(),
            fault.label()
        );
        return Ok(());
    }
    ensure!(
        fixture_exit_success,
        "fixture shutdown failed: {rejected_categories:?}"
    );
    if context.probe_mode == "real" {
        // The real-provider probe is driven directly by the coordinator against
        // its own GUI; the acceptance fixture endpoints are not claimed here.
        println!(
            "Evidence: {} (probe mode real, human verdict pending)",
            context.output.display()
        );
        return Ok(());
    }
    ensure!(rejected == 0, "fixture rejected {rejected} requests");
    ensure!(
        restart_fresh_issued,
        "the restart MCP Turn never issued the MCP tool (stale call id?)"
    );
    ensure!(
        restart_fresh_call,
        "the restart MCP Turn did not make a fresh MCP tools/call"
    );
    ensure!(
        openai_search,
        "OpenAI standalone search endpoint was never exercised"
    );
    ensure!(
        deepseek_search,
        "DeepSeek native search endpoint was never exercised"
    );
    ensure!(
        ["initialize", "tools/list", "tools/call"]
            .iter()
            .all(|method| mcp_methods.contains(*method)),
        "MCP initialize/tools/list/tools/call were not all observed: {mcp_methods:?}"
    );
    ensure!(
        search_tool_models.len() >= 3,
        "expected all three session models to declare both search tools, saw {search_tool_models:?}"
    );
    ensure!(
        search_tools_absent >= 1,
        "the disabled-search configuration never removed both search tools"
    );
    // The first Turn also runs the auxiliary Thread-title completion, which
    // embeds the user prompt as untrusted data and declares no tools. It must be
    // recognised as a title request (never judged as a search-capability Turn),
    // otherwise the reused prompt marker would make it a false rejection.
    ensure!(
        auxiliary_title >= 1,
        "the auxiliary Thread-title request was never recognised as a title"
    );
    // Child Thread proof: the root actually spawned and awaited a real child,
    // that child coexisted the three search/MCP tools with a normal tool, called
    // each path, and the called rows match the declared tools.
    ensure!(
        spawn_requested >= 1,
        "the fixture root never asked for spawn_agent"
    );
    ensure!(
        spawn_wait >= 1,
        "the fixture root never awaited the child with wait"
    );
    ensure!(
        child_tools_present >= 1,
        "no child request coexisted both search tools and a normal tool"
    );
    ensure!(
        child_deepseek >= 1 && child_discover >= 1,
        "the child did not call each search path (deepseek={child_deepseek}, discover={child_discover})"
    );
    ensure!(
        child_mcp_present >= 1,
        "no child request coexisted the discovered MCP tool after discovery"
    );
    let child_thread = child_thread_id.context("driver summary is missing the child Thread id")?;
    ensure!(
        child_root_thread_id.is_some_and(|root| root != child_thread),
        "child Thread id is missing or equals its root: {child_thread_id:?} / {child_root_thread_id:?}"
    );
    ensure!(
        child_has_search_tools && child_has_mcp,
        "child tool rows are missing a search/MCP tool: {child_tools:?}"
    );
    ensure!(
        child_tools_declared,
        "child tool rows are not a subset of the declared tools: {child_tools:?} vs {child_declared:?}"
    );
    println!(
        "Evidence: {} (human verdict pending)",
        context.output.display()
    );
    Ok(())
}
