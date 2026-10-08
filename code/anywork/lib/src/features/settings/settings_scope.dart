import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';

/// 当前可见的设置 tab 索引（由设置壳的 TabController 驱动）。
///
/// 设置页为保留编辑器草稿、搜索筛选与滚动位置，用 keep-alive 保活已访问过的 tab；
/// 但「保留 UI 状态」不等于「持有 topic 租约」。每个数据页据此只在自身可见时
/// `watch` 自己的 scope provider，隐藏时停止 `watch` 让 autoDispose 租约释放，
/// 重新可见时先取得最新基线。lease 本身由 data/domain 单一接线，页面不复制 reducer。
final settingsVisibleTabProvider =
    NotifierProvider<SettingsVisibleTabNotifier, int>(
      SettingsVisibleTabNotifier.new,
      name: 'settingsVisibleTabProvider',
    );

class SettingsVisibleTabNotifier extends Notifier<int> {
  @override
  int build() => 0;

  void select(int index) {
    if (state != index) state = index;
  }
}

/// 单个设置页局部 topic 的传输连接提示。
///
/// 只表达该页自己的传输事实：业务内容始终来自 canonical state 的 last-known 值，
/// 本控件不缓存业务数据。连接正常时不占用布局；失败/重连预算耗尽时给出可读文字、
/// 图标与显式 retry，避免把空列表伪装成「无数据」。
class SettingsTopicStatus extends ConsumerWidget {
  const SettingsTopicStatus({required this.topic, super.key});

  final ProductTopic topic;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final connection = ref.watch(productTopicConnectionProvider(topic));
    final retry = ref.watch(productTopicRetryProvider);
    // connected 无需提示；connecting 是租约建立后的正常等待，也不打扰。
    if (connection == null ||
        connection.phase == ProductTopicConnectionPhase.connected ||
        connection.phase == ProductTopicConnectionPhase.connecting) {
      return const SizedBox.shrink();
    }
    final failed = connection.phase == ProductTopicConnectionPhase.failed;
    final message = connection.errorMessage?.trim();
    // 无具体错误时用通用连接阶段文案（待 ARB 补充通用 key 后替换为 settingsTopic*）。
    final label = (message == null || message.isEmpty)
        ? (failed
              ? context.l10n.settingsSshStateFailed
              : context.l10n.settingsSshStateReconnecting)
        : message;
    return Padding(
      padding: const EdgeInsets.only(top: 12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Icon(
            failed ? Icons.error_outline : Icons.sync_problem_outlined,
            size: 16,
            color: failed ? context.colors.error : context.statusColors.warning,
          ),
          const SizedBox(width: 8),
          Expanded(
            child: Text(
              label,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                color: context.colors.onSurfaceVariant,
              ),
            ),
          ),
          if (failed)
            TextButton.icon(
              key: ValueKey('settings-topic-retry-${topic.runtimeType}'),
              onPressed: () => retry(topic),
              icon: const Icon(Icons.refresh, size: 16),
              label: Text(context.l10n.sidebarRetry),
            ),
        ],
      ),
    );
  }
}
