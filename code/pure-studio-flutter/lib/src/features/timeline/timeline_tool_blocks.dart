part of 'timeline_view.dart';

class _ToolGroupPart extends StatefulWidget {
  const _ToolGroupPart({required this.group, super.key});

  final TimelineToolGroup group;

  @override
  State<_ToolGroupPart> createState() => _ToolGroupPartState();
}

class _ToolGroupPartState extends State<_ToolGroupPart> {
  bool expanded = false;

  @override
  Widget build(BuildContext context) {
    final group = widget.group;
    return _TimelinePanel(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Material(
            color: Colors.transparent,
            child: InkWell(
              borderRadius: BorderRadius.circular(StudioRadii.md),
              onTap: () => setState(() => expanded = !expanded),
              child: _TimelineMetaRow(
                icon: expanded
                    ? Icons.keyboard_arrow_up_rounded
                    : Icons.keyboard_arrow_down_rounded,
                title: context.l10n.timelineToolGroupTitle,
                subtitle: _toolGroupSubtitle(context, group),
                trailing: _StatusPill(label: group.status),
              ),
            ),
          ),
          if (expanded)
            Padding(
              padding: const EdgeInsets.fromLTRB(13, 0, 13, 13),
              child: Column(
                children: [
                  Divider(
                    height: 14,
                    color: Theme.of(
                      context,
                    ).colorScheme.outlineVariant.withValues(alpha: 0.5),
                  ),
                  for (final item in group.items) _ToolGroupItemRow(item: item),
                ],
              ),
            ),
        ],
      ),
    );
  }

  String _toolGroupSubtitle(BuildContext context, TimelineToolGroup group) {
    final issueCount = group.issueCount;
    final runningCount = group.runningCount;
    if (issueCount > 0 && runningCount > 0) {
      return context.l10n.timelineToolGroupSummaryRunningWithIssues(
        group.count,
        runningCount,
        issueCount,
      );
    }
    if (issueCount > 0) {
      return context.l10n.timelineToolGroupSummaryIssues(
        group.count,
        issueCount,
      );
    }
    if (runningCount > 0) {
      return context.l10n.timelineToolGroupSummaryRunning(
        group.count,
        runningCount,
      );
    }
    return context.l10n.timelineToolGroupSummary(group.count);
  }
}

class _ToolGroupItemRow extends StatelessWidget {
  const _ToolGroupItemRow({required this.item});

  final TimelineToolGroupItem item;

  @override
  Widget build(BuildContext context) {
    final tool = item.tool;
    final detailLines = [
      item.summary,
      tool?.workingDirectory,
      if (tool?.exitCode != null)
        context.l10n.timelineToolExitCode(tool!.exitCode!),
      if (tool?.timedOut == true) context.l10n.timelineToolTimedOut,
      tool?.denialReason,
      item.part.error,
      _resultDetail(item.part.status, tool?.result),
    ].whereType<String>().where((value) => value.trim().isNotEmpty).toList();
    return Padding(
      padding: const EdgeInsets.only(top: 9),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 3),
            child: Icon(Icons.terminal, size: 16, color: context.studioInkSoft),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        _toolTitle(context, item.name, item.status),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: context.text.labelLarge?.copyWith(
                          color: context.studioInk,
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    _StatusPill(label: item.status),
                  ],
                ),
                if (detailLines.isNotEmpty) ...[
                  const SizedBox(height: 3),
                  SelectionArea(
                    child: Text(
                      detailLines.join('\n'),
                      maxLines: 8,
                      overflow: TextOverflow.ellipsis,
                      style: context.text.bodySmall?.copyWith(
                        color: context.studioInkSoft,
                        height: 1.38,
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
        ],
      ),
    );
  }

  String _toolTitle(BuildContext context, String name, String status) {
    return switch (status) {
      'completed' => context.l10n.timelineToolCompleted(name),
      'failed' => context.l10n.timelineToolFailed(name),
      'denied' => context.l10n.timelineToolDenied(name),
      'awaitingApproval' => context.l10n.timelineToolAwaitingApproval(name),
      'running' ||
      'streaming' ||
      'approved' ||
      'started' => context.l10n.timelineToolRunning(name),
      _ => name,
    };
  }

  String? _resultDetail(String status, String? result) {
    if (result == null || result.trim().isEmpty) {
      return null;
    }
    if (status == 'completed') {
      return null;
    }
    return result;
  }
}
