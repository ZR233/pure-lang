//! Shared runtime usage/panel builders: one folding algorithm for cold rebuild and hot apply.
//!
//! `PanelState` keeps only a bounded summary: a single accumulated compaction contribution and a
//! per-name skill refcount, never one entry per historical extension id. Per-id history lives in
//! the keyed facts (`CompactionFacts.accounting`, `Facts.skill_views`) so `advance` corrects a
//! re-written or deleted entry from the affected old value instead of cloning a growing map.
//!
//! Live model/tool previews are not part of this state: they stay ephemeral on the projection.

use super::ProjectionError;
use pl_core::{
    context::OpaquePayload,
    model::{ModelError, ModelStepOutput, ModelUsage},
    thread::{
        AttemptOutcome, ThreadSnapshot, extensions::ExtensionChange, journal::AttemptUpdate,
        journal::ThreadCommit,
    },
};
use pl_protocol::{
    InferenceAccounting, RuntimeCostAmount, ThreadRuntimeSnapshot, ThreadRuntimeUsage, UsageReport,
};
use std::{collections::BTreeMap, sync::Arc};

use std::collections::BTreeSet;

/// Bounded accumulated auxiliary compaction usage. Corrected per commit from the affected old row.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct CompactionUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub inference_count: u64,
    pub estimated_costs: Vec<RuntimeCostAmount>,
    pub estimated_cache_savings: Vec<RuntimeCostAmount>,
    pub has_incomplete_usage: bool,
    pub has_unpriced_usage: bool,
}

