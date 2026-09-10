//! Read-only product queries over authorized Thread journals and current Profile configuration.
mod submissions;
use super::*;
use anyhow::{Context, Result, bail};
use base64::Engine;
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Tool, ToolError},
};
use pl_protocol::{
    AgentSessionPage, AgentSessionReadDetail, AgentSessionReadOrder, AgentSubmissionPage,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub(super) struct QueryServices {
    config: crate::config::ConfigRuntime,
    store: crate::studio::StudioStore,
    events: crate::studio::ProductEventBus,
}
impl std::fmt::Debug for QueryServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentQueryServices")
    }
}
#[derive(Debug, Clone, Copy)]
enum Kind {
    Profiles,
    Session,
    Submissions,
}
#[derive(Debug)]
struct QueryTool {
    registry: std::sync::Weak<Registry>,
    kind: Kind,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum Order {
    Ascending,
    #[default]
    Descending,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum Detail {
    #[default]
    Text,
    Full,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SessionInput {
    target: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "page_size")]
    limit: usize,
    #[serde(default)]
    order: Order,
    #[serde(default)]
    detail: Detail,
}
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionsInput {
    target: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    offset: usize,
    #[serde(default = "page_size")]
    limit: usize,
}
fn page_size() -> usize {
    20
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    version: u32,
    target: String,
    through: u64,
    order: Order,
    detail: Detail,
    anchor: u64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Profiles {
    profiles: Vec<pl_protocol::AgentProfileSnapshot>,
    diagnostics: Vec<crate::config::AgentProfileDiagnostic>,
}

#[derive(Debug, thiserror::Error)]
#[error("Agent query failed: {0}")]
struct QueryError(#[source] anyhow::Error);

impl Tool for QueryTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.query(input, context)
            .await
            .map_err(|error| ToolError::new(QueryError(error)))
    }
    async fn close(&self) -> Result<(), ToolError> {
        Ok(())
    }
}
impl QueryTool {
    async fn query(
        &self,
        input: pl_core::context::OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput> {
        let registry = self
            .registry
            .upgrade()
            .context("agent query coordinator is closed")?;
        let (services, owner) = {
            let state = registry.state();
            if state.closing
                || !state.entries.get(&context.thread_id).is_some_and(|entry| {
                    entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open
                })
            {
                bail!("agent query caller is not active");
            }
            (
                state
                    .agent_queries
                    .clone()
                    .context("agent product queries were not configured")?,
                StudioThreadAssembler(registry.clone()),
            )
        };
        match self.kind {
            Kind::Profiles => {
                let _: Empty = serde_json::from_str(input.content())?;
                let catalog =
                    tokio::task::spawn_blocking(move || services.config.agent_profiles()).await??;
                output(&Profiles {
                    profiles: catalog.profiles,
                    diagnostics: catalog.diagnostics,
                })
            }
            Kind::Session => {
                let input: SessionInput = serde_json::from_str(input.content())?;
                let path = services
                    .authorize(&context.thread_id, &input.target)
                    .await?;
                let history = services.history(&owner, &input.target).await?;
                output(&session_page(input, path, history)?)
            }
            Kind::Submissions => {
                let input: SubmissionsInput = serde_json::from_str(input.content())?;
                if !(1..=50).contains(&input.limit) {
                    bail!("submission limit must be between 1 and 50");
                }
                services
                    .authorize(&context.thread_id, &input.target)
                    .await?;
                let history = services.history(&owner, &input.target).await?;
                submissions::page(input, history)
            }
        }
    }
}
impl QueryServices {
    async fn record(&self, id: &str) -> Result<pl_protocol::Thread> {
        match self.events.thread_snapshot(id) {
            Some(thread) => Ok(thread),
            None => self
                .store
                .read_thread(id)
                .await?
                .map(pl_protocol::Thread::from)
                .context("agent product association is missing"),
        }
    }
    async fn authorize(&self, caller: &str, target: &str) -> Result<Vec<pl_protocol::ThreadId>> {
        let caller = self.record(caller).await?;
        let mut record = self.record(target).await?;
        let target_root = record.root_thread_id.clone();
        let mut path = Vec::new();
        loop {
            if path
                .iter()
                .any(|id: &pl_protocol::ThreadId| id.as_str() == record.id)
            {
                bail!("cyclic agent ancestry");
            }
            path.push(pl_protocol::ThreadId::new(record.id.clone())?);
            let Some(parent) = record.parent_thread_id else {
                break;
            };
            record = self.record(&parent).await?;
        }
        validate_scope(&caller.id, &caller.root_thread_id, &target_root, &path)?;
        path.reverse();
        Ok(path)
    }
    async fn history(
        &self,
        owner: &StudioThreadAssembler,
        id: &str,
    ) -> Result<Vec<Arc<ThreadCommit>>> {
        let handle = owner
            .observed_threads()
            .into_iter()
            .find(|(thread_id, _)| thread_id == id)
            .map(|(_, handle)| handle);
        let Some(handle) = handle else {
            return Ok(self.store.sessions().read_thread_journal(id).await?);
        };
        let through = handle.snapshot().commit_sequence;
        let mut history = Vec::new();
        while (history.len() as u64) < through {
            let page = handle
                .journal_page(
                    history.len() as u64,
                    std::num::NonZeroUsize::new(128).expect("constant is nonzero"),
                )
                .await?;
            let previous = history.len();
            history.extend(
                page.into_iter()
                    .take_while(|commit| commit.sequence <= through),
            );
            if previous == history.len() {
                bail!("agent journal is incomplete");
            }
        }
        Ok(history)
    }
}
fn session_page(
    input: SessionInput,
    path: Vec<pl_protocol::ThreadId>,
    mut history: Vec<Arc<ThreadCommit>>,
) -> Result<AgentSessionPage> {
    if !(1..=50).contains(&input.limit) {
        bail!("session limit must be between 1 and 50");
    }
    let cursor = input
        .cursor
        .as_ref()
        .map(|value| {
            let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(value)?;
            Ok::<Cursor, anyhow::Error>(serde_json::from_slice(&bytes)?)
        })
        .transpose()?;
    if let Some(cursor) = &cursor {
        if cursor.version != 1
            || cursor.target != input.target
            || cursor.order != input.order
            || cursor.detail != input.detail
        {
            bail!("session cursor does not belong to this query");
        }
        if cursor.through > history.len() as u64 {
            bail!("session cursor is ahead of saved history");
        }
        history.truncate(usize::try_from(cursor.through)?);
    }
    let snapshot = pl_core::thread::journal::replay(&history)?;
    let mut items =
        crate::studio::thread_projection::project_items(&input.target, &snapshot, &history)?;
    if input.detail == Detail::Text {
        items.retain(|item| item.text().is_some());
    }
    if let Some(cursor) = &cursor {
        if !items.iter().any(|item| item.ordinal == cursor.anchor) {
            bail!("session cursor anchor is absent");
        }
        items.retain(|item| match input.order {
            Order::Ascending => item.ordinal > cursor.anchor,
            Order::Descending => item.ordinal < cursor.anchor,
        });
    }
    if input.order == Order::Descending {
        items.reverse();
    }
    let has_more = items.len() > input.limit;
    items.truncate(input.limit);
    let next_cursor = if has_more {
        let anchor = items
            .last()
            .context("session continuation has no anchor")?
            .ordinal;
        Some(
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&Cursor {
                version: 1,
                target: input.target.clone(),
                through: snapshot.commit_sequence,
                order: input.order,
                detail: input.detail,
                anchor,
            })?),
        )
    } else {
        None
    };
    Ok(AgentSessionPage {
        agent_id: pl_protocol::ThreadId::new(input.target)?,
        path,
        through_sequence: snapshot.commit_sequence,
        order: match input.order {
            Order::Ascending => AgentSessionReadOrder::Ascending,
            Order::Descending => AgentSessionReadOrder::Descending,
        },
        detail: match input.detail {
            Detail::Text => AgentSessionReadDetail::Text,
            Detail::Full => AgentSessionReadDetail::Full,
        },
        items,
        has_more,
        next_cursor,
    })
}
fn output(value: &impl Serialize) -> Result<ToolOutput> {
    let text = serde_json::to_string(value)?;
    Ok(ToolOutput::new(
        pl_core::context::OpaquePayload::new("pl.studio.agent-query", 1, text.clone())?,
        vec![pl_core::context::ContextContent::Text { text: text.into() }],
    ))
}
impl StudioThreadAssembler {
    pub(crate) fn set_agent_queries(
        &self,
        config: crate::config::ConfigRuntime,
        store: crate::studio::StudioStore,
        events: crate::studio::ProductEventBus,
    ) -> Result<(), ThreadAssemblyError> {
        let mut state = self.0.state();
        if state.closing
            || !state.entries.is_empty()
            || !state.creating.is_empty()
            || state.agent_queries.is_some()
        {
            return Err(ThreadAssemblyError::Closed);
        }
        state.agent_queries = Some(QueryServices {
            config,
            store,
            events,
        });
        Ok(())
    }
    pub(super) fn agent_query_tools(&self) -> Result<Vec<Registration>, ThreadAssemblyError> {
        if self.0.state().agent_queries.is_none() {
            return Ok(Vec::new());
        }
        let definitions = [
            (
                "list_agent_profiles",
                "List enabled Agent Profiles from the configured catalog.",
                Kind::Profiles,
                serde_json::to_value(schemars::schema_for!(Empty)),
            ),
            (
                "read_agent_session",
                "Read this agent or a descendant's saved timeline. Continuation cursors freeze the original history watermark and filters.",
                Kind::Session,
                serde_json::to_value(schemars::schema_for!(SessionInput)),
            ),
            (
                "read_agent_submissions",
                "Read this agent or a descendant's explicit progress submissions in oldest-first order, with targetState frozen at the same history watermark. Running and readyForCompletion progress are not terminal delivery: continue wait until consuming the matching child/Turn successful completion notification before advancing dependent work.",
                Kind::Submissions,
                serde_json::to_value(schemars::schema_for!(SubmissionsInput)),
            ),
        ];
        definitions
            .into_iter()
            .map(|(name, description, kind, schema)| {
                let schema = schema.map_err(|source| ThreadAssemblyError::Resource {
                    operation: "encode agent query schema",
                    source: Box::new(source),
                })?;
                let declaration = pl_model::runtime::thread_tool_declaration(
                    &pl_protocol::ToolSpec::function(name, description, schema),
                )?;
                Ok(Registration::new(
                    name.into(),
                    declaration,
                    QueryTool {
                        registry: Arc::downgrade(&self.0),
                        kind,
                    },
                )?)
            })
            .collect()
    }
}

fn validate_scope(
    caller: &str,
    caller_root: &str,
    target_root: &str,
    target_ancestry: &[pl_protocol::ThreadId],
) -> Result<()> {
    if caller_root != target_root {
        bail!("agent query target belongs to another tree");
    }
    if !target_ancestry.iter().any(|id| id.as_str() == caller) {
        bail!("agent query target is not the caller or its descendant");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_authorization_rejects_other_trees_ancestors_and_siblings() {
        let path = |ids: &[&str]| {
            ids.iter()
                .map(|id| pl_protocol::ThreadId::new(*id).unwrap())
                .collect::<Vec<_>>()
        };
        assert!(validate_scope("root", "root", "root", &path(&["child", "root"])).is_ok());
        assert!(validate_scope("child", "root", "root", &path(&["child", "root"])).is_ok());
        assert!(validate_scope("child", "root", "root", &path(&["root"])).is_err());
        assert!(validate_scope("child", "root", "root", &path(&["sibling", "root"])).is_err());
        assert!(validate_scope("root", "root", "other", &path(&["other"])).is_err());
        assert!(validate_scope("root", "root", "root", &[]).is_err());
    }
}
