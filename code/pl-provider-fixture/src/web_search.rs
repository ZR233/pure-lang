//! Loopback fixture for the native web-search + MCP acceptance scenario.
//!
//! One listener serves three independent wire surfaces:
//! * OpenAI standalone `/alpha/search` (`SearchResponse`-shaped JSON),
//! * DeepSeek native `/anthropic/v1/messages` (Anthropic-shaped result with
//!   structured sources and usage),
//! * a generic Streamable-HTTP MCP server at `/mcp`
//!   (`initialize` / `tools/list` / `tools/call`).
//!
//! Every request is validated against its real payload and recorded through the
//! shared request log, and the model endpoint answers with a tool call only for
//! tools the request actually declares. Nothing is answered with a blanket
//! success, so a misplanned or missing tool shows up as a missing endpoint hit
//! rather than a false pass.
//!
//! Each endpoint uses a distinct key so a credential routed to the wrong
//! service is rejected with 401 instead of silently accepted.

use super::*;
use axum::http::HeaderMap;

/// Expected bearer token for the OpenAI standalone search endpoint.
pub const OPENAI_SEARCH_KEY: &str = "fixture-openai-search-key";
/// Expected bearer token for the DeepSeek native search endpoint.
pub const DEEPSEEK_SEARCH_KEY: &str = "fixture-deepseek-search-key";
/// Expected `Authorization: Bearer` value for the Streamable-HTTP MCP server.
pub const MCP_KEY: &str = "fixture-mcp-key";

/// User MCP server id the coordinator registers against this fixture.
pub const MCP_SERVER_ID: &str = "fixture_search";
/// MCP tool exposed by the fixture (`搜索`-style single-query search tool).
pub const MCP_TOOL_NAME: &str = "search";

pub const OPENAI_SEARCH_PATH: &str = "/v1/alpha/search";
pub const DEEPSEEK_MESSAGES_PATH: &str = "/v1/anthropic/v1/messages";
pub const MCP_PATH: &str = "/mcp";

/// Prompt markers the acceptance Driver embeds so the dynamic model endpoint
/// calls one *specific* tool.
///
/// Selecting by marker instead of declaration order is required because the
/// integrated planner now offers both search tools to every function-calling
/// session model, so "which tool is declared first" can no longer distinguish
/// the OpenAI, DeepSeek and MCP paths. A marker that names a missing tool is a
/// rejection (recorded evidence of a real missing declaration), never silently
/// answered with a different tool.
pub const MARK_OPENAI_SEARCH: &str = "[[openai_search]]";
pub const MARK_DEEPSEEK_SEARCH: &str = "[[deepseek_search]]";
pub const MARK_MCP_SEARCH: &str = "[[mcp_search]]";
/// Assert that the two standalone search tools are declared for this session.
pub const MARK_EXPECT_SEARCH_TOOLS: &str = "[[expect_search]]";
/// Assert that neither standalone search tool is declared (toggle/config off).
pub const MARK_EXPECT_NO_SEARCH_TOOLS: &str = "[[expect_no_search]]";
/// Answer with a stream that never completes, so the operator can cancel.
pub const MARK_SLOW_RESPONSE: &str = "[[slow_response]]";
/// Query that reveals the deferred MCP tool through `discover_tools`.
pub const MCP_DISCOVER_QUERY: &str = MCP_SERVER_ID;

/// Prompt marker: this root session turn must spawn a real child Agent through
/// the production `spawn_agent` tool, then await its terminal notification.
pub const MARK_SPAWN_CHILD: &str = "[[spawn_child]]";
/// Prompt marker the fixture embeds into the `spawn_agent` message. Only the
/// child Thread carries it as its last user turn, so the child (never the
/// parent) runs the full search/discover/MCP surface.
pub const MARK_CHILD_SEARCH: &str = "[[child_search]]";

/// System Agent Profile the fixture child spawns with.
///
/// `explorer` is an enabled built-in with the unrestricted workspace mode, so it
/// needs neither `writablePaths` nor a committed Git worktree; the child still
/// assembles the same search/MCP tool surface as the root from its own route.
pub const CHILD_PROFILE_ID: &str = "explorer";