impl CompactionUsage {
    fn add(&mut self, accounting: &InferenceAccounting) {
        let totals = accounting.usage.totals();
        self.prompt_tokens = self.prompt_tokens.saturating_add(totals.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(totals.completion_tokens);
        self.cached_prompt_tokens = self
            .cached_prompt_tokens
            .saturating_add(totals.cached_prompt_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(totals.cache_write_tokens);
        self.reasoning_tokens = self
            .reasoning_tokens
            .saturating_add(totals.reasoning_tokens);
        self.total_tokens = self.total_tokens.saturating_add(totals.total_tokens);
        self.inference_count = self.inference_count.saturating_add(1);
        self.has_incomplete_usage |= accounting.has_incomplete_usage();
        self.has_unpriced_usage |= accounting.has_unpriced_usage();
        merge_costs(&mut self.estimated_costs, &accounting.estimated_costs());
        merge_costs(
            &mut self.estimated_cache_savings,
            &accounting.estimated_cache_savings(),
        );
    }

    fn remove(&mut self, accounting: &InferenceAccounting) {
        let totals = accounting.usage.totals();
        self.prompt_tokens = self.prompt_tokens.saturating_sub(totals.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_sub(totals.completion_tokens);
        self.cached_prompt_tokens = self
            .cached_prompt_tokens
            .saturating_sub(totals.cached_prompt_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_sub(totals.cache_write_tokens);
        self.reasoning_tokens = self
            .reasoning_tokens
            .saturating_sub(totals.reasoning_tokens);
        self.total_tokens = self.total_tokens.saturating_sub(totals.total_tokens);
        self.inference_count = self.inference_count.saturating_sub(1);
        subtract_costs(&mut self.estimated_costs, &accounting.estimated_costs());
        subtract_costs(
            &mut self.estimated_cache_savings,
            &accounting.estimated_cache_savings(),
        );
    }

    fn fold_into(&self, usage: &mut ThreadRuntimeUsage) {
        usage.prompt_tokens = usage.prompt_tokens.saturating_add(self.prompt_tokens);
        usage.completion_tokens = usage
            .completion_tokens
            .saturating_add(self.completion_tokens);
        usage.cached_prompt_tokens = usage
            .cached_prompt_tokens
            .saturating_add(self.cached_prompt_tokens);
        usage.cache_write_tokens = usage
            .cache_write_tokens
            .saturating_add(self.cache_write_tokens);
        usage.reasoning_tokens = usage.reasoning_tokens.saturating_add(self.reasoning_tokens);
        usage.total_tokens = usage.total_tokens.saturating_add(self.total_tokens);
        usage.inference_count = usage.inference_count.saturating_add(self.inference_count);
        usage.has_incomplete_usage |= self.has_incomplete_usage;
        usage.has_unpriced_usage |= self.has_unpriced_usage;
        merge_costs(&mut usage.estimated_costs, &self.estimated_costs);
        merge_costs(
            &mut usage.estimated_cache_savings,
            &self.estimated_cache_savings,
        );
    }
}

/// Bounded, recoverable runtime panel summary for one Thread.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PanelState {
    /// Usage from model attempts only, including flags and latest context tokens.
    pub usage: ThreadRuntimeUsage,
    /// Single accumulated auxiliary compaction contribution, corrected per commit.
    pub compaction: CompactionUsage,
    pub turn_completion_tokens: u64,
    pub turn_decode_millis: u64,
    pub last_turn_id: Option<String>,
    pub todo_id: Option<String>,
    pub todo_payload: Option<OpaquePayload>,
    pub todo_call_id: String,
    pub workflow_id: Option<String>,
    pub workflow_payload: Option<OpaquePayload>,
    /// Current active skill names, bounded by the distinct viewed skills.
    pub active_skills: BTreeSet<String>,
    /// Refcount per active skill name, so a delete or rewrite removes it without a history map.
    pub skill_name_counts: BTreeMap<String, usize>,
    pub updated_at: i64,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            usage: empty_usage(0),
            compaction: CompactionUsage::default(),
            turn_completion_tokens: 0,
            turn_decode_millis: 0,
            last_turn_id: None,
            todo_id: None,
            todo_payload: None,
            todo_call_id: String::new(),
            workflow_id: None,
            workflow_payload: None,
            active_skills: BTreeSet::new(),
            skill_name_counts: BTreeMap::new(),
            updated_at: 0,
        }
    }
}

impl PanelState {
    /// Advances the panel by exactly one canonical commit, cloning only its bounded summary.
    ///
    /// `previous_compactions`/`previous_skills` carry only the affected old rows (keyed by the
    /// extension ids this commit rewrites or deletes), so a correction never reads a history map.
    /// The todo owning call id is matched against this commit's deliveries only, so no scan is
    /// required.
    ///
    /// # Errors
    /// Rejects a saved model/compaction/skill receipt the producers cannot decode.
    pub(crate) fn advance(
        &self,
        commit: &ThreadCommit,
        previous_compactions: &BTreeMap<String, InferenceAccounting>,
        previous_skills: &BTreeMap<String, String>,
    ) -> Result<PanelState, ProjectionError> {
        let mut next = self.clone();
        next.updated_at = commit.committed_at;
        if let Some(update) = &commit.attempt {
            next.absorb_attempt(update)?;
        }
        if let Some(turn) = &commit.turn {
            // Only the newest Turn's attempts contribute decode accounting, so a new Turn resets it.
            next.last_turn_id = Some(turn.turn_id.clone());
            next.turn_completion_tokens = 0;
            next.turn_decode_millis = 0;
        }
        for change in commit.extensions.iter() {
            next.absorb_extension(change, previous_compactions, previous_skills)?;
        }
        for delivery in commit.deliveries.iter() {
            if next.todo_payload.as_ref() == Some(delivery.output.payload()) {
                next.todo_call_id = delivery.call_id.clone();
            }
        }
        Ok(next)
    }

