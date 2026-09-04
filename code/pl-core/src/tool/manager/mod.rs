//! Per-agent tool ownership, registration and immutable model-step plans.
//!
//! 目录页：[`ToolManager`] 拥有全局工具作用域并生产按 agent 隔离的
//! [`AgentToolSet`]；注册代次的 RAII 语义见 `registration`，模型步冻结计划见
//! `plan`，作用域发布与冲突校验见 `scope`，延迟目录的搜索工具见 `search_tool`。

mod agent_tool_set;
mod plan;
mod registration;
mod scope;
mod search_tool;

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use pl_protocol::{PureError, Result};

pub use agent_tool_set::{AgentToolSet, GlobalToolInheritance};
pub use plan::ToolPlan;
pub use registration::ToolRegistration;
pub use scope::{ToolExposure, ToolGroupId, ToolInstallGroup};

use crate::tool::{ToolCallContext, ToolInput, ToolInvocation};

type RefreshHandler =
    dyn Fn(ModelStepToolContext) -> futures::future::BoxFuture<'static, Result<()>> + Send + Sync;

/// Identity and mutable registration window exposed immediately before one model step.
#[derive(Debug, Clone)]
pub struct ModelStepToolContext {
    /// Persistent registration scope that may be atomically refreshed.
    pub agent_tools: AgentToolSet,
    /// Current session identity.
    pub session_id: String,
    /// Current turn identity.
    pub turn_id: String,
    /// Zero-based model-step index within the turn.
    pub step: u32,
}

/// Host callback used to refresh an agent's tools before each model step.
#[derive(Clone)]
pub struct BeforeModelStepHook {
    handler: Arc<RefreshHandler>,
}

impl fmt::Debug for BeforeModelStepHook {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BeforeModelStepHook")
            .finish_non_exhaustive()
    }
}

impl BeforeModelStepHook {
    /// Creates a host refresh hook.
    pub fn new<F, Fut>(handler: F) -> Self
    where
        F: Fn(ModelStepToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            handler: Arc::new(move |context| Box::pin(handler(context))),
        }
    }

    /// Runs the hook for one model-step registration window.
    ///
    /// # Errors
    ///
    /// Propagates the host refresh error and aborts the model step.
    pub async fn refresh(&self, context: ModelStepToolContext) -> Result<()> {
        (self.handler)(context).await
    }
}

/// The sole owner of global tools and the factory for isolated per-agent tool sets.
#[derive(Clone)]
pub struct ToolManager {
    inner: Arc<ToolManagerInner>,
}

struct ToolManagerInner {
    id: u64,
    global: scope::ToolScope,
    next_generation: AtomicU64,
}

impl fmt::Debug for ToolManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolManager")
            .field("id", &self.inner.id)
            .field("global_revision", &self.inner.global.revision())
            .finish_non_exhaustive()
    }
}

impl Default for ToolManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolManager {
    /// Creates an empty manager.
    pub fn new() -> Self {
        static NEXT_MANAGER_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_MANAGER_ID.fetch_add(1, Ordering::Relaxed);
        Self {
            inner: Arc::new(ToolManagerInner {
                id,
                global: scope::ToolScope::new("global"),
                next_generation: AtomicU64::new(1),
            }),
        }
    }

    /// Creates a persistent tool set owned by one agent.
    pub fn agent_tool_set(
        &self,
        agent_id: impl Into<String>,
        inheritance: GlobalToolInheritance,
    ) -> AgentToolSet {
        let agent_id = agent_id.into();
        AgentToolSet::new(self.clone(), agent_id, inheritance)
    }

    /// Atomically replaces one global registration group.
    ///
    /// The returned guard owns that exact generation. Dropping an obsolete guard
    /// cannot unregister a newer replacement.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when the group is empty, a definition has
    /// an empty name, or two tools in the global scope have the same visible name.
    pub fn replace_global(&self, group: ToolInstallGroup) -> Result<ToolRegistration> {
        self.replace_global_batch(vec![group])
    }

