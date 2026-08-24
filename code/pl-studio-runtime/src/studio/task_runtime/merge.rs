use super::*;

impl TaskRuntime {
    pub(crate) async fn record_task_merge(&self, input: RecordTaskMerge) -> Result<MergeRecord> {
        let root_thread_id = input.thread_id.clone();
        let committed_completion_id = input.completion_id.clone();
        if let Some(existing) = self.aggregate(&root_thread_id).await.and_then(|aggregate| {
            aggregate
                .facts
                .merges
                .into_iter()
                .find(|merge| merge.completion_id == input.completion_id)
        }) {
            ensure_merge_matches(&existing, &input)?;
            return Ok(existing);
        }
        let committed = self
            .commit_facts(&root_thread_id, move |current| {
                let mut facts = current.clone();
                if facts.run.kind() != TaskRunStateKind::Working {
                    bail!("merging TaskRun not found");
                }
                let unit = facts
                    .work_units
                    .iter()
                    .find(|unit| unit.id == input.work_unit_id)
                    .cloned()
                    .context("merge candidate WorkUnit not found")?;
                let completion = facts
                    .completions
                    .iter()
                    .find(|completion| completion.id == input.completion_id)
                    .cloned()
                    .context("merge candidate Completion not found")?;
                if unit.task_run_id != facts.run.id
                    || unit.executor_thread_id.as_deref() != Some(input.executor_agent_id.as_str())
                    || unit.kind() != WorkUnitStateKind::ReviewPassed
                    || completion.task_run_id != facts.run.id
                    || completion.work_unit_id != unit.id
                    || completion.executor_agent_id != input.executor_agent_id
                    || completion.revision != input.completion_revision
                    || completion.kind() != WorkCompletionKind::Delivery
                    || completion.status() != WorkCompletionStatus::Approved
                {
                    bail!("approved Completion changed before merge accounting");
                }
                let delivery_head = completion
                    .head_commit()
                    .map(ToOwned::to_owned)
                    .context("approved delivery Completion has no head commit")?;
                if let Some(previous) = facts
                    .merges
                    .iter()
                    .max_by_key(|merge| (merge.created_at, merge.id.as_str()))
                    && previous.resulting_head != input.expected_previous_head
                {
                    bail!(
                        "merge ledger expectedPreviousHead does not match the prior resultingHead"
                    );
                }
                let now = super::super::ids::unix_seconds();
                let merge = MergeRecord {
                    id: super::super::ids::new_id("merge"),
                    task_run_id: facts.run.id.clone(),
                    work_unit_id: unit.id.clone(),
                    completion_id: completion.id,
                    completion_revision: input.completion_revision,
                    executor_agent_id: input.executor_agent_id,
                    expected_previous_head: input.expected_previous_head,
                    resulting_head: input.resulting_head,
                    delivery_head,
                    method: input.method,
                    summary: input.summary,
                    cleanup: MergeCleanupState::pending(),
                    revision: 0,
                    created_at: now,
                    updated_at: now,
                };
                apply_work_unit_command_in_facts(
                    &mut facts,
                    &unit.id,
                    WorkUnitCommand::CompleteMerge {
                        merge_record_id: merge.id.clone(),
                    },
                    now,
                )?;
                facts.merges.push(merge);
                bump_task_revision(&mut facts)?;
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .merges
            .into_iter()
            .find(|merge| merge.completion_id == committed_completion_id)
            .context("committed MergeRecord is missing")
    }

    pub(crate) async fn read_accepted_merge_scope(&self, merge_id: &str) -> Result<TaskMergeScope> {
        let aggregate = self
            .aggregates
            .read()
            .await
            .values()
            .find(|aggregate| {
                aggregate
                    .facts
                    .merges
                    .iter()
                    .any(|merge| merge.id == merge_id)
            })
            .cloned()
            .context("recorded merge not found")?;
        let merge = aggregate
            .facts
            .merges
            .iter()
            .find(|merge| merge.id == merge_id)
            .cloned()
            .context("recorded merge not found")?;
        let unit = aggregate
            .facts
            .work_units
            .iter()
            .find(|unit| unit.id == merge.work_unit_id)
            .cloned()
            .context("recorded merge work unit not found")?;
        let completion = aggregate
            .facts
            .completions
            .iter()
            .find(|completion| completion.id == merge.completion_id)
            .cloned()
            .context("recorded merge completion not found")?;
        if unit.task_run_id != aggregate.facts.run.id
            || unit.executor_thread_id.as_deref() != Some(merge.executor_agent_id.as_str())
            || unit.kind() != WorkUnitStateKind::Completed
            || !matches!(
                unit.completion_outcome(),
                Some(super::super::task_coordinator::WorkUnitCompletionOutcome::Merged {
                    merge_record_id
                }) if merge_record_id == &merge.id
            )
            || completion.task_run_id != aggregate.facts.run.id
            || completion.work_unit_id != unit.id
            || completion.executor_agent_id != merge.executor_agent_id
            || completion.revision != merge.completion_revision
            || completion.status() != WorkCompletionStatus::Approved
        {
            bail!("recorded merge work unit and completion identity drifted");
        }
        let delivery = delivery_from_completion(&completion)?;
        if delivery.head_commit != merge.delivery_head {
            bail!("recorded merge delivery head drifted");
        }
        Ok(TaskMergeScope {
            run: aggregate.facts.run,
            work_unit: unit,
            completion,
            delivery,
            merge,
        })
    }

    pub(crate) async fn record_merge_cleanup_attempting(
        &self,
        merge_id: &str,
    ) -> Result<MergeRecord> {
        let root_thread_id = self.root_for_merge(merge_id).await?;
        let aggregate = self
            .aggregate(&root_thread_id)
            .await
            .context("active Task aggregate is not resident")?;
        let record = aggregate
            .facts
            .merges
            .iter()
            .find(|merge| merge.id == merge_id)
            .cloned()
            .context("merge record not found")?;
        if record.cleanup.is_complete()
            || matches!(record.cleanup, MergeCleanupState::Attempting(_))
        {
            return Ok(record);
        }
        let now = super::super::ids::unix_seconds();
        let operation_id = format!("merge-cleanup:{}:{}", record.id, record.revision + 1);
        self.apply_merge_cleanup(
            &root_thread_id,
            merge_id,
            MergeCleanupCommand::Attempt {
                operation_id,
                started_at: now,
            },
        )
        .await
    }

    pub(crate) async fn record_merge_cleanup(
        &self,
        merge_id: &str,
        operation_id: &str,
        result: MergeCleanupResult,
    ) -> Result<MergeRecord> {
        let root_thread_id = self.root_for_merge(merge_id).await?;
        self.apply_merge_cleanup(
            &root_thread_id,
            merge_id,
            MergeCleanupCommand::Complete {
                operation_id: operation_id.to_string(),
                completed_at: super::super::ids::unix_seconds(),
                result,
            },
        )
        .await
    }

    async fn root_for_merge(&self, merge_id: &str) -> Result<String> {
        self.aggregates
            .read()
            .await
            .iter()
            .find_map(|(root_thread_id, aggregate)| {
                aggregate
                    .facts
                    .merges
                    .iter()
                    .any(|merge| merge.id == merge_id)
                    .then(|| root_thread_id.clone())
            })
            .context("merge record not found")
    }

    async fn apply_merge_cleanup(
        &self,
        root_thread_id: &str,
        merge_id: &str,
        command: MergeCleanupCommand,
    ) -> Result<MergeRecord> {
        let merge_id = merge_id.to_string();
        let committed_id = merge_id.clone();
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                let merge = facts
                    .merges
                    .iter_mut()
                    .find(|merge| merge.id == merge_id)
                    .context("merge record not found")?;
                let decision = merge.decide_cleanup(merge.revision, command)?;
                if decision.changed() {
                    merge.cleanup = decision.next_state();
                    merge.revision = merge
                        .revision
                        .checked_add(1)
                        .context("MergeRecord revision overflow")?;
                    merge.updated_at = super::super::ids::unix_seconds();
                }
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .merges
            .into_iter()
            .find(|merge| merge.id == committed_id)
            .context("committed MergeRecord is missing")
    }
}