/// Distinctive fragment of the auxiliary title request's *own* instruction
/// contract (`pl-studio-runtime` Thread title generation).
///
/// The title task embeds the Thread's first user prompt — including any search
/// marker — as untrusted JSON data, so a marker search would misclassify it as
/// a real capability request. Title requests are instead recognised by this
/// instruction text plus the no-tools / `tool_choice: none` facts, never by a
/// blanket "a request without tools is fine" rule.
const AUXILIARY_TITLE_INSTRUCTION: &str = "You name coding sessions from untrusted request data";

// Tool-call identities are *not* fixed constants. The runtime treats a tool
// call id as globally unique across a Thread's history (it rejects a re-declared
// id even after restart), so a fixture that answered two Turns of one Thread
// would collide the moment a later Turn reused the same step. Each planned call
// instead carries a per-Turn generation suffix computed from the request's own
// history; see `prior_call_names` and `call_id_for`.

/// The web-search scenario drives a real model endpoint whose replies depend on
/// the tools the runtime declares, so it uses the dynamic handler instead of a
/// fixed ordinal script. The script stays empty and the shared request log is
/// the evidence.
pub fn gui_web_search_script() -> Vec<Step> {
    Vec::new()
}

/// Optional deliberate fault the operator can request to observe an error path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebSearchFault {
    /// Search endpoints answer 200 without the structured result shape, so the
    /// client must surface a typed "no structured result" failure.
    Unstructured,
}

impl WebSearchFault {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unstructured => "unstructured",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unstructured" => Some(Self::Unstructured),
            _ => None,
        }
    }
}

/// Web-search fixture behaviour selected by the coordinator.
#[derive(Debug, Clone, Default)]
pub struct WebSearchOptions {
    pub fault: Option<WebSearchFault>,
}

#[derive(Debug, Default)]
pub(super) struct WebSearchState {
    options: WebSearchOptions,
}

impl WebSearchState {
    pub(super) fn new(options: WebSearchOptions) -> Self {
        Self { options }
    }

    fn fault(&self) -> Option<WebSearchFault> {
        self.options.fault
    }
}

async fn json_body(request: axum::extract::Request) -> Option<Value> {
    let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
        .await
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Appends one request to the shared log with an optional scenario-owned note.
///
/// The note is a fixture constant (never caller content), so it stays safe to
/// persist in `fixture.log` and `requests.json`.
fn record(
    state: &AppState,
    method: &str,
    path: &str,
    body: Value,
    accepted: bool,
    note: Option<&str>,
) {
    let diagnostic = note.map(|note| RejectDiagnostic {
        category: note.to_owned(),
        expected_method: Some(method.to_owned()),
        expected_path: Some(path.to_owned()),
        expected_kind: "web_search".to_owned(),
        expected_prompt: None,
        expected_step: None,
        actual_prompt_present: true,
        prompt_matches_expected: accepted,
        delivery_seen: false,
        tool_output_seen: false,
    });
    state
        .script
        .lock()
        .expect("fixture state poisoned")
        .requests
        .push(RecordedRequest {
            method: method.to_owned(),
            path: path.to_owned(),
            body,
            accepted,
            diagnostic,
        });
}

fn authorized(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token.trim() == expected)
}

/// Anthropic-style `x-api-key` credential, which the DeepSeek native search
/// client uses instead of a bearer token.
fn authorized_api_key(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|token| token.trim() == expected)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(json!({"error": {"code": "invalid_api_key", "message": "fixture rejected the credential"}})),
    )
        .into_response()
}

fn content_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(text) => (!text.trim().is_empty()).then(|| text.clone()),
        Value::Array(parts) => {
            let joined = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ");
            (!joined.trim().is_empty()).then_some(joined)
        }
        _ => None,
    }
}

fn last_user_text(body: &Value) -> Option<String> {
    let messages = body
        .get("input")
        .or_else(|| body.get("messages"))?
        .as_array()?;
    for item in messages.iter().rev() {
        if item.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        if let Some(text) = content_text(item.get("content")) {
            return Some(text);
        }
    }
    None
}

