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
        key: ValueKey(row.part!.id),
        part: row.part!,
      ),
      TimelineRowType.toolActivity => _ToolPart(
        key: ValueKey(row.part!.id),
        part: row.part!,
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

class _ReasoningPart extends StatefulWidget {
  const _ReasoningPart({required this.part, super.key});

  final TimelinePart part;

  @override
  State<_ReasoningPart> createState() => _ReasoningPartState();
}

class _ReasoningPartState extends State<_ReasoningPart> {
  late bool expanded = !widget.part.collapsed;

  @override
  void didUpdateWidget(covariant _ReasoningPart oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.part.id != widget.part.id) {
      expanded = !widget.part.collapsed;
    }
  }

  @override
  Widget build(BuildContext context) {
    final title = widget.part.title ?? context.l10n.timelineReasoningFallback;
    return _TimelinePanel(
      child: ExpansionTile(
        tilePadding: const EdgeInsets.symmetric(horizontal: 12),
        childrenPadding: EdgeInsets.zero,
        initiallyExpanded: expanded,
        leading: const Icon(Icons.psychology_alt_outlined, size: 18),
        title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
        onExpansionChanged: (value) => setState(() => expanded = value),
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 0, 14, 12),
            child: Align(
              alignment: Alignment.centerLeft,
              child: Text(
                widget.part.status,
                style: Theme.of(
                  context,
                ).textTheme.bodySmall?.copyWith(color: context.studioInkSoft),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _ToolPart extends StatelessWidget {
  const _ToolPart({required this.part, super.key});

  final TimelinePart part;

  @override
  Widget build(BuildContext context) {
    final tool = part.tool;
    final title = tool == null
        ? part.title ?? context.l10n.timelineToolFallback
        : _toolTitle(context, tool.name, part.status);
    return _TimelinePanel(
      child: _TimelineMetaRow(
        icon: Icons.terminal,
        title: title,
        subtitle: part.text,
        trailing: _StatusPill(label: part.status),
      ),
    );
  }

  String _toolTitle(BuildContext context, String name, String status) {
    return switch (status) {
      'completed' => context.l10n.timelineToolCompleted(name),
      'failed' => context.l10n.timelineToolFailed(name),
      'denied' => context.l10n.timelineToolDenied(name),
      'running' ||
      'streaming' ||
      'started' => context.l10n.timelineToolRunning(name),
      _ => name,
    };
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
                    part.title ?? context.l10n.timelinePlanFallback,
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
