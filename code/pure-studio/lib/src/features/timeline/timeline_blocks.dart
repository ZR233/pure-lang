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

class _TurnActivityBlock extends StatelessWidget {
  const _TurnActivityBlock({
    required this.turn,
    required this.reasoningExpanded,
    required this.onToggleReasoning,
    this.reasoningGroup,
    this.toolGroup,
    super.key,
  });

  final StudioTurnView turn;
  final bool reasoningExpanded;
  final VoidCallback onToggleReasoning;
  final TimelineReasoningGroup? reasoningGroup;
  final TimelineToolGroup? toolGroup;

  @override
  Widget build(BuildContext context) {
    final activity = turn.state.activity;
    final reasoning = reasoningGroup;
    if (activity == StudioTurnActivity.thinking && reasoning != null) {
      return Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: _ReasoningPart(
          threadId: turn.threadId,
          group: reasoning,
          isCurrentActivity: true,
          expanded: reasoningExpanded,
          onToggle: onToggleReasoning,
        ),
      );
    }
    final tools = toolGroup;
    if (activity == StudioTurnActivity.runningTool && tools != null) {
      return Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: _ToolGroupPart(group: tools, isCurrentActivity: true),
      );
    }
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: _TimelineActivitySummary(
        icon: activity?.icon ?? Icons.schedule_outlined,
        label: _turnActivityLabel(context, turn.state),
        isCurrentActivity: true,
        muted: activity == StudioTurnActivity.thinking,
      ),
    );
  }
}

String _turnActivityLabel(BuildContext context, StudioTurnState state) {
  return switch (state.status) {
    StudioTurnStatus.queued => context.l10n.statusTurnQueued,
    StudioTurnStatus.running => context.turnActivityLabel(state.activity!),
    StudioTurnStatus.completed ||
    StudioTurnStatus.failed ||
    StudioTurnStatus.cancelled ||
    StudioTurnStatus.budgetLimited => '',
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
    required this.isReasoningExpanded,
    required this.onToggleReasoning,
    super.key,
  });

  final TimelineRow row;
  final bool isCurrentActivity;
  final bool isReasoningExpanded;
  final ValueChanged<String> onToggleReasoning;

  @override
  Widget build(BuildContext context) {
    final isUser = row.type == TimelineRowType.userMessage;
    final isCompactActivity =
        row.type == TimelineRowType.reasoningSummary ||
        row.type == TimelineRowType.toolGroup ||
        row.type == TimelineRowType.skillActivation;
    return Padding(
      padding: EdgeInsets.only(bottom: isCompactActivity ? 12 : 24),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisAlignment: isUser
            ? MainAxisAlignment.end
            : MainAxisAlignment.start,
        children: [
          Flexible(
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: isUser ? 560 : 700),
              child: Column(
                crossAxisAlignment: isUser
                    ? CrossAxisAlignment.end
                    : CrossAxisAlignment.start,
                children: [
                  Opacity(
                    opacity: row.isRolledBack ? 0.52 : 1,
                    child: _RowCard(
                      key: ValueKey(row.id),
                      row: row,
                      isCurrentActivity: isCurrentActivity,
                      isReasoningExpanded: isReasoningExpanded,
                      onToggleReasoning: onToggleReasoning,
                    ),
                  ),
                  if (row.isRolledBack)
                    Padding(
                      padding: const EdgeInsets.only(top: 6),
                      child: DecoratedBox(
                        key: StudioDriverKeys.timelineRolledBack(row.id),
                        decoration: BoxDecoration(
                          color: context.studioPaper2,
                          border: Border.all(color: context.studioLine),
                          borderRadius: BorderRadius.circular(999),
                        ),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 8,
                            vertical: 3,
                          ),
                          child: Text(
                            context.l10n.timelineRolledBack,
                            style: Theme.of(context).textTheme.labelSmall
                                ?.copyWith(color: context.studioInkSoft),
                          ),
                        ),
                      ),
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
    required this.isReasoningExpanded,
    required this.onToggleReasoning,
    super.key,
  });

  final TimelineRow row;
  final bool isCurrentActivity;
  final bool isReasoningExpanded;
  final ValueChanged<String> onToggleReasoning;

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
        key: ValueKey('${row.threadId}:${row.reasoningGroup!.id}'),
        threadId: row.threadId,
        group: row.reasoningGroup!,
        isCurrentActivity: isCurrentActivity,
        expanded: isReasoningExpanded,
        onToggle: () => onToggleReasoning(row.reasoningGroup!.id),
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
      TimelineRowType.skillActivation => _SkillActivationPart(
        key: StudioDriverKeys.timelineSkillActivation(row.part!.id),
        activation: row.part!.skill!,
      ),
      TimelineRowType.agentActivity => _AgentPart(
        key: ValueKey(row.agentEvent!.eventId),
        event: row.agentEvent!,
      ),
    };
  }
}

