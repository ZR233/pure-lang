use super::*;

impl TaskRuntime {
    pub(crate) async fn begin_delivery_review(
        &self,
        root_thread_id: &str,
        executor_agent_id: &str,
        requested_by_call_id: &str,
    ) -> Result<ReviewRoundRecord> {
        let executor_agent_id = executor_agent_id.to_string();
        let requested_by_call_id = requested_by_call_id.to_string();
        let committed_call_id = requested_by_call_id.clone();
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                if facts.run.kind() != TaskRunStateKind::Working {
                    bail!("delivery review requires working state");
                }
                ensure_review_call_unused(&facts, &requested_by_call_id)?;
                let units = facts
                    .work_units
                    .iter()
                    .filter(|unit| {
                        unit.executor_thread_id.as_deref() == Some(executor_agent_id.as_str())
                    })
                    .collect::<Vec<_>>();
                let unit = match units.as_slice() {
                    [unit] if unit.kind() == WorkUnitStateKind::WaitingReview => (*unit).clone(),
                    [unit] => bail!(
                        "executor work unit is {}, not readyForReview",
                        unit.kind().as_str()
                    ),
                    [] => bail!("executor work unit not found"),
                    _ => bail!("executor owns multiple work units"),
                };
                let completion = facts
                    .completions
                    .iter()
                    .filter(|completion| completion.work_unit_id == unit.id)
                    .max_by_key(|completion| completion.revision)
                    .cloned()
                    .context("work unit has no completion")?;
                if completion.executor_agent_id != executor_agent_id
                    || completion.status() != WorkCompletionStatus::ReadyForReview
                    || !matches!(
                        unit.waiting_review_phase(),
                        Some(WaitingReviewPhase::Ready(_))
                    )
                {
                    bail!("latest completion is not ready for review");
                }
                if facts.reviews.iter().any(|round| {
                    round.scope == ReviewScope::Delivery
                        && round.work_unit_id.as_deref() == Some(unit.id.as_str())
                        && round.kind().is_active()
                }) {
                    bail!("work unit already has a pending delivery review");
                }
                let reviewed_head = completion
                    .head_commit()
                    .unwrap_or(unit.base_commit.as_str())
                    .to_string();
                let round = new_review_round(
                    &facts,
                    ReviewScope::Delivery,
                    Some(unit.id.clone()),
                    Some(completion.id.clone()),
                    Some(completion.revision),
                    reviewed_head,
                    requested_by_call_id.clone(),
                    completion.changed_files().to_vec(),
                )?;
                apply_work_unit_command_in_facts(
                    &mut facts,
                    &unit.id,
                    WorkUnitCommand::BeginReview {
                        review_round_id: round.id.clone(),
                    },
                    super::super::ids::unix_seconds(),
                )?;
                facts.reviews.push(round);
                bump_task_revision(&mut facts)?;
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .reviews
            .into_iter()
            .find(|round| round.requested_by_call_id == committed_call_id)
            .context("committed ReviewRound is missing")
    }

    pub(crate) async fn begin_integrated_review(
        &self,
        root_thread_id: &str,
        request: BeginIntegratedReview,
    ) -> Result<ReviewRoundRecord> {
        let requested_by_call_id = request.requested_by_call_id.clone();
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                ensure_task_version(
                    &facts.run,
                    request.expected_revision,
                    request.expected_generation,
                )?;
                match facts.run.kind() {
                    TaskRunStateKind::Working => {
                        let latest_merge = facts
                            .merges
                            .iter()
                            .max_by_key(|merge| (merge.created_at, merge.id.as_str()))
                            .context("integrated review requires durable merge evidence")?;
                        if latest_merge.resulting_head != request.reviewed_head {
                            bail!("integrated review target changed before round creation");
                        }
                    }
                    TaskRunStateKind::Reviewing => {
                        let target = facts
                            .run
                            .state
                            .review_target()
                            .context("reviewing task omitted its frozen target")?;
                        if target.reviewed_head != request.reviewed_head
                            || target.changed_files != request.changed_files
                        {
                            bail!("integrated review continuation must reuse the frozen target");
                        }
                    }
                    state => bail!(
                        "integrated review requires working or reviewing state, not {}",
                        state.as_str()
                    ),
                }
                ensure_review_call_unused(&facts, &request.requested_by_call_id)?;
                if !super::super::task_coordinator::current_work_units(&facts.work_units)
                    .into_iter()
                    .all(|unit| unit.kind() == WorkUnitStateKind::Completed)
                {
                    bail!("integrated review requires every work unit to be merged or noDelivery");
                }
                if facts.reviews.iter().any(|round| round.kind().is_active()) {
                    bail!("task already has a pending review");
                }
                let round = new_review_round(
                    &facts,
                    ReviewScope::Integrated,
                    None,
                    None,
                    None,
                    request.reviewed_head.clone(),
                    request.requested_by_call_id.clone(),
                    request.changed_files.clone(),
                )?;
                let decision = facts.run.decide(TaskCommand::BeginIntegratedReview {
                    target: IntegratedReviewTarget {
                        review_round_id: round.id.clone(),
                        reviewed_head: request.reviewed_head,
                        changed_files: request.changed_files,
                    },
                })?;
                facts.run.state = decision.next_state;
                bump_task_revision(&mut facts)?;
                facts.reviews.push(round);
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .reviews
            .into_iter()
            .find(|round| round.requested_by_call_id == requested_by_call_id)
            .context("committed integrated ReviewRound is missing")
    }

