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

class _MessageBlock extends StatelessWidget {
  const _MessageBlock({required this.message, super.key});

  final TimelineMessage message;

  @override
  Widget build(BuildContext context) {
    final isUser = message.role == 'user';
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
                children: [
                  for (final part in message.parts)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 8),
                      child: _PartCard(part: part, isUser: isUser),
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

class _PartCard extends StatelessWidget {
  const _PartCard({required this.part, required this.isUser});

  final TimelinePart part;
  final bool isUser;

  @override
  Widget build(BuildContext context) {
    return switch (part.type) {
      TimelinePartType.text => _MarkdownBubble(part: part, isUser: isUser),
      TimelinePartType.reasoning => _ReasoningPart(part: part),
      TimelinePartType.tool => _ToolPart(part: part),
      TimelinePartType.plan => _PlanPart(part: part),
      TimelinePartType.agent => _AgentPart(part: part),
    };
  }
}

class _MarkdownBubble extends StatelessWidget {
  const _MarkdownBubble({required this.part, required this.isUser});

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
  const _ReasoningPart({required this.part});

  final TimelinePart part;

  @override
  State<_ReasoningPart> createState() => _ReasoningPartState();
}

class _ReasoningPartState extends State<_ReasoningPart> {
  late bool expanded = !widget.part.collapsed;

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
  const _ToolPart({required this.part});

  final TimelinePart part;

  @override
  Widget build(BuildContext context) {
    final tool = part.tool;
    final title = tool == null
        ? part.title ?? context.l10n.timelineToolFallback
        : _toolTitle(context, tool.name, part.status);
    final subtitle = tool == null ? part.text : _toolSubtitle(tool);
    return _TimelinePanel(
      child: _TimelineMetaRow(
        icon: Icons.terminal,
        title: title,
        subtitle: subtitle,
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

  String _toolSubtitle(TimelineToolPart tool) {
    return [
      _firstLine(tool.arguments),
      tool.workingDirectory,
      tool.denialReason,
      tool.result,
    ].whereType<String>().where((part) => part.trim().isNotEmpty).join('\n');
  }

  String? _firstLine(String value) {
    final trimmed = value.trim();
    if (trimmed.isEmpty) {
      return null;
    }
    return trimmed.split('\n').first;
  }
}

class _PlanPart extends StatelessWidget {
  const _PlanPart({required this.part});

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
  const _AgentPart({required this.part});

  final TimelinePart part;

  @override
  Widget build(BuildContext context) {
    return _TimelinePanel(
      child: _TimelineMetaRow(
        icon: Icons.account_tree_outlined,
        title: _agentTitle(context, part.title),
        subtitle: part.text,
      ),
    );
  }

  String _agentTitle(BuildContext context, String? title) {
    return switch (title) {
      'agentTimeline.spawn' => context.l10n.timelineAgentSubagent,
      'agentTimeline.message' => context.l10n.timelineAgentSubagentMessage,
      'agentTimeline.waiting' => context.l10n.timelineAgentWaiting,
      'agentTimeline.close' => context.l10n.timelineAgentClose,
      'agentTimeline.agent' => context.l10n.timelineAgentFallback,
      _ => title ?? context.l10n.timelineAgentFallback,
    };
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