    /// Folds a complete snapshot for explicit reconstruction, using the same account rules.
    ///
    /// # Errors
    /// Rejects a saved model/compaction/skill receipt the producers cannot decode.
    pub(crate) fn from_snapshot(
        state: &ThreadSnapshot,
        updated_at: i64,
    ) -> Result<PanelState, ProjectionError> {
        let mut panel = PanelState {
            updated_at,
            ..Default::default()
        };
        let last_turn = state.turns.last().map(|turn| turn.turn_id.clone());
        panel.last_turn_id = last_turn.clone();
        for attempt in state.attempts.iter() {
            panel.absorb_binding(attempt.request_metadata.as_ref())?;
            let contribution = attempt_contribution(&attempt.outcome)?;
            if contribution.incomplete {
                panel.usage.has_incomplete_usage = true;
                panel.usage.has_unpriced_usage = true;
            }
            if let Some(accounting) = &contribution.accounting {
                add_usage(&mut panel.usage, accounting)?;
            }
            if let Some((tokens, millis)) = contribution.decode
                && last_turn.as_deref() == Some(attempt.turn_id.as_str())
            {
                panel.turn_completion_tokens = panel
                    .turn_completion_tokens
                    .checked_add(tokens)
                    .ok_or(ProjectionError::Count)?;
                panel.turn_decode_millis = panel
                    .turn_decode_millis
                    .checked_add(millis)
                    .ok_or(ProjectionError::Count)?;
            }
        }
        for (id, record) in state.extensions.iter() {
            if let Some(receipt) = super::compactions::receipt(&record.payload)? {
                panel.compaction.add(&receipt.accounting);
            }
            if record.payload.format() == "pl.tool.todo" {
                panel.todo_id = Some(id.clone());
                panel.todo_payload = Some(record.payload.clone());
                panel.todo_call_id = latest_delivery_call(&state.deliveries, &record.payload);
            } else if record.payload.format() == crate::workflow_tool::WORKFLOW_EXTENSION {
                panel.workflow_id = Some(id.clone());
                panel.workflow_payload = Some(record.payload.clone());
            } else if record.payload.format() == "pl.tool.skill-view" {
                let name = pl_tool::skill::saved_skill_name(&record.payload)?;
                panel.skill_add(name);
            }
        }
        Ok(panel)
    }

    /// Materializes the product runtime snapshot, folding compaction usage after attempt usage.
    ///
    /// # Errors
    /// Rejects a saved todo/workflow payload the producers cannot decode.
    pub(crate) fn materialize(
        &self,
        thread_id: &str,
    ) -> Result<ThreadRuntimeSnapshot, ProjectionError> {
        let mut usage = self.usage.clone();
        let latest_context_tokens = usage.latest_context_tokens;
        self.compaction.fold_into(&mut usage);
        // Auxiliary compaction usage never changes the main Thread context capacity.
        usage.latest_context_tokens = latest_context_tokens;
        usage.updated_at = self.updated_at;
        usage.cache_miss_tokens = usage
            .prompt_tokens
            .saturating_sub(usage.cached_prompt_tokens);
        if !usage.has_incomplete_usage && usage.prompt_tokens > 0 {
            usage.cache_hit_rate =
                Some(usage.cached_prompt_tokens as f64 / usage.prompt_tokens as f64);
        }
        let todo = self
            .todo_payload
            .as_ref()
            .map(|payload| pl_tool::todo::saved_snapshot(payload, self.todo_call_id.clone()))
            .transpose()?;
        let workflow = self
            .workflow_payload
            .as_ref()
            .map(|payload| {
                crate::workflow_tool::decode_workflow_state(payload)
                    .map(|state| pl_protocol::WorkflowRuntimeSnapshot::from(&state))
            })
            .transpose()?;
        Ok(ThreadRuntimeSnapshot {
            thread_id: thread_id.into(),
            usage,
            turn_completion_tokens: self.turn_completion_tokens,
            turn_decode_millis: self.turn_decode_millis,
            todo,
            workflow,
            active_skills: self.active_skills.iter().cloned().collect(),
            active_mcp_servers: Vec::new(),
            active_lsp_servers: Vec::new(),
            progress: None,
            mcp_health: None,
            updated_at: self.updated_at,
        })
    }

    fn skill_add(&mut self, name: String) {
        *self.skill_name_counts.entry(name.clone()).or_insert(0) += 1;
        self.active_skills.insert(name);
    }

