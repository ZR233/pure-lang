part of 'timeline_view.dart';

class TimelinePlanSummaryCard extends StatelessWidget {
  const TimelinePlanSummaryCard({
    required this.plan,
    required this.expanded,
    required this.onPressed,
    super.key,
  });

  final PlanConfirmationView plan;
  final bool expanded;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final summary = plan.summary;
    final title = plan.title.isEmpty
        ? context.l10n.interactionPlanReadyTitle
        : plan.title;
    return Semantics(
      button: true,
      expanded: expanded,
      child: StudioPanel(
        key: StudioDriverKeys.planSummary,
        backgroundColor: expanded
            ? context.colors.surfaceContainerLow
            : context.colors.surfaceContainerLowest,
        borderColor: Colors.transparent,
        radius: StudioRadii.lg,
        shadow: false,
        child: InkWell(
          onTap: onPressed,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(12, 11, 12, 10),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(
                  Icons.checklist_outlined,
                  size: 20,
                  color: context.colors.onSurfaceVariant,
                ),
                const SizedBox(width: 11),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: context.text.titleSmall?.copyWith(
                          color: context.colors.onSurface,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      if (summary.isNotEmpty) ...[
                        const SizedBox(height: 2),
                        Text(
                          summary,
                          maxLines: 2,
                          overflow: TextOverflow.ellipsis,
                          style: context.text.bodySmall?.copyWith(
                            color: context.colors.onSurfaceVariant,
                          ),
                        ),
                      ],
                      const SizedBox(height: 8),
                      Wrap(
                        spacing: 12,
                        runSpacing: 6,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          StudioPill(
                            tone: StudioTone.warning,
                            icon: Icons.schedule_outlined,
                            label: context
                                .l10n
                                .interactionPlanAwaitingConfirmation,
                          ),
                          Text(
                            context.l10n.interactionPlanViewDetails,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: context.text.labelMedium?.copyWith(
                              color: context.colors.onPrimaryContainer,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: 6),
                Padding(
                  padding: const EdgeInsets.only(top: 7),
                  child: Icon(
                    expanded ? Icons.chevron_left : Icons.chevron_right,
                    size: 20,
                    color: context.colors.onSurfaceVariant,
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class PlanDetailPanel extends StatelessWidget {
  const PlanDetailPanel({
    required this.plan,
    required this.onClose,
    this.overlay = false,
    super.key,
  });

  /// 固定头部 + 独立滚动正文至少需要的可用高度。
  ///
  /// 低于该高度时（短窗口 / 大字号 / 覆盖在很矮的 Timeline 上），固定头部自身就可能
  /// 超过可用高度并触发 RenderFlex 溢出；此时整块退化为单一滚动容器，读法与锚点不变。
  static const double _minimumPinnedHeight = 260;

  final PlanConfirmationView plan;
  final VoidCallback onClose;
  final bool overlay;

  @override
  Widget build(BuildContext context) {
    final header = Padding(
      padding: const EdgeInsets.fromLTRB(16, 14, 10, 12),
      child: Row(
        children: [
          Icon(
            Icons.checklist_outlined,
            size: 19,
            color: context.colors.primary,
          ),
          const SizedBox(width: 9),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  context.l10n.interactionPlanDetailsTitle,
                  style: context.text.titleSmall?.copyWith(
                    color: context.colors.onSurface,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                Text(
                  context.l10n.interactionPlanAwaitingConfirmation,
                  style: context.text.labelSmall?.copyWith(
                    color: context.colors.onPrimaryContainer,
                  ),
                ),
              ],
            ),
          ),
          IconButton(
            key: StudioDriverKeys.planDetailsClose,
            tooltip: MaterialLocalizations.of(context).closeButtonTooltip,
            icon: const Icon(Icons.chevron_right),
            onPressed: onClose,
          ),
        ],
      ),
    );
    final divider = Divider(height: 1, color: context.colors.outlineVariant);
    final markdown = _AgentMarkdown(
      id: 'plan:${plan.interactionId}',
      status: 'pending',
      text: plan.markdown,
      surface: _MarkdownSurface.panel,
    );
    return Material(
      key: StudioDriverKeys.planDetails,
      color: context.colors.surfaceContainerLowest,
      elevation: overlay ? 8 : 0,
      shadowColor: context.colors.shadow.withValues(alpha: 0.2),
      shape: Border(left: BorderSide(color: context.colors.outline)),
      child: SafeArea(
        left: false,
        child: LayoutBuilder(
          builder: (context, constraints) {
            if (constraints.hasBoundedHeight &&
                constraints.maxHeight < _minimumPinnedHeight) {
              // 放不下固定头部时整块滚动，避免 RenderFlex 溢出（§19.10 短视口可读）。
              return SelectionArea(
                child: SingleChildScrollView(
                  key: StudioDriverKeys.planDetailsScroll,
                  primary: false,
                  padding: const EdgeInsets.only(bottom: 28),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      header,
                      divider,
                      Padding(
                        padding: const EdgeInsets.fromLTRB(18, 18, 18, 0),
                        child: markdown,
                      ),
                    ],
                  ),
                ),
              );
            }
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                header,
                divider,
                Expanded(
                  child: SelectionArea(
                    child: KeyedSubtree(
                      key: StudioDriverKeys.planDetailsScroll,
                      child: ListView(
                        key: PageStorageKey<String>(
                          'plan-details:${plan.interactionId}',
                        ),
                        primary: false,
                        padding: const EdgeInsets.fromLTRB(18, 18, 18, 28),
                        children: [markdown],
                      ),
                    ),
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }
}
