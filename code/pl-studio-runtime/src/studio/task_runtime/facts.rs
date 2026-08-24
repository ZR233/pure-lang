use super::*;

pub(super) fn ensure_task_version(run: &TaskRun, revision: u64, generation: u64) -> Result<()> {
    if run.revision != revision {
        bail!(
            "task revision conflict: expected {revision}, actual {}",
            run.revision
        );
    }
    if run.generation() != generation {
        bail!(
            "task generation conflict: expected {generation}, actual {}",
            run.generation()
        );
    }
    Ok(())
}

pub(super) fn same_task_domain_facts(
    left: &task_projection::LoadedTaskAggregate,
    right: &task_projection::LoadedTaskAggregate,
) -> bool {
    left.run.context == right.run.context
        && left.run.state == right.run.state
        && left.run.created_at == right.run.created_at
        && left.work_units == right.work_units
        && left.completions == right.completions
        && left.merges == right.merges
        && left.reviews == right.reviews
        && left.issues == right.issues
}

pub(super) fn planner_wakes_for_facts(
    facts: &task_projection::LoadedTaskAggregate,
) -> Result<Vec<TaskPlannerWakeRequest>> {
    if facts.run.kind().is_terminal() {
        return Ok(Vec::new());
    }

    let mut units = facts.work_units.iter().collect::<Vec<_>>();
    units.sort_by_key(|unit| (unit.updated_at, unit.id.as_str()));
    let mut wakes = Vec::new();
    for unit in units {
        let continuation = unit.continuation_state();
        let continuation_wake = matches!(
            continuation,
            ExecutorContinuationStateKind::PlannerWakePending
                | ExecutorContinuationStateKind::NeedsAttention
        );
        if continuation_wake
            && continuation == ExecutorContinuationStateKind::NeedsAttention
            && unit.budget_limit().is_none()
        {
            continue;
        }
        let source_turn_id = if continuation_wake {
            unit.continuation_source_turn_id()
                .map(ToOwned::to_owned)
                .context("executor terminal wake has no source Turn")?
        } else if unit.kind() == WorkUnitStateKind::Failed {
            let Some(issue) = facts
                .issues
                .iter()
                .filter(|issue| {
                    issue.work_unit_id.as_deref() == Some(unit.id.as_str())
                        && issue.state.kind()
                            == super::super::task_coordinator::TaskIssueStateKind::OpenRecoverable
                })
                .max_by_key(|issue| (issue.updated_at, issue.id.as_str()))
            else {
                continue;
            };
            issue.source_turn_id.clone()
        } else {
            continue;
        };
        wakes.push(TaskPlannerWakeRequest {
            task_run_id: facts.run.id.clone(),
            root_thread_id: facts.run.root_thread_id.clone(),
            source: TaskPlannerWakeSource::ExecutorTerminal {
                work_unit_id: unit.id.clone(),
                executor_thread_id: unit
                    .executor_thread_id
                    .clone()
                    .context("executor terminal wake has no executor Thread")?,
                source_turn_id,
            },
        });
    }

    if let Some(round) = facts
        .reviews
        .iter()
        .filter(|round| !round.kind().is_active())
        .max_by_key(|round| round.round)
    {
        wakes.push(TaskPlannerWakeRequest {
            task_run_id: facts.run.id.clone(),
            root_thread_id: facts.run.root_thread_id.clone(),
            source: TaskPlannerWakeSource::Review {
                review_round_id: round.id.clone(),
                scope: round.scope,
            },
        });
    }
    Ok(wakes)
}

pub(super) fn settle_children_for_failure(
    facts: &mut task_projection::LoadedTaskAggregate,
    operation_id: &str,
    cause: &str,
    now: i64,
) -> Result<()> {
    for unit in &mut facts.work_units {
        if unit.kind().is_terminal() {
            continue;
        }
        let decision = unit.decide(
            unit.revision,
            WorkUnitCommand::FailExecution {
                operation_id: operation_id.to_string(),
                detail: cause.to_string(),
                disposition: TaskWorktreeDisposition::Protect,
            },
        )?;
        if decision.changed() {
            unit.state = decision.next_state();
            unit.revision = unit
                .revision
                .checked_add(1)
                .context("WorkUnit revision overflow")?;
            unit.updated_at = now;
        }
    }
    for review in &mut facts.reviews {
        if review.kind().is_terminal() {
            continue;
        }
        let reviewer_thread_id = review.reviewer_thread_id().map(ToOwned::to_owned);
        let decision = review.decide(
            review.revision,
            ReviewRoundCommand::Fail {
                reviewer_thread_id,
                error: cause.to_string(),
                summary: cause.to_string(),
            },
        )?;
        if decision.changed() {
            review.state = decision.next_state();
            review.revision = review
                .revision
                .checked_add(1)
                .context("ReviewRound revision overflow")?;
            review.updated_at = now;
        }
    }
    Ok(())
}

