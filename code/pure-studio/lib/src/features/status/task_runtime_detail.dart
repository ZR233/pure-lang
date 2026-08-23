import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/runtime_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import '../interaction/task_recovery_dialog.dart';
import 'status_detail_popover.dart';

class TaskRuntimeDetail extends StatelessWidget {
  const TaskRuntimeDetail({
    required this.task,
    required this.rootThreadId,
    super.key,
  });

  final TaskRuntimeView task;
  final String rootThreadId;

  @override
  Widget build(BuildContext context) {
    return ConstrainedBox(
      key: StudioDriverKeys.taskRuntime(task.runId),
      constraints: const BoxConstraints(maxHeight: 520),
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            StatusDetailPanel(
              title: context.l10n.statusTaskSection,
              children: [
                StatusDetailRow(
                  key: StudioDriverKeys.taskPhase(task.runId, task.state.kind),
                  label: context.taskPhaseLabel(task.state.kind),
                  value: task.stateSummary,
                  valueKey: StudioDriverKeys.taskStatus(
                    task.runId,
                    task.stateSummary,
                  ),
                  valueMaxLines: 2,
                ),
                StatusDetailRow(label: 'Task ID', value: task.runId),
                StatusDetailRow(
                  label: 'Generation',
                  value: task.generation.toString(),
                ),
                if (_canRecoverConversation(task))
                  Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: OutlinedButton.icon(
                      key: StudioDriverKeys.taskRecoveryOpen,
                      onPressed: () =>
                          showTaskRecoveryDialog(context, rootThreadId),
                      icon: const Icon(Icons.history_outlined, size: 17),
                      label: Text(context.l10n.taskRecoveryDialogTitle),
                    ),
                  ),
              ],
            ),
            if (task.issues.isNotEmpty) ...[
              const _SectionDivider(),
              StatusDetailPanel(
                title: context.l10n.statusTaskFailures,
                children: [
                  for (final issue in task.issues)
                    _TaskIssueDetail(issue: issue),
                ],
              ),
            ],
            if (task.workUnits.isNotEmpty) ...[
              const _SectionDivider(),
              StatusDetailPanel(
                title: context.l10n.statusTaskWorkUnits,
                children: [
                  for (final unit in task.workUnits)
                    _WorkUnitDetail(task: task, unit: unit),
                ],
              ),
            ],
            if (task.completions.isNotEmpty) ...[
              const _SectionDivider(),
              StatusDetailPanel(
                title: context.l10n.statusTaskCompletions,
                children: [
                  for (final completion in task.completions)
                    _CompletionDetail(completion: completion),
                ],
              ),
            ],
            if (task.merges.isNotEmpty) ...[
              const _SectionDivider(),
              StatusDetailPanel(
                title: context.l10n.statusTaskMerges,
                children: [
                  for (final merge in task.merges) _MergeDetail(merge: merge),
                ],
              ),
            ],
            if (task.reviews.isNotEmpty) ...[
              const _SectionDivider(),
              StatusDetailPanel(
                title: context.l10n.statusTaskReviews,
                children: [
                  for (final review in task.reviews)
                    _ReviewDetail(review: review),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}

bool _canRecoverConversation(TaskRuntimeView task) {
  if (!task.isActive) return false;
  return task.issues.any(
        (issue) => issue.state is OpenRecoverableTaskIssueView,
      ) ||
      task.workUnits.any(
        (unit) => const {
          TaskWorkUnitStateKind.paused,
          TaskWorkUnitStateKind.failed,
        }.contains(unit.state.kind),
      );
}

class _TaskIssueDetail extends StatelessWidget {
  const _TaskIssueDetail({required this.issue});

  final TaskIssueView issue;

  @override
  Widget build(BuildContext context) {
    final metadata = [
      issue.providerKind ?? issue.category,
      ?issue.code,
      if (issue.httpStatus case final status?) 'HTTP $status',
    ].join(' · ');
    return Padding(
      key: StudioDriverKeys.taskIssue(issue.id),
      padding: const EdgeInsets.only(bottom: 9),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ItemHeading(
            title: '${issue.sourceRole} · ${issue.sourceAgentId}',
            status: issue.isFatal
                ? context.l10n.statusTaskFailed
                : context.l10n.statusTaskRecoverable,
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskError,
            value: issue.message,
            valueMaxLines: 4,
          ),
          if (metadata.isNotEmpty)
            StatusDetailRow(label: 'Provider', value: metadata),
          StatusDetailRow(
            label: context.l10n.statusTaskNextStep,
            value: issue.isFatal
                ? context.l10n.statusTaskFatalHint
                : context.l10n.statusTaskRecoverableHint,
            valueMaxLines: 2,
          ),
        ],
      ),
    );
  }
}

class _CompletionDetail extends StatelessWidget {
  const _CompletionDetail({required this.completion});

  final TaskCompletionView completion;

  @override
  Widget build(BuildContext context) {
    return Padding(
      key: StudioDriverKeys.taskCompletion(completion.id),
      padding: const EdgeInsets.only(bottom: 9),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ItemHeading(
            title: '${completion.kind} · ${completion.executorAgentId}',
            status: context.taskStatusLabel(completion.status),
            titleKey: StudioDriverKeys.taskCompletionExecutor(completion.id),
            statusKey: StudioDriverKeys.taskCompletionStatus(completion.id),
          ),
          StatusDetailRow(
            key: StudioDriverKeys.taskCompletionRevision(
              completion.id,
              completion.revision,
            ),
            label: context.l10n.statusTaskCompletionRevision,
            value: '${completion.revision}',
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskVerification,
            value: completion.verificationSummary,
            valueMaxLines: 3,
          ),
          if (completion.headCommit case final commit?)
            StatusDetailRow(
              label: context.l10n.statusTaskCommit,
              value: _shortCommit(commit),
            ),
        ],
      ),
    );
  }
}

