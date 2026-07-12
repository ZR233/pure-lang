import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/runtime_models.dart';
import '../../l10n/studio_l10n.dart';
import 'status_detail_popover.dart';

class TaskRuntimeDetail extends StatelessWidget {
  const TaskRuntimeDetail({required this.task, super.key});

  final TaskRuntimeView task;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        StatusDetailPanel(
          title: context.l10n.statusTaskSection,
          children: [
            StatusDetailRow(
              label: context.taskPhaseLabel(task.phase),
              value: task.statusMessage ?? task.runId,
              valueMaxLines: 2,
            ),
            StatusDetailRow(
              label: context.l10n.statusTaskBranch,
              value: task.branch,
            ),
            StatusDetailRow(
              label: context.l10n.statusTaskHead,
              value: _shortCommit(task.expectedHead),
            ),
          ],
        ),
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
              for (final review in task.reviews) _ReviewDetail(review: review),
            ],
          ),
        ],
      ],
    );
  }
}

class _WorkUnitDetail extends StatelessWidget {
  const _WorkUnitDetail({required this.task, required this.unit});

  final TaskRuntimeView task;
  final TaskWorkUnitView unit;

  @override
  Widget build(BuildContext context) {
    final outcome = task.agents
        .where((agent) => agent.agentId == unit.agentId)
        .firstOrNull;
    return Padding(
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
            value: outcome == null
                ? (unit.agentId ?? '-')
                : '${outcome.role} · ${outcome.initiatedBy}',
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskWorktree,
            value: unit.worktreePath,
            valueMaxLines: 2,
          ),
          if (outcome?.headCommit case final commit?)
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
          _ItemHeading(
            title: merge.agentId,
            status: context.taskStatusLabel(merge.status),
          ),
          if (merge.mergeCommit case final commit?)
            StatusDetailRow(
              label: context.l10n.statusTaskCommit,
              value: _shortCommit(commit),
            ),
          if (merge.conflictFiles.isNotEmpty)
            StatusDetailRow(
              label: context.l10n.statusTaskConflicts,
              value: merge.conflictFiles.join(', '),
              valueMaxLines: 3,
            ),
          if (merge.resolutionSummary case final summary?)
            StatusDetailRow(
              label: context.taskPhaseLabel('resolvingConflict'),
              value: summary,
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
      padding: const EdgeInsets.only(bottom: 9),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ItemHeading(
            title: '#${review.round}',
            status: context.taskStatusLabel(review.verdict),
          ),
          StatusDetailRow(
            label: context.l10n.statusTaskHead,
            value: _shortCommit(review.headCommit),
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
              value: reference,
              valueMaxLines: 2,
            ),
        ],
      ),
    );
  }
}

class _ItemHeading extends StatelessWidget {
  const _ItemHeading({required this.title, required this.status});

  final String title;
  final String status;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Row(
        children: [
          Expanded(
            child: Text(
              title,
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