pub(super) fn apply_work_unit_command_in_facts(
    facts: &mut task_projection::LoadedTaskAggregate,
    work_unit_id: &str,
    command: WorkUnitCommand,
    now: i64,
) -> Result<()> {
    let unit = facts
        .work_units
        .iter_mut()
        .find(|unit| unit.id == work_unit_id)
        .context("work unit not found")?;
    let decision = unit.decide(unit.revision, command)?;
    if decision.changed() {
        unit.state = decision.next_state();
        unit.revision = unit
            .revision
            .checked_add(1)
            .context("WorkUnit revision overflow")?;
        unit.updated_at = now;
    }
    Ok(())
}

pub(super) fn executor_terminal_decision(
    agent_id: &str,
    work_unit_id: &str,
    unit: &super::super::task_coordinator::WorkUnit,
    outcome: &AgentTurnOutcome,
) -> Result<(WorkUnitCommand, Option<ExecutorContinuationRequest>)> {
    if let TurnOutcome::BudgetLimited(budget) = &outcome.outcome {
        let limit = *budget.limit();
        let current_slice = unit.budget_slice_count();
        let can_continue = limit.kind == BudgetLimitKind::WallClock
            && matches!(budget.rollover(), TurnRolloverOutcome::Succeeded)
            && current_slice < MAX_EXECUTOR_BUDGET_SLICES;
        if can_continue {
            let next_slice = current_slice.saturating_add(1);
            return Ok((
                WorkUnitCommand::ContinueAfterBudget {
                    source_turn_id: outcome.turn_id.to_string(),
                    next_slice,
                    limit,
                },
                Some(ExecutorContinuationRequest {
                    agent_id: agent_id.to_string(),
                    work_unit_id: work_unit_id.to_string(),
                    source_turn_id: outcome.turn_id.to_string(),
                    slice_count: next_slice,
                }),
            ));
        }
        let detail = match budget.rollover() {
            TurnRolloverOutcome::Failed { error } => error.clone(),
            TurnRolloverOutcome::NotAttempted | TurnRolloverOutcome::Succeeded => {
                if limit.kind == BudgetLimitKind::WallClock {
                    format!("executor reached the {MAX_EXECUTOR_BUDGET_SLICES}-slice limit")
                } else {
                    format!(
                        "executor stopped at the {} budget limit",
                        limit.kind.as_str()
                    )
                }
            }
        };
        return Ok((
            WorkUnitCommand::PauseForBudget {
                source_turn_id: outcome.turn_id.to_string(),
                limit,
                detail,
            },
            None,
        ));
    }
    let command = match &outcome.outcome {
        TurnOutcome::Completed(_) => WorkUnitCommand::FinishTurn {
            outcome: ExecutorTerminalOutcome::Completed {
                source_turn_id: outcome.turn_id.to_string(),
                detail: "executor turn ended without a successful report_completion".to_string(),
            },
        },
        TurnOutcome::Failed(value) => WorkUnitCommand::FinishTurn {
            outcome: ExecutorTerminalOutcome::Failed {
                source_turn_id: outcome.turn_id.to_string(),
                detail: value.failure().message.clone(),
            },
        },
        TurnOutcome::Cancelled(value) => WorkUnitCommand::Cancel {
            operation_id: outcome.turn_id.to_string(),
            reason: format!("executor turn cancelled: {:?}", value.cause()),
            disposition: TaskWorktreeDisposition::CleanupRequested,
        },
        TurnOutcome::BudgetLimited(_) => unreachable!("handled above"),
    };
    Ok((command, None))
}

pub(super) fn normalize_executor_title(title: &str) -> Result<String> {
    let normalized = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        bail!("task executor title must not be empty");
    }
    Ok(normalized)
}