    fn skill_remove(&mut self, name: &str) {
        if let Some(count) = self.skill_name_counts.get_mut(name) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.skill_name_counts.remove(name);
                self.active_skills.remove(name);
            }
        }
    }

    fn absorb_attempt(&mut self, update: &AttemptUpdate) -> Result<(), ProjectionError> {
        self.absorb_binding(update.request_metadata.as_ref())?;
        let contribution = attempt_contribution(&update.outcome)?;
        if contribution.incomplete {
            self.usage.has_incomplete_usage = true;
            self.usage.has_unpriced_usage = true;
        }
        if let Some(accounting) = &contribution.accounting {
            add_usage(&mut self.usage, accounting)?;
        }
        if let Some((tokens, millis)) = contribution.decode
            && self.last_turn_id.as_deref() == Some(update.turn_id.as_str())
        {
            self.turn_completion_tokens = self
                .turn_completion_tokens
                .checked_add(tokens)
                .ok_or(ProjectionError::Count)?;
            self.turn_decode_millis = self
                .turn_decode_millis
                .checked_add(millis)
                .ok_or(ProjectionError::Count)?;
        }
        Ok(())
    }

    fn absorb_binding(&mut self, metadata: Option<&OpaquePayload>) -> Result<(), ProjectionError> {
        match metadata {
            Some(metadata) => {
                let request = pl_model::runtime::model_request_receipt(metadata)?;
                self.usage.model = request.binding.requested_model;
                self.usage.context_window = request.binding.context_window;
            }
            // A newer attempt without a saved binding clears the previous model's capacity.
            None => self.usage.context_window = None,
        }
        Ok(())
    }

    fn absorb_extension(
        &mut self,
        change: &ExtensionChange,
        previous_compactions: &BTreeMap<String, InferenceAccounting>,
        previous_skills: &BTreeMap<String, String>,
    ) -> Result<(), ProjectionError> {
        match change {
            ExtensionChange::Put { id, record } => {
                let format = record.payload.format();
                if format == "pl.studio.compaction" {
                    if let Some(old) = previous_compactions.get(id) {
                        self.compaction.remove(old);
                    }
                    if let Some(receipt) = super::compactions::receipt(&record.payload)? {
                        self.compaction.add(&receipt.accounting);
                    }
                } else if format == "pl.tool.todo" {
                    self.todo_id = Some(id.clone());
                    self.todo_payload = Some(record.payload.clone());
                } else if format == crate::workflow_tool::WORKFLOW_EXTENSION {
                    self.workflow_id = Some(id.clone());
                    self.workflow_payload = Some(record.payload.clone());
                } else if format == "pl.tool.skill-view" {
                    if let Some(old) = previous_skills.get(id) {
                        self.skill_remove(old);
                    }
                    let name = pl_tool::skill::saved_skill_name(&record.payload)?;
                    self.skill_add(name);
                }
            }
            ExtensionChange::Delete { id, .. } => {
                if let Some(old) = previous_compactions.get(id) {
                    self.compaction.remove(old);
                }
                if let Some(old) = previous_skills.get(id) {
                    self.skill_remove(old);
                }
                if self.todo_id.as_deref() == Some(id.as_str()) {
                    self.todo_id = None;
                    self.todo_payload = None;
                    self.todo_call_id.clear();
                }
                if self.workflow_id.as_deref() == Some(id.as_str()) {
                    self.workflow_id = None;
                    self.workflow_payload = None;
                }
            }
        }
        Ok(())
    }
}

/// Accounting contribution of one attempt outcome: tokens to add, optional decode timing and the
/// interrupted flag, so both folds apply identical rules.
struct AttemptContribution {
    accounting: Option<InferenceAccounting>,
    decode: Option<(u64, u64)>,
    incomplete: bool,
}

impl AttemptContribution {
    fn none() -> Self {
        Self {
            accounting: None,
            decode: None,
            incomplete: false,
        }
    }
}

fn attempt_contribution(outcome: &AttemptOutcome) -> Result<AttemptContribution, ProjectionError> {
    match outcome {
        AttemptOutcome::Running => Ok(AttemptContribution::none()),
        AttemptOutcome::Interrupted => Ok(AttemptContribution {
            incomplete: true,
            ..AttemptContribution::none()
        }),
        AttemptOutcome::Committed(output)
        | AttemptOutcome::Rejected { output, .. }
        | AttemptOutcome::Cancelled { result: Ok(output) } => output_contribution(output),
        AttemptOutcome::Cancelled { result: Err(error) } => error_contribution(error),
        AttemptOutcome::Failed(error) => error_contribution(error),
    }
}

fn output_contribution(output: &ModelStepOutput) -> Result<AttemptContribution, ProjectionError> {
    let mut contribution = AttemptContribution::none();
    match pl_model::runtime::model_response_receipt(output)? {
        Some(receipt) => {
            contribution.decode = receipt.response.timing.map(|timing| {
                (
                    receipt.response.accounting.usage.output_tokens.unwrap_or(0),
                    timing.decode_millis,
                )
            });
            contribution.accounting = Some(receipt.response.accounting);
        }
        None => contribution.accounting = Some(unknown_accounting(&output.usage)),
    }
    Ok(contribution)
}

