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

  /// 展开后的诊断明细高度上界。
  ///
  /// 默认折叠时整条只有一行原因 + 单个动作；展开的多行明细（seq/threadId/字节/原始错误）
  /// 在这里内部滚动，不会把消息列顶下去（此前默认展开多行约占 176px，且重复两个动作）。
  static const _diagnosticsMaxHeight = 132.0;

  bool _retrying = false;
  bool _loadingQueue = false;
  bool _queueSupported = true;
  PersistenceQueueSnapshot? _queue;
  Timer? _historyStatusTimer;
  final Set<String> _retryingThreads = {};
  final Set<String> _resumingThreads = {};

  /// 展开的诊断明细所属的**存储阻塞身份**（会话 + 故障代数 + 阶段）。
  ///
  /// 与活动条同一套折叠规则：同一身份内的水位/字节更新保持展开；身份变化（换会话、故障
  /// 代数推进、从「执行已暂停」进入「已保存待继续」）即视为收起，主流程回到一行原因。
  String? _expandedDiagnosticsIdentity;

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
    final studio = ref.watch(studioControllerProvider).value;
    final workspaces =
        studio?.workspacesByThread ?? const <String, ThreadWorkspace>{};
    // 会话级阻塞只认 canonical typed storage：是“执行已暂停”还是“已保存待继续”，以及
    // 允许哪个动作，全部取自 [ThreadStorageStateView]，不从全局文案或错误字符串推断。
    // 并且**优先当前选中会话**：其他会话的存储状态只能作为“其他会话”的次要提示，不能
    // 掩盖、也不能冒充当前会话自己的阻塞事实。
    final block = _storageBlock(workspaces, studio?.selectedThreadId);
    final state = snapshot.state;
    final attention = state.needsAttention;
    final queue = _queue;
    final historyFault =
        queue?.threads.any((thread) => thread.fault != null) ?? false;
    // 存储压力（自动恢复的短背压）：暂停的是新推理准入，不是“执行已经失败”。
    final pressurePaused =
        queue?.pressurePaused == true ||
        workspaces.values.any(
          (workspace) => workspace.storage?.pressurePaused == true,
        );
    final statisticsGap = queue?.statisticsGap == true;
    if (!attention &&
        !historyFault &&
        block == null &&
        !pressurePaused &&
        !statisticsGap) {
      return const SizedBox.shrink();
    }
    final colors = Theme.of(context).colorScheme;
    // 执行被暂停（硬故障闩住准入/存储暂停执行）用错误色；已保存待继续与短背压是中性块。
    final paused = block != null && !block.ready;
    // 强调色只在“确实需要处理”时使用：有会话级事实时以它为准（`ready` 不能因为 core 残留的
    // 上次错误或进程级 attention 继续显示成错误）；没有会话级事实时才沿用全局判断。
    final errorTone = paused || (block == null && (attention || historyFault));
    final message = _summaryMessage(
      context,
      block: block,
      state: state,
      historyFault: historyFault,
      pressurePaused: pressurePaused,
    );
    final identity = _diagnosticsIdentity(block);
    final expanded = _expandedDiagnosticsIdentity == identity;
    return ColoredBox(
      key: const ValueKey('persistence-state-banner'),
      color: errorTone ? colors.errorContainer : colors.surfaceContainer,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 5),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(
                  errorTone ? Icons.save_outlined : Icons.save_as_outlined,
                  size: 18,
                ),
                const SizedBox(width: 8),
                Expanded(
                  // 主流程只有一行原因：窄窗口只做视觉省略，完整原因在悬浮提示里。
                  child: Tooltip(
                    message: message,
                    child: Text(
                      message,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      softWrap: false,
                    ),
                  ),
                ),
                IconButton(
                  key: const ValueKey('persistence-diagnostics-toggle'),
                  tooltip: expanded
                      ? context.l10n.persistenceQueueHideDetails
                      : context.l10n.persistenceQueueShowDetails,
                  onPressed: () => setState(() {
                    _expandedDiagnosticsIdentity = expanded ? null : identity;
                  }),
                  iconSize: 18,
                  padding: EdgeInsets.zero,
                  constraints: const BoxConstraints.tightFor(
                    width: 32,
                    height: 32,
                  ),
                  visualDensity: VisualDensity.compact,
                  icon: Icon(expanded ? Icons.expand_less : Icons.expand_more),
                ),
                ..._summaryAction(context, block: block, attention: attention),
              ],
            ),
            if (expanded)
              _diagnostics(
                context,
                workspaces,
                primaryThreadId: block?.threadId,
              ),
          ],
        ),
      ),
    );
  }

  /// 折叠在「展开诊断」之后的明细：水位序号、逐 Thread 原始 id、字节数、原始错误。
  ///
  /// 只有用户主动展开时才渲染，整块高度有界并在内部滚动，因此不会占据默认主流程，也不会
  /// 把消息列顶下去。逐 Thread 的动作**不**为主流程已经给过动作的那个会话重复按钮。
  Widget _diagnostics(
    BuildContext context,
    Map<String, ThreadWorkspace> workspaces, {
    required String? primaryThreadId,
  }) {
    final colors = Theme.of(context).colorScheme;
    final queue = _queue;
    final children = <Widget>[];
    if (!_queueSupported) {
      children.add(
        _diagnosticText(context, context.l10n.persistenceQueueUnavailable),
      );
    } else if (queue == null) {
      children.addAll(
        _resumeRows(context, workspaces, primaryThreadId: primaryThreadId),
      );
    } else {
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
      children.addAll([
        for (final line in lines) _diagnosticText(context, line),
      ]);
      for (final thread in [
        ...queue.threads.where((thread) => thread.fault != null),
        ...queue.threads.where((thread) => thread.fault == null),
      ].take(_maxThreadRows)) {
        children.add(
          Row(
            children: [
              Expanded(
                child: Text(
                  '${_persistenceThreadLine(thread)}${thread.fault == null ? '' : ' · ${thread.fault == 'queueFull' ? context.l10n.persistenceHistoryQueueFull : context.l10n.persistenceHistoryWriteFailed}'}',
                  style: Theme.of(context).textTheme.labelSmall
                      ?.copyWith(color: colors.onSurfaceVariant),
                ),
              ),
              if (thread.fault != null && thread.threadId != primaryThreadId)
                TextButton.icon(
                  key: ValueKey('history-retry-${thread.threadId}'),
                  onPressed: _retryingThreads.contains(thread.threadId)
                      ? null
                      : () => unawaited(
                          _retryStorage(
                            thread.threadId,
                            thread.faultGeneration,
                          ),
                        ),
                  icon: const Icon(Icons.refresh, size: 16),
                  label: Text(context.l10n.persistenceHistoryRetrySave),
                ),
            ],
          ),
        );
      }
      if (queue.threads.length > _maxThreadRows) {
        children.add(
          _diagnosticText(context, '+${queue.threads.length - _maxThreadRows}'),
        );
      }
      children.addAll(
        _resumeRows(context, workspaces, primaryThreadId: primaryThreadId),
      );
    }
    return Padding(
      key: StudioDriverKeys.persistenceQueueDiagnostics,
      padding: const EdgeInsets.only(left: 26, top: 2, bottom: 4),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: _diagnosticsMaxHeight),
        child: SingleChildScrollView(
          primary: false,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: children,
          ),
        ),
      ),
    );
  }

  Widget _diagnosticText(BuildContext context, String line) {
    return Text(
      line,
      style: Theme.of(context).textTheme.labelSmall
          ?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant),
    );
  }

  /// 显式继续入口：事实源是后端 typed 存储状态（`resumeRequired`/`canResume`），与重试保存分开。
  ///
  /// 只对仍被硬故障闩住（`resumeRequired`）的 Thread 显示；按钮**只有**后端 typed
  /// `canResume` 为真（重试保存已按同一代数确认、fence 核验通过）时才启用，重试保存
  /// 进行中或本 Thread 正在继续时禁用。不在前端从错误文本推断、也不因点击重试而本地置真。
  ///
  /// [primaryThreadId] 是主流程那一行已经在表达的会话：它的动作已经在那里给出，这里只保留
  /// 原因文本，避免同一个会话出现两个「重试保存/继续执行」入口。
  List<Widget> _resumeRows(
    BuildContext context,
    Map<String, ThreadWorkspace> workspaces, {
    required String? primaryThreadId,
  }) {
    final colors = Theme.of(context).colorScheme;
    final rows = <Widget>[];
    for (final entry in workspaces.entries) {
      final storage = entry.value.storage;
      if (storage == null || !storage.resumeRequired) continue;
      final title = entry.value.thread.title.trim();
      final label = title.isEmpty ? entry.key : title;
      final busy =
          _retryingThreads.contains(entry.key) ||
          _resumingThreads.contains(entry.key);
      rows.add(
        Row(
          children: [
            Expanded(
              child: Text(
                '$label · ${_storageReasonLabel(context, storage)}',
                style: Theme.of(context).textTheme.labelSmall
                    ?.copyWith(color: colors.onSurfaceVariant),
              ),
            ),
            if (entry.key != primaryThreadId)
              TextButton.icon(
                key: ValueKey('history-resume-${entry.key}'),
                onPressed: storage.canResume && !busy
                    ? () => unawaited(
                        _resumeThread(entry.key, storage.faultGeneration),
                      )
                    : null,
                icon: const Icon(Icons.play_arrow, size: 16),
                label: Text(context.l10n.persistenceHistoryResume),
              ),
          ],
        ),
      );
    }
    return rows;
  }

  /// 主流程要表达的那一条**会话级**存储阻塞；全部取自 canonical typed storage 状态。
  ///
  /// 先看**当前选中会话**：它自己「已保存待继续 / 执行已暂停 / 新工作收束」的事实必须在主
  /// 流程第一行，不能被其他会话的状态掩盖。只有当前选中会话没有存储阻塞时，才退到其他会话，
  /// 并带上可辨认的来源名字——此时摘要必须说清是**其他会话**，绝不暗示当前会话保存失败或可以继续。
  _StorageBlock? _storageBlock(
    Map<String, ThreadWorkspace> workspaces,
    String? selectedThreadId,
  ) {
    if (selectedThreadId != null) {
      final selected = workspaces[selectedThreadId];
      final storage = selected?.storage;
      if (selected != null && storage != null) {
        final stage = _storageStage(storage);
        if (stage != _StorageStage.none) {
          return _StorageBlock(
            threadId: selectedThreadId,
            storage: storage,
            stage: stage,
            isSelected: true,
            label: _workspaceLabel(selected, selectedThreadId),
          );
        }
      }
    }
    _StorageBlock? ready;
    _StorageBlock? paused;
    for (final entry in workspaces.entries) {
      if (entry.key == selectedThreadId) continue;
      final storage = entry.value.storage;
      if (storage == null) continue;
      final stage = _storageStage(storage);
      if (stage == _StorageStage.none) continue;
      final block = _StorageBlock(
        threadId: entry.key,
        storage: storage,
        stage: stage,
        isSelected: false,
        label: _workspaceLabel(entry.value, entry.key),
      );
      if (stage == _StorageStage.ready) {
        ready ??= block;
      } else {
        paused ??= block;
      }
    }
    return ready ?? paused;
  }

  /// 会话可辨认的显示名：标题优先，缺失时退回 threadId，避免诊断指向匿名会话。
  String _workspaceLabel(ThreadWorkspace workspace, String threadId) {
    final title = workspace.thread.title.trim();
    return title.isEmpty ? threadId : title;
  }

  /// 由 canonical typed 存储事实判定会话级阶段；不解析错误文本、不用前端计时猜测。
  ///
  /// `hasFault` 可能在模型/工具仍在途时立即出现，届时新准入已停但执行阶段还没到 `Paused`，
  /// 只能报「新工作已暂停、正在收束」；只有执行阶段真的到 `Paused` 才算「执行已暂停」。
  /// 没有 typed 故障的短暂压力/背压（`pressurePaused`）不算会话级阻塞，由全局压力文案表达。
  _StorageStage _storageStage(ThreadStorageStateView storage) {
    // 已恢复、水位核验通过，只等一次显式继续。
    if (storage.resumeRequired && storage.canResume) {
      return _StorageStage.ready;
    }
    final faulting = storage.hasFault || storage.resumeRequired;
    if (!faulting) return _StorageStage.none;
    return storage.execution == ThreadStorageExecution.paused
        ? _StorageStage.executionPaused
        : _StorageStage.windingDown;
  }

  /// 主流程那一行的原因：先看会话级 typed 存储事实，再看进程级 persistence 状态。
  ///
  /// 执行被暂停时**不**沿用全局「保存已阻塞……可以继续会话」那种笼统文案：会话级的
  /// `resumeRequired`/故障/暂停执行才是真事实，`canResume` 为真时说明保存已恢复、只等一次
  /// 显式继续。于是“可以继续会话”只会出现在确实不阻塞的 degraded/recovering 上：会话级
  /// 暂停、存储压力暂停准入与 blocked 文案都不提继续。
  ///
  /// 当阻塞来自**其他会话**（当前选中会话本身没有存储阻塞）时，文案必须点名来源会话，
  /// 说清是“另一个会话”，而不是让用户以为当前会话保存失败或已经可以继续。
  String _summaryMessage(
    BuildContext context, {
    required _StorageBlock? block,
    required PersistenceState state,
    required bool historyFault,
    required bool pressurePaused,
  }) {
    final l10n = context.l10n;
    if (block != null) {
      // 仅当阻塞不是当前选中会话时才带上来源名字。
      final other = block.isSelected ? null : block.label;
      return switch (block.stage) {
        // 已保存、水位核验通过，只等一次显式继续。
        _StorageStage.ready =>
          other == null
              ? l10n.conversationActivityResumeReady
              : l10n.persistenceResumeReadyOtherSession(other),
        // 执行阶段确实到 Paused：执行已暂停（尚未恢复，不能继续）。
        _StorageStage.executionPaused =>
          other == null
              ? l10n.persistenceExecutionPaused
              : l10n.persistenceExecutionPausedOtherSession(other),
        // 故障初现/在途收束：新工作已暂停，但还不能宣称“执行已暂停”。
        _StorageStage.windingDown =>
          other == null
              ? l10n.persistenceNewWorkPaused
              : l10n.persistenceNewWorkPausedOtherSession(other),
        // 不存在 none 的 block；保持穷尽 switch。
        _StorageStage.none => l10n.persistenceQueueTitle,
      };
    }
    // 存储压力暂停了新准入：只说“正在等待保存”，不承诺可以继续会话。
    if (pressurePaused) return l10n.persistenceSavingBackpressure;
    return switch (state) {
      DegradedPersistenceState(:final pendingCommits) =>
        l10n.persistenceDegraded(pendingCommits),
      RecoveringPersistenceState(:final pendingCommits) =>
        l10n.persistenceRecovering(pendingCommits),
      BlockedPersistenceState(:final pendingCommits) => l10n.persistenceBlocked(
        pendingCommits,
      ),
      ReadyPersistenceState() || FlushingPersistenceState() =>
        historyFault
            ? l10n.persistenceHistoryPaused
            : l10n.persistenceQueueTitle,
    };
  }

  /// 主流程的**单个**正确动作：按会话级 typed 状态决定阶段，两个阶段各有自己的 typed 闸门。
  ///
  /// 已保存待继续 → 「继续执行」，只有后端核验的 `canResume` 才可点击，重试保存不代替继续；
  /// 执行已暂停 → 「重试保存」，成功后后端才核验 `canResume`。没有会话级阻塞时才回落到
  /// 进程级重试或诊断刷新。返回列表是为了让调用处直接展开到 Row 里。
  List<Widget> _summaryAction(
    BuildContext context, {
    required _StorageBlock? block,
    required bool attention,
  }) {
    if (block != null) {
      final storage = block.storage;
      final busy =
          _retryingThreads.contains(block.threadId) ||
          _resumingThreads.contains(block.threadId);
      if (block.ready) {
        return [
          TextButton.icon(
            key: ValueKey('history-resume-${block.threadId}'),
            onPressed: storage.canResume && !busy
                ? () => unawaited(
                    _resumeThread(block.threadId, storage.faultGeneration),
                  )
                : null,
            icon: const Icon(Icons.play_arrow, size: 16),
            label: Text(context.l10n.persistenceHistoryResume),
          ),
        ];
      }
      return [
        TextButton.icon(
          key: ValueKey('history-retry-${block.threadId}'),
          onPressed: busy
              ? null
              : () => unawaited(
                  _retryStorage(block.threadId, storage.faultGeneration),
                ),
          icon: const Icon(Icons.refresh, size: 17),
          label: Text(context.l10n.persistenceHistoryRetrySave),
        ),
      ];
    }
    if (attention) {
      return [
        TextButton.icon(
          key: const ValueKey('persistence-retry'),
          onPressed: _retrying ? null : _retry,
          icon: const Icon(Icons.refresh, size: 17),
          label: Text(context.l10n.persistenceRetry),
        ),
      ];
    }
    return [
      IconButton(
        key: StudioDriverKeys.persistenceQueueRefresh,
        tooltip: context.l10n.persistenceQueueRefresh,
        onPressed: _loadingQueue ? null : _refreshQueue,
        icon: const Icon(Icons.refresh, size: 17),
      ),
    ];
  }

  /// 诊断明细的折叠身份：会话级阻塞按「会话 + 故障代数 + 阶段」，其余按进程级队列。
  String _diagnosticsIdentity(_StorageBlock? block) {
    if (block == null) return 'queue';
    final scope = block.isSelected ? 'selected' : 'other';
    return 'storage:$scope:${block.threadId}:${block.storage.faultGeneration}:'
        '${block.stage.name}';
  }

  /// 存储状态的人类可读原因：先区分后端核验的“已恢复待继续”，再按 typed 故障类别、
  /// 压力背压，最后落到“等待保存恢复”。只依据 typed `canResume`/`fault`，不解析错误文本。
  String _storageReasonLabel(
    BuildContext context,
    ThreadStorageStateView storage,
  ) {
    // 已核验可继续：保存已恢复、水位核验通过。core 故障闩在显式继续前可能仍保留上次
    // 错误文本，这里显示“已保存，等待继续执行”，不把历史错误当作正在失败。
    if (storage.resumeRequired && storage.canResume) {
      return context.l10n.conversationActivityResumeReady;
    }
    if (storage.hasFault) {
      return switch (storage.fault) {
        ThreadHistoryFault.queueFull =>
          context.l10n.persistenceHistoryQueueFull,
        ThreadHistoryFault.writeFailed =>
          context.l10n.persistenceHistoryWriteFailed,
        _ => context.l10n.persistenceHistoryPaused,
      };
    }
    if (storage.pressurePaused) {
      return context.l10n.conversationActivityStoragePressure;
    }
    // 仍被硬故障闩住、后端尚未核验可继续：等待保存恢复（重试保存成功并核验后才可继续）。
    return context.l10n.persistenceHistoryResumeHint;
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

  /// 会话级「重试保存」：代数取自 canonical typed 存储状态，与显式继续是同一个世代。
  ///
  /// 成功只说明这次重试被接受；能否继续仍由后端核验的 `canResume` 决定，前端不在本地置真。
  Future<void> _retryStorage(String threadId, int faultGeneration) async {
    setState(() => _retryingThreads.add(threadId));
    try {
      final queue = await ref
          .read(studioControllerProvider.notifier)
          .retryThreadHistory(threadId, faultGeneration);
      if (mounted) setState(() => _queue = queue);
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('$error')));
        await _refreshQueue();
      }
    } finally {
      if (mounted) setState(() => _retryingThreads.remove(threadId));
    }
  }

  /// 显式继续执行：严格区别于重试保存。
  ///
  /// 代数来自后端 typed 存储状态（不是从错误文本推断）；后端只在保存重试已按同一代数
  /// 确认、且后端 canonical 状态允许时才解除准入闩。拒绝时后端返回错误，界面按返回值与
  /// typed 状态重试，**不**本地假定已恢复、**不**自动继续。
  Future<void> _resumeThread(String threadId, int faultGeneration) async {
    setState(() => _resumingThreads.add(threadId));
    try {
      await ref
          .read(studioControllerProvider.notifier)
          .resumeThreadHistory(threadId, faultGeneration);
    } on Object catch (error) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('$error')));
      }
    } finally {
      if (mounted) {
        setState(() => _resumingThreads.remove(threadId));
        await _refreshQueue();
      }
    }
  }
}

