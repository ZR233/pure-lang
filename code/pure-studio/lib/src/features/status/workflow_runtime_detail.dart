import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import 'status_detail_popover.dart';

/// 展示任意 Mode Skill 编译出的阶段图与 canonical 转换历史。
class WorkflowRuntimeDetail extends StatelessWidget {
  const WorkflowRuntimeDetail({required this.run, super.key});

  final WorkflowRunView run;

  @override
  Widget build(BuildContext context) {
    final visited = <String>{
      if (run.stages.isNotEmpty) run.stages.first.id,
      for (final entry in run.history) entry.toStageId,
    };
    return StatusDetailPanel(
      key: ValueKey('workflow-history-${run.runId}'),
      title: run.title.isEmpty ? run.modeDisplayName : run.title,
      children: [
        Text(
          '${run.modeId} · ${run.runId}',
          style: Theme.of(context).textTheme.labelSmall,
        ),
        const SizedBox(height: 12),
        for (final stage in run.stages)
          StatusDetailIconRow(
            icon: stage.id == run.currentStageId
                ? (run.terminal ? Icons.check_circle : Icons.play_circle)
                : visited.contains(stage.id)
                ? Icons.check_circle_outline
                : Icons.circle_outlined,
            title: stage.title.isEmpty ? stage.id : stage.title,
            detail: stage.id,
            iconColor: stage.id == run.currentStageId
                ? StudioColors.clayDeep
                : context.studioInkSoft,
            backgroundColor: StudioColors.clay.withValues(alpha: 0.12),
          ),
        if (run.history.isNotEmpty) ...[
          const SizedBox(height: 12),
          Text(
            context.l10n.workflowHistoryTitle,
            style: Theme.of(context).textTheme.titleSmall,
          ),
          const SizedBox(height: 6),
          for (final entry in run.history)
            ListTile(
              key: ValueKey(
                'workflow-transition-${run.runId}-${entry.revision}',
              ),
              dense: true,
              contentPadding: EdgeInsets.zero,
              leading: Text('#${entry.revision}'),
              title: Text('${entry.fromStageId} → ${entry.toStageId}'),
              subtitle: Text(
                [
                  if (entry.summary.isNotEmpty) entry.summary,
                  if (entry.evidence.isNotEmpty) entry.evidence.join('\n'),
                ].join('\n'),
              ),
            ),
        ],
      ],
    );
  }
}
