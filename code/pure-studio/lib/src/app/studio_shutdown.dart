import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../data/frb/studio_api.dart';
import '../domain/models/studio_models.dart';
import '../l10n/app_localizations.dart';
import '../l10n/studio_l10n.dart';
import '../shared/studio_driver_state.dart';

part 'studio_shutdown.g.dart';

/// 当前关机进度；null 表示未在关机。
@Riverpod(keepAlive: true)
class StudioShutdownProgressState extends _$StudioShutdownProgressState {
  @override
  StudioShutdownProgress? build() => null;

  void update(StudioShutdownProgress progress) => state = progress;
}

/// 顺序关闭 runtime：先订阅关机进度流再触发 shutdown，保证阶段事件可达。
Future<void> runStudioShutdown(
  StudioApi api,
  void Function(StudioShutdownProgress progress) onProgress,
) async {
  final subscription = api.subscribeShutdownProgress().listen((progress) {
    onProgress(progress);
    StudioDriverState.publishShutdownProgress(progress);
  });
  try {
    await api.shutdownRuntime();
  } finally {
    await subscription.cancel();
  }
}

/// 关机阶段 overlay：不可关闭，展示阶段文案与落库进度，等待数据库存完。
class StudioShutdownOverlay extends ConsumerWidget {
  const StudioShutdownOverlay({required this.child, super.key});

  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final progress = ref.watch(studioShutdownProgressStateProvider);
    if (progress == null) return child;
    return Stack(
      children: [
        child,
        Positioned.fill(
          child: ColoredBox(
            color: Colors.black.withValues(alpha: 0.45),
            child: Center(child: _ShutdownProgressCard(progress: progress)),
          ),
        ),
      ],
    );
  }
}

class _ShutdownProgressCard extends StatelessWidget {
  const _ShutdownProgressCard({required this.progress});

  final StudioShutdownProgress progress;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return Card(
      key: const ValueKey('studio-shutdown-overlay'),
      elevation: 6,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 28, vertical: 22),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                const SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(strokeWidth: 2.4),
                ),
                const SizedBox(width: 12),
                Text(
                  l10n.shutdownTitle,
                  style: Theme.of(context).textTheme.titleMedium
                      ?.copyWith(fontWeight: FontWeight.w700),
                ),
              ],
            ),
            const SizedBox(height: 14),
            Text(
              shutdownPhaseLabel(l10n, progress),
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            const SizedBox(height: 10),
            Text(
              '${progress.phase.index1} / ${StudioShutdownPhase.values.length}',
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ],
        ),
      ),
    );
  }
}

String shutdownPhaseLabel(
  AppLocalizations l10n,
  StudioShutdownProgress progress,
) {
  final String label = switch (progress.phase) {
    StudioShutdownPhase.stoppingSubscriptions =>
      l10n.shutdownPhaseStoppingSubscriptions,
    StudioShutdownPhase.cancellingTurns => l10n.shutdownPhaseCancellingTurns,
    StudioShutdownPhase.flushingPersistence =>
      l10n.shutdownPhaseFlushingPersistence,
    StudioShutdownPhase.stoppingAgents => l10n.shutdownPhaseStoppingAgents,
    StudioShutdownPhase.stoppingMcp => l10n.shutdownPhaseStoppingMcp,
    StudioShutdownPhase.stoppingLsp => l10n.shutdownPhaseStoppingLsp,
    StudioShutdownPhase.stopped => l10n.shutdownPhaseStopped,
  };
  if (progress case FlushingPersistenceProgress(:final pendingCommits)
      when pendingCommits > 0) {
    return '$label（${l10n.shutdownPendingCommits(pendingCommits)}）';
  }
  return label;
}
