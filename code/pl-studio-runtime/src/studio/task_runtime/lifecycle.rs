use super::*;

impl TaskRuntime {
    /// 新任务先进入内存 owner，再由共享 writer 异步创建 SQLite 冷基线。
    pub(crate) async fn create_task(&self, input: CreateTaskRun) -> Result<TaskRun> {
        for (label, value) in [
            ("projectId", input.project_id.as_str()),
            ("rootThreadId", input.root_thread_id.as_str()),
            ("request", input.request.as_str()),
            ("workspaceRoot", input.workspace_root.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("{label} must not be empty");
            }
        }
        let now = super::super::ids::unix_seconds();
        let run = TaskRun {
            context: TaskContext {
                id: super::super::ids::new_id("task-run"),
                project_id: input.project_id,
                root_thread_id: input.root_thread_id.clone(),
                request: input.request.trim().to_string(),
                plan: None,
                workspace_root: input.workspace_root,
            },
            state: TaskRunState::new(),
            revision: 0,
            created_at: now,
            updated_at: now,
        };
        let facts = task_projection::LoadedTaskAggregate::new(run.clone())?;
        let committed = self
            .commit_hot(&input.root_thread_id, move |current| {
                if current.is_some_and(|facts| !facts.run.kind().is_terminal()) {
                    bail!("root Thread already owns an unfinished TaskRun");
                }
                Ok(facts)
            })
            .await?;
        Ok(committed.run)
    }

    /// 提交只改变 TaskRun 的纯状态机命令。
    pub(crate) async fn apply_run_command(
        &self,
        root_thread_id: &str,
        expected_revision: u64,
        expected_generation: u64,
        command: TaskCommand,
    ) -> Result<TaskRun> {
        let committed = self
            .commit_hot(root_thread_id, move |current| {
                let mut facts = current
                    .context("active Task aggregate is not resident")?
                    .clone();
                ensure_task_version(&facts.run, expected_revision, expected_generation)?;
                let decision = facts.run.decide(command)?;
                facts.run.state = decision.next_state;
                facts.run.revision = facts
                    .run
                    .revision
                    .checked_add(1)
                    .context("TaskRun revision overflow")?;
                facts.run.updated_at = super::super::ids::unix_seconds();
                facts.refresh_run_projection()?;
                Ok(facts)
            })
            .await?;
        Ok(committed.run)
    }

    /// 冻结计划并进入等待确认；Interaction 由 Thread owner 使用同一热事实发布。
    pub(crate) async fn submit_plan(
        &self,
        root_thread_id: &str,
        content: &str,
        expected_revision: u64,
        expected_generation: u64,
    ) -> Result<TaskRun> {
        let content = content.trim();
        if content.is_empty() {
            bail!("task plan must not be empty");
        }
        let content = content.to_string();
        let committed = self
            .commit_hot(root_thread_id, move |current| {
                let mut facts = current
                    .context("active Task aggregate is not resident")?
                    .clone();
                ensure_task_version(&facts.run, expected_revision, expected_generation)?;
                let plan_revision = facts
                    .run
                    .plan
                    .as_ref()
                    .map_or(1, |plan| plan.revision.saturating_add(1));
                let plan = TaskPlan {
                    content,
                    revision: plan_revision,
                    submitted_at: super::super::ids::unix_seconds(),
                };
                let decision = facts
                    .run
                    .decide(TaskCommand::SubmitPlan { plan_revision })?;
                facts.run.context.plan = Some(plan);
                facts.run.state = decision.next_state;
                facts.run.revision = facts
                    .run
                    .revision
                    .checked_add(1)
                    .context("TaskRun revision overflow")?;
                facts.run.updated_at = super::super::ids::unix_seconds();
                facts.refresh_run_projection()?;
                Ok(facts)
            })
            .await?;
        Ok(committed.run)
    }

    /// 保存停止事实并推进执行代次；主任务状态保持不变。
    pub(crate) async fn stop_task(
        &self,
        root_thread_id: &str,
        origin: TaskStopOrigin,
        reason: &TaskStopReason,
    ) -> Result<TaskRun> {
        let aggregate = self
            .aggregate(root_thread_id)
            .await
            .context("active Task aggregate is not resident")?;
        if aggregate.facts.run.kind().is_terminal() {
            bail!("completed TaskRun cannot be stopped");
        }
        let expected_revision = aggregate.facts.run.revision;
        let expected_generation = aggregate.facts.run.generation();
        let now = super::super::ids::unix_seconds();
        let stop_event = TaskStopEventFact {
            id: super::super::ids::new_id("task-stop"),
            task_run_id: aggregate.facts.run.id.clone(),
            generation: expected_generation
                .checked_add(1)
                .context("Task generation overflow")?,
            origin: origin.as_str().to_string(),
            reason: reason.as_str().to_string(),
            source_turn_id: None,
            created_at: now,
        };
        let committed = self
            .commit_hot_with_stop_events(root_thread_id, vec![stop_event], move |current| {
                let mut facts = current
                    .context("active Task aggregate is not resident")?
                    .clone();
                ensure_task_version(&facts.run, expected_revision, expected_generation)?;
                let decision = facts.run.decide(TaskCommand::Stop)?;
                facts.run.state = decision.next_state;
                facts.run.revision = facts
                    .run
                    .revision
                    .checked_add(1)
                    .context("TaskRun revision overflow")?;
                facts.run.updated_at = now;
                facts.refresh_run_projection()?;
                Ok(facts)
            })
            .await?;
        Ok(committed.run)
    }