    /// Atomically replaces multiple global registration groups.
    ///
    /// All definitions are validated against the final scope before one write is
    /// published. An empty tool vector removes the visible tools from that group
    /// while keeping the returned RAII ownership for the published generation.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when the batch is empty, repeats a group,
    /// contains an invalid definition, or creates any same-scope name conflict.
    pub fn replace_global_batch(&self, groups: Vec<ToolInstallGroup>) -> Result<ToolRegistration> {
        let generation = self.next_generation();
        self.inner.global.replace_batch(groups, generation)
    }

    /// Executes a local tool strictly through a frozen plan.
    ///
    /// # Errors
    ///
    /// Returns an error when the plan belongs to another manager, the name was not
    /// present in that step, or the definition is provider-hosted.
    pub async fn execute(
        &self,
        plan: &ToolPlan,
        name: &str,
        input: ToolInput,
        context: ToolCallContext,
    ) -> Result<crate::tool::ToolResult> {
        if plan.manager_id != self.inner.id {
            return Err(PureError::ConfigError(
                "tool plan belongs to a different ToolManager".to_string(),
            ));
        }
        let Some(binding) = plan.binding(name) else {
            return Err(PureError::ToolExecutionFailed {
                tool: name.to_string(),
                error: "tool is not present in the frozen model-step plan".to_string(),
            });
        };
        if binding.tool().execution() == crate::tool::ToolExecution::ProviderHosted {
            return Err(PureError::ToolExecutionFailed {
                tool: name.to_string(),
                error: "provider-hosted tool cannot be executed locally".to_string(),
            });
        }
        binding
            .tool()
            .execute(ToolInvocation::new(input, context))
            .await
    }

    fn next_generation(&self) -> u64 {
        self.inner.next_generation.fetch_add(1, Ordering::Relaxed)
    }
}

