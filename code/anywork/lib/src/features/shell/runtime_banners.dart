part of 'studio_shell.dart';

class _StudioFatalError extends ConsumerWidget {
  const _StudioFatalError({required this.error});

  final Object error;

  @override
  Widget build(BuildContext context, WidgetRef ref) => Scaffold(
    body: Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560),
        child: Padding(
          padding: const EdgeInsets.all(32),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                Icons.error_outline,
                size: 44,
                color: Theme.of(context).colorScheme.error,
              ),
              const SizedBox(height: 16),
              Text(context.l10n.runtimeFatalTitle),
              const SizedBox(height: 10),
              SelectableText(error.toString(), textAlign: TextAlign.center),
              const SizedBox(height: 20),
              FilledButton.icon(
                key: const ValueKey('runtime-fatal-retry'),
                onPressed: () => ref
                    .read(studioControllerProvider.notifier)
                    .retryInitialization(),
                icon: const Icon(Icons.refresh),
                label: Text(context.l10n.runtimeFatalRetry),
              ),
            ],
          ),
        ),
      ),
    ),
  );
}

class _ApplicationRecoveryBanner extends StatefulWidget {
  const _ApplicationRecoveryBanner({required this.issues});

  final List<StudioRecoveryIssue> issues;

  @override
  State<_ApplicationRecoveryBanner> createState() =>
      _ApplicationRecoveryBannerState();
}