    /// 提交任务终态；失败终态在同一内存提交中收束所有未结束子事实。
    pub(crate) async fn complete_task(
        &self,
        root_thread_id: &str,
        expected_revision: u64,
        expected_generation: u64,
        outcome: TaskOutcome,
    ) -> Result<TaskRun> {
        let committed = self
            .commit_hot(root_thread_id, move |current| {
                let mut facts = current
                    .context("active Task aggregate is not resident")?
                    .clone();
                ensure_task_version(&facts.run, expected_revision, expected_generation)?;
                if let TaskOutcome::Failed { cause, .. } = &outcome {
                    let operation_id = format!("task-terminal:{}", facts.run.id);
                    let now = super::super::ids::unix_seconds();
                    for unit in &mut facts.work_units {
                        if unit.kind().is_terminal() {
                            continue;
                        }
                        let decision = unit.decide(
                            unit.revision,
                            WorkUnitCommand::FailExecution {
                                operation_id: operation_id.clone(),
                                detail: cause.clone(),
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
                                error: cause.clone(),
                                summary: cause.clone(),
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
                }
                let decision = facts.run.decide(TaskCommand::Complete { outcome })?;
                facts.run.state = decision.next_state;
                facts.run.revision = facts
                    .run
                    .revision
                    .checked_add(1)
                    .context("TaskRun revision overflow")?;
                facts.run.updated_at = super::super::ids::unix_seconds();
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        Ok(committed.run)
    }

    /// 将类型化 Agent 失败直接收束到 Task 聚合；SQLite 不参与角色处置。
    pub(crate) async fn record_agent_failure(
        &self,
        input: RecordTaskAgentFailure,
    ) -> Result<Option<TaskIssueSettlement>> {
        let Some(current) = self.aggregate(&input.root_thread_id).await else {
            return Ok(None);
        };
        if current.facts.run.kind().is_terminal() {
            return Ok(None);
        }
        if current
            .facts
            .issues
            .iter()
            .any(|issue| issue.source_turn_id == input.source_turn_id)
        {
            return Ok(Some(TaskIssueSettlement {
                terminalized: false,
            }));
        }

        let root_thread_id = input.root_thread_id.clone();
        let terminalized = input.disposition == TaskIssueDisposition::Fatal;
        self.commit_hot(&root_thread_id, move |current| {
            let mut facts = current
                .context("active Task aggregate is not resident")?
                .clone();
            if facts.run.kind().is_terminal()
                || facts
                    .issues
                    .iter()
                    .any(|issue| issue.source_turn_id == input.source_turn_id)
            {
                return Ok(facts);
            }
            let now = super::super::ids::unix_seconds();
            let issue_id = super::super::ids::new_id("task-issue");
            let work_unit_id = facts
                .work_units
                .iter()
                .find(|unit| {
                    unit.executor_thread_id.as_deref() == Some(input.source_thread_id.as_str())
                })
                .map(|unit| unit.id.clone());
            let review_round_id = facts
                .reviews
                .iter()
                .filter(|round| round.reviewer_thread_id() == Some(input.source_thread_id.as_str()))
                .max_by_key(|round| round.round)
                .map(|round| round.id.clone());
            facts.issues.push(TaskIssueRecord {
                id: issue_id.clone(),
                task_run_id: facts.run.id.clone(),
                source_thread_id: input.source_thread_id.clone(),
                source_turn_id: input.source_turn_id.clone(),
                source_agent_id: input.source_agent_id.clone(),
                source_role: input.source_role.clone(),
                work_unit_id,
                review_round_id,
                state: TaskIssueState::open_with_disposition(
                    input.failure.clone(),
                    input.disposition,
                ),
                revision: 0,
                created_at: now,
                updated_at: now,
            });
            if terminalized {
                let operation_id = format!("task-failure:{}", facts.run.id);
                settle_children_for_failure(
                    &mut facts,
                    &operation_id,
                    &input.failure.message,
                    now,
                )?;
                let decision = facts.run.decide(TaskCommand::Complete {
                    outcome: TaskOutcome::Failed {
                        kind: TaskFailureKind::Fatal,
                        summary: input.failure.message.clone(),
                        evidence: format!(
                            "Task issue {issue_id} from turn {}",
                            input.source_turn_id
                        ),
                        cause: input.failure.message.clone(),
                        completed_at: now,
                    },
                })?;
                facts.run.state = decision.next_state;
                facts.run.revision = facts
                    .run
                    .revision
                    .checked_add(1)
                    .context("TaskRun revision overflow")?;
                facts.run.updated_at = now;
            }
            facts.refresh_projection()?;
            Ok(facts)
        })
        .await?;
        Ok(Some(TaskIssueSettlement { terminalized }))
    }

    pub(in crate::studio) async fn resolve_issue(
        &self,
        root_thread_id: &str,
        input: ResolveTaskIssue<'_>,
    ) -> Result<TaskRun> {
        let issue_id = input.issue_id.trim().to_string();
        let operation_id = input.operation_id.to_string();
        let summary = input.summary.trim().to_string();
        let evidence = input.evidence.trim().to_string();
        let expected_revision = input.expected_revision;
        let expected_generation = input.expected_generation;
        if issue_id.is_empty() || summary.is_empty() || evidence.is_empty() {
            bail!("resolveIssue requires issueId, summary, and resolutionEvidence");
        }
        let committed = self
            .commit_facts(root_thread_id, move |current| {
                let mut facts = current.clone();
                ensure_task_version(&facts.run, expected_revision, expected_generation)?;
                let issue = facts
                    .issues
                    .iter_mut()
                    .find(|issue| issue.id == issue_id)
                    .context("Task issue not found")?;
                let decision = issue.state.decide(
                    &issue.id,
                    TaskIssueCommand::Resolve {
                        operation_id,
                        summary,
                        evidence,
                        resolved_at: super::super::ids::unix_seconds(),
                    },
                )?;
                if decision.changed() {
                    issue.state = decision.next_state();
                    issue.revision = issue
                        .revision
                        .checked_add(1)
                        .context("Task issue revision overflow")?;
                    issue.updated_at = super::super::ids::unix_seconds();
                }
                facts.run.revision = facts
                    .run
                    .revision
                    .checked_add(1)
                    .context("TaskRun revision overflow")?;
                facts.run.updated_at = super::super::ids::unix_seconds();
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        Ok(committed.run)
    }

    pub(crate) async fn resolve_recoverable_issues(&self, source_thread_id: &str) -> Result<()> {
        let root_thread_ids = self
            .aggregates
            .read()
            .await
            .iter()
            .filter(|(_, aggregate)| {
                aggregate.facts.issues.iter().any(|issue| {
                    issue.source_thread_id == source_thread_id
                        && issue.state.kind()
                            == super::super::task_coordinator::TaskIssueStateKind::OpenRecoverable
                })
            })
            .map(|(root_thread_id, _)| root_thread_id.clone())
            .collect::<Vec<_>>();
        for root_thread_id in root_thread_ids {
            let source_thread_id = source_thread_id.to_string();
            self.commit_facts(&root_thread_id, move |current| {
                let mut facts = current.clone();
                let now = super::super::ids::unix_seconds();
                for issue in &mut facts.issues {
                    if issue.source_thread_id != source_thread_id
                        || issue.state.kind()
                            != super::super::task_coordinator::TaskIssueStateKind::OpenRecoverable
                    {
                        continue;
                    }
                    let decision = issue.state.decide(
                        &issue.id,
                        TaskIssueCommand::Resolve {
                            operation_id: format!(
                                "resolve-task-failure:{}:{}",
                                source_thread_id, issue.source_turn_id
                            ),
                            summary: "后续执行已成功启动".to_string(),
                            evidence: format!("sourceThreadId={source_thread_id}"),
                            resolved_at: now,
                        },
                    )?;
                    if decision.changed() {
                        issue.state = decision.next_state();
                        issue.revision = issue
                            .revision
                            .checked_add(1)
                            .context("Task issue revision overflow")?;
                        issue.updated_at = now;
                    }
                }
                facts.refresh_projection()?;
                Ok(facts)
            })
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn resolve_active_completion_scope(
        &self,
        agent_id: &str,
        worktree_path: &str,
    ) -> Result<Option<DeliveryScope>> {
        let aggregates = self.aggregates.read().await;
        let mut matching = aggregates
            .values()
            .filter(|aggregate| aggregate.facts.run.kind() == TaskRunStateKind::Working)
            .flat_map(|aggregate| {
                aggregate
                    .facts
                    .work_units
                    .iter()
                    .filter(|unit| {
                        unit.executor_thread_id.as_deref() == Some(agent_id)
                            && workspace_paths_match(
                                std::path::Path::new(&unit.worktree_path),
                                std::path::Path::new(worktree_path),
                            )
                    })
                    .map(|unit| DeliveryScope {
                        run: aggregate.facts.run.clone(),
                        work_unit: unit.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        match matching.len() {
            0 => Ok(None),
            1 => Ok(matching.pop()),
            _ => bail!("ambiguous active completion scope for executor worktree"),
        }
    }
}
