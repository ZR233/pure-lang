part of 'timeline_view.dart';

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
