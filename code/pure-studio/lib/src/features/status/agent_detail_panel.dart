import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/agent_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';

/// 子代理状态弹层。
///
/// 在状态栏的活动摘要上悬停或点击时展示，按 depth/parentPath
/// 渲染树形结构，每个 agent 卡片可点击展开查看摘要、错误、路径等详情。
class AgentDetailPanel extends StatelessWidget {
  const AgentDetailPanel({required this.agents, super.key});

  final List<StudioAgentView> agents;

  @override
  Widget build(BuildContext context) {
    if (agents.isEmpty) {
      return _AgentDetailEmpty();
    }
    final runningCount = agents.where((agent) => agent.state.isActive).length;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _AgentDetailHeader(count: agents.length, runningCount: runningCount),
        const SizedBox(height: 12),
        Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            for (final agent in agents)
              Padding(
                padding: EdgeInsets.only(bottom: 6),
                child: AgentTreeCard(agent: agent),
              ),
          ],
        ),
      ],
    );
  }
}

class _AgentDetailHeader extends StatelessWidget {
  const _AgentDetailHeader({required this.count, required this.runningCount});

  final int count;
  final int runningCount;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Icon(
          Icons.account_tree_outlined,
          size: 15,
          color: StudioColors.clayDeep,
        ),
        const SizedBox(width: 8),
        Expanded(
          child: Text(
            context.l10n.agentDetailTitle.toUpperCase(),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.labelSmall?.copyWith(
              color: context.studioInkSoft.withValues(alpha: 0.72),
              fontFamily: 'Consolas',
              fontSize: 9.5,
              fontWeight: FontWeight.w600,
              letterSpacing: 0,
            ),
          ),
        ),
        Text(
          context.l10n.agentDetailSummary(count, runningCount),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: context.text.labelSmall?.copyWith(
            color: runningCount > 0
                ? StudioColors.clayDeep
                : context.studioInkSoft,
            fontWeight: FontWeight.w600,
          ),
        ),
      ],
    );
  }
}

class _AgentDetailEmpty extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return StudioEmptyState(
      icon: Icons.account_tree_outlined,
      title: context.l10n.agentDetailTitle,
      message: context.l10n.agentDetailEmpty,
    );
  }
}

/// 单个 agent 卡片，可点击展开详情。
class AgentTreeCard extends StatefulWidget {
  const AgentTreeCard({required this.agent, super.key});

  final StudioAgentView agent;

  @override
  State<AgentTreeCard> createState() => _AgentTreeCardState();
}

class _AgentTreeCardState extends State<AgentTreeCard> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final agent = widget.agent;
    final style = _statusStyle(context, agent.state);
    final indent = (agent.depth.clamp(0, 6)) * 22.0;
    final hasDetails =
        (agent.summary?.isNotEmpty ?? false) ||
        (agent.error?.isNotEmpty ?? false);
    return Padding(
      padding: EdgeInsets.only(left: indent),
      child: _AgentTreeConnector(
        depth: agent.depth,
        child: _card(context, agent, style, hasDetails),
      ),
    );
  }

  Widget _card(
    BuildContext context,
    StudioAgentView agent,
    _AgentStatusStyle style,
    bool hasDetails,
  ) {
    return Material(
      color: context.studioPaper2,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(StudioRadii.sm),
        side: BorderSide(
          color: agent.state.isActive
              ? style.color.withValues(alpha: 0.34)
              : context.colors.outlineVariant.withValues(alpha: 0.5),
        ),
      ),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: hasDetails ? () => setState(() => _expanded = !_expanded) : null,
        child: Padding(
          padding: const EdgeInsets.fromLTRB(11, 9, 11, 9),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _headerRow(context, agent, style, hasDetails),
              if (_expanded) ...[
                const SizedBox(height: 9),
                _expandedDetails(context, agent, style),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _headerRow(
    BuildContext context,
    StudioAgentView agent,
    _AgentStatusStyle style,
    bool hasDetails,
  ) {
    return Row(
      children: [
        _AgentStatusDot(style: style, active: agent.state.isActive),
        const SizedBox(width: 9),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Flexible(
                    child: Text(
                      agent.role.isNotEmpty
                          ? context.roleLabel(agent.role)
                          : agent.id,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: context.text.labelLarge?.copyWith(
                        color: context.studioInk,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Flexible(
                    child: Text(
                      agent.task,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: context.text.bodySmall?.copyWith(
                        color: context.studioInkSoft,
                      ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
        const SizedBox(width: 8),
        _AgentStatusLabel(style: style),
        if (hasDetails) ...[
          const SizedBox(width: 4),
          Icon(
            _expanded
                ? Icons.keyboard_arrow_up_rounded
                : Icons.keyboard_arrow_down_rounded,
            size: 16,
            color: context.studioInkSoft,
          ),
        ],
      ],
    );
  }

  Widget _expandedDetails(
    BuildContext context,
    StudioAgentView agent,
    _AgentStatusStyle style,
  ) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Divider(
          height: 1,
          color: context.colors.outlineVariant.withValues(alpha: 0.5),
        ),
        const SizedBox(height: 9),
        if (agent.summary?.isNotEmpty ?? false)
          _DetailLine(
            label: context.l10n.agentDetailSummaryLabel,
            value: agent.summary!,
          ),
        if (agent.error?.isNotEmpty ?? false)
          _DetailLine(
            label: context.l10n.agentDetailErrorLabel,
            value: agent.error!,
            valueColor: StudioColors.rose,
          ),
        _DetailLine(
          label: context.l10n.agentDetailPathLabel,
          value: agent.path,
          monospace: true,
        ),
      ],
    );
  }
}

class _AgentTreeConnector extends StatelessWidget {
  const _AgentTreeConnector({required this.depth, required this.child});

  final int depth;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    if (depth <= 0) {
      return child;
    }
    // 渲染每层缩进的竖向连接线，体现父子层级。
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (var level = 1; level < depth; level++)
          Padding(
            padding: const EdgeInsets.only(right: 10),
            child: SizedBox(
              width: 1,
              height: 28,
              child: DecoratedBox(
                decoration: BoxDecoration(color: context.studioLine2),
              ),
            ),
          ),
        Padding(
          padding: const EdgeInsets.only(right: 8, top: 2),
          child: SizedBox(
            width: 12,
            child: Column(
              children: [
                SizedBox(
                  width: 10,
                  height: 9,
                  child: CustomPaint(
                    painter: _TreeElbowPainter(color: context.studioLine2),
                  ),
                ),
              ],
            ),
          ),
        ),
        Expanded(child: child),
      ],
    );
  }
}

class _TreeElbowPainter extends CustomPainter {
  const _TreeElbowPainter({required this.color});

  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = color
      ..strokeWidth = 1
      ..style = PaintingStyle.stroke;
    // 从顶部中间画到中间，再向右延伸
    final mid = Offset(size.width * 0.5, size.height * 0.6);
    canvas.drawLine(Offset(size.width * 0.5, 0), mid, paint);
    canvas.drawLine(mid, Offset(size.width, mid.dy), paint);
  }

  @override
  bool shouldRepaint(covariant _TreeElbowPainter oldDelegate) =>
      oldDelegate.color != color;
}

class _AgentStatusDot extends StatelessWidget {
  const _AgentStatusDot({required this.style, required this.active});

  final _AgentStatusStyle style;
  final bool active;

  @override
  Widget build(BuildContext context) {
    return SizedBox.square(
      dimension: 16,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: style.backgroundColor,
          shape: BoxShape.circle,
        ),
        child: Icon(style.icon, size: 10, color: style.color),
      ),
    );
  }
}