    pub(crate) async fn authorize_reviewer_spawn(
        &self,
        root_thread_id: &str,
        requested_by_call_id: &str,
        agent_id: &str,
    ) -> Result<ReviewRoundRecord> {
        let requested_by_call_id = requested_by_call_id.to_string();
        let committed_call_id = requested_by_call_id.clone();
        let agent_id = agent_id.to_string();
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                let round = facts
                    .reviews
                    .iter_mut()
                    .find(|round| round.requested_by_call_id == requested_by_call_id)
                    .context("pending review round not found")?;
                if round.kind() != ReviewRoundStateKind::PendingDispatch {
                    bail!("reviewer spawn authorization is already consumed");
                }
                apply_review_command(
                    round,
                    ReviewRoundCommand::Dispatch {
                        reviewer_thread_id: agent_id,
                    },
                    super::super::ids::unix_seconds(),
                )?;
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .reviews
            .into_iter()
            .find(|round| round.requested_by_call_id == committed_call_id)
            .context("committed ReviewRound is missing")
    }

    pub(crate) async fn apply_review_command(
        &self,
        review_round_id: &str,
        command: ReviewRoundCommand,
    ) -> Result<ReviewRoundRecord> {
        let root_thread_id = self
            .root_for_review(review_round_id)
            .await
            .context("review round not found")?;
        let review_round_id = review_round_id.to_string();
        let committed_id = review_round_id.clone();
        let committed = self
            .commit_facts(&root_thread_id, move |current| {
                let mut facts = current.clone();
                let round = facts
                    .reviews
                    .iter_mut()
                    .find(|round| round.id == review_round_id)
                    .context("review round not found")?;
                apply_review_command(round, command, super::super::ids::unix_seconds())?;
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .reviews
            .into_iter()
            .find(|round| round.id == committed_id)
            .context("committed ReviewRound is missing")
    }

    pub(crate) async fn fail_reviewer_spawn(
        &self,
        root_thread_id: &str,
        agent_id: Option<&str>,
        requested_by_call_id: &str,
        error: &str,
    ) -> Result<()> {
        let agent_id = agent_id.map(ToOwned::to_owned);
        let requested_by_call_id = requested_by_call_id.to_string();
        let error = error.to_string();
        self.commit_facts(root_thread_id, move |current| {
            let mut facts = current.clone();
            let round_index = facts
                .reviews
                .iter()
                .position(|round| round.requested_by_call_id == requested_by_call_id)
                .context("pending review round not found")?;
            let round_id = facts.reviews[round_index].id.clone();
            let scope = facts.reviews[round_index].scope;
            let work_unit_id = facts.reviews[round_index].work_unit_id.clone();
            apply_review_command(
                &mut facts.reviews[round_index],
                ReviewRoundCommand::Fail {
                    reviewer_thread_id: agent_id,
                    error: error.clone(),
                    summary: error.clone(),
                },
                super::super::ids::unix_seconds(),
            )?;
            match scope {
                ReviewScope::Delivery => {
                    apply_work_unit_command_in_facts(
                        &mut facts,
                        work_unit_id
                            .as_deref()
                            .context("delivery review has no work unit")?,
                        WorkUnitCommand::ReviewFailed {
                            review_round_id: round_id,
                        },
                        super::super::ids::unix_seconds(),
                    )?;
                    bump_task_revision(&mut facts)?;
                }
                ReviewScope::Integrated => {
                    let decision = facts.run.decide(TaskCommand::ReturnToWorking {
                        summary: format!("reviewer spawn failed: {error}"),
                    })?;
                    facts.run.state = decision.next_state;
                    bump_task_revision(&mut facts)?;
                }
            }
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn settle_reviewer_turn_finished(
        &self,
        reviewer_agent_id: &str,
        outcome: &TurnOutcome,
    ) -> Result<()> {
        let matches = self
            .aggregates
            .read()
            .await
            .iter()
            .flat_map(|(root_thread_id, aggregate)| {
                aggregate
                    .facts
                    .reviews
                    .iter()
                    .filter(|round| {
                        round.reviewer_thread_id() == Some(reviewer_agent_id)
                            && round.kind().is_active()
                    })
                    .map(|round| (root_thread_id.clone(), round.id.clone()))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let (root_thread_id, review_round_id) = match matches.as_slice() {
            [] => return Ok(()),
            [value] => value.clone(),
            _ => bail!("reviewer owns multiple pending review rounds"),
        };
        let reviewer_agent_id = reviewer_agent_id.to_string();
        let outcome = outcome.clone();
        self.commit_facts(&root_thread_id, move |current| {
            let mut facts = current.clone();
            let round_index = facts
                .reviews
                .iter()
                .position(|round| round.id == review_round_id)
                .context("pending review round not found")?;
            let detail = reviewer_outcome_detail(&outcome);
            let command = match &outcome {
                TurnOutcome::Cancelled(_) => ReviewRoundCommand::Cancel {
                    reviewer_thread_id: Some(reviewer_agent_id.clone()),
                    reason: detail.clone(),
                    summary: detail.clone(),
                },
                TurnOutcome::Completed(_)
                | TurnOutcome::Failed(_)
                | TurnOutcome::BudgetLimited(_) => ReviewRoundCommand::Fail {
                    reviewer_thread_id: Some(reviewer_agent_id.clone()),
                    error: detail.clone(),
                    summary: detail.clone(),
                },
            };
            let scope = facts.reviews[round_index].scope;
            let work_unit_id = facts.reviews[round_index].work_unit_id.clone();
            let round_id = facts.reviews[round_index].id.clone();
            apply_review_command(
                &mut facts.reviews[round_index],
                command,
                super::super::ids::unix_seconds(),
            )?;
            match scope {
                ReviewScope::Delivery => {
                    let work_unit_id = work_unit_id.context("delivery review has no work unit")?;
                    if facts
                        .work_units
                        .iter()
                        .find(|unit| unit.id == work_unit_id)
                        .is_some_and(|unit| unit.kind() == WorkUnitStateKind::WaitingReview)
                    {
                        apply_work_unit_command_in_facts(
                            &mut facts,
                            &work_unit_id,
                            WorkUnitCommand::ReviewFailed {
                                review_round_id: round_id,
                            },
                            super::super::ids::unix_seconds(),
                        )?;
                    }
                }
                ReviewScope::Integrated => {
                    let keep_reviewing = matches!(
                        &outcome,
                        TurnOutcome::Cancelled(value)
                            if value.cause()
                                == &pl_protocol::TurnCancellationCause::UserRequested
                    );
                    if facts.run.kind() == TaskRunStateKind::Reviewing && !keep_reviewing {
                        let decision = facts
                            .run
                            .decide(TaskCommand::ReturnToWorking { summary: detail })?;
                        facts.run.state = decision.next_state;
                        bump_task_revision(&mut facts)?;
                    }
                }
            }
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn cancel_integrated_review(
        &self,
        root_thread_id: &str,
        review_round_id: &str,
        reason: &str,
        expected_revision: u64,
        expected_generation: u64,
    ) -> Result<TaskRun> {
        let reason = reason.trim().to_string();
        if reason.is_empty() {
            bail!("cancelIntegratedReview requires a non-empty reason");
        }
        let review_round_id = review_round_id.to_string();
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                ensure_task_version(&facts.run, expected_revision, expected_generation)?;
                let target = facts
                    .run
                    .state
                    .review_target()
                    .context("cancelIntegratedReview requires reviewing state")?;
                if target.review_round_id != review_round_id {
                    bail!("reviewRoundId does not match the frozen integrated review target");
                }
                let round = facts
                    .reviews
                    .iter_mut()
                    .find(|round| round.id == review_round_id)
                    .context("integrated review round not found")?;
                if round.kind().is_active() {
                    apply_review_command(
                        round,
                        ReviewRoundCommand::Cancel {
                            reviewer_thread_id: round.reviewer_thread_id().map(ToOwned::to_owned),
                            reason: reason.clone(),
                            summary: reason.clone(),
                        },
                        super::super::ids::unix_seconds(),
                    )?;
                }
                let decision = facts
                    .run
                    .decide(TaskCommand::ReturnToWorking { summary: reason })?;
                facts.run.state = decision.next_state;
                bump_task_revision(&mut facts)?;
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        Ok(committed.run)
    }

    pub(crate) async fn complete_task_review(
        &self,
        root_thread_id: &str,
        reviewer_agent_id: &str,
        review: AgentReview,
        file_reviews: ReviewFileCoverage,
    ) -> Result<ReviewRoundRecord> {
        let reviewer_agent_id = reviewer_agent_id.to_string();
        let committed_reviewer_id = reviewer_agent_id.clone();
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                let round_index = pending_review_index(&facts, &reviewer_agent_id)?;
                if facts.reviews[round_index].kind() != ReviewRoundStateKind::Running {
                    bail!("reviewer Thread does not match the pending review");
                }
                let coverage = prepare_file_reviews(
                    facts.reviews[round_index]
                        .file_reviews
                        .as_ref()
                        .context("review round has no file coverage snapshot")?,
                    file_reviews,
                    true,
                )?;
                let round_id = facts.reviews[round_index].id.clone();
                let scope = facts.reviews[round_index].scope;
                match scope {
                    ReviewScope::Delivery => {
                        settle_delivery_review(&mut facts, round_index, &review, &round_id)?;
                        bump_task_revision(&mut facts)?;
                    }
                    ReviewScope::Integrated => {
                        let round = &facts.reviews[round_index];
                        let target_matches =
                            facts.run.state.review_target().is_some_and(|target| {
                                target.review_round_id == round.id
                                    && target.reviewed_head == round.reviewed_head
                            });
                        if !target_matches {
                            bail!("integrated review no longer matches the durable Task target");
                        }
                        match review.verdict {
                            ReviewVerdict::Pass => {}
                            ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked => {
                                let decision = facts.run.decide(TaskCommand::ReturnToWorking {
                                    summary: review.summary.clone(),
                                })?;
                                facts.run.state = decision.next_state;
                                bump_task_revision(&mut facts)?;
                            }
                            ReviewVerdict::Pending | ReviewVerdict::Failed => {
                                bail!("reviewer cannot select pending or failed")
                            }
                        }
                    }
                }
                let command = match review.verdict {
                    ReviewVerdict::Pass => ReviewRoundCommand::Pass {
                        reviewer_thread_id: reviewer_agent_id.clone(),
                        summary: review.summary.clone(),
                    },
                    ReviewVerdict::ChangesRequired => ReviewRoundCommand::RequireChanges {
                        reviewer_thread_id: reviewer_agent_id.clone(),
                        summary: review.summary.clone(),
                    },
                    ReviewVerdict::Blocked => ReviewRoundCommand::Block {
                        reviewer_thread_id: reviewer_agent_id.clone(),
                        summary: review.summary.clone(),
                    },
                    ReviewVerdict::Pending | ReviewVerdict::Failed => {
                        bail!("reviewer cannot select pending or failed")
                    }
                };
                let round = &mut facts.reviews[round_index];
                apply_review_command(round, command, super::super::ids::unix_seconds())?;
                round.design_references = review.design_references;
                round.findings = review.findings;
                round.file_reviews = Some(coverage);
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .reviews
            .into_iter()
            .find(|round| round.reviewer_thread_id() == Some(committed_reviewer_id.as_str()))
            .context("completed ReviewRound is missing")
    }

    pub(crate) async fn record_review_rejection(
        &self,
        root_thread_id: &str,
        reviewer_agent_id: &str,
        file_reviews: ReviewFileCoverage,
    ) -> Result<ReviewRoundRecord> {
        let reviewer_agent_id = reviewer_agent_id.to_string();
        let committed_reviewer_id = reviewer_agent_id.clone();
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                let round_index = pending_review_index(&facts, &reviewer_agent_id)?;
                if facts.reviews[round_index].kind() != ReviewRoundStateKind::Running {
                    bail!("reviewer Thread does not match the pending review");
                }
                let coverage = prepare_file_reviews(
                    facts.reviews[round_index]
                        .file_reviews
                        .as_ref()
                        .context("review round has no file coverage snapshot")?,
                    file_reviews,
                    false,
                )?;
                let round = &mut facts.reviews[round_index];
                round.file_reviews = Some(coverage);
                round.revision = round
                    .revision
                    .checked_add(1)
                    .context("ReviewRound revision overflow")?;
                round.updated_at = super::super::ids::unix_seconds();
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        committed
            .reviews
            .into_iter()
            .find(|round| round.reviewer_thread_id() == Some(committed_reviewer_id.as_str()))
            .context("rejected ReviewRound is missing")
    }
}
