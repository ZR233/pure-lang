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

class _TimelinePhaseActivityBlock extends StatelessWidget {
  const _TimelinePhaseActivityBlock({required this.phase, super.key});

  final TurnPhase phase;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: _TimelineActivitySummary(
        icon: _phaseActivityIcon(phase),
        label: _phaseActivityLabel(context, phase),
        isCurrentActivity: true,
      ),
    );
  }
}

String _phaseActivityLabel(BuildContext context, TurnPhase phase) {
  return switch (phase) {
    TurnPhase.waitingForModel ||
    TurnPhase.streaming => context.l10n.timelineReasoningActive,
    _ => context.turnPhaseLabel(phase),
  };
}

IconData _phaseActivityIcon(TurnPhase phase) {
  return switch (phase) {
    TurnPhase.queued => Icons.schedule_outlined,
    TurnPhase.contextLoading => Icons.menu_book_outlined,
    TurnPhase.waitingForModel ||
    TurnPhase.streaming => Icons.psychology_alt_outlined,
    TurnPhase.waitingForInteraction => Icons.pending_actions_outlined,
    TurnPhase.runningTool => Icons.build_outlined,
    TurnPhase.idle ||
    TurnPhase.completed ||
    TurnPhase.failed ||
    TurnPhase.cancelled => Icons.check_circle_outline,
  };
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
      key: ValueKey('timeline-jump-to-latest:$pendingCount'),
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
  const _TimelineRowBlock({
    required this.row,
    required this.isCurrentActivity,
    super.key,
  });

  final TimelineRow row;
  final bool isCurrentActivity;

  @override
  Widget build(BuildContext context) {
    final isUser = row.type == TimelineRowType.userMessage;
    final isCompactActivity =
        row.type == TimelineRowType.reasoningSummary ||
        row.type == TimelineRowType.toolGroup;
    return Padding(
      padding: EdgeInsets.only(bottom: isCompactActivity ? 12 : 24),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisAlignment: isUser
            ? MainAxisAlignment.end
            : MainAxisAlignment.start,
        children: [
          if (!isUser && !isCompactActivity)
            const _Avatar(icon: Icons.auto_awesome),
          Flexible(
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: isUser ? 560 : 700),
              child: Column(
                crossAxisAlignment: isUser
                    ? CrossAxisAlignment.end
                    : CrossAxisAlignment.start,
                children: [
                  _RowCard(
                    key: ValueKey(row.id),
                    row: row,
                    isCurrentActivity: isCurrentActivity,
                  ),
                ],
              ),
            ),
          ),
          if (isUser) const _Avatar(icon: Icons.person_outline),
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

class _RowCard extends StatelessWidget {
  const _RowCard({
    required this.row,
    required this.isCurrentActivity,
    super.key,
  });

  final TimelineRow row;
  final bool isCurrentActivity;

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
        key: ValueKey('${row.sessionId}:${row.reasoningGroup!.id}'),
        sessionId: row.sessionId,
        group: row.reasoningGroup!,
        isCurrentActivity: isCurrentActivity,
      ),
      TimelineRowType.toolGroup => _ToolGroupPart(
        key: ValueKey(row.toolGroup!.id),
        group: row.toolGroup!,
        isCurrentActivity: isCurrentActivity,
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
    return DecoratedBox(
      decoration: BoxDecoration(
        color: isUser ? context.studioPaper2 : Colors.transparent,
        border: isUser
            ? Border.all(color: scheme.outlineVariant.withValues(alpha: 0.72))
            : null,
        borderRadius: BorderRadius.circular(StudioRadii.md),
      ),
      child: Padding(
        padding: EdgeInsets.symmetric(
          horizontal: isUser ? 14 : 0,
          vertical: isUser ? 10 : 0,
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
    required this.group,
    required this.isCurrentActivity,
    super.key,
  });

  final String sessionId;
  final TimelineReasoningGroup group;
  final bool isCurrentActivity;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final expansionKey = _ReasoningExpansionKey(
      sessionId: sessionId,
      groupId: group.id,
    );
    final expanded = ref.watch(_reasoningExpandedProvider(expansionKey));
    final label = _reasoningGroupLabel(context, group, isCurrentActivity);
    final details = group.details;
    void toggleExpanded() {
      ref.read(_reasoningExpandedProvider(expansionKey).notifier).state =
          !expanded;
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Semantics(
          container: true,
          button: true,
          expanded: expanded,
          label: label,
          onTap: toggleExpanded,
          excludeSemantics: true,
          child: Material(
            key: ValueKey('reasoning:$sessionId:${group.id}:$expanded'),
            color: Colors.transparent,
            child: InkWell(
              borderRadius: BorderRadius.circular(StudioRadii.xs),
              onTap: toggleExpanded,
              excludeFromSemantics: true,
              child: _TimelineActivitySummary(
                icon: Icons.psychology_alt_outlined,
                label: label,
                isCurrentActivity: isCurrentActivity,
                isIssue: const {
                  'failed',
                  'interrupted',
                  'cancelled',
                  'denied',
                  'budgetLimited',
                }.contains(group.status),
                expanded: expanded,
              ),
            ),
          ),
        ),
        if (expanded)
          DecoratedBox(
            key: const ValueKey('timeline-reasoning-group-details'),
            decoration: BoxDecoration(
              border: Border(
                left: BorderSide(
                  color: context.studioLine.withValues(alpha: 0.82),
                ),
              ),
            ),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(16, 4, 2, 6),
              child: Align(
                alignment: Alignment.centerLeft,
                child: details.isEmpty
                    ? Text(
                        context.l10n.timelineReasoningEmpty,
                        style: context.text.bodySmall?.copyWith(
                          color: context.studioInkSoft,
                        ),
                      )
                    : SelectionArea(
                        child: _AgentMarkdown(
                          id: group.id,
                          status: group.status,
                          text: details,
                          surface: _MarkdownSurface.reasoning,
                        ),
                      ),
              ),
            ),
          ),
      ],
    );
  }
}