class _AgentStatusLabel extends StatelessWidget {
  const _AgentStatusLabel({required this.style});

  final _AgentStatusStyle style;

  @override
  Widget build(BuildContext context) {
    return StudioPill(
      label: style.label,
      backgroundColor: style.backgroundColor,
      foregroundColor: style.color,
    );
  }
}

class _DetailLine extends StatelessWidget {
  const _DetailLine({
    required this.label,
    required this.value,
    this.valueColor,
    this.monospace = false,
  });

  final String label;
  final String value;
  final Color? valueColor;
  final bool monospace;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 5),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 42,
            child: Text(
              label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.labelSmall?.copyWith(
                color: context.studioInkSoft.withValues(alpha: 0.72),
                fontWeight: FontWeight.w500,
              ),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              value,
              maxLines: 4,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                color: valueColor ?? context.studioInk,
                height: 1.4,
                fontFamily: monospace ? 'Consolas' : null,
                fontSize: monospace ? 11 : null,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

_AgentStatusStyle _statusStyle(BuildContext context, StudioAgentState state) {
  final l10n = context.l10n;
  return switch (state) {
    RunningStudioAgent() => _AgentStatusStyle(
      icon: Icons.play_arrow_rounded,
      color: StudioColors.clayDeep,
      backgroundColor: StudioColors.claySoft,
      label: l10n.agentDetailStatusRunning,
    ),
    QueuedStudioAgent() => _AgentStatusStyle(
      icon: Icons.schedule_rounded,
      color: StudioColors.clayDeep,
      backgroundColor: StudioColors.claySoft,
      label: l10n.agentDetailStatusQueued,
    ),
    WaitingToolStudioAgent() ||
    WaitingInteractionStudioAgent() => _AgentStatusStyle(
      icon: Icons.hourglass_top_rounded,
      color: StudioColors.ochre,
      backgroundColor: StudioColors.ochre.withValues(alpha: 0.16),
      label: l10n.agentDetailStatusWaiting,
    ),
    IdleStudioAgent() => _AgentStatusStyle(
      icon: Icons.check_rounded,
      color: StudioColors.sage,
      backgroundColor: StudioColors.sageSoft,
      label: l10n.agentDetailStatusCompleted,
    ),
    FaultedStudioAgent() => _AgentStatusStyle(
      icon: Icons.error_outline_rounded,
      color: StudioColors.rose,
      backgroundColor: StudioColors.rose.withValues(alpha: 0.14),
      label: l10n.agentDetailStatusErrored,
    ),
    CancellingStudioAgent() => _AgentStatusStyle(
      icon: Icons.do_not_disturb_on_outlined,
      color: StudioColors.rose,
      backgroundColor: StudioColors.rose.withValues(alpha: 0.14),
      label: l10n.agentDetailStatusInterrupted,
    ),
    ClosingStudioAgent() || ClosedStudioAgent() => _AgentStatusStyle(
      icon: Icons.power_settings_new_rounded,
      color: context.studioInkSoft,
      backgroundColor: context.studioPaper3,
      label: l10n.agentDetailStatusShutdown,
    ),
  };
}

class _AgentStatusStyle {
  const _AgentStatusStyle({
    required this.icon,
    required this.color,
    required this.backgroundColor,
    required this.label,
  });

  final IconData icon;
  final Color color;
  final Color backgroundColor;
  final String label;
}
