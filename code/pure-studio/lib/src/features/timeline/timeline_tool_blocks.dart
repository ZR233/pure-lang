part of 'timeline_view.dart';

class _ToolGroupPart extends StatefulWidget {
  const _ToolGroupPart({
    required this.group,
    required this.isCurrentActivity,
    super.key,
  });

  final TimelineToolGroup group;
  final bool isCurrentActivity;

  @override
  State<_ToolGroupPart> createState() => _ToolGroupPartState();
}

class _ToolGroupPartState extends State<_ToolGroupPart> {
  bool expanded = false;

  void _toggleExpanded() {
    setState(() => expanded = !expanded);
  }

  @override
  Widget build(BuildContext context) {
    final group = widget.group;
    final activityLabel = _toolGroupActivityLabel(
      context,
      group,
      activeOnly: widget.isCurrentActivity,
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Semantics(
          container: true,
          button: true,
          expanded: expanded,
          label: activityLabel,
          onTap: _toggleExpanded,
          excludeSemantics: true,
          child: Material(
            key: const ValueKey('timeline-tool-group-summary'),
            color: Colors.transparent,
            child: InkWell(
              borderRadius: BorderRadius.circular(StudioRadii.xs),
              onTap: _toggleExpanded,
              excludeFromSemantics: true,
              child: _TimelineActivitySummary(
                icon: _toolGroupIcon(group),
                label: activityLabel,
                isCurrentActivity: widget.isCurrentActivity,
                isIssue: group.issueCount > 0,
                expanded: expanded,
              ),
            ),
          ),
        ),
        if (expanded)
          DecoratedBox(
            key: const ValueKey('timeline-tool-group-details'),
            decoration: BoxDecoration(
              border: Border(
                left: BorderSide(
                  color: context.studioLine.withValues(alpha: 0.82),
                ),
              ),
            ),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 2, 6),
              child: Column(
                children: [
                  for (final item in group.items)
                    _isWebSearch(item)
                        ? _WebSearchToolCard(item: item, embedded: true)
                        : _ToolGroupItemRow(item: item),
                ],
              ),
            ),
          ),
      ],
    );
  }

  String _toolGroupActivityLabel(
    BuildContext context,
    TimelineToolGroup group, {
    required bool activeOnly,
  }) {
    final candidateItems = activeOnly
        ? group.items.where(_isActiveToolItem).toList(growable: false)
        : group.items;
    final items = candidateItems.isEmpty ? group.items : candidateItems;
    final visibleItems = items.take(3).toList();
    final labels = [
      for (final item in visibleItems)
        activeOnly
            ? _activeToolTitle(context, item)
            : _toolTitle(context, item.name, item.status),
    ];
    final hiddenCount = items.length - visibleItems.length;
    if (hiddenCount > 0) {
      labels.add(context.l10n.timelineToolGroupSummary(hiddenCount));
    }
    final issueReason = group.firstIssueReason;
    if (issueReason != null) {
      labels.add(issueReason);
    }
    return labels.isEmpty
        ? context.l10n.timelineToolGroupTitle
        : labels.join(' · ');
  }
}

bool _isActiveToolItem(TimelineToolGroupItem item) {
  return const {
    'awaitingApproval',
    'started',
    'streaming',
    'approved',
    'running',
  }.contains(item.status);
}

String _activeToolTitle(BuildContext context, TimelineToolGroupItem item) {
  final title = _toolTitle(context, item.name, item.status);
  final detail = [item.summary, item.tool?.workingDirectory]
      .whereType<String>()
      .firstWhere((value) => value.trim().isNotEmpty, orElse: () => '');
  return detail.isEmpty ? title : '$title — $detail';
}

