part of 'timeline_view.dart';

class _EmptyTimeline extends StatelessWidget {
  const _EmptyTimeline();

  @override
  Widget build(BuildContext context) {
    return Center(
      child: StudioEmptyState(
        icon: Icons.forum_outlined,
        title: context.l10n.timelineEmptyTitle,
        message: context.l10n.timelineEmptyMessage,
      ),
    );
  }
}

class _JumpToLatestButton extends StatelessWidget {
  const _JumpToLatestButton({
    required this.pendingCount,
    required this.onPressed,
  });

  final int pendingCount;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final textStyle = Theme.of(context).textTheme.labelSmall;
    return Tooltip(
      message: context.l10n.timelineJumpToLatest,
      child: Material(
        elevation: 0,
        color: StudioColors.claySoft,
        shape: StadiumBorder(
          side: BorderSide(color: StudioColors.clay.withValues(alpha: 0.18)),
        ),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: onPressed,
          child: Padding(
            padding: EdgeInsets.symmetric(
              horizontal: pendingCount > 0 ? 10 : 8,
              vertical: 7,
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Icon(
                  Icons.keyboard_arrow_down_rounded,
                  size: 18,
                  color: StudioColors.clayDeep,
                ),
                if (pendingCount > 0) ...[
                  const SizedBox(width: 4),
                  Text(
                    context.l10n.timelineNew,
                    style: textStyle?.copyWith(color: StudioColors.clayDeep),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _TimelineRowBlock extends StatelessWidget {
  const _TimelineRowBlock({required this.row, super.key});

  final TimelineRow row;

  @override
  Widget build(BuildContext context) {
    final isUser = row.type == TimelineRowType.userMessage;
    return Padding(
      padding: const EdgeInsets.only(bottom: 24),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisAlignment: isUser
            ? MainAxisAlignment.end
            : MainAxisAlignment.start,
        children: [
          if (!isUser) const _Avatar(icon: Icons.auto_awesome),
          Flexible(
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: isUser ? 560 : 700),
              child: Column(
                crossAxisAlignment: isUser
                    ? CrossAxisAlignment.end
                    : CrossAxisAlignment.start,
                children: [_RowCard(key: ValueKey(row.id), row: row)],
              ),
            ),
          ),
          if (isUser) const _Avatar(icon: Icons.person_outline),
        ],
      ),
    );
  }
}

class _TimelineProgressGroupBlock extends StatelessWidget {
  const _TimelineProgressGroupBlock({required this.block, super.key});

  final _TimelineDisplayBlock block;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 18),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const _Avatar(icon: Icons.auto_awesome),
          Flexible(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 700),
              child: _RuntimeProgressGroup(rows: block.rows),
            ),
          ),
        ],
      ),
    );
  }
}

class _Avatar extends StatelessWidget {
  const _Avatar({required this.icon});

  final IconData icon;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 9),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: colors.surfaceContainerLow,
          border: Border.all(color: colors.outlineVariant),
          shape: BoxShape.circle,
        ),
        child: SizedBox.square(
          dimension: 27,
          child: Icon(icon, size: 15, color: colors.onSurfaceVariant),
        ),
      ),
    );
  }
}

class _RuntimeProgressGroup extends StatefulWidget {
  const _RuntimeProgressGroup({required this.rows});

  final List<TimelineRow> rows;

  @override
  State<_RuntimeProgressGroup> createState() => _RuntimeProgressGroupState();
}

class _RuntimeProgressGroupState extends State<_RuntimeProgressGroup> {
  bool expanded = false;