fn declared_tool_names(body: &Value) -> Vec<String> {
    body.get("tools")
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

struct PlannedToolCall {
    name: String,
    arguments: String,
    item_id: String,
    call_id: String,
}

/// One step's identity inside one Turn: `{slug}-t{generation}`.
///
/// The `slug` names the scenario step (for example `mcp-search`), and the
/// `generation` is how many earlier Turns of the same Thread already declared a
/// call to the same tool. The pair is unique across a Thread's history while
/// staying constant for every model request that belongs to the same Turn.
fn call_slug(slug: &str, generation: usize) -> String {
    format!("{slug}-t{generation}")
}

fn call_id_for(slug: &str, generation: usize) -> String {
    format!("{}-call", call_slug(slug, generation))
}

fn planned(name: &str, arguments: Value, slug: &str, generation: usize) -> PlannedToolCall {
    let identity = call_slug(slug, generation);
    PlannedToolCall {
        name: name.to_owned(),
        arguments: arguments.to_string(),
        item_id: format!("{identity}-item"),
        call_id: format!("{identity}-call"),
    }
}

/// Which declared tool the active prompt asks the fixture model to call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    OpenAi,
    DeepSeek,
    Mcp,
    ExpectSearchTools,
    ExpectNoSearchTools,
    SpawnChild,
    ChildSearch,
    Slow,
    None,
}

fn marker_of(prompt: &str) -> Marker {
    if prompt.contains(MARK_EXPECT_NO_SEARCH_TOOLS) {
        Marker::ExpectNoSearchTools
    } else if prompt.contains(MARK_EXPECT_SEARCH_TOOLS) {
        Marker::ExpectSearchTools
    } else if prompt.contains(MARK_SLOW_RESPONSE) {
        Marker::Slow
    } else if prompt.contains(MARK_SPAWN_CHILD) {
        Marker::SpawnChild
    } else if prompt.contains(MARK_CHILD_SEARCH) {
        Marker::ChildSearch
    } else if prompt.contains(MARK_OPENAI_SEARCH) {
        Marker::OpenAi
    } else if prompt.contains(MARK_DEEPSEEK_SEARCH) {
        Marker::DeepSeek
    } else if prompt.contains(MARK_MCP_SEARCH) {
        Marker::Mcp
    } else {
        Marker::None
    }
}

fn declared(tools: &[String], name: &str) -> bool {
    tools.iter().any(|tool| tool == name)
}

/// Whether a declared tool name belongs to the web-search surface under test.
///
/// Used only to find a "normal" (non-search) tool for the declaration probe, so
/// a session that only exposes search/discovery is visible instead of passing.
fn is_search_tool(name: &str) -> bool {
    name == "web_search"
        || name == "deepseek_web_search"
        || name == "discover_tools"
        || name.starts_with("mcp__")
}

fn declared_mcp_tool(tools: &[String]) -> Option<String> {
    tools.iter().find(|tool| tool.starts_with("mcp__")).cloned()
}

/// Whether one projected item is the parent-side child terminal notification
/// (`pl.studio.turn-report`), which product context renders as a `user`-role
/// message rather than an authored prompt.
///
/// The report body carries the immutable `childId`/`commitSequence` watermark,
/// so a parent Turn that receives the child's completion in the same request as
/// its own tool outputs can still tell the notification apart from the real
/// prompt this Turn was started by.
fn is_child_report_user_item(item: &Value) -> bool {
    item.get("role").and_then(Value::as_str) == Some("user")
        && content_text(item.get("content"))
            .is_some_and(|text| text.contains("\"childId\"") && text.contains("\"commitSequence\""))
}