IconData _toolGroupIcon(TimelineToolGroup group) {
  if (group.items.length != 1) {
    return Icons.build_outlined;
  }
  final name = group.items.first.name.toLowerCase();
  if (name == 'web_search') {
    return Icons.travel_explore;
  }
  if (name.contains('exec') ||
      name.contains('command') ||
      name.contains('shell') ||
      name.contains('stdin')) {
    return Icons.terminal;
  }
  if (name.contains('edit') ||
      name.contains('write') ||
      name.contains('patch') ||
      name.contains('move') ||
      name.contains('copy') ||
      name.contains('delete') ||
      name.contains('create')) {
    return Icons.edit_outlined;
  }
  if (name.contains('read') ||
      name.contains('search') ||
      name.contains('list') ||
      name.contains('stat') ||
      name.contains('glob') ||
      name.contains('grep')) {
    return Icons.menu_book_outlined;
  }
  return Icons.build_outlined;
}

bool _isWebSearch(TimelineToolGroupItem item) {
  return item.tool?.name == 'web_search';
}

class _WebSearchToolCard extends StatelessWidget {
  const _WebSearchToolCard({required this.item, this.embedded = false});

  final TimelineToolGroupItem item;
  final bool embedded;

  @override
  Widget build(BuildContext context) {
    final data = _WebSearchCardData.fromTool(item.tool);
    final content = Padding(
      padding: const EdgeInsets.all(14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.travel_explore, size: 19, color: StudioColors.clay),
              const SizedBox(width: 9),
              Expanded(
                child: Text(
                  data.title(context),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: context.text.titleSmall?.copyWith(
                    color: context.studioInk,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              _StatusPill(label: item.status),
            ],
          ),
          if (data.details.isNotEmpty) ...[
            const SizedBox(height: 9),
            SelectionArea(
              child: Text(
                data.details.join('\n'),
                style: context.text.bodySmall?.copyWith(
                  color: context.studioInkSoft,
                  height: 1.45,
                ),
              ),
            ),
          ],
          if (data.links.isNotEmpty) ...[
            const SizedBox(height: 10),
            Text(
              context.l10n.timelineWebSearchResults,
              style: context.text.labelSmall?.copyWith(
                color: context.studioInkSoft,
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 4),
            for (final link in data.links)
              Padding(
                padding: const EdgeInsets.only(top: 3),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Icon(Icons.link, size: 14, color: context.colors.primary),
                    const SizedBox(width: 6),
                    Expanded(
                      child: SelectionArea(
                        child: Text(
                          link,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                          style: context.text.bodySmall?.copyWith(
                            color: context.colors.primary,
                          ),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
          ],
          if (item.part.error?.trim().isNotEmpty == true) ...[
            const SizedBox(height: 8),
            Text(
              item.part.error!,
              style: context.text.bodySmall?.copyWith(
                color: context.colors.error,
              ),
            ),
          ] else if (item.tool?.result?.trim().isNotEmpty == true) ...[
            const SizedBox(height: 8),
            Text(
              item.tool!.result!,
              maxLines: 4,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                color: context.studioInkSoft,
              ),
            ),
          ],
        ],
      ),
    );
    if (embedded) {
      return Padding(
        padding: const EdgeInsets.only(top: 9),
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: context.studioPaper2,
            borderRadius: BorderRadius.circular(StudioRadii.sm),
            border: Border.all(color: context.studioLine),
          ),
          child: content,
        ),
      );
    }
    return _TimelinePanel(child: content);
  }
}

enum _WebSearchActionKind { search, open, find, other }

class _WebSearchCardData {
  const _WebSearchCardData({
    required this.kind,
    required this.details,
    required this.links,
  });

