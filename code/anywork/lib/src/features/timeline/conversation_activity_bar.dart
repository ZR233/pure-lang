import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/conversation_activity_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'timeline_wait_indicator.dart';

/// 输入框上方的固定当前活动条（单行）。
///
/// - 只消费 [ConversationActivityView]：阶段/摘要来自后端 typed 活动投影，完整详情由
///   宿主在展开时按活动身份**按需**读取（与消息窗口无关，滚历史时依旧可用）。
/// - 标签与摘要都单行、只做视觉 ellipsis，不改内容；完整内容在向上展开的有界详情里。
/// - 详情的实际高度上限是**本帧布局约束**：宿主只把输入区与状态栏之后的剩余高度交给
///   本条（shell 的 footer 用 `Flexible` 给出），所以展开详情不可能把输入区顶出窗口，
///   超出部分在详情内部滚动。窗口不足以放下一行时，本条先收缩详情、再收缩自身。
/// - 展开态按 activity 身份保存：同一身份在更新中保持展开，身份变化即重置。
class ConversationActivityBar extends StatefulWidget {
  const ConversationActivityBar({
    required this.view,
    this.onExpand,
    this.onCollapse,
    this.onExpandedChanged,
    super.key,
  });

  final ConversationActivityView view;

  /// 用户展开详情时触发按需读取；为空表示宿主不提供详情。
  final VoidCallback? onExpand;

  /// 用户收起详情或活动身份变化时触发，用于停止后续跟随刷新。
  final VoidCallback? onCollapse;

  /// 实际展开态变化时上报（Driver 可观察的“此刻是否展开”）。
  ///
  /// 这与 [ConversationActivityView.expandable] 的“可展开能力”是两件事：能力描述该活动
  /// 是否存在可展开的完整内容，展开态描述用户此刻是否真的把它展开。
  final ValueChanged<bool>? onExpandedChanged;

  @override
  State<ConversationActivityBar> createState() =>
      _ConversationActivityBarState();
}

class _ConversationActivityBarState extends State<ConversationActivityBar> {
  /// 详情高度的**上界** = min(窗口高度 * 比例, 绝对上限)。
  ///
  /// 真正的上限是本帧布局约束（宿主给出的剩余高度，见 [build]）；这里只保证详情不会
  /// 占满一个很高的窗口。详情内容超出后在详情内部滚动，绝不吃掉输入区的可用空间。
  static const _detailsHeightFraction = 0.45;
  static const _detailsHeightCeiling = 320.0;

  /// 展开详情的**理想最小**可读高度（约数行）：布局约束允许时至少给到这么多，
  /// 约束不够时由 [BoxConstraints.enforce] 自动收紧，不会顶出输入区。
  static const _detailsMinHeight = 56.0;

  /// 窄窗口下先让出的元素阈值（宽度不足时不再显示数量徽标/摘要文本）。
  static const _badgeMinWidth = 220.0;
  static const _summaryMinWidth = 260.0;

  final ScrollController _detailsController = ScrollController();
  String? _expandedIdentity;

  @override
  void initState() {
    super.initState();
    // 首帧即上报初始展开态（新建的状态总是收起）；只写诊断边界，不触发 rebuild。
    widget.onExpandedChanged?.call(false);
  }