class _WorkUnitDetail extends StatelessWidget {
  const _WorkUnitDetail({required this.task, required this.unit});

  final TaskRuntimeView task;
  final TaskWorkUnitView unit;

  @override
  Widget build(BuildContext context) {
    final completion = task.completions
        .where((completion) => completion.workUnitId == unit.id)
        .lastOrNull;
    return Padding(
      key: StudioDriverKeys.taskWorkUnit(unit.id),
      padding: const EdgeInsets.only(bottom: 9),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ItemHeading(
            title: unit.title,
            status: context.taskStatusLabel(unit.status),
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskSource,
            value: unit.agentId ?? '-',
          ),
          StatusDetailRow(
            key: StudioDriverKeys.taskWorkUnitExecution(unit.id),
            label: context.l10n.statusTaskExecution,
            value: context.taskStatusLabel(unit.executionStatus),
          ),
          StatusDetailRow(
            key: StudioDriverKeys.taskWorkUnitBudgetSlice(unit.id),
            label: context.l10n.statusTaskBudgetSlice,
            value: context.l10n.statusTaskBudgetSliceValue(
              unit.budgetSliceCount,
              unit.budgetSliceLimit,
            ),
          ),
          if (unit.budgetLimit case final budgetLimit?)
            StatusDetailRow(
              label: context.l10n.statusTaskBudget,
              value:
                  '${context.taskBudgetKindLabel(budgetLimit.kind)} · '
                  '${context.l10n.statusTaskBudgetUsage(budgetLimit.usage.modelSteps, budgetLimit.usage.toolCalls, budgetLimit.usage.waitCalls, budgetLimit.usage.elapsedMs.toString())}',
              valueMaxLines: 2,
            ),
          StatusDetailRow(
            key: StudioDriverKeys.taskWorkUnitContinuation(unit.id),
            label: context.l10n.statusTaskContinuation,
            value: context.taskContinuationStateLabel(unit.continuationState),
          ),
          if (unit.executionError case final error?)
            StatusDetailRow(
              label: context.l10n.statusTaskError,
              value: error,
              valueMaxLines: 3,
            ),
          StatusDetailRow(
            label: context.l10n.statusTaskWorktree,
            value: unit.worktreePath,
            valueMaxLines: 2,
          ),
          if (completion?.headCommit case final commit?)
            StatusDetailRow(
              label: context.l10n.statusTaskCommit,
              value: _shortCommit(commit),
            ),
        ],
      ),
    );
  }
}

class _MergeDetail extends StatelessWidget {
  const _MergeDetail({required this.merge});

