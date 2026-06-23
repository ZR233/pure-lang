part of 'timeline_view.dart';

class _EmptyTimeline extends StatelessWidget {
  const _EmptyTimeline();

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 380),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const StudioIconBadge(icon: Icons.forum_outlined, size: 44),
            const SizedBox(height: 14),
            Text(
              'No messages yet',
              style: Theme.of(context).textTheme.titleMedium?.copyWith(
                color: context.studioInk,
                fontWeight: FontWeight.w700,
              ),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 6),
            Text(
              'Open a project or start a session to begin.',
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: colors.onSurfaceVariant),
              textAlign: TextAlign.center,
            ),
          ],
        ),
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
      message: '跳到最新',
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
                    'New',
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
    return DecoratedBox(
      decoration: BoxDecoration(
        color: isUser ? context.studioPaper2 : Colors.transparent,
        border: isUser
            ? Border.all(color: scheme.outlineVariant.withValues(alpha: 0.78))
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
    final title = widget.part.title ?? 'Reasoning';
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
              child: SelectionArea(
                child: _AgentMarkdown(
                  id: widget.part.id,
                  status: widget.part.status,
                  text: widget.part.text,
                  surface: _MarkdownSurface.panel,
                ),
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
    return _TimelinePanel(
      child: ListTile(
        dense: true,
        leading: const Icon(Icons.terminal, size: 18),
        title: Text(part.title ?? 'Tool', overflow: TextOverflow.ellipsis),
        subtitle: Text(part.text, maxLines: 4, overflow: TextOverflow.ellipsis),
        trailing: _StatusPill(label: part.status),
      ),
    );
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
                    part.title ?? 'Plan',
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
      child: ListTile(
        dense: true,
        leading: const Icon(Icons.account_tree_outlined, size: 18),
        title: Text(part.title ?? 'Agent', overflow: TextOverflow.ellipsis),
        subtitle: Text(part.text, overflow: TextOverflow.ellipsis),
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
      backgroundColor: colors.surfaceContainerLowest.withValues(alpha: 0.84),
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
      key: ValueKey('gpt-markdown-$id-$status-${repaired.length}'),
      style: _markdownBodyStyle(context),
      codeBuilder: (context, name, code, closed) {
        return _CodeBlock(code: code, language: name, surface: surface);
      },
    );
  }
}

class _CodeBlock extends StatelessWidget {
  const _CodeBlock({
    required this.code,
    required this.language,
    required this.surface,
  });

  final String code;
  final String language;
  final _MarkdownSurface surface;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final textTheme = Theme.of(context).textTheme;
    final codeBackground = surface == _MarkdownSurface.user
        ? scheme.surfaceContainerHigh
        : scheme.surfaceContainerLow;
    final languageLabel = language.trim();

    return Container(
      width: double.infinity,
      margin: const EdgeInsets.symmetric(vertical: 6),
      decoration: BoxDecoration(
        color: codeBackground,
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        border: Border.all(color: scheme.outlineVariant),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          if (languageLabel.isNotEmpty)
            DecoratedBox(
              decoration: BoxDecoration(color: scheme.surfaceContainerHighest),
              child: Padding(
                padding: const EdgeInsets.fromLTRB(12, 7, 12, 6),
                child: Text(
                  languageLabel,
                  style: textTheme.labelSmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                    fontFamily: 'JetBrains Mono',
                    fontFamilyFallback: const ['Consolas', 'monospace'],
                  ),
                ),
              ),
            ),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: SelectableText(
                code,
                style: textTheme.bodyMedium?.copyWith(
                  color: scheme.onSurface,
                  fontFamily: 'JetBrains Mono',
                  fontFamilyFallback: const ['Consolas', 'monospace'],
                  fontSize: (textTheme.bodyMedium?.fontSize ?? 14) * 0.92,
                  height: 1.35,
                ),
              ),
            ),
          ),
        ],
      ),
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
