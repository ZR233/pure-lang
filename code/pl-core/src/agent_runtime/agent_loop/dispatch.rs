//! Actor command dispatch with an independent polling frame for each command.

use super::{AgentLoop, AgentLoopCommand, AgentRuntimeError, AgentRuntimeHost, AgentState};
use crate::ThreadRepository;
use futures::{FutureExt, future::BoxFuture};
use std::ops::ControlFlow;

impl<H: AgentRuntimeHost> AgentLoop<H> {
    pub(super) fn dispatch_command(
        &mut self,
        command: AgentLoopCommand,
    ) -> BoxFuture<'_, ControlFlow<()>> {
        match command {
            AgentLoopCommand::AdmitToolTasks {
                turn_id,
                tasks,
                reply,
            } => async move {
                let result = self.admit_tool_tasks(&turn_id, tasks).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::SelectToolTaskResults {
                turn_id,
                ids,
                deadline,
                reply,
            } => async move {
                let result = self
                    .select_tool_task_results(&turn_id, &ids, deadline)
                    .boxed()
                    .await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ToolTaskOutput { id, delta } => async move {
                if let Err(error) = self.append_session_task_output(&id, &delta).boxed().await {
                    tracing::warn!(%error, task_id = %id, "task output preview projection failed");
                }
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ListToolTasks {
                status,
                cursor,
                reply,
            } => async move {
                let _ = reply.send(Ok(self.state.session.tasks.list(status, cursor.as_deref())));
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ReadToolTaskResult { id, reply } => async move {
                let result = self.read_complete_task(&id).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::GetToolTask { id, reply } => async move {
                let result = self
                    .state
                    .session
                    .tasks
                    .get(&id)
                    .cloned()
                    .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()));
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::CancelToolTask { id, reply } => async move {
                let result = self.cancel_session_task(&id).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ToolTaskRunning { id, reply } => async move {
                let result = self.mark_session_task_running(&id).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::WaitSessionEvents { reply } => async move {
                self.wait_session_events(reply);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::PublishSessionEvent {
                source,
                id,
                event,
                reply,
            } => async move {
                let result = self
                    .publish_session_event(&source, &id, *event)
                    .boxed()
                    .await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::Submit { request, reply } => async move {
                // `.boxed()`：把命令处理状态机放堆上，避免 debug 构建下
                // 全部命令分支内联进 run 的 select! 状态机导致超大栈帧。
                let result = self.submit(request).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::SubmitCurrentSession {
                root_agent_id,
                request,
                reply,
            } => async move {
                let result = self
                    .submit_current_session(root_agent_id, request)
                    .boxed()
                    .await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::SubmitInteractionContinuation {
                root_agent_id,
                request,
                reply,
            } => async move {
                let result = self
                    .submit_interaction_continuation(root_agent_id, *request)
                    .boxed()
                    .await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ReconfigureIdleRole { role, reply } => async move {
                let result = self.reconfigure_idle_role(role).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ChangeIdleThreadMode { mode_id, reply } => async move {
                let result = self.change_idle_thread_mode(mode_id).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::PreviewConversationRecovery { target, reply } => async move {
                let _ = reply.send(self.preview_conversation_recovery(target));
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::RecoverConversation { request, reply } => async move {
                let result = self.recover_conversation(request).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::RecoverFaulted { reply } => async move {
                let result = self.recover_faulted().boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::CancelTurn { turn_id, reply } => async move {
                let result = self.cancel_turn(turn_id).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::SetActivity {
                turn_id,
                activity,
                reply,
            } => async move {
                let result = self.set_activity(turn_id, activity).boxed().await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::Checkpoint { checkpoint, reply } => async move {
                let result = self.checkpoint(*checkpoint).boxed().await;
                if let Err(error) = &result {
                    tracing::error!(
                        agent_id = %self.state.snapshot.identity.id,
                        error = %error,
                        "checkpoint was rejected without faulting the agent"
                    );
                }
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::RecordThreadFacts {
                thread_id,
                facts,
                reply,
            } => async move {
                let result = self.record_thread_facts(thread_id, facts).boxed().await;
                if let Err(error) = &result {
                    tracing::error!(
                        agent_id = %self.state.snapshot.identity.id,
                        error = %error,
                        "thread facts were rejected without faulting the agent"
                    );
                }
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::Snapshot { reply } => async move {
                let _ = reply.send(Ok(self.state.snapshot.clone()));
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ReportProgress {
                stage,
                summary,
                next_step,
                detail,
                reply,
            } => async move {
                let result = self
                    .report_progress(stage, summary, next_step, detail)
                    .boxed()
                    .await;
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ReadThreadContext { reply } => async move {
                let _ = reply.send(Ok(self.state.session.clone()));
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::ReadSubmissions {
                offset,
                limit,
                reply,
            } => async move {
                let result = self.read_submissions(offset, limit);
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::StartPendingInputs { reply } => async move {
                self.dispatch_enabled = true;
                let _ = reply.send(Ok(()));
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::Close {
                workspace_disposition,
                reply,
            } => async move {
                let result = self.close(workspace_disposition).boxed().await;
                self.wake_session_waiter();
                let _ = reply.send(result);
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::TurnFinished(completion) => async move {
                self.finish_turn(*completion).boxed().await;
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::Evict { reply } => async move {
                // 与输入接受共享 owner 命令序列，避免检查后又提交了新事实。
                let snapshot = &self.state.snapshot;
                if (snapshot.identity.parent_id.is_some()
                    && !matches!(snapshot.state, AgentState::Closed(_)))
                    || self.active.is_some()
                    || snapshot.pending_inputs > 0
                    || self
                        .session_waiter
                        .as_ref()
                        .is_some_and(|waiter| !waiter.is_closed())
                    || self.task_resources.has_work()
                    || self.state.session.tasks.active_ids().next().is_some()
                    || self.session_runtime.has_sources()
                    || self.closing.is_some()
                    || matches!(snapshot.state, AgentState::Closing(_))
                    || snapshot.active_turn_id().is_some()
                    || snapshot.state.is_budget_paused()
                    || !self
                        .host
                        .repository()
                        .is_durable(&snapshot.identity.id, snapshot.revision)
                {
                    let _ = reply.send(Err(AgentRuntimeError::InvalidInput(format!(
                        "agent {} is busy or has unsaved facts",
                        snapshot.identity.id
                    ))));
                } else {
                    let _ = reply.send(Ok(snapshot.clone()));
                    return ControlFlow::Break(());
                }
                ControlFlow::Continue(())
            }
            .boxed(),
            AgentLoopCommand::Shutdown { reply } => async move {
                let result = self.shutdown().boxed().await;
                let finished = result.is_ok();
                if let Err(error) = &result {
                    if matches!(self.state.snapshot.state, AgentState::Closing(_)) {
                        let _ = self.record_close_error(error.clone()).boxed().await;
                    } else {
                        self.fault(error.to_string()).boxed().await;
                    }
                }
                let _ = reply.send(result);
                if finished {
                    return ControlFlow::Break(());
                }
                ControlFlow::Continue(())
            }
            .boxed(),
        }
    }
}
