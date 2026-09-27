import 'dart:ui' show AppExitType;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../data/frb/studio_api.dart';
import '../domain/models/studio_models.dart';
import '../l10n/app_localizations.dart';
import '../l10n/studio_l10n.dart';
import '../shared/studio_driver_state.dart';

part 'studio_shutdown.g.dart';

class StudioShutdownView {
  const StudioShutdownView({this.progress, this.error});

  final StudioShutdownProgress? progress;
  final String? error;
}

/// 当前关机进度或失败状态；两者均为空表示未在关机。
@Riverpod(keepAlive: true)
class StudioShutdownProgressState extends _$StudioShutdownProgressState {
  @override
  StudioShutdownView build() => const StudioShutdownView();

  void update(StudioShutdownProgress progress) =>
      state = StudioShutdownView(progress: progress);

  void fail(Object error) =>
      state = StudioShutdownView(error: error.toString());
}

/// 顺序关闭 runtime：先订阅关机进度流再触发 shutdown，保证阶段事件可达。
Future<void> runStudioShutdown(
  StudioApi api,
  void Function(StudioShutdownProgress progress) onProgress,
) async {
  var stopped = false;
  void publish(StudioShutdownProgress progress) {
    if (stopped) return;
    stopped = progress.phase == StudioShutdownPhase.stopped;
    onProgress(progress);
    StudioDriverState.publishShutdownProgress(progress);
  }

  final subscription = api.subscribeShutdownProgress().listen(publish);
  try {
    await api.shutdownRuntime();
    // Successful command completion acknowledges native Stopped even if bridge
    // disposal prevented the terminal progress-stream event from arriving.
    publish(const StoppedProgress());
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
    final shutdown = ref.watch(studioShutdownProgressStateProvider);
    if (shutdown.progress == null && shutdown.error == null) return child;
    return Stack(
      children: [
        child,
        Positioned.fill(
          child: ColoredBox(
            color: Theme.of(context).colorScheme.scrim.withValues(alpha: 0.45),
            child: Center(
              child: shutdown.error != null
                  ? _ShutdownFailureCard(error: shutdown.error!)
                  : _ShutdownProgressCard(progress: shutdown.progress!),
            ),
          ),
        ),
      ],
    );
  }
}

class _ShutdownFailureCard extends StatelessWidget {
  const _ShutdownFailureCard({required this.error});

  final String error;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              l10n.shutdownFailed,
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 12),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 440),
              child: Text(error, maxLines: 5, overflow: TextOverflow.ellipsis),
            ),
            const SizedBox(height: 16),
            FilledButton(
              onPressed: () => ServicesBinding.instance.exitApplication(
                AppExitType.cancelable,
              ),
              child: Text(l10n.shutdownRetryExit),
            ),
          ],
        ),
      ),
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