/// `call_id`s of committed `function_call_output` items that belong to the
/// *current* Turn.
///
/// Only outputs after the newest real user input count: a previous Turn that
/// already committed a call must not make this Turn skip its own tool call and
/// answer from a stale result. Within one Turn the real user input stays the
/// newest authored user item, so this window is exactly that Turn's tool
/// history.
fn committed_tool_call_ids(items: &[Value], start: usize) -> Vec<String> {
    items
        .get(start..)
        .unwrap_or_default()
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call_output"))
        .filter_map(|item| {
            item.get("call_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// Index just after the newest authored user input of the active Turn.
///
/// A parent-side child terminal report is delivered as a trailing `user` item
/// when a child finishes; it is skipped so the window still starts at the real
/// prompt and keeps this Turn's own `spawn`/`wait` outputs. Every other `user`
/// item is authored, so the newest one is this Turn's prompt.
fn real_turn_window_start(items: &[Value]) -> usize {
    items
        .iter()
        .enumerate()
        .rev()
        .find(|(_, item)| {
            item.get("role").and_then(Value::as_str) == Some("user")
                && !is_child_report_user_item(item)
        })
        .map_or(0, |(index, _)| index + 1)
}

/// Tool names of the `function_call` items declared before the active Turn.
///
/// Taken from the request's own history up to the active Turn's prompt. It is
/// constant for every model request of one Turn and grows whenever a later Turn
/// reuses a tool, so counting matches per name gives a stable per-Turn
/// generation that makes a re-used step (for example the restart MCP Turn)
/// declare a distinct call id instead of repeating a thread-global identity the
/// runtime has already admitted.
fn prior_call_names(items: &[Value], start: usize) -> Vec<String> {
    items[..start]
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .filter_map(|item| item.get("name").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

/// Whether this request is the auxiliary Thread-title completion rather than a
/// user Turn.
///
/// Recognised by the title task's own instruction contract together with the
/// no-tools and `tool_choice: none` facts it always sends, so a real user Turn
/// that genuinely lacks a tool declaration is never silently accepted here.
fn is_auxiliary_title_request(body: &Value) -> bool {
    let instructions = body
        .get("instructions")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let tools_absent = body
        .get("tools")
        .and_then(Value::as_array)
        .is_none_or(|tools| tools.is_empty());
    body.get("tool_choice").and_then(Value::as_str) == Some("none")
        && tools_absent
        && instructions.contains(AUXILIARY_TITLE_INSTRUCTION)
}

/// Real Responses endpoint for the scenario: returns a declared-tool call, then
/// a final answer once the tool result is present.
pub(super) async fn responses(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Response {
    let Some(body) = json_body(request).await else {
        record(
            &state,
            "POST",
            "/v1/responses",
            Value::Null,
            false,
            Some("responses_invalid_json"),
        );
        return StatusCode::BAD_REQUEST.into_response();
    };
    // The auxiliary Thread-title completion has its own instruction contract and
    // always sends no tools. It embeds the first user prompt (and thus its
    // marker) as untrusted data, so it must be answered as a title instead of
    // being judged as a search-capability request.
    if is_auxiliary_title_request(&body) {
        record(
            &state,
            "POST",
            "/v1/responses",
            body,
            true,
            Some("auxiliary_title_answered"),
        );
        return sse_reply(
            responses_text("Fixture Session", "title-response", "fixture-model"),
            false,
            state.stopping.clone(),
        );
    }
    let prompt = last_user_text(&body).unwrap_or_default();
    let tools = declared_tool_names(&body);
    let items: &[Value] = body
        .get("input")
        .and_then(Value::as_array)
        .map(|items| items.as_slice())
        .unwrap_or(&[]);
    let start = real_turn_window_start(items);
    let outputs = committed_tool_call_ids(items, start);
    // Owned copies so the match below no longer borrows `body`; the request body
    // is moved into the evidence log after the branch decides the reply.
    let prior_calls = prior_call_names(items, start);
    let query = prompt.trim().to_owned();

    // The current-Turn identity of one step: the generation counts earlier Turns
    // that declared the same tool, so a re-used step stays unique per Thread.
    let generation = |name: &str| {
        prior_calls
            .iter()
            .filter(|call| call.as_str() == name)
            .count()
    };
    let expected = |slug: &str, name: &str| call_id_for(slug, generation(name));

    let mut accepted = true;
    let mut note: Option<&'static str> = None;
    let mut tool_call: Option<PlannedToolCall> = None;
    let mut slow = false;
    match marker_of(&prompt) {
        Marker::Slow => slow = true,
        Marker::ExpectSearchTools => {
            let openai = declared(&tools, "web_search");
            let deepseek = declared(&tools, "deepseek_web_search");
            let discovery = declared(&tools, "discover_tools");
            let normal = tools.iter().any(|tool| !is_search_tool(tool));
            // The locked contract is that every function-calling session model
            // sees both search tools; discovery and a normal tool are recorded
            // as separate facts so a partial declaration is not a false pass.
            accepted = openai && deepseek;
            note = Some(match (openai, deepseek, discovery, normal) {
                (true, true, true, true) => "search_tools_present",
                (true, true, _, _) => "search_tools_present_partial",
                _ => "search_tools_missing",
            });
        }
        Marker::ExpectNoSearchTools => {
            let absent =
                !declared(&tools, "web_search") && !declared(&tools, "deepseek_web_search");
            accepted = absent;
            note = Some(if absent {
                "search_tools_absent"
            } else {
                "search_tools_unexpected"
            });
        }
        Marker::OpenAi => {
            if outputs
                .iter()
                .any(|id| id == &expected("openai-search", "web_search"))
            {
                // Tool result already committed; fall through to the final answer.
            } else if declared(&tools, "web_search") {
                tool_call = Some(planned(
                    "web_search",
                    json!({"search_query": [{"q": query}]}),
                    "openai-search",
                    generation("web_search"),
                ));
            } else {
                accepted = false;
                note = Some("openai_search_tool_not_declared");
            }
        }
        Marker::DeepSeek => {
            if outputs
                .iter()
                .any(|id| id == &expected("ds-search", "deepseek_web_search"))
            {
                // Tool result already committed.
            } else if declared(&tools, "deepseek_web_search") {
                tool_call = Some(planned(
                    "deepseek_web_search",
                    json!({"query": query}),
                    "ds-search",
                    generation("deepseek_web_search"),
                ));
            } else {
                accepted = false;
                note = Some("deepseek_search_tool_not_declared");
            }
        }
        Marker::Mcp => {
            let mcp_name = declared_mcp_tool(&tools);
            let mcp_committed = mcp_name
                .as_deref()
                .is_some_and(|name| outputs.iter().any(|id| id == &expected("mcp-search", name)));
            let discover_committed = outputs
                .iter()
                .any(|id| id == &expected("discover", "discover_tools"));
            if mcp_committed {
                // This Turn's tool result is already committed; answer.
            } else if let Some(name) = mcp_name {
                let mcp_generation = generation(&name);
                tool_call = Some(planned(
                    &name,
                    json!({"query": query}),
                    "mcp-search",
                    mcp_generation,
                ));
                // Fresh evidence: the MCP tool is actually called in this Turn.
                note = Some("mcp_tool_call_issued");
            } else if declared(&tools, "discover_tools") && !discover_committed {
                // MCP tools are deferred; discover the fixture server's tool first.
                tool_call = Some(planned(
                    "discover_tools",
                    json!({"query": MCP_DISCOVER_QUERY}),
                    "discover",
                    generation("discover_tools"),
                ));
                note = Some("mcp_discover_issued");
            } else {
                // Discovery was already attempted but no MCP tool was revealed
                // (for example the server failed its handshake); answer instead
                // of replaying discover_tools forever.
                accepted = false;
                note = Some("mcp_tool_not_revealed");
            }
        }
        Marker::SpawnChild => {
            // Root-side child flow: request the real `spawn_agent` tool once,
            // then await the child's terminal notification with the real `wait`
            // tool so its result is consumed inside this same Turn instead of
            // racing a fresh parent Turn.
            if outputs
                .iter()
                .any(|id| id == &expected("child-spawn", "spawn_agent"))
            {
                if declared(&tools, "wait")
                    && !outputs
                        .iter()
                        .any(|id| id == &expected("child-wait", "wait"))
                {
                    tool_call = Some(planned(
                        "wait",
                        json!({"taskIds": [], "timeoutMs": 120000}),
                        "child-wait",
                        generation("wait"),
                    ));
                    note = Some("spawn_child_wait");
                } else {
                    note = Some("spawn_child_answered");
                }
            } else if declared(&tools, "spawn_agent") {
                tool_call = Some(planned(
                    "spawn_agent",
                    json!({
                        "profileId": CHILD_PROFILE_ID,
                        "taskSummary": "Fixture child search probe",
                        "message": format!(
                            "{} Call each declared web search tool, \
                             discover the MCP tool, and call it.",
                            MARK_CHILD_SEARCH
                        ),
                    }),
                    "child-spawn",
                    generation("spawn_agent"),
                ));
                note = Some("spawn_child_requested");
            } else {
                // No `spawn_agent` declaration is a real gap, never a silent pass.
                accepted = false;
                note = Some("spawn_agent_tool_not_declared");
            }
        }
        Marker::ChildSearch => {
            // The child notification echoed back into the parent carries this
            // same marker text; only a real child lacks `spawn_agent` (the
            // product disables agent controls for children). A parent request
            // therefore answers instead of replaying the child chain.
            if declared(&tools, "spawn_agent") {
                note = Some("child_marker_on_parent");
            } else {
                let web = declared(&tools, "web_search");
                let deepseek = declared(&tools, "deepseek_web_search");
                let discovery = declared(&tools, "discover_tools");
                let normal = tools.iter().any(|tool| !is_search_tool(tool));
                let mcp = declared_mcp_tool(&tools);
                let web_committed = outputs
                    .iter()
                    .any(|id| id == &expected("child-web", "web_search"));
                let deepseek_committed = outputs
                    .iter()
                    .any(|id| id == &expected("child-deepseek", "deepseek_web_search"));
                let discover_committed = outputs
                    .iter()
                    .any(|id| id == &expected("child-discover", "discover_tools"));
                let mcp_committed = mcp.as_deref().is_some_and(|name| {
                    outputs.iter().any(|id| id == &expected("child-mcp", name))
                });
                if !web_committed {
                    // First child step: assert the three search tools plus a
                    // normal tool coexist, then really call the OpenAI path.
                    accepted = web && deepseek && discovery && normal;
                    note = Some(if accepted {
                        "child_search_tools_present"
                    } else {
                        "child_search_tools_missing"
                    });
                    if web {
                        tool_call = Some(planned(
                            "web_search",
                            json!({"search_query": [{"q": query}]}),
                            "child-web",
                            generation("web_search"),
                        ));
                    }
                } else if !deepseek_committed {
                    accepted = deepseek;
                    note = Some(if deepseek {
                        "child_search_deepseek"
                    } else {
                        "child_search_deepseek_missing"
                    });
                    if deepseek {
                        tool_call = Some(planned(
                            "deepseek_web_search",
                            json!({"query": query}),
                            "child-deepseek",
                            generation("deepseek_web_search"),
                        ));
                    }
                } else if !discover_committed {
                    accepted = discovery;
                    note = Some(if discovery {
                        "child_search_discover"
                    } else {
                        "child_search_discover_missing"
                    });
                    if discovery {
                        tool_call = Some(planned(
                            "discover_tools",
                            json!({"query": MCP_DISCOVER_QUERY}),
                            "child-discover",
                            generation("discover_tools"),
                        ));
                    }
                } else if !mcp_committed {
                    // After discovery the MCP tool must coexist with both
                    // search tools and the normal tool before it is called.
                    accepted = web && deepseek && normal && mcp.is_some();
                    note = Some(if accepted {
                        "child_search_mcp_present"
                    } else {
                        "child_search_mcp_missing"
                    });
                    if let Some(name) = mcp {
                        let mcp_generation = generation(&name);
                        tool_call = Some(planned(
                            &name,
                            json!({"query": query}),
                            "child-mcp",
                            mcp_generation,
                        ));
                    }
                } else {
                    note = Some("child_search_answered");
                }
            }
        }
        Marker::None => {}
    }
    record(&state, "POST", "/v1/responses", body, accepted, note);
    if slow {
        return sse_reply(
            vec![
                json!({"type":"response.created","response":{"id":"fixture-slow","model":"fixture-model"}}),
            ],
            true,
            state.stopping.clone(),
        );
    }
    if let Some(call) = tool_call {
        let PlannedToolCall {
            name,
            arguments,
            item_id,
            call_id,
        } = call;
        return sse_reply(
            responses_tool_calls(
                "fixture-web-search",
                "fixture-model",
                &[RealtimeToolCall {
                    item_id: &item_id,
                    call_id: &call_id,
                    name: &name,
                    arguments,
                }],
            ),
            false,
            state.stopping.clone(),
        );
    }
    let answer = if prompt.trim().is_empty() {
        "fixture answer".to_owned()
    } else {
        format!("fixture answer: {}", prompt.trim())
    };
    sse_reply(
        responses_text(&answer, "fixture-answer", "fixture-model"),
        false,
        state.stopping.clone(),
    )
}

fn search_queries(body: &Value) -> Vec<String> {
    body.get("commands")
        .and_then(|commands| commands.get("search_query"))
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("q").and_then(Value::as_str))
                .filter(|query| !query.trim().is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// OpenAI standalone `/alpha/search`: validates the real command payload.
pub(super) async fn openai_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Response {
    let Some(body) = json_body(request).await else {
        record(
            &state,
            "POST",
            OPENAI_SEARCH_PATH,
            Value::Null,
            false,
            Some("openai_search_invalid_json"),
        );
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !authorized(&headers, OPENAI_SEARCH_KEY) {
        record(
            &state,
            "POST",
            OPENAI_SEARCH_PATH,
            body,
            false,
            Some("openai_search_auth_failed"),
        );
        return unauthorized();
    }
    let queries = search_queries(&body);
    if queries.is_empty() {
        record(
            &state,
            "POST",
            OPENAI_SEARCH_PATH,
            body,
            false,
            Some("openai_search_missing_query"),
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"code": "fixture_missing_query", "message": "search commands carry no query"}})),
        )
            .into_response();
    }
    let fault = state
        .web_search
        .lock()
        .expect("fixture web search state poisoned")
        .fault();
    let note = fault.map(|fault| match fault {
        WebSearchFault::Unstructured => "openai_search_unstructured_fault",
    });
    record(&state, "POST", OPENAI_SEARCH_PATH, body, true, note);
    if fault == Some(WebSearchFault::Unstructured) {
        // Deliberately omits the structured `output` field.
        return Json(json!({"status": "ok"})).into_response();
    }
    let results = queries
        .iter()
        .map(|query| {
            json!({
                "title": format!("Fixture result for {query}"),
                "url": "https://fixture.test/openai-search-result",
                "snippet": format!("fixture snippet for {query}"),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "id": "fixture-openai-search",
        "output": format!("fixture openai search output for {}", queries.join(", ")),
        "results": results,
    }))
    .into_response()
}

fn deepseek_query(body: &Value) -> Option<String> {
    if let Some(query) = body
        .get("query")
        .and_then(Value::as_str)
        .filter(|query| !query.trim().is_empty())
    {
        return Some(query.to_owned());
    }
    let messages = body
        .get("messages")
        .or_else(|| body.get("input"))?
        .as_array()?;
    for message in messages.iter().rev() {
        if message.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        if let Some(text) = content_text(message.get("content")) {
            return Some(text);
        }
    }
    None
}

/// DeepSeek native search `/anthropic/v1/messages`.
///
/// The reply carries both the Anthropic-shaped `web_search_tool_result` content
/// block and a top-level structured `sources` / `usage` pair so either parsing
/// contract sees the same real result.
pub(super) async fn deepseek_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Response {
    let Some(body) = json_body(request).await else {
        record(
            &state,
            "POST",
            DEEPSEEK_MESSAGES_PATH,
            Value::Null,
            false,
            Some("deepseek_messages_invalid_json"),
        );
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !authorized_api_key(&headers, DEEPSEEK_SEARCH_KEY) {
        record(
            &state,
            "POST",
            DEEPSEEK_MESSAGES_PATH,
            body,
            false,
            Some("deepseek_messages_auth_failed"),
        );
        return unauthorized();
    }
    let Some(query) = deepseek_query(&body) else {
        record(
            &state,
            "POST",
            DEEPSEEK_MESSAGES_PATH,
            body,
            false,
            Some("deepseek_messages_missing_query"),
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": {"code": "fixture_missing_query", "message": "no search query in messages"}})),
        )
            .into_response();
    };
    let fault = state
        .web_search
        .lock()
        .expect("fixture web search state poisoned")
        .fault();
    let note = fault.map(|fault| match fault {
        WebSearchFault::Unstructured => "deepseek_messages_unstructured_fault",
    });
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("deepseek-flash")
        .to_owned();
    record(&state, "POST", DEEPSEEK_MESSAGES_PATH, body, true, note);
    if fault == Some(WebSearchFault::Unstructured) {
        // Deliberately omits both `sources` and Anthropic content blocks.
        return Json(json!({"id": "msg_fixture", "type": "message", "role": "assistant"}))
            .into_response();
    }
    let url = "https://fixture.test/deepseek-result";
    let title = format!("Fixture DeepSeek result for {query}");
    let snippet = format!("fixture snippet for {query}");
    // Anthropic-shaped `web_search_result` items, plus the top-level `sources`
    // pair so both parsing contracts see the same real result.
    let sources = json!([{
        "url": url,
        "title": &title,
        "snippet": &snippet,
        "publishedAt": "2026-01-01T00:00:00Z",
        "encrypted_content": "fixture-encrypted",
    }]);
    let query_input = query.clone();
    let answer = format!("fixture deepseek answer for {query}");
    Json(json!({
        "id": "msg_fixture",
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": [
            {"type": "server_tool_use", "id": "srvtoolu_fixture", "name": "web_search", "input": {"query": query_input}},
            {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_fixture", "content": [{
                "type": "web_search_result",
                "url": url,
                "title": title,
                "page_age": "2026-01-01T00:00:00Z",
                "encrypted_content": "fixture-encrypted",
            }]},
            {"type": "text", "text": answer, "citations": [{
                "type": "web_search_result_location",
                "url": url,
                "cited_text": snippet,
            }]},
        ],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 12, "output_tokens": 34, "cache_read_input_tokens": 0},
        "sources": sources,
    }))
    .into_response()
}

/// Any GET on the MCP endpoint reports that this fixture has no server->client
/// SSE stream, which the rmcp client treats as "no resume stream".
pub(super) async fn mcp_get() -> Response {
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

fn jsonrpc_ok(id: Value, result: Value) -> Response {
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

fn jsonrpc_error(id: Value, code: i64, message: &str) -> Response {
    Json(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}))
        .into_response()
}

fn mcp_tool_spec() -> Value {
    json!({
        "name": MCP_TOOL_NAME,
        "description": "Fixture web search tool exposed over Streamable HTTP MCP.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"}
            },
            "required": ["query"],
            "additionalProperties": false
        }
    })
}