fn error_contribution(error: &Arc<ModelError>) -> Result<AttemptContribution, ProjectionError> {
    Ok(AttemptContribution {
        accounting: Some(match pl_model::runtime::model_failure_receipt(error)? {
            Some(receipt) => receipt.accounting,
            None => unknown_accounting(&error.usage),
        }),
        ..AttemptContribution::none()
    })
}

fn empty_usage(updated_at: i64) -> ThreadRuntimeUsage {
    ThreadRuntimeUsage {
        has_incomplete_usage: false,
        model: String::new(),
        context_window: None,
        latest_context_tokens: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_prompt_tokens: 0,
        cache_write_tokens: 0,
        cache_miss_tokens: 0,
        reasoning_tokens: 0,
        inference_count: 0,
        total_tokens: 0,
        cache_hit_rate: None,
        estimated_costs: Vec::new(),
        estimated_cache_savings: Vec::new(),
        has_unpriced_usage: false,
        prompt_generation: None,
        prompt_cache_policy: None,
        prefix_changed_reason: None,
        updated_at,
    }
}

fn unknown_accounting(usage: &ModelUsage) -> InferenceAccounting {
    InferenceAccounting {
        usage: UsageReport {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            total_tokens: usage
                .input_tokens
                .zip(usage.output_tokens)
                .and_then(|(a, b)| a.checked_add(b)),
        },
        ..Default::default()
    }
}

fn add_usage(
    target: &mut ThreadRuntimeUsage,
    accounting: &InferenceAccounting,
) -> Result<(), ProjectionError> {
    let totals = accounting.usage.totals();
    target.has_incomplete_usage |= accounting.has_incomplete_usage();
    target.has_unpriced_usage |= accounting.has_unpriced_usage();
    target.inference_count = target
        .inference_count
        .checked_add(1)
        .ok_or(ProjectionError::Count)?;
    for (current, value) in [
        (&mut target.prompt_tokens, totals.prompt_tokens),
        (&mut target.completion_tokens, totals.completion_tokens),
        (
            &mut target.cached_prompt_tokens,
            totals.cached_prompt_tokens,
        ),
        (&mut target.cache_write_tokens, totals.cache_write_tokens),
        (&mut target.reasoning_tokens, totals.reasoning_tokens),
        (&mut target.total_tokens, totals.total_tokens),
    ] {
        *current = current.checked_add(value).ok_or(ProjectionError::Count)?;
    }
    target.cache_miss_tokens = target
        .prompt_tokens
        .saturating_sub(target.cached_prompt_tokens);
    if let Some(tokens) = accounting.usage.known_total_tokens() {
        target.latest_context_tokens = tokens;
    }
    merge_costs(&mut target.estimated_costs, &accounting.estimated_costs());
    merge_costs(
        &mut target.estimated_cache_savings,
        &accounting.estimated_cache_savings(),
    );
    Ok(())
}

fn merge_costs(target: &mut Vec<RuntimeCostAmount>, incoming: &[RuntimeCostAmount]) {
    for cost in incoming {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.currency == cost.currency)
        {
            existing.amount += cost.amount;
        } else {
            target.push(cost.clone());
        }
    }
    target.sort_by(|left, right| left.currency.cmp(&right.currency));
}

fn subtract_costs(target: &mut Vec<RuntimeCostAmount>, outgoing: &[RuntimeCostAmount]) {
    for cost in outgoing {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| existing.currency == cost.currency)
        {
            existing.amount = (existing.amount - cost.amount).max(0.0);
        }
    }
    target.retain(|cost| cost.amount != 0.0);
    target.sort_by(|left, right| left.currency.cmp(&right.currency));
}

