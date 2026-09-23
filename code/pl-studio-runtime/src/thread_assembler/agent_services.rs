//! Saved agent identities, explicit cold activation and read-only history/Profile queries.
use super::*;
use anyhow::{Context, Result, bail};
use base64::Engine;
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Tool, ToolError},
};
use pl_protocol::{AgentSessionPage, AgentSessionReadDetail, AgentSessionReadOrder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub(super) struct AgentServices {
    factory: crate::studio::StudioThreadFactory,
    config: crate::config::ConfigRuntime,
    store: crate::studio::StudioStore,
    events: crate::studio::ProductEventBus,
}
impl std::fmt::Debug for AgentServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AgentAgentServices")
    }
}
#[derive(Debug, Clone, Copy)]
enum Kind {
    Profiles,
    Session,
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
fn page_size() -> usize {
    20
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    version: u32,
    target: String,
    through: u64,
    ceiling: u64,
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
#[error("Agent service failed: {0}")]
pub(super) struct ServiceError(#[source] pub(super) anyhow::Error);

impl Tool for QueryTool {
    async fn execute(
        &self,
        input: pl_core::context::OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.query(input, context)
            .await
            .map_err(|error| ToolError::new(ServiceError(error)))
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
        let services = {
            let state = registry.state();
            if state.closing
                || !state.entries.get(&context.thread_id).is_some_and(|entry| {
                    entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open
                })
            {
                bail!("agent query caller is not active");
            }
            state
                .agent_services
                .clone()
                .context("agent product queries were not configured")?
        };
        let owner = StudioThreadAssembler(registry.clone());
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
                output(&services.session_page(&owner, input, path).await?)
            }
        }
    }
}
impl AgentServices {
    pub(super) async fn saved_agents(
        &self,
        caller: &str,
        loaded: &std::collections::BTreeSet<String>,
    ) -> Result<Vec<serde_json::Value>> {
        let caller = self.record(caller).await?;
        let mut rows = Vec::new();
        // 冷 agent 查询只消费 `catalog.toml` 的目录摘要，不逐个 child 读取 checkpoint。
        // 摘要只有 durable status，因此这里只投影 lifecycle，不虚构 Turn/Task 运行态计数。
        for record in self
            .store
            .list_threads_for_root(&caller.root_thread_id)
            .await?
        {
            if loaded.contains(&record.id) {
                continue;
            }
            rows.push(super::agents::agent_row(
                &record.id,
                record.parent_thread_id.as_deref(),
                &directory_snapshot(record.status),
            ));
        }
        Ok(rows)
    }

    pub(super) async fn activate_child(
        &self,
        owner: &StudioThreadAssembler,
        caller: &str,
        target: &str,
    ) -> Result<()> {
        self.authorize(caller, target).await?;
        let record = self.record(target).await?;
        anyhow::ensure!(
            record.parent_thread_id.as_deref() == Some(caller) && !record.archived,
            "only an active direct child can be resumed"
        );
        self.events.warm_thread_index(vec![record.clone()]);
        owner
            .activate(
                super::ThreadActivation {
                    id: record.id,
                    parent_id: record.parent_thread_id,
                },
                self.factory.clone(),
            )
            .await?;
        Ok(())
    }

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
    async fn session_page(
        &self,
        owner: &StudioThreadAssembler,
        input: SessionInput,
        path: Vec<pl_protocol::ThreadId>,
    ) -> Result<AgentSessionPage> {
        if !(1..=50).contains(&input.limit) {
            bail!("session limit must be between 1 and 50");
        }
        // A resident target must be durable before its history keyset is read.
        if let Some(thread) = owner.thread(&input.target) {
            thread.flush().await?;
        }
        let cursor = input
            .cursor
            .as_ref()
            .map(|value| {
                let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(value)?;
                Ok::<Cursor, anyhow::Error>(serde_json::from_slice(&bytes)?)
            })
            .transpose()?;
        if let Some(cursor) = &cursor
            && (cursor.version != 2
                || cursor.target != input.target
                || cursor.order != input.order
                || cursor.detail != input.detail)
        {
            bail!("session cursor does not belong to this query");
        }
        let history = self.store.history(&input.target).await?;
        let (watermark, ceiling, items, has_more) = history
            .agent_page(
                input.order == Order::Descending,
                input.detail == Detail::Text,
                cursor.as_ref().map(|cursor| cursor.anchor),
                cursor.as_ref().map(|cursor| cursor.ceiling),
                cursor.as_ref().map(|cursor| cursor.through),
                input.limit,
            )
            .await?;
        let through = cursor.as_ref().map_or(watermark, |cursor| cursor.through);
        let next_cursor = if has_more {
            let anchor = items
                .last()
                .context("session continuation has no anchor")?
                .ordinal;
            Some(
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(
                    &Cursor {
                        version: 2,
                        target: input.target.clone(),
                        through,
                        ceiling,
                        order: input.order,
                        detail: input.detail,
                        anchor,
                    },
                )?),
            )
        } else {
            None
        };
        Ok(AgentSessionPage {
            agent_id: pl_protocol::ThreadId::new(input.target)?,
            path,
            through_sequence: through,
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
}
fn output(value: &impl Serialize) -> Result<ToolOutput> {
    let text = serde_json::to_string(value)?;
    Ok(ToolOutput::new(
        pl_core::context::OpaquePayload::new("pl.studio.agent-query", 1, text.clone())?,
        vec![pl_core::context::ContextContent::Text { text: text.into() }],
    ))
}
impl StudioThreadAssembler {
    pub(crate) fn set_agent_services(
        &self,
        config: crate::config::ConfigRuntime,
        store: crate::studio::StudioStore,
        events: crate::studio::ProductEventBus,
        factory: crate::studio::StudioThreadFactory,
    ) -> Result<(), ThreadAssemblyError> {
        let mut state = self.0.state();
        if state.closing
            || !state.entries.is_empty()
            || !state.creating.is_empty()
            || state.agent_services.is_some()
        {
            return Err(ThreadAssemblyError::Closed);
        }
        state.agent_services = Some(AgentServices {
            factory,
            config,
            store,
            events,
        });
        Ok(())
    }
    pub(super) fn agent_query_tools(&self) -> Result<Vec<Registration>, ThreadAssemblyError> {
        if self.0.state().agent_services.is_none() {
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

/// 由 catalog 目录摘要构造只读 snapshot：仅承载 durable lifecycle，不含 Turn/Task 明细。
fn directory_snapshot(status: pl_protocol::ThreadStatus) -> pl_core::thread::ThreadSnapshot {
    use pl_core::thread::{ThreadLifecycle, ThreadSnapshot};
    use pl_protocol::ThreadStatus;
    ThreadSnapshot {
        lifecycle: match status {
            ThreadStatus::Closing => ThreadLifecycle::Closing,
            ThreadStatus::Closed => ThreadLifecycle::Closed,
            ThreadStatus::Idle
            | ThreadStatus::Queued
            | ThreadStatus::Running
            | ThreadStatus::WaitingTool
            | ThreadStatus::WaitingInteraction
            | ThreadStatus::Cancelling
            | ThreadStatus::Faulted => ThreadLifecycle::Open,
        },
        ..Default::default()
    }
}