class _SkillActivationPart extends StatelessWidget {
  const _SkillActivationPart({required this.activation, super.key});

  final TimelineSkillActivation activation;

  @override
  Widget build(BuildContext context) {
    final label = switch (activation.cause.kind) {
      SkillActivationCauseKind.tool => context.l10n.timelineSkillAgentActivated(
        activation.name,
      ),
      SkillActivationCauseKind.userGesture =>
        context.l10n.timelineSkillUserActivated(activation.name),
    };
    return Semantics(
      container: true,
      label: label,
      value: activation.source,
      child: Tooltip(
        message: activation.resourceBase.value,
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: StudioColors.claySoft.withValues(alpha: 0.6),
            border: Border.all(color: context.studioLine),
            borderRadius: BorderRadius.circular(StudioRadii.md),
          ),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 7),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Icon(
                  Icons.extension_outlined,
                  size: 15,
                  color: StudioColors.clayDeep,
                ),
                const SizedBox(width: 7),
                Flexible(
                  child: Text(
                    label,
                    overflow: TextOverflow.ellipsis,
                    style: context.text.bodySmall?.copyWith(
                      color: context.studioInk,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
                if (activation.source.trim().isNotEmpty) ...[
                  const SizedBox(width: 8),
                  DecoratedBox(
                    decoration: BoxDecoration(
                      color: context.studioPaper2,
                      borderRadius: BorderRadius.circular(999),
                    ),
                    child: Padding(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 7,
                        vertical: 2,
                      ),
                      child: Text(
                        activation.source,
                        style: context.text.labelSmall?.copyWith(
                          color: context.studioInkSoft,
                        ),
                      ),
                    ),
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

class _MarkdownBubble extends StatelessWidget {
  const _MarkdownBubble({required this.part, required this.isUser, super.key});

  final TimelineEntry part;
  final bool isUser;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final isIssue =
        !isUser &&
        const {
          'failed',
          'interrupted',
          'cancelled',
          'denied',
          'budgetLimited',
        }.contains(part.status);
    final surface = isUser
        ? _MarkdownSurface.user
        : isIssue
        ? _MarkdownSurface.error
        : _MarkdownSurface.assistant;
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
            text: part.text.trim().isEmpty ? part.error ?? '' : part.text,
            surface: surface,
          ),
        ),
      ),
    );
  }
}

class _ReasoningPart extends StatelessWidget {
  const _ReasoningPart({
    required this.threadId,
    required this.group,
    required this.isCurrentActivity,
    required this.expanded,
    required this.onToggle,
    super.key,
  });

  final String threadId;
  final TimelineReasoningGroup group;
  final bool isCurrentActivity;
  final bool expanded;
  final VoidCallback onToggle;

  @override
  Widget build(BuildContext context) {
    final label = _reasoningGroupLabel(context, group, isCurrentActivity);
    final details = group.details;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Semantics(
          container: true,
          button: true,
          expanded: expanded,
          label: label,
          onTap: onToggle,
          excludeSemantics: true,
          child: Material(
            key: ValueKey('reasoning:$threadId:${group.id}:$expanded'),
            color: Colors.transparent,
            child: InkWell(
              borderRadius: BorderRadius.circular(StudioRadii.xs),
              onTap: onToggle,
              excludeFromSemantics: true,
              child: _TimelineActivitySummary(
                icon: Icons.psychology_alt_outlined,
                label: label,
                isCurrentActivity: isCurrentActivity,
                muted: true,
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
    this.muted = false,
    this.expanded,
  });

  final IconData icon;
  final String label;
  final bool isCurrentActivity;
  final bool isIssue;
  final bool muted;
  final bool? expanded;

  @override
  Widget build(BuildContext context) {
    final color = isIssue
        ? Theme.of(context).colorScheme.error
        : muted
        ? context.studioInkSoft
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
                    children: [...previousChildren, ?currentChild],
                  );
                },
                child: Text(
                  label,
                  key: ValueKey(label),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.bodySmall?.copyWith(
                    color: color,
                    fontWeight: (isCurrentActivity && !muted) || isIssue
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
  });

  final IconData icon;
  final String title;
  final String subtitle;

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