  factory _WebSearchCardData.fromTool(TimelineToolPart? tool) {
    final arguments = _decodeWebSearchArguments(tool?.arguments ?? '');
    final details = <String>[];
    var kind = _WebSearchActionKind.other;
    final type = arguments['type']?.toString();
    final queries = <String>[];
    final directQuery = arguments['query']?.toString().trim();
    if (directQuery?.isNotEmpty == true) {
      queries.add(directQuery!);
    }
    final actionQueries = arguments['queries'];
    if (actionQueries is List) {
      queries.addAll(
        actionQueries
            .map((value) => value.toString().trim())
            .where((value) => value.isNotEmpty),
      );
    }
    for (final key in const ['search_query', 'image_query']) {
      final commands = arguments[key];
      if (commands is List) {
        for (final command in commands) {
          final query = _webSearchMap(command)['q']?.toString().trim();
          if (query?.isNotEmpty == true) {
            queries.add(query!);
          }
        }
      }
    }
    if (type == 'search' || queries.isNotEmpty) {
      kind = _WebSearchActionKind.search;
      details.addAll(queries);
    }

    String? url = arguments['url']?.toString();
    final openCommands = arguments['open'];
    if (openCommands is List && openCommands.isNotEmpty) {
      final command = openCommands.first;
      if (command is Map) {
        url = command['ref_id']?.toString();
      }
    }
    if (type == 'open_page' || openCommands is List) {
      kind = _WebSearchActionKind.open;
      if (url?.isNotEmpty == true) details.add(url!);
    }

    String? pattern = arguments['pattern']?.toString();
    final findCommands = arguments['find'];
    if (findCommands is List && findCommands.isNotEmpty) {
      final command = findCommands.first;
      if (command is Map) {
        url = command['ref_id']?.toString();
        pattern = command['pattern']?.toString();
      }
    }
    if (type == 'find_in_page' || findCommands is List) {
      kind = _WebSearchActionKind.find;
      if (url?.isNotEmpty == true) details.add(url!);
      if (pattern?.isNotEmpty == true) details.add(pattern!);
    }

    final links = <String>{};
    for (final artifact in tool?.outputArtifacts ?? const []) {
      _collectWebLinks(artifact, links);
    }
    return _WebSearchCardData(
      kind: kind,
      details: details,
      links: links.take(6).toList(),
    );
  }

  final _WebSearchActionKind kind;
  final List<String> details;
  final List<String> links;

  String title(BuildContext context) {
    return switch (kind) {
      _WebSearchActionKind.search => context.l10n.timelineWebSearchSearching,
      _WebSearchActionKind.open => context.l10n.timelineWebSearchOpening,
      _WebSearchActionKind.find => context.l10n.timelineWebSearchFinding,
      _WebSearchActionKind.other => context.l10n.timelineWebSearchTitle,
    };
  }
}

Map<String, Object?> _decodeWebSearchArguments(String value) {
  if (value.trim().isEmpty) {
    return const {};
  }
  try {
    return _webSearchMap(jsonDecode(value));
  } catch (_) {
    return const {};
  }
}

Map<String, Object?> _webSearchMap(Object? value) {
  if (value is Map<String, Object?>) {
    return value;
  }
  if (value is Map) {
    return value.map((key, value) => MapEntry(key.toString(), value));
  }
  return const {};
}

void _collectWebLinks(Object? value, Set<String> links) {
  if (links.length >= 6) {
    return;
  }
  if (value is String) {
    final trimmed = value.trim();
    if (trimmed.startsWith('https://') || trimmed.startsWith('http://')) {
      links.add(trimmed);
    }
    return;
  }
  if (value is List) {
    for (final item in value) {
      _collectWebLinks(item, links);
    }
    return;
  }
  if (value is Map) {
    for (final entry in value.entries) {
      _collectWebLinks(entry.value, links);
    }
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
      _resultDetail(item, tool),
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

  String? _resultDetail(TimelineToolGroupItem item, TimelineToolPart? tool) {
    final result = tool?.result;
    if (result == null || result.trim().isEmpty) {
      return null;
    }
    if (item.part.status == 'completed') {
      return null;
    }
    if (item.name == 'task_complete') {
      return _taskCompleteRejectionDetail(result);
    }
    return result;
  }

  String _taskCompleteRejectionDetail(String result) {
    try {
      final decoded = jsonDecode(result);
      if (decoded is! Map) return result;
      final code = decoded['code']?.toString();
      final message = decoded['message']?.toString();
      final lines = <String>[
        if (code?.trim().isNotEmpty == true) code!,
        if (message?.trim().isNotEmpty == true) message!,
      ];
      return lines.isEmpty ? result : lines.join('\n');
    } catch (_) {
      return result;
    }
  }
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