fn fingerprint_json(value: &serde_json::Value) -> String {
    crate::canonical_json_hash(value)
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use pretty_assertions::assert_eq;
    use schemars::JsonSchema;
    use serde::Deserialize;

    use super::*;
    use crate::tool::{
        DynTool, StaticTool, StaticToolDefinition, ToolName, ToolPolicy, ToolResult,
    };
    use crate::turn::ToolEffect;
    use std::future::Future;

    #[derive(Debug)]
    struct NamedTool {
        name: &'static str,
        output: &'static str,
    }

    #[derive(Debug, Deserialize, JsonSchema)]
    #[serde(deny_unknown_fields)]
    struct EmptyPolicyInput {}

    impl NamedTool {
        fn arc(name: &'static str, output: &'static str) -> DynTool {
            Self { name, output }.into()
        }
    }

    impl StaticTool for NamedTool {
        type Input = serde_json::Value;

        fn definition(&self) -> StaticToolDefinition {
            StaticToolDefinition::new(ToolName::builtin(self.name), self.name)
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }

        fn policy(&self) -> ToolPolicy {
            ToolPolicy::read_only()
        }

        fn execute(
            &self,
            _input: Self::Input,
            _context: ToolCallContext,
        ) -> impl Future<Output = Result<ToolResult>> + Send {
            async move { Ok(ToolResult::success(self.output)) }.boxed()
        }
    }

    fn policy_tool(name: &'static str, policy: ToolPolicy) -> DynTool {
        crate::tool::static_tool::<EmptyPolicyInput>(StaticToolDefinition::new(
            ToolName::bare(name).unwrap(),
            name,
        ))
        .policy(policy)
        .build(|_input, _context| async { Ok(ToolResult::success("ok")) })
    }

    fn group(id: &'static str, tools: Vec<DynTool>) -> ToolInstallGroup {
        ToolInstallGroup::direct(ToolGroupId::new(id), tools)
    }

    #[test]
    fn local_tools_shadow_inherited_globals() {
        let manager = ToolManager::new();
        let _global = manager
            .replace_global(group("global", vec![NamedTool::arc("shared", "global")]))
            .unwrap();
        let tools = manager.agent_tool_set("agent-a", GlobalToolInheritance::Inherit);
        let _local = tools
            .replace(group("local", vec![NamedTool::arc("shared", "local")]))
            .unwrap();

        let plan = tools.freeze();

        assert_eq!(plan.names().collect::<Vec<_>>(), vec!["shared"]);
        assert_ne!(
            plan.executor_generation("shared"),
            manager
                .agent_tool_set("other", GlobalToolInheritance::Inherit)
                .freeze()
                .executor_generation("shared")
        );
    }

    #[test]
    fn global_inheritance_is_explicit_and_agents_are_isolated() {
        let manager = ToolManager::new();
        let _global = manager
            .replace_global(group("global", vec![NamedTool::arc("shared", "global")]))
            .expect("publish global tool");
        let inherited = manager.agent_tool_set("inherited", GlobalToolInheritance::Inherit);
        let isolated = manager.agent_tool_set("isolated", GlobalToolInheritance::Isolated);
        let other = manager.agent_tool_set("other", GlobalToolInheritance::Inherit);
        inherited
            .install(group("local", vec![NamedTool::arc("private", "inherited")]))
            .expect("install private tool");

        assert_eq!(inherited.tool_names(), vec!["private", "shared"]);
        assert!(isolated.tool_names().is_empty());
        assert_eq!(other.tool_names(), vec!["shared"]);
    }

    #[tokio::test]
    async fn dropping_a_registration_unpublishes_only_new_plans() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent", GlobalToolInheritance::Isolated);
        let registration = tools
            .replace(group(
                "ephemeral",
                vec![NamedTool::arc("ephemeral", "old handler")],
            ))
            .expect("register ephemeral tool");
        let old_plan = tools.freeze();
        drop(registration);

        assert!(tools.freeze().specs().is_empty());
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let output = manager
            .execute(
                &old_plan,
                "ephemeral",
                ToolInput {
                    arguments: serde_json::json!({}),
                },
                ToolCallContext::test(event_tx),
            )
            .await
            .expect("old immutable plan retains its handler");
        assert_eq!(output.canonical_output(), "old handler");
    }

    #[test]
    fn conflict_rolls_back_the_whole_group() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent-a", GlobalToolInheritance::Isolated);
        let _first = tools
            .replace(group("first", vec![NamedTool::arc("alpha", "old")]))
            .unwrap();
        let _second = tools
            .replace(group("second", vec![NamedTool::arc("beta", "old")]))
            .unwrap();

        let error = tools
            .replace(group(
                "second",
                vec![
                    NamedTool::arc("alpha", "conflict"),
                    NamedTool::arc("gamma", "partial"),
                ],
            ))
            .unwrap_err();

        assert!(error.to_string().contains("alpha"));
        assert_eq!(tools.tool_names(), vec!["alpha", "beta"]);
    }

    #[test]
    fn multi_group_batch_is_atomic_and_its_registration_is_raii_owned() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent-a", GlobalToolInheritance::Isolated);
        tools
            .install_batch(vec![
                group("first", vec![NamedTool::arc("alpha", "old alpha")]),
                group("second", vec![NamedTool::arc("beta", "old beta")]),
            ])
            .expect("install initial groups");

        let error = tools
            .replace_batch(vec![
                group("first", vec![NamedTool::arc("gamma", "new gamma")]),
                group("third", vec![NamedTool::arc("beta", "conflict")]),
            ])
            .expect_err("cross-group conflict rejects the whole batch");
        assert!(error.to_string().contains("beta"));
        assert_eq!(tools.tool_names(), vec!["alpha", "beta"]);

        let registration = tools
            .replace_batch(vec![
                group("first", vec![NamedTool::arc("gamma", "new gamma")]),
                group("second", Vec::new()),
            ])
            .expect("replace both groups atomically");
        assert_eq!(tools.tool_names(), vec!["gamma"]);
        drop(registration);
        assert!(tools.tool_names().is_empty());
    }

    #[test]
    fn registration_order_does_not_change_wire_fingerprint() {
        let manager = ToolManager::new();
        let first = manager.agent_tool_set("first", GlobalToolInheritance::Isolated);
        let second = manager.agent_tool_set("second", GlobalToolInheritance::Isolated);
        let _first = first
            .replace(group(
                "tools",
                vec![NamedTool::arc("zeta", "z"), NamedTool::arc("alpha", "a")],
            ))
            .unwrap();
        let _second = second
            .replace(group(
                "tools",
                vec![NamedTool::arc("alpha", "a"), NamedTool::arc("zeta", "z")],
            ))
            .unwrap();

        assert_eq!(
            first.freeze().wire_fingerprint(),
            second.freeze().wire_fingerprint()
        );
        assert_ne!(
            first.freeze().execution_fingerprint(),
            second.freeze().execution_fingerprint()
        );
    }

    #[test]
    fn execution_policy_removes_disallowed_tools_from_the_model_plan() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("reviewer", GlobalToolInheritance::Isolated);
        let read = policy_tool("read", ToolPolicy::read_only());
        let write = policy_tool(
            "write",
            ToolPolicy::default().with_effect(ToolEffect::WorkspaceWrite),
        );
        let _registration = tools.replace(group("tools", vec![read, write])).unwrap();
        let policy = crate::AgentExecutionPolicy {
            allowed_effects: crate::ToolEffectSet::from_effects([ToolEffect::Read]),
            ..Default::default()
        };

        let plan = tools.freeze().allowed_by(Some(&policy));

        assert_eq!(plan.names().collect::<Vec<_>>(), vec!["read"]);
        assert_eq!(
            plan.specs()
                .iter()
                .map(pl_protocol::ToolSpec::name)
                .collect::<Vec<_>>(),
            vec!["read"]
        );
    }

    #[test]
    fn programmatic_callers_are_projected_only_with_the_hosted_coordinator() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent", GlobalToolInheritance::Isolated);
        let read = policy_tool("lookup", ToolPolicy::read_only().with_programmatic_calls());
        tools
            .install(group("read", vec![read]))
            .expect("install eligible read tool");

        let direct = tools.freeze();
        let pl_protocol::ToolSpec::Function {
            allowed_callers, ..
        } = &direct.specs()[0]
        else {
            panic!("lookup must remain a function tool");
        };
        assert!(allowed_callers.is_empty());

        tools
            .install(group(
                "programmatic",
                vec![DynTool::new_executor(
                    crate::tool::ProgrammaticToolCallingTool::default(),
                )],
            ))
            .expect("install hosted coordinator");
        let hosted = tools.freeze();
        let pl_protocol::ToolSpec::Function {
            allowed_callers,
            output_schema,
            ..
        } = hosted
            .specs()
            .iter()
            .find(|spec| spec.name() == "lookup")
            .expect("lookup spec")
        else {
            panic!("lookup must remain a function tool");
        };
        assert_eq!(
            allowed_callers,
            &[
                pl_protocol::ToolCallerMode::Direct,
                pl_protocol::ToolCallerMode::Programmatic
            ]
        );
        assert!(output_schema.is_some());
    }

    #[test]
    fn non_read_tool_cannot_claim_programmatic_eligibility() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent", GlobalToolInheritance::Isolated);
        let write = policy_tool(
            "write",
            ToolPolicy::default()
                .with_effect(ToolEffect::WorkspaceWrite)
                .with_programmatic_calls(),
        );

        let error = tools
            .replace(group("write", vec![write]))
            .expect_err("write tool must not be programmatic");

        assert!(error.to_string().contains("local Read effect"));
        assert!(tools.freeze().specs().is_empty());
    }

    #[test]
    fn obsolete_guard_cannot_unregister_a_replacement() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent-a", GlobalToolInheritance::Isolated);
        let old = tools
            .replace(group("tools", vec![NamedTool::arc("old", "old")]))
            .unwrap();
        let new = tools
            .replace(group("tools", vec![NamedTool::arc("new", "new")]))
            .unwrap();

        drop(old);
        assert_eq!(tools.tool_names(), vec!["new"]);
        drop(new);
        assert!(tools.tool_names().is_empty());
    }

    #[tokio::test]
    async fn provider_hosted_binding_rejects_local_execution_through_the_manager() {
        let manager = ToolManager::new();
        let tools = manager.agent_tool_set("agent", GlobalToolInheritance::Isolated);
        tools
            .install(group(
                "hosted",
                vec![DynTool::new_executor(
                    crate::tool::ProgrammaticToolCallingTool::default(),
                )],
            ))
            .expect("install hosted tool");
        let plan = tools.freeze();
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);

        let error = manager
            .execute(
                &plan,
                "programmatic_tool_calling",
                ToolInput {
                    arguments: serde_json::json!({}),
                },
                ToolCallContext::test(event_tx),
            )
            .await
            .expect_err("provider-hosted tool must not execute locally");

        assert!(error.to_string().contains("provider-hosted"));
    }
}
