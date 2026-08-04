part of 'timeline_view.dart';

class _PlanPart extends StatelessWidget {
  const _PlanPart({required this.part, super.key});

  final TimelineEntry part;

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
    if (event.payload case final TimelineTodoListUpdate update) {
      return _TodoListPart(update: update);
    }
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

class _TodoListPart extends StatelessWidget {
  const _TodoListPart({required this.update});

  final TimelineTodoListUpdate update;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final title = update.explanation?.trim().isNotEmpty == true
        ? update.explanation!.trim()
        : context.l10n.timelineTodoListFallback;
    return _TimelinePanel(
      child: Padding(
        padding: const EdgeInsets.all(13),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(Icons.checklist_outlined, size: 18, color: colors.primary),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    title,
                    style: Theme.of(context).textTheme.titleSmall,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                _StatusPill(label: update.status),
              ],
            ),
            const SizedBox(height: 10),
            for (final item in update.items) _TodoItemRow(item: item),
          ],
        ),
      ),
    );
  }
}

class _TodoItemRow extends StatelessWidget {
  const _TodoItemRow({required this.item});

  final TimelineTodoItem item;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final status = item.status;
    final completed = status == 'completed';
    final inProgress = status == 'inProgress';
    final icon = completed
        ? Icons.check_circle_outline
        : inProgress
        ? Icons.radio_button_checked
        : Icons.radio_button_unchecked;
    final color = completed
        ? colors.onSurfaceVariant
        : inProgress
        ? colors.primary
        : colors.outline;
    final label = switch (status) {
      'completed' => context.l10n.timelineTodoCompleted,
      'inProgress' => context.l10n.timelineTodoInProgress,
      _ => context.l10n.timelineTodoPending,
    };
    final textStyle = Theme.of(context).textTheme.bodyMedium?.copyWith(
      color: completed ? colors.onSurfaceVariant : colors.onSurface,
      fontWeight: inProgress ? FontWeight.w600 : FontWeight.w400,
      decoration: completed ? TextDecoration.lineThrough : null,
      decorationColor: colors.onSurfaceVariant,
    );
    return Padding(
      padding: const EdgeInsets.only(top: 7),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Icon(icon, size: 17, color: color),
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 86,
            child: Text(
              label,
              style: Theme.of(context).textTheme.labelSmall?.copyWith(
                color: color,
                fontWeight: inProgress ? FontWeight.w700 : FontWeight.w500,
              ),
              overflow: TextOverflow.ellipsis,
            ),
          ),
          const SizedBox(width: 8),
          Expanded(child: Text(item.step, style: textStyle)),
        ],
      ),
    );
  }
}