String _reasoningGroupLabel(
  BuildContext context,
  TimelineReasoningGroup group,
  bool isCurrentActivity,
) {
  if (isCurrentActivity) {
    return group.latestSummary ?? context.l10n.timelineReasoningActive;
  }
  final summaries = group.summaries.take(3).toList(growable: true);
  final hiddenCount = group.summaries.length - summaries.length;
  if (hiddenCount > 0) {
    summaries.add('+$hiddenCount');
  }
  return summaries.isEmpty
      ? context.l10n.timelineReasoningCompleted
      : summaries.join(' · ');
}

class _TimelineActivitySummary extends StatelessWidget {
  const _TimelineActivitySummary({
    required this.icon,
    required this.label,
    required this.isCurrentActivity,
    this.isIssue = false,
    this.expanded,
  });

  final IconData icon;
  final String label;
  final bool isCurrentActivity;
  final bool isIssue;
  final bool? expanded;

  @override
  Widget build(BuildContext context) {
    final color = isIssue
        ? Theme.of(context).colorScheme.error
        : isCurrentActivity
        ? context.studioInk
        : context.studioInkSoft;
    return ConstrainedBox(
      key: isCurrentActivity
          ? const ValueKey('timeline-current-activity')
          : null,
      constraints: const BoxConstraints(minHeight: 32),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 2, vertical: 6),
        child: Row(
          children: [
            Icon(
              icon,
              size: 16,
              color: color.withValues(alpha: isCurrentActivity ? 0.9 : 0.76),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: AnimatedSwitcher(
                duration: const Duration(milliseconds: 140),
                switchInCurve: Curves.easeOut,
                switchOutCurve: Curves.easeIn,
                layoutBuilder: (currentChild, previousChildren) {
                  return Stack(
                    alignment: AlignmentDirectional.centerStart,
                    children: [
                      ...previousChildren,
                      ?currentChild,
                    ],
                  );
                },
                child: Text(
                  label,
                  key: ValueKey(label),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.bodySmall?.copyWith(
                    color: color,
                    fontWeight: isCurrentActivity || isIssue
                        ? FontWeight.w600
                        : FontWeight.w400,
                    height: 1.25,
                  ),
                ),
              ),
            ),
            if (expanded != null) ...[
              const SizedBox(width: 4),
              Icon(
                expanded!
                    ? Icons.keyboard_arrow_up_rounded
                    : Icons.keyboard_arrow_down_rounded,
                size: 17,
                color: context.studioInkSoft.withValues(alpha: 0.64),
              ),
            ],
          ],
        ),
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