  @override
  Widget build(BuildContext context) {
    final parts = [
      for (final row in widget.rows)
        if (row.part != null) row.part!,
    ];
    final latest = parts.last;
    final scheme = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.studioPaper2.withValues(alpha: 0.58),
        border: Border.all(color: scheme.outlineVariant.withValues(alpha: 0.6)),
        borderRadius: BorderRadius.circular(StudioRadii.md),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Material(
            color: Colors.transparent,
            child: InkWell(
              borderRadius: BorderRadius.circular(StudioRadii.md),
              onTap: () => setState(() => expanded = !expanded),
              child: Padding(
                padding: const EdgeInsets.fromLTRB(12, 9, 10, 9),
                child: Row(
                  children: [
                    Icon(
                      expanded
                          ? Icons.keyboard_arrow_up_rounded
                          : Icons.keyboard_arrow_down_rounded,
                      size: 18,
                      color: context.studioInkSoft,
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        latest.text,
                        maxLines: expanded ? 1 : 2,
                        overflow: TextOverflow.ellipsis,
                        style: context.text.bodySmall?.copyWith(
                          color: context.studioInk,
                          height: 1.38,
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    StudioPill(label: parts.length.toString()),
                  ],
                ),
              ),
            ),
          ),
          if (expanded)
            Padding(
              padding: const EdgeInsets.fromLTRB(15, 0, 15, 12),
              child: SelectionArea(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Divider(
                      height: 12,
                      color: scheme.outlineVariant.withValues(alpha: 0.45),
                    ),
                    for (final part in parts)
                      _RuntimeProgressStep(
                        text: part.text,
                        isLatest: part.id == latest.id,
                      ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _RuntimeProgressStep extends StatelessWidget {
  const _RuntimeProgressStep({required this.text, required this.isLatest});

  final String text;
  final bool isLatest;

  @override
  Widget build(BuildContext context) {
    final color = isLatest ? context.studioInk : context.studioInkSoft;
    return Padding(
      padding: const EdgeInsets.only(top: 7),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 7),
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: color.withValues(alpha: isLatest ? 0.76 : 0.34),
                shape: BoxShape.circle,
              ),
              child: const SizedBox.square(dimension: 5),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              text,
              style: context.text.bodySmall?.copyWith(
                color: color,
                height: 1.42,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _RowCard extends StatelessWidget {
  const _RowCard({required this.row, super.key});

  final TimelineRow row;

  @override
  Widget build(BuildContext context) {
    return switch (row.type) {
      TimelineRowType.userMessage => _MarkdownBubble(
        key: ValueKey(row.part!.id),
        part: row.part!,
        isUser: true,
      ),
      TimelineRowType.commentary ||
      TimelineRowType.finalAnswer => _MarkdownBubble(
        key: ValueKey(row.part!.id),
        part: row.part!,
        isUser: false,
      ),
      TimelineRowType.reasoningSummary => _ReasoningPart(
        key: ValueKey('${row.sessionId}:${row.part!.id}'),
        sessionId: row.sessionId,
        part: row.part!,
      ),
      TimelineRowType.toolGroup => _ToolGroupPart(
        key: ValueKey(row.toolGroup!.id),
        group: row.toolGroup!,
      ),
      TimelineRowType.plan => _PlanPart(
        key: ValueKey(row.part!.id),
        part: row.part!,
      ),
      TimelineRowType.agentActivity =>
        row.agentEvent == null
            ? _AgentSnapshotPart(key: ValueKey(row.part!.id), part: row.part!)
            : _AgentPart(
                key: ValueKey(row.agentEvent!.eventId),
                event: row.agentEvent!,
              ),
    };
  }
}

class _MarkdownBubble extends StatelessWidget {
  const _MarkdownBubble({required this.part, required this.isUser, super.key});

  final TimelinePart part;
  final bool isUser;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final surface = isUser ? _MarkdownSurface.user : _MarkdownSurface.assistant;
    final isCommentary =
        part.textChannel == TimelineTextChannel.commentary && !isUser;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: isUser
            ? context.studioPaper2
            : isCommentary
            ? context.studioPaper2.withValues(alpha: 0.52)
            : Colors.transparent,
        border: isUser || isCommentary
            ? Border.all(color: scheme.outlineVariant.withValues(alpha: 0.72))
            : null,
        borderRadius: BorderRadius.circular(StudioRadii.md),
      ),
      child: Padding(
        padding: EdgeInsets.symmetric(
          horizontal: isUser || isCommentary ? 14 : 0,
          vertical: isUser || isCommentary ? 10 : 0,
        ),
        child: SelectionArea(
          child: _AgentMarkdown(
            id: part.id,
            status: part.status,
            text: part.text,
            surface: surface,
          ),
        ),
      ),
    );
  }
}

class _ReasoningPart extends ConsumerWidget {
  const _ReasoningPart({
    required this.sessionId,
    required this.part,
    super.key,
  });

  final String sessionId;
  final TimelinePart part;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final expansionKey = _ReasoningExpansionKey(
      sessionId: sessionId,
      partId: part.id,
    );
    final expanded = ref.watch(_reasoningExpandedProvider(expansionKey));
    final title = part.status == 'completed'
        ? context.l10n.timelineReasoningCompleted
        : context.l10n.timelineReasoningActive;
    final details = part.text.trim();
    return _TimelinePanel(
      child: ExpansionTile(
        key: ValueKey('reasoning:$sessionId:${part.id}:$expanded'),
        tilePadding: const EdgeInsets.symmetric(horizontal: 12),
        childrenPadding: const EdgeInsets.fromLTRB(14, 0, 14, 12),
        initiallyExpanded: expanded,
        leading: const Icon(Icons.psychology_alt_outlined, size: 18),
        title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
        subtitle: part.title?.isNotEmpty == true
            ? Text(
                part.title!,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: context.studioInkSoft),
              )
            : null,
        onExpansionChanged: (value) {
          ref.read(_reasoningExpandedProvider(expansionKey).notifier).state =
              value;
        },
        children: [
          Align(
            alignment: Alignment.centerLeft,
            child: details.isEmpty
                ? Text(
                    context.l10n.timelineReasoningEmpty,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: context.studioInkSoft,
                    ),
                  )
                : SelectionArea(
                    child: _AgentMarkdown(
                      id: part.id,
                      status: part.status,
                      text: details,
                      surface: _MarkdownSurface.assistant,
                    ),
                  ),
          ),
        ],
      ),
    );
  }
}

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

class _PlanPart extends StatelessWidget {
  const _PlanPart({required this.part, super.key});

  final TimelinePart part;

  @override
  Widget build(BuildContext context) {
    return _TimelinePanel(
      child: Padding(
        padding: const EdgeInsets.all(13),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Icon(Icons.route_outlined, size: 18),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    (part.title?.trim().isNotEmpty ?? false)
                        ? part.title!.trim()
                        : context.l10n.timelinePlanFallback,
                    style: Theme.of(context).textTheme.titleSmall,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                _StatusPill(label: part.status),
              ],
            ),
            const SizedBox(height: 8),
            SelectionArea(
              child: _AgentMarkdown(
                id: part.id,
                status: part.status,
                text: part.text,
                surface: _MarkdownSurface.panel,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _AgentPart extends StatelessWidget {
  const _AgentPart({required this.event, super.key});

  final TimelineAgentEvent event;

  @override
  Widget build(BuildContext context) {
    return _TimelinePanel(
      child: _TimelineMetaRow(
        icon: Icons.account_tree_outlined,
        title: _agentTitle(context, event.title),
        subtitle: event.text,
      ),
    );
  }

  String _agentTitle(BuildContext context, String title) {
    return switch (title) {
      'agentTimeline.spawn' => context.l10n.timelineAgentSubagent,
      'agentTimeline.message' => context.l10n.timelineAgentSubagentMessage,
      'agentTimeline.waiting' => context.l10n.timelineAgentWaiting,
      'agentTimeline.close' => context.l10n.timelineAgentClose,
      'agentTimeline.agent' => context.l10n.timelineAgentFallback,
      _ => title,
    };
  }
}

class _AgentSnapshotPart extends StatelessWidget {
  const _AgentSnapshotPart({required this.part, super.key});

  final TimelinePart part;

  @override
  Widget build(BuildContext context) {
    final agent = part.agent;
    return _TimelinePanel(
      child: _TimelineMetaRow(
        icon: Icons.account_tree_outlined,
        title: agent?.role.isNotEmpty == true
            ? agent!.role
            : (part.title ?? context.l10n.timelineAgentFallback),
        subtitle: part.text,
        trailing: _StatusPill(label: part.status),
      ),
    );
  }
}

class _TimelineMetaRow extends StatelessWidget {
  const _TimelineMetaRow({
    required this.icon,
    required this.title,
    required this.subtitle,
    this.trailing,
  });

  final IconData icon;
  final String title;
  final String subtitle;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
      child: Row(
        children: [
          Icon(icon, size: 17, color: context.studioInkSoft),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.labelLarge?.copyWith(
                    color: context.studioInk,
                  ),
                ),
                if (subtitle.isNotEmpty) ...[
                  const SizedBox(height: 2),
                  Text(
                    subtitle,
                    maxLines: 3,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.bodySmall?.copyWith(
                      color: context.studioInkSoft,
                    ),
                  ),
                ],
              ],
            ),
          ),
          if (trailing != null) ...[const SizedBox(width: 10), trailing!],
        ],
      ),
    );
  }
}

class _TimelinePanel extends StatelessWidget {
  const _TimelinePanel({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return StudioPanel(
      backgroundColor: context.studioPaper2,
      borderColor: colors.outlineVariant.withValues(alpha: 0.82),
      radius: StudioRadii.md,
      child: child,
    );
  }
}

class _StatusPill extends StatelessWidget {
  const _StatusPill({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    return StudioPill(label: label);
  }
}

enum _MarkdownSurface { assistant, user, panel }

class _AgentMarkdown extends StatelessWidget {
  const _AgentMarkdown({
    required this.id,
    required this.status,
    required this.text,
    required this.surface,
  });

