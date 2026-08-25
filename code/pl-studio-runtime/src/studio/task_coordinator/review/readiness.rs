//! Task 成功完成门槛的统一只读判断。

use super::super::{
    MergeRecord, ReviewRoundRecord, TaskRun, TaskRunStateKind, WorkCompletionRecord, WorkUnit,
    current_work_units,
};
use super::{ModelExecutionActivity, integrated_review_gate_now};
use crate::StudioIntegratedReviewGate;

#[derive(Debug, Clone)]
pub(in crate::studio::task_coordinator) struct CompletionReadiness {
    review_gate: StudioIntegratedReviewGate,
    blockers: Vec<String>,
}

impl CompletionReadiness {
    pub(in crate::studio::task_coordinator) fn is_available(&self) -> bool {
        self.blockers.is_empty()
    }

    pub(in crate::studio::task_coordinator) fn review_gate(&self) -> &StudioIntegratedReviewGate {
        &self.review_gate
    }

    pub(in crate::studio::task_coordinator) fn blockers(&self) -> &[String] {
        &self.blockers
    }
}

pub(in crate::studio::task_coordinator) struct CompletionReadinessInput<'a> {
    pub(in crate::studio::task_coordinator) run: &'a TaskRun,
    pub(in crate::studio::task_coordinator) work_units: &'a [WorkUnit],
    pub(in crate::studio::task_coordinator) completions: &'a [WorkCompletionRecord],
    pub(in crate::studio::task_coordinator) reviews: &'a [ReviewRoundRecord],
    pub(in crate::studio::task_coordinator) merges: &'a [MergeRecord],
    pub(in crate::studio::task_coordinator) pending_interactions:
        &'a [pl_protocol::InteractionRequest],
    pub(in crate::studio::task_coordinator) todo: Option<&'a pl_protocol::TodoListSnapshot>,
    pub(in crate::studio::task_coordinator) execution: &'a ModelExecutionActivity,
}

pub(in crate::studio::task_coordinator) fn completion_readiness(
    input: CompletionReadinessInput<'_>,
) -> CompletionReadiness {
    let CompletionReadinessInput {
        run,
        work_units,
        completions,
        reviews,
        merges,
        pending_interactions,
        todo,
        execution,
    } = input;
    let review_gate = integrated_review_gate_now(run, work_units, completions, merges, reviews);
    let mut blockers = Vec::new();
    if !matches!(
        run.kind(),
        TaskRunStateKind::Working | TaskRunStateKind::Reviewing
    ) {
        blockers.push("成功完成要求任务处于 working 或 reviewing".to_string());
    }
    for unit in current_work_units(work_units) {
        if unit.kind() != super::super::WorkUnitStateKind::Completed {
            blockers.push(format!(
                "当前有效工作单 {} 尚未结算，状态为 {}",
                unit.id,
                unit.kind().as_str()
            ));
        }
    }
    for review in reviews.iter().filter(|review| review.kind().is_active()) {
        blockers.push(format!("审查轮 {} 尚未结束", review.id));
    }
    for interaction in pending_interactions {
        blockers.push(format!("用户交互 {} 尚未处理", interaction.interaction_id));
    }
    blockers.extend(todo_blockers(todo));
    for activity in execution
        .executor_turns
        .iter()
        .chain(&execution.reviewer_turns)
        .filter(|activity| activity.active_turn_id.is_some() || activity.pending_inputs > 0)
    {
        blockers.push(format!(
            "{} {} 仍有模型执行活动",
            activity.role, activity.agent_id
        ));
    }
    if let StudioIntegratedReviewGate::Required { reason } = &review_gate {
        blockers.push(format!("综合审查门槛尚未满足：{reason}"));
    }
    CompletionReadiness {
        review_gate,
        blockers,
    }
}

fn todo_blockers(todo: Option<&pl_protocol::TodoListSnapshot>) -> Vec<String> {
    todo.into_iter()
        .flat_map(|todo| &todo.items)
        .filter(|item| item.status != pl_protocol::TodoStatus::Completed)
        .map(|item| format!("待办尚未完成：{}", item.step))
        .collect()
}

#[cfg(test)]
mod tests {
    use pl_protocol::{TodoItem, TodoListSnapshot, TodoStatus};
    use pretty_assertions::assert_eq;

    use super::todo_blockers;

    #[test]
    fn completed_todo_does_not_block_task_completion() {
        let todo = todo_with([TodoStatus::Completed]);

        assert_eq!(todo_blockers(Some(&todo)), Vec::<String>::new());
    }

    #[test]
    fn pending_and_in_progress_todos_block_task_completion() {
        let todo = todo_with([TodoStatus::Pending, TodoStatus::InProgress]);

        assert_eq!(
            todo_blockers(Some(&todo)),
            vec![
                "待办尚未完成：step-0".to_string(),
                "待办尚未完成：step-1".to_string(),
            ]
        );
    }

    fn todo_with<const N: usize>(statuses: [TodoStatus; N]) -> TodoListSnapshot {
        TodoListSnapshot {
            call_id: "todo-call".to_string(),
            agent_id: None,
            path: Some("/root".to_string()),
            parent_path: None,
            explanation: None,
            items: statuses
                .into_iter()
                .enumerate()
                .map(|(index, status)| TodoItem {
                    step: format!("step-{index}"),
                    status,
                })
                .collect(),
        }
    }
}
