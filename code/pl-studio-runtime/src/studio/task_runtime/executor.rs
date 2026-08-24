use super::*;

impl TaskRuntime {
    pub(crate) async fn allocate_executor(
        &self,
        input: AllocateExecutor,
    ) -> Result<ExecutorAllocation> {
        let AllocateExecutor {
            thread_id,
            title,
            mut scope_hints,
            agent_id,
            requested_by_call_id,
        } = input;
        let title = normalize_executor_title(&title)?;
        scope_hints.sort();
        scope_hints.dedup();
        let current = self
            .aggregate(&thread_id)
            .await
            .context("active task run not found for this session")?;
        if current.facts.run.kind() != TaskRunStateKind::Working {
            bail!("executor allocation requires working state");
        }
        if let Some(existing) = current
            .facts
            .work_units
            .iter()
            .find(|unit| unit.requested_by_call_id == requested_by_call_id)
        {
            if existing.executor_thread_id.as_deref() != Some(agent_id.as_str())
                || normalize_executor_title(&existing.title)? != title
                || normalized_scope(&existing.scope_hints) != scope_hints
            {
                bail!("task executor call id is already owned by a different allocation");
            }
            return Ok(ExecutorAllocation {
                run: current.facts.run,
                work_unit: existing.clone(),
                reused: true,
            });
        }
        if let Some(existing) = current.facts.work_units.iter().find(|unit| {
            !unit.kind().is_terminal()
                && normalize_executor_title(&unit.title).ok().as_deref() == Some(title.as_str())
                && normalized_scope(&unit.scope_hints) == scope_hints
        }) {
            return Ok(ExecutorAllocation {
                run: current.facts.run,
                work_unit: existing.clone(),
                reused: true,
            });
        }
        if current
            .facts
            .work_units
            .iter()
            .filter(|unit| !unit.kind().is_terminal())
            .count()
            >= 4
        {
            bail!("task executor concurrency limit reached: at most 4 active executors");
        }
        let previous = current
            .facts
            .work_units
            .iter()
            .filter(|unit| {
                normalize_executor_title(&unit.title).ok().as_deref() == Some(title.as_str())
                    && normalized_scope(&unit.scope_hints) == scope_hints
            })
            .max_by_key(|unit| (unit.attempt, unit.id.as_str()));
        let attempt = previous
            .map_or(0, |unit| unit.attempt)
            .checked_add(1)
            .context("executor attempt overflow")?;
        let work_unit_id = super::super::ids::new_id("work-unit");
        let worktree_path = git_compatible_path(
            std::path::Path::new(&current.facts.run.workspace_root)
                .join(".pure")
                .join("worktrees")
                .join(&current.facts.run.id)
                .join(&agent_id),
        )
        .to_string_lossy()
        .to_string();
        let branch = format!("pure-task-{}-{agent_id}", current.facts.run.id);
        let now = super::super::ids::unix_seconds();
        let new_unit = WorkUnit {
            context: WorkUnitContext {
                id: work_unit_id.clone(),
                task_run_id: current.facts.run.id.clone(),
                title,
                scope_hints,
                base_commit: "HEAD".to_string(),
                worktree_path,
                branch,
                attempt,
                supersedes_work_unit_id: previous.map(|unit| unit.id.clone()),
                executor_thread_id: Some(agent_id),
                requested_by_call_id,
            },
            state: WorkUnitState::pending(),
            revision: 0,
            created_at: now,
            updated_at: now,
        };
        let committed_unit = new_unit.clone();
        let committed = self
            .commit_facts(&thread_id, move |current| {
                let mut facts = current.clone();
                if facts.run.kind() != TaskRunStateKind::Working {
                    bail!("executor allocation requires working state");
                }
                if facts
                    .work_units
                    .iter()
                    .any(|unit| unit.requested_by_call_id == committed_unit.requested_by_call_id)
                {
                    bail!("task executor allocation changed while committing");
                }
                facts.work_units.push(committed_unit);
                facts.run.revision = facts
                    .run
                    .revision
                    .checked_add(1)
                    .context("TaskRun revision overflow")?;
                facts.run.updated_at = now;
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        Ok(ExecutorAllocation {
            run: committed.run,
            work_unit: committed
                .work_units
                .into_iter()
                .find(|unit| unit.id == work_unit_id)
                .context("committed WorkUnit is missing")?,
            reused: false,
        })
    }

    pub(crate) async fn update_executor_allocation(
        &self,
        work_unit_id: &str,
        agent_id: &str,
        command: WorkUnitCommand,
    ) -> Result<()> {
        let root_thread_id = self
            .root_for_work_unit(work_unit_id)
            .await
            .context("executor work unit not found")?;
        let work_unit_id = work_unit_id.to_string();
        let agent_id = agent_id.to_string();
        self.commit_facts(&root_thread_id, move |current| {
            let mut facts = current.clone();
            let unit = facts
                .work_units
                .iter()
                .find(|unit| unit.id == work_unit_id)
                .context("executor work unit not found")?;
            if unit.executor_thread_id.as_deref() != Some(agent_id.as_str()) {
                bail!("executor work unit belongs to another agent");
            }
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                command,
                super::super::ids::unix_seconds(),
            )?;
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn record_executor_worktree_base(
        &self,
        work_unit_id: &str,
        agent_id: &str,
        actual_base_commit: &str,
    ) -> Result<WorkUnit> {
        let actual_base_commit = actual_base_commit.trim();
        if actual_base_commit.is_empty() {
            bail!("executor worktree resolved an empty base commit");
        }
        let root_thread_id = self
            .root_for_work_unit(work_unit_id)
            .await
            .context("executor work unit not found while recording worktree base")?;
        let work_unit_id = work_unit_id.to_string();
        let committed_work_unit_id = work_unit_id.clone();
        let agent_id = agent_id.to_string();
        let actual_base_commit = actual_base_commit.to_string();
        let committed = self
            .commit_facts(&root_thread_id, move |current| {
                let mut facts = current.clone();
                let unit = facts
                    .work_units
                    .iter_mut()
                    .find(|unit| unit.id == work_unit_id)
                    .context("executor work unit not found while recording worktree base")?;
                if unit.executor_thread_id.as_deref() != Some(agent_id.as_str()) {
                    bail!("executor work unit belongs to another agent");
                }
                if unit.kind() != WorkUnitStateKind::Pending {
                    bail!("executor worktree base can only be recorded for a pending WorkUnit");
                }
                if unit.base_commit != actual_base_commit {
                    if unit.base_commit != "HEAD" {
                        bail!(
                            "executor WorkUnit base commit changed before worktree creation completed"
                        );
                    }
                    unit.context.base_commit = actual_base_commit;
                    unit.revision = unit
                        .revision
                        .checked_add(1)
                        .context("WorkUnit revision overflow")?;
                    unit.updated_at = super::super::ids::unix_seconds();
                }
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .work_units
            .into_iter()
            .find(|unit| unit.id == committed_work_unit_id)
            .context("committed WorkUnit is missing")
    }

    pub(crate) async fn executor_close_disposition(
        &self,
        root_thread_id: &str,
        work_unit_id: &str,
        agent_id: &str,
        commit: bool,
    ) -> Result<ExecutorCloseDisposition> {
        let aggregate = self
            .aggregate(root_thread_id)
            .await
            .context("active Task aggregate is not resident")?;
        let unit = aggregate
            .facts
            .work_units
            .iter()
            .find(|unit| unit.id == work_unit_id)
            .context("executor work unit not found")?;
        if unit.task_run_id != aggregate.facts.run.id
            || unit.executor_thread_id.as_deref() != Some(agent_id)
        {
            bail!("executor close lifecycle identity does not match durable assignment");
        }
        if unit.kind() == WorkUnitStateKind::ReviewPassed {
            return Ok(ExecutorCloseDisposition::PreserveForMerge);
        }
        if unit.kind() == WorkUnitStateKind::ChangesRequired
            || matches!(
                unit.waiting_review_phase(),
                Some(WaitingReviewPhase::Ready(_) | WaitingReviewPhase::Reviewing(_))
            )
        {
            bail!("executor cannot close while its completion review is active");
        }
        if commit && !unit.kind().is_terminal() {
            self.update_executor_allocation(
                work_unit_id,
                agent_id,
                WorkUnitCommand::Cancel {
                    operation_id: format!("executor-close:{agent_id}"),
                    reason: "executor discarded by planner".to_string(),
                    disposition: TaskWorktreeDisposition::CleanupRequested,
                },
            )
            .await?;
        }
        Ok(ExecutorCloseDisposition::Discard)
    }

    pub(crate) async fn create_work_completion(
        &self,
        root_thread_id: &str,
        work_unit_id: &str,
        content: WorkCompletionContent,
        verification_summary: &str,
    ) -> Result<WorkCompletionRecord> {
        let work_unit_id = work_unit_id.to_string();
        let verification_summary = verification_summary.trim().to_string();
        if verification_summary.is_empty() {
            bail!("verificationSummary must not be empty");
        }
        let committed_work_unit_id = work_unit_id.clone();
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                if facts.run.kind() != TaskRunStateKind::Working {
                    bail!("task is not accepting executor completion");
                }
                let unit_index = facts
                    .work_units
                    .iter()
                    .position(|unit| unit.id == work_unit_id)
                    .context("work unit not found")?;
                if facts.work_units[unit_index].kind() != WorkUnitStateKind::Running {
                    bail!("work unit is not accepting a completion");
                }
                if facts.completions.iter().any(|completion| {
                    completion.work_unit_id == work_unit_id
                        && completion.status() == WorkCompletionStatus::ReadyForReview
                }) {
                    bail!("work unit already has an active completion review");
                }
                let revision = facts
                    .completions
                    .iter()
                    .filter(|completion| completion.work_unit_id == work_unit_id)
                    .map(|completion| completion.revision)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1)
                    .context("completion revision overflow")?;
                let unit = facts.work_units[unit_index].clone();
                let now = super::super::ids::unix_seconds();
                let completion = WorkCompletionRecord {
                    id: super::super::ids::new_id("completion"),
                    task_run_id: facts.run.id.clone(),
                    work_unit_id: unit.id.clone(),
                    executor_agent_id: unit
                        .executor_thread_id
                        .clone()
                        .context("work unit has no executor Thread")?,
                    revision,
                    content,
                    state: WorkCompletionState::ready_for_review(),
                    state_revision: 0,
                    base_commit: unit.base_commit.clone(),
                    verification_summary: verification_summary.clone(),
                    worktree_path: unit.worktree_path.clone(),
                    branch: unit.branch.clone(),
                    created_at: now,
                    updated_at: now,
                };
                apply_work_unit_command_in_facts(
                    &mut facts,
                    &work_unit_id,
                    WorkUnitCommand::SubmitCompletion {
                        completion_id: completion.id.clone(),
                        completion_revision: completion.revision,
                        verification_summary: verification_summary.clone(),
                    },
                    now,
                )?;
                facts.completions.push(completion);
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .completions
            .iter()
            .filter(|completion| completion.work_unit_id == committed_work_unit_id)
            .max_by_key(|completion| completion.revision)
            .cloned()
            .context("committed WorkCompletion is missing")
    }

    pub(crate) async fn mark_executor_turn_started(
        &self,
        agent_id: &str,
        turn_id: &str,
        budget_action: MailboxBudgetAction,
    ) -> Result<()> {
        let (root_thread_id, work_unit_id) = self.executor_owner(agent_id).await?;
        let turn_id = turn_id.to_string();
        self.commit_facts(&root_thread_id, move |current| {
            let mut facts = current.clone();
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                WorkUnitCommand::StartTurn {
                    turn_id,
                    reset_budget: budget_action == MailboxBudgetAction::Refresh,
                },
                super::super::ids::unix_seconds(),
            )?;
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn settle_executor_turn_finished(
        &self,
        agent_id: &str,
        outcome: &AgentTurnOutcome,
    ) -> Result<Option<ExecutorContinuationRequest>> {
        let (root_thread_id, work_unit_id) = self.executor_owner(agent_id).await?;
        let aggregate = self
            .aggregate(&root_thread_id)
            .await
            .context("active Task aggregate is not resident")?;
        let unit = aggregate
            .facts
            .work_units
            .iter()
            .find(|unit| unit.id == work_unit_id)
            .context("executor work unit not found")?;
        if unit.continuation_source_turn_id() == Some(outcome.turn_id.as_str()) {
            if matches!(&outcome.outcome, TurnOutcome::BudgetLimited(_))
                && unit.continuation_state() == ExecutorContinuationStateKind::PendingStart
            {
                return Ok(Some(ExecutorContinuationRequest {
                    agent_id: agent_id.to_string(),
                    work_unit_id,
                    source_turn_id: outcome.turn_id.to_string(),
                    slice_count: unit.budget_slice_count(),
                }));
            }
            return Ok(None);
        }
        if unit.kind() != WorkUnitStateKind::Running {
            return Ok(None);
        }

        let (command, continuation) =
            executor_terminal_decision(agent_id, &work_unit_id, unit, outcome)?;
        self.commit_facts(&root_thread_id, move |current| {
            let mut facts = current.clone();
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                command,
                super::super::ids::unix_seconds(),
            )?;
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(continuation)
    }

    /// Agent 自身 Faulted 时，执行者 WorkUnit 直接失败；健康计划者通过同一稳定
    /// executor terminal wake 接管，不把故障执行者保留为等待报告状态。
    pub(crate) async fn fail_faulted_executor(
        &self,
        agent_id: &str,
        source_turn_id: &str,
        detail: &str,
    ) -> Result<()> {
        let (root_thread_id, work_unit_id) = self.executor_owner(agent_id).await?;
        let operation_id = format!("executor-fault:{source_turn_id}");
        let detail = detail.to_string();
        self.commit_facts(&root_thread_id, move |current| {
            let mut facts = current.clone();
            let unit = facts
                .work_units
                .iter()
                .find(|unit| unit.id == work_unit_id)
                .context("executor work unit not found")?;
            if unit.kind().is_terminal() {
                return Ok(facts);
            }
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                WorkUnitCommand::FailExecution {
                    operation_id,
                    detail,
                    disposition: TaskWorktreeDisposition::Protect,
                },
                super::super::ids::unix_seconds(),
            )?;
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn fail_executor_continuation(
        &self,
        continuation: &ExecutorContinuationRequest,
        error: &str,
    ) -> Result<()> {
        let root_thread_id = self
            .root_for_work_unit(&continuation.work_unit_id)
            .await
            .context("executor continuation work unit not found")?;
        let aggregate = self
            .aggregate(&root_thread_id)
            .await
            .context("active Task aggregate is not resident")?;
        let Some(unit) = aggregate
            .facts
            .work_units
            .iter()
            .find(|unit| unit.id == continuation.work_unit_id)
        else {
            return Ok(());
        };
        if unit.executor_thread_id.as_deref() != Some(continuation.agent_id.as_str())
            || unit.continuation_source_turn_id() != Some(continuation.source_turn_id.as_str())
            || unit.continuation_state() != ExecutorContinuationStateKind::PendingStart
        {
            return Ok(());
        }
        let limit = unit
            .budget_limit()
            .cloned()
            .context("pending executor continuation has no budget snapshot")?;
        let work_unit_id = continuation.work_unit_id.clone();
        let source_turn_id = continuation.source_turn_id.clone();
        let detail = error.to_string();
        self.commit_facts(&root_thread_id, move |current| {
            let mut facts = current.clone();
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                WorkUnitCommand::PauseForBudget {
                    source_turn_id,
                    limit,
                    detail,
                },
                super::super::ids::unix_seconds(),
            )?;
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(())
    }

    async fn executor_owner(&self, agent_id: &str) -> Result<(String, String)> {
        let aggregates = self.aggregates.read().await;
        let mut matches = aggregates.iter().flat_map(|(root_thread_id, aggregate)| {
            aggregate
                .facts
                .work_units
                .iter()
                .filter(|unit| unit.executor_thread_id.as_deref() == Some(agent_id))
                .map(|unit| (root_thread_id.clone(), unit.id.clone()))
                .collect::<Vec<_>>()
        });
        let first = matches.next().context("executor work unit not found")?;
        if matches.next().is_some() {
            bail!("executor Thread owns multiple work units");
        }
        Ok(first)
    }

    pub(crate) async fn mark_executor_handoff_needs_attention(
        &self,
        agent_id: &str,
        error: &str,
    ) -> Result<()> {
        let (root_thread_id, work_unit_id) = self.executor_owner(agent_id).await?;
        let operation_id = format!("handoff-needs-attention:{agent_id}");
        let detail = error.to_string();
        self.commit_facts(&root_thread_id, move |current| {
            let mut facts = current.clone();
            apply_work_unit_command_in_facts(
                &mut facts,
                &work_unit_id,
                WorkUnitCommand::PauseOperational {
                    operation_id,
                    detail,
                },
                super::super::ids::unix_seconds(),
            )?;
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(())
    }
}