/// Generic Streamable-HTTP MCP endpoint (`initialize` / `tools/list` /
/// `tools/call`). `server/discover` is rejected as legacy so the client falls
/// back to the standard initialize handshake.
pub(super) async fn mcp_post(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: axum::extract::Request,
) -> Response {
    let Some(body) = json_body(request).await else {
        record(
            &state,
            "POST",
            MCP_PATH,
            Value::Null,
            false,
            Some("mcp_invalid_json"),
        );
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !authorized(&headers, MCP_KEY) {
        record(
            &state,
            "POST",
            MCP_PATH,
            body,
            false,
            Some("mcp_auth_failed"),
        );
        return unauthorized();
    }
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    match method.as_str() {
        "server/discover" => {
            record(
                &state,
                "POST",
                MCP_PATH,
                body,
                true,
                Some("mcp_legacy_fallback"),
            );
            jsonrpc_error(id, -32601, "server/discover is not supported")
        }
        "initialize" => {
            // Read the client's requested revision before the body is moved
            // into the request log, so no oversized clone is needed.
            let version = body
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-11-25")
                .to_owned();
            record(&state, "POST", MCP_PATH, body, true, None);
            // Echo the client's requested revision so a version mismatch cannot
            // reject an otherwise valid handshake.
            jsonrpc_ok(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "fixture-search", "version": "0.1.0"},
                }),
            )
        }
        "notifications/initialized" => {
            record(&state, "POST", MCP_PATH, body, true, None);
            StatusCode::ACCEPTED.into_response()
        }
        "tools/list" => {
            record(&state, "POST", MCP_PATH, body, true, None);
            jsonrpc_ok(id, json!({"tools": [mcp_tool_spec()]}))
        }
        "tools/call" => {
            let name = body
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let query = body
                .pointer("/params/arguments/query")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned();
            if name != MCP_TOOL_NAME || query.is_empty() {
                record(
                    &state,
                    "POST",
                    MCP_PATH,
                    body,
                    false,
                    Some("mcp_tool_call_invalid"),
                );
                return jsonrpc_error(id, -32602, "unsupported tools/call");
            }
            record(&state, "POST", MCP_PATH, body, true, None);
            jsonrpc_ok(
                id,
                json!({
                    "content": [{"type": "text", "text": format!("fixture mcp search result for {query}")}],
                    "isError": false,
                }),
            )
        }
        _ => {
            record(
                &state,
                "POST",
                MCP_PATH,
                body,
                false,
                Some("mcp_unknown_method"),
            );
            jsonrpc_error(id, -32601, "method not found")
        }
    }
}