  final TaskMergeView merge;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 9),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ItemHeading(title: merge.executorAgentId, status: merge.method.name),
          StatusDetailRow(
            label: context.l10n.statusTaskCompletionRevision,
            value: '${merge.completionRevision}',
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskPreviousHead,
            value: _shortCommit(merge.expectedPreviousHead),
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskDeliveryHead,
            value: _shortCommit(merge.deliveryHead),
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskResultingHead,
            value: _shortCommit(merge.resultingHead),
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskSummary,
            value: merge.summary,
            valueMaxLines: 3,
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskCleanup,
            value: [merge.cleanupStatus, ?merge.cleanupDetail].join(' · '),
            valueMaxLines: 2,
          ),
        ],
      ),
    );
  }
}

class _ReviewDetail extends StatelessWidget {
  const _ReviewDetail({required this.review});

  final TaskReviewView review;

  @override
  Widget build(BuildContext context) {
    return Padding(
      key: StudioDriverKeys.taskReview(review.id),
      padding: const EdgeInsets.only(bottom: 9),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ItemHeading(
            title: '${review.scope.name} · #${review.round}',
            status: context.taskStatusLabel(review.verdict),
            statusKey: StudioDriverKeys.taskReviewVerdict(review.id),
          ),
          if (review.reviewerAgentId case final reviewerAgentId?)
            StatusDetailRow(
              key: StudioDriverKeys.taskReviewReviewer(review.id),
              label: context.l10n.statusTaskReviews,
              value: reviewerAgentId,
            ),
          StatusDetailRow(
            label: context.l10n.statusTaskScope,
            value: review.scope.name,
          ),
          if (review.completionRevision case final revision?)
            StatusDetailRow(
              label: context.l10n.statusTaskCompletionRevision,
              value: '$revision',
            ),
          StatusDetailRow(
            label: context.l10n.statusTaskHead,
            value: _shortCommit(review.reviewedHead),
          ),
          if (review.summary case final summary?)
            StatusDetailRow(
              label: review.reviewerAgentId ?? context.l10n.statusTaskReviews,
              value: summary,
              valueMaxLines: 3,
            ),
          for (final reference in review.designReferences)
            StatusDetailRow(
              label: 'design',
              value: '${reference.path}#${reference.section}',
              valueMaxLines: 2,
            ),
          for (final (index, finding) in review.findings.indexed)
            Padding(
              key: StudioDriverKeys.taskFinding(review.id, index),
              padding: const EdgeInsets.only(top: 4),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  _ItemHeading(
                    title: finding.title,
                    status: finding.severity,
                    statusKey: StudioDriverKeys.taskFindingSeverity(
                      review.id,
                      index,
                    ),
                  ),
                  StatusDetailRow(
                    label: context.l10n.statusTaskFindings,
                    value: finding.body,
                    valueMaxLines: 4,
                  ),
                  if (finding.recommendation.trim().isNotEmpty)
                    StatusDetailRow(
                      label: context.l10n.statusTaskRecommendation,
                      value: finding.recommendation,
                      valueMaxLines: 6,
                    ),
                  if (finding.path case final path?)
                    StatusDetailRow(
                      label: context.l10n.statusTaskSource,
                      value: finding.line == null
                          ? path
                          : '$path:${finding.line}',
                      valueMaxLines: 2,
                    ),
                ],
              ),
            ),
        ],
      ),
    );
  }
}

class _ItemHeading extends StatelessWidget {
  const _ItemHeading({
    required this.title,
    required this.status,
    this.titleKey,
    this.statusKey,
  });

  final String title;
  final String status;
  final Key? titleKey;
  final Key? statusKey;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Row(
        children: [
          Expanded(
            child: Text(
              title,
              key: titleKey,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          const SizedBox(width: 10),
          Text(
            status,
            key: statusKey,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.labelSmall?.copyWith(
              color: StudioColors.clayDeep,
            ),
          ),
        ],
      ),
    );
  }
}

class _SectionDivider extends StatelessWidget {
  const _SectionDivider();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: Divider(height: 1, color: context.studioLine),
    );
  }
}

String _shortCommit(String value) =>
    value.length <= 10 ? value : value.substring(0, 10);