class _ApplicationRecoveryBannerState
    extends State<_ApplicationRecoveryBanner> {
  bool _archiveDismissed = false;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final archiveIssues = widget.issues
        .where((issue) => issue.id == 'fresh-start-archive')
        .toList();
    final otherIssues = widget.issues
        .where((issue) => issue.id != 'fresh-start-archive')
        .toList();
    final archived = _archiveDismissed
        ? const <StudioRecoveryIssue>[]
        : archiveIssues;
    if (archived.isEmpty && otherIssues.isEmpty) {
      return const SizedBox.shrink();
    }
    return Tooltip(
      message: [
        ...archived,
        ...otherIssues,
      ].map((issue) => issue.detail).join('\n'),
      child: ColoredBox(
        color: colors.errorContainer,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 7),
          child: Row(
            children: [
              Icon(Icons.warning_amber_rounded, size: 18, color: colors.error),
              const SizedBox(width: 8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    for (final issue in archived) ...[
                      Text(context.l10n.recoveryArchiveNotice),
                      SelectableText(issue.detail),
                    ],
                    if (otherIssues.isNotEmpty)
                      Text(
                        context.l10n.recoveryGlobalWarning(otherIssues.length),
                      ),
                  ],
                ),
              ),
              if (archived.isNotEmpty)
                IconButton(
                  key: const ValueKey('recovery-archive-dismiss'),
                  tooltip: context.l10n.recoveryArchiveDismiss,
                  onPressed: () => setState(() => _archiveDismissed = true),
                  icon: const Icon(Icons.close),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 持久化状态与队列压力面板：始终挂载、自行决定是否可见。
///
/// 状态消息来自产品事件流里的 canonical 快照（保存进度、错误、落后量）；队列压力
/// （排队操作数/字节、在途字节、最老待保存年龄、压力暂停、最近错误、逐 Thread 水位）
/// 经 `readPersistenceQueue` 按需从持久化协调器读取。不可观测时显示为“未知”，
/// 而不是编造本地零值；每次 persistence 修订至多刷新一次，避免无界轮询。
class _PersistenceStatusPanel extends ConsumerStatefulWidget {
  const _PersistenceStatusPanel();

  @override
  ConsumerState<_PersistenceStatusPanel> createState() =>
      _PersistenceStatusPanelState();
}

class _PersistenceStatusPanelState
    extends ConsumerState<_PersistenceStatusPanel> {
  /// 面板在一次 persistence 修订内最多展示的逐 Thread 水位条数，保持块有界。
  static const _maxThreadRows = 3;

  bool _retrying = false;
  bool _loadingQueue = false;
  bool _queueSupported = true;
  PersistenceQueueSnapshot? _queue;
  Timer? _historyStatusTimer;
  final Set<String> _retryingThreads = {};

  @override
  void initState() {
    super.initState();
    unawaited(_refreshQueue());
    _historyStatusTimer = Timer.periodic(
      const Duration(seconds: 3),
      (_) => unawaited(_refreshQueue()),
    );
  }

  @override
  void dispose() {
    _historyStatusTimer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    // 保存修订推进（错误出现、恢复、落后量变化）后刷新一次队列压力。
    ref.listen(shellChromeProvider, (previous, next) {
      final previousRevision = previous?.value?.persistenceState.revision;
      final nextRevision = next.value?.persistenceState.revision;
      if (nextRevision != null && nextRevision != previousRevision) {
        unawaited(_refreshQueue());
      }
    });
    final snapshot =
        ref.watch(shellChromeProvider).value?.persistenceState ??
        const PersistenceStateSnapshot.ready();
    final state = snapshot.state;
    final attention = state.needsAttention;
    final queue = _queue;
    final historyFault =
        queue?.threads.any((thread) => thread.fault != null) ?? false;
    if (!attention &&
        !historyFault &&
        queue?.pressurePaused != true &&
        queue?.statisticsGap != true) {
      return const SizedBox.shrink();
    }
    final colors = Theme.of(context).colorScheme;
    final message = switch (state) {
      DegradedPersistenceState(:final pendingCommits) =>
        context.l10n.persistenceDegraded(pendingCommits),
      RecoveringPersistenceState(:final pendingCommits) =>
        context.l10n.persistenceRecovering(pendingCommits),
      BlockedPersistenceState(:final pendingCommits) =>
        context.l10n.persistenceBlocked(pendingCommits),
      ReadyPersistenceState() || FlushingPersistenceState() => null,
    };
    return ColoredBox(
      key: const ValueKey('persistence-state-banner'),
      color: attention || historyFault
          ? colors.errorContainer
          : colors.surfaceContainer,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 5),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(
                  attention || historyFault
                      ? Icons.save_outlined
                      : Icons.save_as_outlined,
                  size: 18,
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    message ??
                        (historyFault
                            ? context.l10n.persistenceHistoryPaused
                            : context.l10n.persistenceQueueTitle),
                  ),
                ),
                if (attention)
                  TextButton.icon(
                    key: const ValueKey('persistence-retry'),
                    onPressed: _retrying ? null : _retry,
                    icon: const Icon(Icons.refresh, size: 17),
                    label: Text(context.l10n.persistenceRetry),
                  )
                else
                  IconButton(
                    key: StudioDriverKeys.persistenceQueueRefresh,
                    tooltip: context.l10n.persistenceQueueRefresh,
                    onPressed: _loadingQueue ? null : _refreshQueue,
                    icon: const Icon(Icons.refresh, size: 17),
                  ),
              ],
            ),
            _queueDiagnostics(context),
          ],
        ),
      ),
    );
  }

  Widget _queueDiagnostics(BuildContext context) {
    if (!_queueSupported) {
      return Padding(
        padding: const EdgeInsets.only(left: 26, bottom: 4),
        child: Text(
          context.l10n.persistenceQueueUnavailable,
          style: Theme.of(context).textTheme.labelSmall
              ?.copyWith(color: context.colors.onSurfaceVariant),
        ),
      );
    }
    final queue = _queue;
    if (queue == null) {
      return const SizedBox.shrink();
    }
    final colors = Theme.of(context).colorScheme;
    final lines = <String>[
      context.l10n.persistenceQueuePending(
        queue.pendingOperations,
        queue.pendingBytes,
      ),
      context.l10n.persistenceQueueInFlight(queue.inFlightBytes),
      if (queue.oldestPendingAgeMillis case final age?)
        context.l10n.persistenceQueueOldestAge(age),
      if (queue.pressurePaused) context.l10n.persistenceQueuePressurePaused,
      if (queue.statisticsGap) context.l10n.persistenceStatisticsGap,
      if (queue.lastError case final error?)
        context.l10n.persistenceQueueError(error),
    ];
    return Padding(
      key: StudioDriverKeys.persistenceQueueDiagnostics,
      padding: const EdgeInsets.only(left: 26, top: 2, bottom: 4),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          for (final line in lines)
            Text(
              line,
              style: Theme.of(context).textTheme.labelSmall
                  ?.copyWith(color: colors.onSurfaceVariant),
            ),
          for (final thread in [
            ...queue.threads.where((thread) => thread.fault != null),
            ...queue.threads.where((thread) => thread.fault == null),
          ].take(_maxThreadRows))
            Row(
              children: [
                Expanded(
                  child: Text(
                    '${_persistenceThreadLine(thread)}${thread.fault == null ? '' : ' · ${thread.fault == 'queueFull' ? context.l10n.persistenceHistoryQueueFull : context.l10n.persistenceHistoryWriteFailed}'}',
                    style: Theme.of(context).textTheme.labelSmall
                        ?.copyWith(color: colors.onSurfaceVariant),
                  ),
                ),
                if (thread.fault != null)
                  TextButton.icon(
                    key: ValueKey('history-retry-${thread.threadId}'),
                    onPressed: _retryingThreads.contains(thread.threadId)
                        ? null
                        : () => unawaited(_retryThread(thread)),
                    icon: const Icon(Icons.refresh, size: 16),
                    label: Text(context.l10n.persistenceHistoryRetry),
                  ),
              ],
            ),
          if (queue.threads.length > _maxThreadRows)
            Text(
              '+${queue.threads.length - _maxThreadRows}',
              style: Theme.of(context).textTheme.labelSmall
                  ?.copyWith(color: colors.onSurfaceVariant),
            ),
        ],
      ),
    );
  }

  Future<void> _refreshQueue() async {
    if (_loadingQueue) return;
    _loadingQueue = true;
    try {
      final queue = await ref
          .read(studioControllerProvider.notifier)
          .readPersistenceQueue();
      if (!mounted) return;
      setState(() {
        _queueSupported = queue != null;
        _queue = queue;
      });
    } on Object {
      if (!mounted) return;
      // 读取失败是“未知”，保留上一次观测，不把它当成零。
      setState(() => _queueSupported = false);
    } finally {
      _loadingQueue = false;
    }
  }

  Future<void> _retry() async {
    setState(() => _retrying = true);
    try {
      await ref.read(studioControllerProvider.notifier).retryPersistence();
      await _refreshQueue();
    } finally {
      if (mounted) setState(() => _retrying = false);
    }
  }

  Future<void> _retryThread(ThreadPersistenceSnapshot thread) async {
    setState(() => _retryingThreads.add(thread.threadId));
    try {
      final queue = await ref
          .read(studioControllerProvider.notifier)
          .retryThreadHistory(thread.threadId, thread.faultGeneration);
      if (mounted) setState(() => _queue = queue);
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('$error')));
        await _refreshQueue();
      }
    } finally {
      if (mounted) setState(() => _retryingThreads.remove(thread.threadId));
    }
  }
}

/// 持久化水位的诊断显示：`null` 是未知（该 Thread 没有上报水位的 writer），不能折叠成 0。
String _persistenceWatermark(int? durable, int? admitted) {
  return '${durable ?? '?'}/${admitted ?? '?'}';
}

/// 一行逐 Thread 持久化诊断：state/history/calls 各自按“未知 vs 已观测”区分显示。
String _persistenceThreadLine(ThreadPersistenceSnapshot thread) {
  final state = _persistenceWatermark(
    thread.stateDurableRevision,
    thread.stateDirtyRevision,
  );
  final history = _persistenceWatermark(
    thread.historyDurableSequence,
    thread.historyAdmittedSequence,
  );
  final calls = _persistenceWatermark(
    thread.callsDurableSequence,
    thread.callsAdmittedSequence,
  );
  return '${thread.threadId} · state $state · history $history · calls $calls';
}
