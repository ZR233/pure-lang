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
            ? Color.alphaBlend(
                StudioColors.claySoft.withValues(
                  alpha: context.isDark ? 0.24 : 0.5,
                ),
                context.colors.surfaceContainerLowest,
              )
            : context.colors.surfaceContainerLowest,
        borderColor: expanded
            ? StudioColors.clay.withValues(alpha: 0.66)
            : context.studioLine2,
        radius: StudioRadii.lg,
        shadow: true,
        child: InkWell(
          onTap: onPressed,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(12, 11, 12, 10),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const StudioIconBadge(icon: Icons.checklist_outlined, size: 36),
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
                          color: context.studioInk,
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
                            color: context.studioInkSoft,
                          ),
                        ),
                      ],
                      const SizedBox(height: 8),
                      Row(
                        children: [
                          StudioPill(
                            icon: Icons.schedule_outlined,
                            label: context
                                .l10n
                                .interactionPlanAwaitingConfirmation,
                            backgroundColor: StudioColors.claySoft,
                            foregroundColor: StudioColors.clayDeep,
                            borderColor: StudioColors.clay.withValues(
                              alpha: 0.24,
                            ),
                          ),
                          const Spacer(),
                          Flexible(
                            child: Text(
                              context.l10n.interactionPlanViewDetails,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: context.text.labelMedium?.copyWith(
                                color: StudioColors.clayDeep,
                                fontWeight: FontWeight.w600,
                              ),
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
                    color: context.studioInkSoft,
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

  final PlanConfirmationView plan;
  final VoidCallback onClose;
  final bool overlay;

  @override
  Widget build(BuildContext context) {
    return Material(
      key: StudioDriverKeys.planDetails,
      color: context.colors.surfaceContainerLowest,
      elevation: overlay ? 8 : 0,
      shadowColor: context.colors.shadow.withValues(alpha: 0.2),
      shape: Border(left: BorderSide(color: context.studioLine2)),
      child: SafeArea(
        left: false,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 14, 10, 12),
              child: Row(
                children: [
                  const Icon(
                    Icons.checklist_outlined,
                    size: 19,
                    color: StudioColors.clay,
                  ),
                  const SizedBox(width: 9),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          context.l10n.interactionPlanDetailsTitle,
                          style: context.text.titleSmall?.copyWith(
                            color: context.studioInk,
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                        Text(
                          context.l10n.interactionPlanAwaitingConfirmation,
                          style: context.text.labelSmall?.copyWith(
                            color: StudioColors.clayDeep,
                          ),
                        ),
                      ],
                    ),
                  ),
                  IconButton(
                    key: StudioDriverKeys.planDetailsClose,
                    tooltip: MaterialLocalizations.of(context)
                        .closeButtonTooltip,
                    icon: const Icon(Icons.chevron_right),
                    onPressed: onClose,
                  ),
                ],
              ),
            ),
            Divider(height: 1, color: context.studioLine),
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
                    children: [
                      _AgentMarkdown(
                        id: 'plan:${plan.interactionId}',
                        status: 'pending',
                        text: plan.markdown,
                        surface: _MarkdownSurface.panel,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