/// 持久化水位的诊断显示：`null` 是未知（该 Thread 没有上报水位的 writer），不能折叠成 0。
String _persistenceWatermark(int? durable, int? admitted) {
  return '${durable ?? '?'}/${admitted ?? '?'}';
}

/// 主流程要表达的那一条会话级存储阻塞。
///
/// 全部字段来自 canonical typed 存储状态（[ThreadStorageStateView]）：原因与阶段不解析错误
/// 文本，动作闸门（`canResume` / 故障代数）也取自同一份事实。
class _StorageBlock {
  const _StorageBlock({
    required this.threadId,
    required this.storage,
    required this.stage,
    required this.isSelected,
    required this.label,
  });

  final String threadId;
  final ThreadStorageStateView storage;

  /// 该会话在界面上的阶段（[ready]/[windingDown]/[executionPaused]）。
  final _StorageStage stage;

  /// 是否就是当前选中会话；否则摘要必须点名来源，避免暗示当前会话出了问题。
  final bool isSelected;

  /// 该会话可辨认的显示名（标题优先，缺失退回 threadId）。
  final String label;

  /// `resumeRequired && canResume`：保存已恢复、水位核验通过，只等一次显式继续。
  bool get ready => stage == _StorageStage.ready;
}

/// 会话级存储阻塞在界面上的阶段；全部由 typed 事实派生，不解析错误文本、不用前端计时。
enum _StorageStage {
  /// `resumeRequired && canResume`：保存已恢复、水位核验通过，只等一次显式继续。
  ready,

  /// 故障初现/在途收束：新准入已停止，但执行阶段还没到 `Paused`，不能宣称“执行已暂停”。
  windingDown,

  /// 执行阶段确实到 `Paused`：执行已暂停，需先重试保存、再显式继续。
  executionPaused,

  /// 不构成本面板要表达的会话级阻塞。
  none,
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