  final String id;
  final String status;
  final String text;
  final _MarkdownSurface surface;

  @override
  Widget build(BuildContext context) {
    final repaired = repairAgentMarkdownForDisplay(text);
    return GptMarkdown(
      repaired,
      key: ValueKey('gpt-markdown-$id-$status'),
      style: _markdownBodyStyle(context),
      codeBuilder: (context, name, code, closed) {
        final scheme = Theme.of(context).colorScheme;
        final textTheme = Theme.of(context).textTheme;
        final codeBackground = surface == _MarkdownSurface.user
            ? scheme.surfaceContainerHigh
            : scheme.surfaceContainerLow;
        return StudioCodeBlock(
          text: code,
          language: name,
          margin: const EdgeInsets.symmetric(vertical: 6),
          backgroundColor: codeBackground,
          borderColor: scheme.outlineVariant,
          textStyle: textTheme.bodyMedium?.copyWith(
            color: scheme.onSurface,
            fontFamily: 'JetBrains Mono',
            fontFamilyFallback: const ['Consolas', 'monospace'],
            fontSize: (textTheme.bodyMedium?.fontSize ?? 14) * 0.92,
            height: 1.35,
          ),
        );
      },
    );
  }
}

TextStyle? _markdownBodyStyle(BuildContext context) {
  final theme = Theme.of(context);
  return theme.textTheme.bodyMedium?.copyWith(
    color: theme.colorScheme.onSurface,
    height: 1.52,
  );
}
