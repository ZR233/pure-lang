import 'dart:async';

import 'package:flutter/material.dart';
import 'package:gpt_markdown/gpt_markdown.dart';

import '../../domain/models/studio_models.dart';
import 'markdown_repair.dart';

class TimelineView extends StatefulWidget {
  const TimelineView({
    required this.sessionId,
    required this.messages,
    super.key,
  });

  final String? sessionId;
  final List<TimelineMessage> messages;

  @override
  State<TimelineView> createState() => _TimelineViewState();
}

class _TimelineViewState extends State<TimelineView> {
  static const _bottomThreshold = 80.0;
  static const _scrollDuration = Duration(milliseconds: 180);

  final ScrollController _controller = ScrollController();
  final Map<String, _TimelineScrollSnapshot> _sessionScroll = {};
  bool _followingBottom = true;
  bool _detachedByUser = false;
  bool _programmaticScroll = false;
  bool _bottomScrollScheduled = false;
  int _pendingNewEvents = 0;
  int _contentVersion = 0;
  _BottomScrollIntent _scheduledBottomIntent = _BottomScrollIntent.jump;
  _TimelineRestore _pendingRestore = const _TimelineRestore.bottom();

  @override
  void initState() {
    super.initState();
    _contentVersion = _timelineContentVersion(widget.messages);
    _restoreSessionState();
    _controller.addListener(_handleScrollPositionChanged);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        _restorePendingPosition();
      }
    });
  }

  @override
  void didUpdateWidget(covariant TimelineView oldWidget) {
    super.didUpdateWidget(oldWidget);
    final sessionChanged = widget.sessionId != oldWidget.sessionId;
    if (sessionChanged) {
      _saveSessionState(oldWidget.sessionId);
      _restoreSessionState();
      _contentVersion = _timelineContentVersion(widget.messages);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          _restorePendingPosition();
        }
      });
      return;
    }

    final nextContentVersion = _timelineContentVersion(widget.messages);
    if (nextContentVersion == _contentVersion) {
      return;
    }
    final wasNearBottom = _isNearBottom();
    final appendedMessage = widget.messages.length > oldWidget.messages.length;
    _contentVersion = nextContentVersion;

    if (!_detachedByUser && (_followingBottom || wasNearBottom)) {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
      _scheduleBottomScroll(
        appendedMessage
            ? _BottomScrollIntent.animate
            : _BottomScrollIntent.jump,
      );
    } else {
      _followingBottom = false;
      _detachedByUser = true;
      _pendingNewEvents += 1;
    }
  }

  @override
  void dispose() {
    _saveSessionState(widget.sessionId);
    _controller.removeListener(_handleScrollPositionChanged);
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (widget.messages.isEmpty) {
      return const _EmptyTimeline();
    }
    return Stack(
      children: [
        Align(
          alignment: Alignment.topCenter,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 860),
            child: ListView.builder(
              key: const ValueKey('timeline-scrollable'),
              controller: _controller,
              padding: const EdgeInsets.fromLTRB(24, 26, 24, 36),
              itemCount: widget.messages.length + 1,
              itemBuilder: (context, index) {
                if (index == widget.messages.length) {
                  return const SizedBox(height: 24);
                }
                return _MessageBlock(
                  key: ValueKey(widget.messages[index].id),
                  message: widget.messages[index],
                );
              },
            ),
          ),
        ),
        if (_showJumpToLatest)
          Positioned.fill(
            child: Align(
              alignment: Alignment.bottomCenter,
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 860),
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(24, 0, 24, 16),
                  child: Align(
                    alignment: Alignment.bottomRight,
                    child: _JumpToLatestButton(
                      pendingCount: _pendingNewEvents,
                      onPressed: _jumpToLatest,
                    ),
                  ),
                ),
              ),
            ),
          ),
      ],
    );
  }

  bool get _showJumpToLatest {
    return widget.messages.isNotEmpty &&
        (_detachedByUser || _pendingNewEvents > 0 || !_isNearBottom());
  }

  bool _isNearBottom() {
    if (!_controller.hasClients) {
      return true;
    }
    return _controller.position.extentAfter <= _bottomThreshold;
  }

  void _handleScrollPositionChanged() {
    if (!_controller.hasClients || _programmaticScroll) {
      return;
    }
    final nearBottom = _isNearBottom();
    if (nearBottom) {
      if (!_followingBottom || _detachedByUser || _pendingNewEvents != 0) {
        setState(() {
          _followingBottom = true;
          _detachedByUser = false;
          _pendingNewEvents = 0;
        });
      }
    } else if (_followingBottom || !_detachedByUser) {
      setState(() {
        _followingBottom = false;
        _detachedByUser = true;
      });
    }
    _saveSessionState(widget.sessionId);
  }

  void _scheduleBottomScroll(_BottomScrollIntent intent) {
    if (intent == _BottomScrollIntent.animate) {
      _scheduledBottomIntent = _BottomScrollIntent.animate;
    }
    if (_bottomScrollScheduled) {
      return;
    }
    _bottomScrollScheduled = true;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) {
        return;
      }
      final scheduledIntent = _scheduledBottomIntent;
      _scheduledBottomIntent = _BottomScrollIntent.jump;
      _bottomScrollScheduled = false;
      _scrollToBottom(animated: scheduledIntent == _BottomScrollIntent.animate);
    });
  }

  Future<void> _scrollToBottom({required bool animated}) async {
    if (!_controller.hasClients) {
      return;
    }
    final target = _controller.position.maxScrollExtent;
    _programmaticScroll = true;
    try {
      if (animated && (_controller.position.pixels - target).abs() > 1) {
        await _controller.animateTo(
          target,
          duration: _scrollDuration,
          curve: Curves.easeOutCubic,
        );
      } else {
        _controller.jumpTo(target);
      }
    } finally {
      _programmaticScroll = false;
    }
    if (!mounted) {
      return;
    }
    setState(() {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
    });
    _saveSessionState(widget.sessionId);
  }

  void _jumpToLatest() {
    unawaited(_scrollToBottom(animated: true));
  }

  void _restoreSessionState() {
    final sessionId = widget.sessionId;
    final snapshot = sessionId == null ? null : _sessionScroll[sessionId];
    if (snapshot == null) {
      _followingBottom = true;
      _detachedByUser = false;
      _pendingNewEvents = 0;
      _pendingRestore = const _TimelineRestore.bottom();
      return;
    }
    _followingBottom = snapshot.followingBottom;
    _detachedByUser = snapshot.detachedByUser;
    _pendingNewEvents = snapshot.pendingNewEvents;
    _pendingRestore = snapshot.followingBottom
        ? const _TimelineRestore.bottom()
        : _TimelineRestore.offset(snapshot.pixels);
  }

  void _restorePendingPosition() {
    if (!_controller.hasClients) {
      return;
    }
    switch (_pendingRestore.kind) {
      case _TimelineRestoreKind.bottom:
        _scrollToBottom(animated: false);
      case _TimelineRestoreKind.offset:
        final target = _pendingRestore.pixels
            .clamp(
              _controller.position.minScrollExtent,
              _controller.position.maxScrollExtent,
            )
            .toDouble();
        _programmaticScroll = true;
        try {
          _controller.jumpTo(target);
        } finally {
          _programmaticScroll = false;
        }
        _saveSessionState(widget.sessionId);
    }
  }

  void _saveSessionState(String? sessionId) {
    if (sessionId == null || !_controller.hasClients) {
      return;
    }
    _sessionScroll[sessionId] = _TimelineScrollSnapshot(
      pixels: _controller.position.pixels,
      followingBottom: _followingBottom && _isNearBottom(),
      detachedByUser: _detachedByUser || !_isNearBottom(),
      pendingNewEvents: _pendingNewEvents,
    );
  }
}

