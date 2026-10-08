part of 'studio_shell.dart';

/// 侧栏会话行右侧固定状态区的单行状态指示。
///
/// 每行只消费自己的 canonical [ThreadStatusView]，不用选中会话的忙碌
/// 状态替代：排队、执行、等待工具、停止中与关闭中显示 16px 小型活动圈；
/// 等待用户确认是静态待确认标识，失败是错误标识，已关闭为静态弱化标识，
/// 空闲不渲染避免噪音。等待用户确认绝不转圈冒充模型工作。
///
/// 减少动画偏好下活动圈改以静态弧呈现，但 [Semantics] 与 [Tooltip] 仍
/// 说明该会话正在工作，不伪装成空闲。
class _ThreadStatusBadge extends StatelessWidget {
  const _ThreadStatusBadge({required this.status});

  final ThreadStatusView status;

  @override
  Widget build(BuildContext context) {
    final label = context.threadStatusLabel(status);
    final Widget indicator = switch (status) {
      ThreadStatusView.queued ||
      ThreadStatusView.running ||
      ThreadStatusView.waitingTool ||
      ThreadStatusView.cancelling ||
      ThreadStatusView.closing => _activeRing(context),
      ThreadStatusView.waitingInteraction => Icon(
        Icons.help_outline,
        size: 16,
        color: context.statusColors.warning,
      ),
      ThreadStatusView.faulted => Icon(
        Icons.error_outline,
        size: 16,
        color: context.colors.error,
      ),
      ThreadStatusView.closed => Icon(
        Icons.check_circle_outlined,
        size: 16,
        color: context.colors.onSurfaceVariant.withValues(alpha: 0.72),
      ),
      ThreadStatusView.idle => const SizedBox.shrink(),
    };
    if (status == ThreadStatusView.idle) {
      return indicator;
    }
    return Semantics(
      container: true,
      label: label,
      child: Tooltip(message: label, child: indicator),
    );
  }

  /// 16px 小型活动圈；`disableAnimations` 时以固定弧静态呈现，
  /// 颜色使用主题的眼眸蓝活动指示语义，不引入第二份状态色。
  Widget _activeRing(BuildContext context) {
    final reducedMotion = MediaQuery.disableAnimationsOf(context);
    return SizedBox.square(
      dimension: 16,
      child: CircularProgressIndicator(
        strokeWidth: 2,
        value: reducedMotion ? 0.75 : null,
        color: context.statusColors.activeIndicator,
        strokeCap: StrokeCap.round,
      ),
    );
  }
}