/// The call id of the last delivery whose output payload matches, by admission order.
fn latest_delivery_call(
    deliveries: &[pl_core::thread::ToolDelivery],
    payload: &OpaquePayload,
) -> String {
    deliveries
        .iter()
        .rfind(|delivery| delivery.output.payload() == payload)
        .map_or_else(String::new, |delivery| delivery.call_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload,
        thread::{
            extensions::{ExtensionChange, ExtensionRecord},
            journal::ThreadCommit,
        },
    };
    use pretty_assertions::assert_eq;

    fn commit(sequence: u64) -> ThreadCommit {
        ThreadCommit {
            committed_at: sequence as i64,
            thread_id: "thread".into(),
            sequence,
            permissions: Vec::new().into(),
            wake_messages_through: None,
            inputs: Vec::new().into(),
            tasks: Vec::new().into(),
            context: None,
            private_context: None,
            attempt: None,
            turn: None,
            discovered_tools: None,
            deliveries: Vec::new().into(),
            extensions: Vec::new().into(),
            inbox: Vec::new().into(),
            consumed_messages: None,
            interactions: Vec::new().into(),
            replacements: Vec::new().into(),
            runtime_facts: None,
            lifecycle: None,
        }
    }

    fn compaction_payload(input: u64) -> OpaquePayload {
        let receipt = crate::compaction::CompactionReceipt {
            inference_id: "compaction-attempt".into(),
            binding: pl_model::runtime::ModelCallBinding {
                provider_instance_id: "frozen-provider".into(),
                requested_model: "frozen-model".into(),
                adapter: pl_model::provider::ProviderAdapterKind::DeepSeek,
                protocol: pl_model::provider::ProviderWireProtocol::ChatCompletions,
                isolation: "frozen-isolation".into(),
                purpose: "compaction".into(),
                context_window: None,
            },
            reasoning_effort: None,
            context_window: None,
            turn_id: "turn".into(),
            implementation: Some("native".into()),
            error: None,
            accounting: pl_protocol::InferenceAccounting {
                usage: pl_protocol::UsageReport {
                    input_tokens: Some(input),
                    output_tokens: Some(1),
                    total_tokens: Some(input + 1),
                    ..Default::default()
                },
                ..Default::default()
            },
        };
        OpaquePayload::new(
            "pl.studio.compaction",
            1,
            serde_json::to_string(&receipt).unwrap(),
        )
        .unwrap()
    }

    fn commit_with_compaction(sequence: u64, payload: OpaquePayload) -> ThreadCommit {
        let mut commit = commit(sequence);
        commit.extensions = vec![ExtensionChange::Put {
            id: "c".into(),
            record: ExtensionRecord {
                revision: 1,
                payload,
            },
        }]
        .into();
        commit
    }

    fn compaction_accounting(input: u64) -> InferenceAccounting {
        crate::studio::thread_projection::compactions::receipt(&compaction_payload(input))
            .unwrap()
            .unwrap()
            .accounting
    }

    #[test]
    fn rewriting_a_compaction_corrects_its_contribution_instead_of_double_counting() {
        let first = commit_with_compaction(1, compaction_payload(10));
        let panel = PanelState::default()
            .advance(&first, &BTreeMap::new(), &BTreeMap::new())
            .unwrap();
        assert_eq!(panel.materialize("thread").unwrap().usage.prompt_tokens, 10);
        assert_eq!(panel.compaction.inference_count, 1);

        // The pre-commit draft facts hold the first contribution for the same extension id.
        let mut previous = BTreeMap::new();
        previous.insert("c".to_string(), compaction_accounting(10));
        let second = commit_with_compaction(2, compaction_payload(25));
        let panel = panel.advance(&second, &previous, &BTreeMap::new()).unwrap();
        // 25 replaces 10; the auxiliary total is corrected, never 10 + 25.
        assert_eq!(panel.materialize("thread").unwrap().usage.prompt_tokens, 25);
        assert_eq!(panel.compaction.inference_count, 1);
    }

    #[test]
    fn deleting_a_compaction_removes_its_contribution() {
        let first = commit_with_compaction(1, compaction_payload(7));
        let panel = PanelState::default()
            .advance(&first, &BTreeMap::new(), &BTreeMap::new())
            .unwrap();
        let mut previous = BTreeMap::new();
        previous.insert("c".to_string(), compaction_accounting(7));
        let mut delete = commit(2);
        delete.extensions = vec![ExtensionChange::Delete {
            id: "c".into(),
            revision: 2,
        }]
        .into();
        let panel = panel.advance(&delete, &previous, &BTreeMap::new()).unwrap();
        assert_eq!(panel.materialize("thread").unwrap().usage.prompt_tokens, 0);
        assert_eq!(panel.compaction.inference_count, 0);
    }
}