  @override
  void didUpdateWidget(covariant ConversationActivityBar oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.view.identity != widget.view.identity) {
      if (_expandedIdentity != null) {
        _expandedIdentity = null;
        widget.onCollapse?.call();
        widget.onExpandedChanged?.call(false);
      }
    }
  }

  @override
  void dispose() {
    // 活动条消失（活动结束/切走会话/面板被移除）必须与收起等价：否则 controller 会为
    // 一个已经结束的活动保留展开标记与尾随刷新，Driver 的 `expanded` 诊断也会残留为 true。
    // 这里只写 controller 的私有标记与只读诊断，不改 provider，因此可以在卸载期安全调用。
    if (_expandedIdentity != null) {
      _expandedIdentity = null;
      widget.onCollapse?.call();
      widget.onExpandedChanged?.call(false);
    }
    _detailsController.dispose();
    super.dispose();
  }

  bool get _expanded =>
      _expandedIdentity == widget.view.identity && (_expandedIdentity != null);

  /// 详情高度的上界（窗口层面的兜底，不是宿主剩余空间）。
  ///
  /// 布局约束严格优先：宿主把剩余高度交给本条后，`Flexible` 已经把详情压进该高度，
  /// 这里只额外限制详情不占满很高的窗口。详情另有 [_detailsMinHeight] 的理想下限，
  /// 但该下限会经 `BoxConstraints.enforce` 被宿主约束收紧，因此约束不足时只会更矮，
  /// 不会把输入区顶出窗口。
  double _detailsMaxHeight(BuildContext context) {
    final window = MediaQuery.sizeOf(context).height;
    if (!window.isFinite || window <= 0) {
      return _detailsHeightCeiling;
    }
    return math.min(_detailsHeightCeiling, window * _detailsHeightFraction);
  }

  void _toggle() {
    final expanding = !_expanded;
    setState(() {
      _expandedIdentity = expanding ? widget.view.identity : null;
    });
    widget.onExpandedChanged?.call(expanding);
    if (expanding) {
      widget.onExpand?.call();
    } else {
      widget.onCollapse?.call();
    }
  }

  @override
  Widget build(BuildContext context) {
    final view = widget.view;
    final label = context.conversationActivityLabel(view);
    final error = view.errorMessage?.trim();
    final summary = view.summary?.trim();
    final secondary = error != null && error.isNotEmpty
        ? error
        : summary != null && summary.isNotEmpty
        ? summary
        : null;
    final expanded = _expanded;
    final semanticsLabel = secondary == null ? label : '$label · $secondary';
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 7, 12, 0),
      child: Align(
        alignment: Alignment.center,
        // 只横向居中：高度按内容收缩，避免把宿主给整条活动条的剩余高度全占满，
        // 否则输入区上方会出现一片无意义的空白。
        heightFactor: 1,
        child: ConstrainedBox(
          constraints: const BoxConstraints(
            maxWidth: StudioLayout.conversationWidth,
          ),
          child: DecoratedBox(
            decoration: BoxDecoration(
              color: context.colors.surfaceContainer,
              borderRadius: BorderRadius.circular(StudioRadii.sm),
              border: Border.all(
                color: context.colors.outlineVariant.withValues(alpha: 0.62),
              ),
            ),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(StudioRadii.sm),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (expanded) ...[
                    // 详情是本列里唯一可伸缩的部分：宿主给到的剩余高度不足时先压缩它
                    // （内容在详情内部滚动、可选择复制），标签行与输入区始终完整可见。
                    Flexible(
                      fit: FlexFit.loose,
                      child: _details(context, view),
                    ),
                    Divider(
                      height: 1,
                      thickness: 1,
                      color: context.colors.outlineVariant.withValues(
                        alpha: 0.6,
                      ),
                    ),
                  ],
                  _summaryRow(
                    context,
                    view: view,
                    label: label,
                    secondary: secondary,
                    semanticsLabel: semanticsLabel,
                    expanded: expanded,
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _summaryRow(
    BuildContext context, {
    required ConversationActivityView view,
    required String label,
    required String? secondary,
    required String semanticsLabel,
    required bool expanded,
  }) {
    final isIssue = view.kind.isIssue;
    final color = isIssue
        ? context.colors.error
        : view.waits
        ? context.colors.onSurface
        : context.colors.onSurfaceVariant;
    return Semantics(
      container: true,
      button: view.hasDetails,
      expanded: view.hasDetails ? expanded : null,
      liveRegion: true,
      label: semanticsLabel,
      onTap: view.hasDetails ? _toggle : null,
      excludeSemantics: true,
      child: Material(
        key: StudioDriverKeys.conversationActivity,
        color: Colors.transparent,
        child: InkWell(
          onTap: view.hasDetails ? _toggle : null,
          // 键盘可达：InkWell 自带 Focus 与 ActivateIntent（Enter/Space）→ onTap，
          // 因此 Tab 聚焦后回车/空格即可展开与收起；没有详情时不进入焦点链。
          canRequestFocus: view.hasDetails,
          excludeFromSemantics: true,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 7),
            child: LayoutBuilder(
              builder: (context, constraints) {
                // 极窄窗口下先让数量徽标、再让摘要退出，标签本身始终可收缩；
                // 这样 Row 里只剩可收缩文本与固定尺寸图标，不会挤出 RenderFlex。
                final width = constraints.maxWidth;
                final showBadge =
                    view.activeToolCount > 1 && width >= _badgeMinWidth;
                final showSummary =
                    secondary != null && width >= _summaryMinWidth;
                return Row(
                  children: [
                    _leading(context, view, color),
                    const SizedBox(width: 8),
                    Flexible(
                      child: ConstrainedBox(
                        // 标签来自固定集合（最长“等待工具授权/保存本轮结果”），
                        // 限宽后仍会随可用宽度收缩并省略。
                        constraints: const BoxConstraints(maxWidth: 168),
                        child: Text(
                          label,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          softWrap: false,
                          style: context.text.bodySmall?.copyWith(
                            color: color,
                            fontWeight: FontWeight.w600,
                            height: 1.25,
                          ),
                        ),
                      ),
                    ),
                    if (showBadge) ...[
                      const SizedBox(width: 8),
                      _toolCountBadge(context, view.activeToolCount),
                    ],
                    if (showSummary) ...[
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          secondary,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          softWrap: false,
                          style: context.text.bodySmall?.copyWith(
                            color: isIssue
                                ? context.colors.error
                                : context.colors.onSurfaceVariant,
                            height: 1.25,
                          ),
                        ),
                      ),
                    ] else
                      const Spacer(),
                    if (view.hasDetails) ...[
                      const SizedBox(width: 6),
                      Icon(
                        expanded
                            ? Icons.keyboard_arrow_down_rounded
                            : Icons.keyboard_arrow_up_rounded,
                        size: 18,
                        color: context.colors.onSurfaceVariant,
                      ),
                    ],
                  ],
                );
              },
            ),
          ),
        ),
      ),
    );
  }

  Widget _leading(
    BuildContext context,
    ConversationActivityView view,
    Color color,
  ) {
    if (view.waits) {
      return const TimelineWaitIndicator(
        key: ValueKey('conversation-activity-pulse'),
      );
    }
    return Icon(view.kind.icon, size: 16, color: color);
  }

  Widget _toolCountBadge(BuildContext context, int count) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.colors.surfaceContainerHigh,
        borderRadius: BorderRadius.circular(StudioRadii.pill),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 1),
        child: Text(
          context.l10n.timelineToolGroupSummary(count),
          style: context.text.labelSmall?.copyWith(
            color: context.colors.onSurfaceVariant,
          ),
        ),
      ),
    );
  }

  /// 向上原位展开的有界详情：完整内容按原样渲染，可选择复制。
  Widget _details(BuildContext context, ConversationActivityView view) {
    final error = view.detailsError?.trim();
    final maxHeight = _detailsMaxHeight(context);
    return Container(
      key: StudioDriverKeys.conversationActivityDetails,
      constraints: BoxConstraints(
        minHeight: math.min(_detailsMinHeight, maxHeight),
        maxHeight: maxHeight,
      ),
      color: context.colors.surfaceContainerLow,
      child: view.details.isEmpty && view.detailsLoading
          ? const Padding(
              padding: EdgeInsets.symmetric(horizontal: 14, vertical: 14),
              child: TimelineWaitIndicator(
                key: ValueKey('conversation-activity-details-loading'),
              ),
            )
          : Scrollbar(
              controller: _detailsController,
              child: SingleChildScrollView(
                controller: _detailsController,
                primary: false,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    if (error != null && error.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
                        child: Text(
                          error,
                          style: context.text.bodySmall?.copyWith(
                            color: context.colors.error,
                            height: 1.45,
                          ),
                        ),
                      ),
                    for (final (index, detail) in view.details.indexed) ...[
                      if (index > 0)
                        Divider(
                          height: 1,
                          thickness: 1,
                          color: context.colors.outlineVariant.withValues(
                            alpha: 0.5,
                          ),
                        ),
                      Padding(
                        padding: const EdgeInsets.fromLTRB(12, 10, 12, 10),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            if (detail.hasTitle) ...[
                              Text(
                                detail.title!.trim(),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: context.text.labelLarge?.copyWith(
                                  color: context.colors.onSurface,
                                ),
                              ),
                              const SizedBox(height: 6),
                            ],
                            SelectableText(
                              detail.body.trim(),
                              style: context.text.bodySmall?.copyWith(
                                color: context.colors.onSurfaceVariant,
                                height: 1.45,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ],
                ),
              ),
            ),
    );
  }
}