enum _BottomScrollIntent { jump, animate }

enum _TimelineRestoreKind { bottom, offset }

class _TimelineRestore {
  const _TimelineRestore.bottom()
    : kind = _TimelineRestoreKind.bottom,
      pixels = 0;

  const _TimelineRestore.offset(this.pixels)
    : kind = _TimelineRestoreKind.offset;

  final _TimelineRestoreKind kind;
  final double pixels;
}

class _TimelineScrollSnapshot {
  const _TimelineScrollSnapshot({
    required this.pixels,
    required this.followingBottom,
    required this.detachedByUser,
    required this.pendingNewEvents,
  });

  final double pixels;
  final bool followingBottom;
  final bool detachedByUser;
  final int pendingNewEvents;
}

int _timelineContentVersion(List<TimelineMessage> messages) {
  return Object.hashAll([
    messages.length,
    for (final message in messages) ...[
      message.id,
      message.role,
      message.parts.length,
      for (final part in message.parts) ...[
        part.id,
        part.type,
        part.status,
        part.title,
        part.text.length,
      ],
    ],
  ]);
}

class _EmptyTimeline extends StatelessWidget {
  const _EmptyTimeline();

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 360),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.forum_outlined, size: 42, color: colors.primary),
            const SizedBox(height: 14),
            Text(
              'No messages yet',
              style: Theme.of(context).textTheme.titleMedium,
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
    final colors = Theme.of(context).colorScheme;
    final textStyle = Theme.of(context).textTheme.labelSmall;
    return Tooltip(
      message: '跳到最新',
      child: Material(
        elevation: 3,
        color: colors.surfaceContainerHigh,
        shape: StadiumBorder(
          side: BorderSide(color: colors.outlineVariant.withValues(alpha: 0.8)),
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
                  color: colors.onSurfaceVariant,
                ),
                if (pendingCount > 0) ...[
                  const SizedBox(width: 4),
                  Text(
                    'New',
                    style: textStyle?.copyWith(color: colors.onSurfaceVariant),
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
      padding: const EdgeInsets.only(bottom: 20),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        mainAxisAlignment: isUser
            ? MainAxisAlignment.end
            : MainAxisAlignment.start,
        children: [
          if (!isUser) const _Avatar(icon: Icons.auto_awesome),
          Flexible(
            child: ConstrainedBox(
              constraints: BoxConstraints(maxWidth: isUser ? 620 : 780),
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
      child: CircleAvatar(
        radius: 13,
        backgroundColor: colors.surfaceContainerHighest,
        child: Icon(icon, size: 15, color: colors.onSurfaceVariant),
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
        color: isUser
            ? scheme.surfaceContainerHighest.withValues(alpha: 0.72)
            : Colors.transparent,
        border: isUser
            ? Border.all(color: scheme.outlineVariant.withValues(alpha: 0.7))
            : null,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Padding(
        padding: EdgeInsets.symmetric(
          horizontal: isUser ? 13 : 0,
          vertical: isUser ? 9 : 0,
        ),
        child: SelectionArea(
          child: GptMarkdown(
            repairAgentMarkdownForDisplay(part.text),
            key: ValueKey('gpt-markdown-${part.id}-${part.status}'),
            style: _markdownBodyStyle(context),
            codeBuilder: (context, name, code, closed) {
              return _CodeBlock(code: code, language: name, surface: surface);
            },
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
                child: GptMarkdown(
                  repairAgentMarkdownForDisplay(widget.part.text),
                  key: ValueKey(
                    'gpt-markdown-${widget.part.id}-${widget.part.status}',
                  ),
                  style: _markdownBodyStyle(context),
                  codeBuilder: (context, name, code, closed) {
                    return _CodeBlock(
                      code: code,
                      language: name,
                      surface: _MarkdownSurface.panel,
                    );
                  },
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
              child: GptMarkdown(
                repairAgentMarkdownForDisplay(part.text),
                key: ValueKey('gpt-markdown-${part.id}-${part.status}'),
                style: _markdownBodyStyle(context),
                codeBuilder: (context, name, code, closed) {
                  return _CodeBlock(
                    code: code,
                    language: name,
                    surface: _MarkdownSurface.panel,
                  );
                },
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
    return Material(
      color: colors.surfaceContainerLow.withValues(alpha: 0.72),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(8),
        side: BorderSide(color: colors.outlineVariant.withValues(alpha: 0.76)),
      ),
      child: child,
    );
  }
}

class _StatusPill extends StatelessWidget {
  const _StatusPill({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(999),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
        child: Text(label, style: Theme.of(context).textTheme.labelSmall),
      ),
    );
  }
}

enum _MarkdownSurface { assistant, user, panel }

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
        : scheme.surfaceContainerHighest;
    final languageLabel = language.trim();

    return Container(
      width: double.infinity,
      margin: const EdgeInsets.symmetric(vertical: 6),
      decoration: BoxDecoration(
        color: codeBackground,
        borderRadius: BorderRadius.circular(8),
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
                    fontFamily: 'Consolas',
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
                  fontFamily: 'Consolas',
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
    height: 1.45,
  );
}