pub(super) fn normalized_scope(scope_hints: &[String]) -> Vec<String> {
    let mut normalized = scope_hints.to_vec();
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(super) fn bump_task_revision(facts: &mut task_projection::LoadedTaskAggregate) -> Result<()> {
    facts.run.revision = facts
        .run
        .revision
        .checked_add(1)
        .context("TaskRun revision overflow")?;
    facts.run.updated_at = super::super::ids::unix_seconds();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn new_review_round(
    facts: &task_projection::LoadedTaskAggregate,
    scope: ReviewScope,
    work_unit_id: Option<String>,
    completion_id: Option<String>,
    completion_revision: Option<u32>,
    reviewed_head: String,
    requested_by_call_id: String,
    changed_files: Vec<String>,
) -> Result<ReviewRoundRecord> {
    let round = u32::try_from(facts.reviews.len())
        .context("review round count overflow")?
        .checked_add(1)
        .context("review round count overflow")?;
    let now = super::super::ids::unix_seconds();
    Ok(ReviewRoundRecord {
        id: super::super::ids::new_id("review"),
        task_run_id: facts.run.id.clone(),
        round,
        scope,
        work_unit_id,
        completion_id,
        completion_revision,
        reviewed_head,
        requested_by_call_id,
        state: ReviewRoundState::pending_dispatch(),
        design_references: Vec::new(),
        findings: Vec::new(),
        file_reviews: Some(ReviewFileCoverage::pending(changed_files)),
        revision: 0,
        created_at: now,
        updated_at: now,
    })
}

pub(super) fn ensure_review_call_unused(
    facts: &task_projection::LoadedTaskAggregate,
    requested_by_call_id: &str,
) -> Result<()> {
    if facts
        .reviews
        .iter()
        .any(|round| round.requested_by_call_id == requested_by_call_id)
    {
        bail!("review call id is already used");
    }
    Ok(())
}

pub(super) fn apply_review_command(
    round: &mut ReviewRoundRecord,
    command: ReviewRoundCommand,
    now: i64,
) -> Result<()> {
    let decision = round.decide(round.revision, command)?;
    if decision.changed() {
        round.state = decision.next_state();
        round.revision = round
            .revision
            .checked_add(1)
            .context("ReviewRound revision overflow")?;
        round.updated_at = now;
    }
    Ok(())
}

pub(super) fn reviewer_outcome_detail(outcome: &TurnOutcome) -> String {
    match outcome {
        TurnOutcome::Completed(_) => "reviewer ended without a successful review_exit".to_string(),
        TurnOutcome::Cancelled(value) => format!("reviewer cancelled: {:?}", value.cause()),
        TurnOutcome::Failed(value) => value.failure().message.clone(),
        TurnOutcome::BudgetLimited(_) => "reviewer stopped at its budget limit".to_string(),
    }
}

pub(super) fn pending_review_index(
    facts: &task_projection::LoadedTaskAggregate,
    reviewer_agent_id: &str,
) -> Result<usize> {
    let matches = facts
        .reviews
        .iter()
        .enumerate()
        .filter(|(_, round)| {
            round.reviewer_thread_id() == Some(reviewer_agent_id) && round.kind().is_active()
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => bail!("pending review round not found for reviewer"),
        _ => bail!("reviewer owns multiple pending review rounds"),
    }
}

pub(super) fn prepare_file_reviews(
    stored: &ReviewFileCoverage,
    mut submitted: ReviewFileCoverage,
    accepted: bool,
) -> Result<ReviewFileCoverage> {
    if submitted.version != stored.version || submitted.expected_paths() != stored.expected_paths()
    {
        bail!("review file coverage no longer matches the frozen round target");
    }
    if accepted {
        if !submitted.is_complete()
            || submitted
                .last_diagnostics
                .as_ref()
                .is_some_and(|diagnostics| !diagnostics.is_empty())
        {
            bail!("accepted review coverage must be complete and have no diagnostics");
        }
    } else if submitted
        .last_diagnostics
        .as_ref()
        .is_none_or(super::super::task_coordinator::ReviewExitDiagnostics::is_empty)
    {
        bail!("rejected review coverage must contain diagnostics");
    }
    submitted.diagnostics_revision = stored
        .diagnostics_revision
        .checked_add(1)
        .context("review diagnostics revision overflow")?;
    Ok(submitted)
}

pub(super) fn settle_delivery_review(
    facts: &mut task_projection::LoadedTaskAggregate,
    round_index: usize,
    review: &AgentReview,
    round_id: &str,
) -> Result<()> {
    let round = facts.reviews[round_index].clone();
    let work_unit_id = round
        .work_unit_id
        .as_deref()
        .context("delivery review has no work unit")?;
    let completion_id = round
        .completion_id
        .as_deref()
        .context("delivery review has no completion")?;
    let completion_revision = round
        .completion_revision
        .context("delivery review has no completion revision")?;
    let unit = facts
        .work_units
        .iter()
        .find(|unit| unit.id == work_unit_id)
        .cloned()
        .context("delivery review work unit not found")?;
    let completion_index = facts
        .completions
        .iter()
        .position(|completion| completion.id == completion_id)
        .context("delivery review completion not found")?;
    let completion = facts.completions[completion_index].clone();
    let reviewed_head = completion
        .head_commit()
        .unwrap_or(unit.base_commit.as_str());
    if !matches!(
        unit.waiting_review_phase(),
        Some(WaitingReviewPhase::Reviewing(_))
    ) || completion.work_unit_id != unit.id
        || completion.revision != completion_revision
        || completion.status() != WorkCompletionStatus::ReadyForReview
        || round.reviewed_head != reviewed_head
    {
        bail!("delivery review target changed after reviewer creation");
    }
    let now = super::super::ids::unix_seconds();
    let (completion_command, work_unit_command) = match review.verdict {
        ReviewVerdict::Pass => {
            let outcome = if completion.kind() == WorkCompletionKind::NoDelivery {
                ReviewPassedOutcome::NoDelivery
            } else {
                ReviewPassedOutcome::Delivery
            };
            (
                WorkCompletionCommand::Approve {
                    review_round_id: round_id.to_string(),
                    decided_at: now,
                },
                WorkUnitCommand::PassReview {
                    review_round_id: round_id.to_string(),
                    outcome,
                },
            )
        }
        ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked => (
            WorkCompletionCommand::RequireChanges {
                review_round_id: round_id.to_string(),
                decided_at: now,
            },
            WorkUnitCommand::RequireChanges {
                review_round_id: round_id.to_string(),
            },
        ),
        ReviewVerdict::Pending | ReviewVerdict::Failed => {
            bail!("reviewer cannot select pending or failed")
        }
    };
    let completion_decision = completion.decide(completion.state_revision, completion_command)?;
    if !completion_decision.changed() {
        bail!("delivery review completion was already settled");
    }
    let stored_completion = &mut facts.completions[completion_index];
    stored_completion.state = completion_decision.next_state();
    stored_completion.state_revision = stored_completion
        .state_revision
        .checked_add(1)
        .context("WorkCompletion state revision overflow")?;
    stored_completion.updated_at = now;
    apply_work_unit_command_in_facts(facts, work_unit_id, work_unit_command, now)
}

pub(super) fn ensure_merge_matches(record: &MergeRecord, input: &RecordTaskMerge) -> Result<()> {
    if record.work_unit_id != input.work_unit_id
        || record.completion_revision != input.completion_revision
        || record.executor_agent_id != input.executor_agent_id
        || record.expected_previous_head != input.expected_previous_head
        || record.resulting_head != input.resulting_head
        || record.method != input.method
        || record.summary != input.summary
    {
        bail!("executor Completion already has a different recorded merge");
    }
    Ok(())
}

pub(super) fn delivery_from_completion(completion: &WorkCompletionRecord) -> Result<AgentDelivery> {
    if completion.kind() != WorkCompletionKind::Delivery
        || completion.status() != WorkCompletionStatus::Approved
    {
        bail!("merge requires an approved delivery completion");
    }
    Ok(AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: completion.worktree_path.clone(),
            branch: completion.branch.clone(),
        },
        base_commit: completion.base_commit.clone(),
        head_commit: completion
            .head_commit()
            .map(ToOwned::to_owned)
            .context("delivery completion has no head commit")?,
        changed_files: completion.changed_files().to_vec(),
        verification_summary: completion.verification_summary.clone(),
    })
}
