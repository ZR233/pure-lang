part of 'studio_api.dart';

frb_attachment_types.BridgeStudioPromptInput _bridgePromptInput(
  StudioPromptInput input,
) {
  return frb_attachment_types.BridgeStudioPromptInput(
    inputId: input.inputId,
    text: input.text,
    attachmentDraftIds: input.attachmentDraftIds,
  );
}

AttachmentDraftView _attachmentDraftFromFrb(
  frb_attachment_types.BridgeAttachmentDraft value,
) {
  return AttachmentDraftView(
    id: value.draftId,
    modality: switch (value.modality) {
      frb_attachment_types.BridgeAttachmentModality.image =>
        AttachmentModalityView.image,
      frb_attachment_types.BridgeAttachmentModality.video =>
        AttachmentModalityView.video,
      frb_attachment_types.BridgeAttachmentModality.file =>
        AttachmentModalityView.file,
    },
    mediaType: value.mediaType,
    filename: value.filename,
    byteSize: value.byteSize.toInt(),
    width: value.width,
    height: value.height,
  );
}

/// Dart 侧一次 FRB 订阅的取消句柄（native 句柄 + 派生 Dart stream 的单一 owner）。
///
/// 取消只启动一条 owned future：normal `stream.onCancel` 与关闭协调器复用同一 future，
/// 绝不重复调用 native `cancel()` / `dispose()`。创建尚未完成时 owned future 会等待
/// [markCreated]，因此 `await createBridgeSubscription` 期间发生的关闭会把迟到的句柄交给
/// registry 持有并等 cancel 完成，而不是误报 settled。
///
/// 只有 owned future 无异常完成才标记 `drained` 并从 registry 删除；超时或异常保留
/// pending owner，safeDispose 不会与仍活动的 bridge 回调并发。具体错误与堆栈仅供脱敏
/// 诊断，报告侧一律使用固定安全文案。
class _DartSubscription {
  _DartSubscription(
    this.stage,
    this._release,
    this._onDrained, {
    this.externallyCoordinated = false,
  });

  final String stage;
  final Future<void> Function() _release;
  final void Function(_DartSubscription subscription) _onDrained;

  /// 由关闭协调器在自身预算内单独 join / 回报取消（如关机进度流）：进程级 registry 仍
  /// 强持有它直到真正 ACK（阻止 GC / safeDispose 并发），但通用 `cancelDartSubscriptions`
  /// 汇总不重复 cancel / settle，避免两份取消事实与重复 issue。
  final bool externallyCoordinated;

  final Completer<void> _created = Completer<void>();
  Future<void>? _cancellation;
  bool _cancelRequested = false;
  bool _drained = false;
  Object? _error;
  StackTrace? _stackTrace;
  // create 本身失败（尚未产出 native 句柄）时的真实原因与堆栈：owner 保留它，并在取消
  // 收束时如实作为取消 ACK failure 上报，绝不把真实创建异常静默成 Clean。
  Object? _createFailure;
  StackTrace? _createFailureStack;

  /// 是否已请求取消（`onCancel` 或关闭协调器），同步可见——`start()` 据此在 create
  /// 之前/之后放弃发布，绝不向已取消的 controller 派发事件。
  bool get cancelRequested => _cancelRequested;

  /// 记录 create 决策过程中的真实失败（原始 error/stack，调用方只放脱敏诊断字段）。
  void recordCreateFailure(Object error, StackTrace stackTrace) {
    _createFailure ??= error;
    _createFailureStack ??= stackTrace;
  }

  /// `start()` 完成创建决策后调用：native 句柄（若建立）已由 [release] 闭包捕获，迟到
  /// 句柄同样走这条释放路径。若取消已请求则立即启动 owned cancellation。
  void markCreated() {
    if (!_created.isCompleted) _created.complete();
    if (_cancelRequested) unawaited(_ownedCancellation());
  }

  /// 启动（或复用）唯一一条 owned cancellation future。
  Future<void> _ownedCancellation() {
    final existing = _cancellation;
    if (existing != null) return existing;
    _cancelRequested = true;
    final started = () async {
      // 先等创建决策再释放：迟到句柄被 registry 持有并等 cancel 完成，绝不绕过闩。
      await _created.future;
      await _release();
      // create 自身失败：owner 没有真正的 native 资源，仍如实把原始失败作为取消 ACK
      // failure 上报（wire 文案固定安全）；绝不把真实创建异常静默成 Clean。
      final createFailure = _createFailure;
      if (createFailure != null) {
        Error.throwWithStackTrace(
          createFailure,
          _createFailureStack ?? StackTrace.current,
        );
      }
    }();
    _cancellation = started;
    unawaited(
      started.then<void>(
        (_) {
          _drained = true;
          _onDrained(this);
        },
        onError: (Object error, StackTrace stackTrace) {
          // 保留 pending owner，但保留具体（脱敏）错因与堆栈供同步诊断。
          _error ??= error;
          _stackTrace ??= stackTrace;
        },
      ),
    );
    return started;
  }

  Future<void> beginCancel() {
    if (_drained) return Future<void>.value();
    return _ownedCancellation();
  }

  /// 有界等待 owned cancellation 收敛；返回是否排空与真实（脱敏）错误、堆栈。
  Future<_DartCancelResult> settle(Duration bound) async {
    if (_drained) return const _DartCancelResult(drained: true);
    final started = _ownedCancellation();
    try {
      await started.timeout(
        bound <= Duration.zero ? const Duration(milliseconds: 1) : bound,
      );
      _drained = true;
      _onDrained(this);
      return const _DartCancelResult(drained: true);
    } on Object catch (error, stackTrace) {
      _error ??= error;
      _stackTrace ??= stackTrace;
      return _DartCancelResult(
        drained: _drained,
        error: _error,
        stackTrace: _stackTrace,
      );
    }
  }
}

/// 一次 Dart 侧订阅取消的有界结果：是否真正排空 + 具体（脱敏）错误与堆栈。
class _DartCancelResult {
  const _DartCancelResult({required this.drained, this.error, this.stackTrace});

  final bool drained;
  final Object? error;
  final StackTrace? stackTrace;
}

/// 一次早期退出封闭（[StudioTransport.beginRuntimeExit]）的单一 one-shot 结果。
///
/// 成功或「无 owner 跳过」时无 issue；失败/超时保留 typed issue、原始错误与堆栈，供调用方
/// 作为 external issue 继续收尾。同一进程只结算一次：重复 close / dispose / driver 复用同一
/// 结果，绝不重复并行封闭，也绝不延长单一期限。
class _BeginExitOutcome {
  const _BeginExitOutcome.success()
    : issue = null,
      error = null,
      stackTrace = null;

  const _BeginExitOutcome.failed({
    required StudioShutdownIssue this.issue,
    required Object this.error,
    required StackTrace this.stackTrace,
  });

  final StudioShutdownIssue? issue;
  final Object? error;
  final StackTrace? stackTrace;
}

frb.ProviderInput _providerInputFromCommand(ProviderCommand provider) {
  return frb.ProviderInput(
    id: provider.id,
    originalId: provider.originalId,
    templateKind: provider.templateKind,
    name: provider.name,
    baseUrl: provider.baseUrl,
    secret: switch (provider.secret.action) {
      ProviderSecretAction.preserve => const frb.ProviderSecretInput.preserve(),
      ProviderSecretAction.replace => frb.ProviderSecretInput.replace(
        value: provider.secret.value!,
      ),
      ProviderSecretAction.clear => const frb.ProviderSecretInput.clear(),
    },
    pricingEnabled: provider.pricingEnabled,
    defaultModel: provider.defaultModel,
    customModels: [
      for (final model in provider.customModels)
        frb.ProviderModelInput(
          slug: model.slug,
          displayName: model.displayName,
          wireProtocol: model.wireProtocol,
          contextWindow: BigInt.from(model.contextWindow),
          maxOutputTokens: BigInt.from(model.maxOutputTokens),
        ),
    ],
    modelConnectionModes: [
      for (final model in provider.modelConnectionModes)
        frb.ProviderModelConnectionInput(
          slug: model.slug,
          connectionMode: model.connectionMode,
        ),
    ],
    modelAutoCompactLimits: [
      for (final limit in provider.modelAutoCompactLimits)
        frb.ProviderModelAutoCompactInput(
          slug: limit.slug,
          limit: BigInt.from(limit.limit),
        ),
    ],
  );
}

class FrbStudioTransport
    implements StudioTransport, PersistenceQueueReader, ChatWindowReader {
  static final startupProgress = ValueNotifier(
    StudioStartupPhase.loadingBridge,
  );
  static final startupRecovery = ValueNotifier<StartupRecoveryReport?>(null);

  @override
  Future<RecoveryStateSnapshot> retryRecovery() async {
    await _ensureReady();
    return _recoveryStateFromFrb(await _bridgeCall(frb.retryRecovery));
  }

  static Future<void>? _initFuture;
  static Future<StudioShutdownReport>? _shutdownFuture;
  // 早期退出封闭的单一 one-shot 结果：首次调用锚定，重复 close / dispose / driver 复用，
  // 绝不重复并行封闭，也绝不延长单一期限。
  static Future<_BeginExitOutcome>? _beginExitOutcome;
  // 早期退出封闭失败时的 memoized 原始错误与 typed issue：协调器据此复用同一 correlation，
  // 避免重复生成 / 重复记录；成功或无 owner 时均为 null。
  static Object? _beginExitFailure;
  static StudioShutdownIssue? _beginExitIssue;
  static Future<void> Function()? _initializationOverrideForTesting;
  static bool _rustInitialized = false;
  // runtime owner 是否已成功安装；无 init 的只读事实，供关闭路径在不触发初始化的
  // 前提下判定「确无 owner」还是「未决」。
  static bool _runtimeReady = false;
  // Dart 侧 FRB 订阅的单一所有权登记：关闭时先封闭创建，再逐个有界取消；未取消
  // 成功的 owner 被保留，因此不会与仍活动的 bridge 回调并发 dispose。
  final List<_DartSubscription> _dartSubscriptions = [];
  // The native library has one process initializer; only its startup observation
  // is process-owned. Product/thread/shutdown handles belong to this transport.
  static final List<_DartSubscription> _startupSubscriptions = [];
  static int _liveDartSubscriptions = 0;
  bool _dartSubscriptionsSealed = false;
  // 只有确认 runtime 从未安装/已 Clean，且初始化已完成时，才在订阅收束后释放 RustLib。
  static bool _safeToDisposeAfterShutdown = false;
  // 单向终止闩：一经置位，本进程余下生命周期内不再启动或发布运行时。
  static bool _closing = false;
  ProviderCatalogView? _providerCatalogCache;
  Future<ProviderCatalogView>? _providerCatalogFuture;

  static Future<void> ensureReady() => _ensureReady();

  /// 无 init 的只读 owner 事实：runtime 是否已安装。
  static bool get runtimeOwnerPresent => _runtimeReady;

  /// 登记一个 Dart 侧 FRB 订阅。封闭后仍**保留登记**并立即启动取消：迟到的 create 句柄
  /// 由 owner 持有并等 cancel 完成，未收束的 owner 继续挡住 safeDispose，绝不误报 settled。
  void _trackDartSubscription(_DartSubscription subscription) {
    _dartSubscriptions.add(subscription);
    _liveDartSubscriptions += 1;
    // 外部协调的 owner（进度流）不在 seal 时抢先 cancel：它按本地显示策略先 create，再由
    // 协调器在预算内取消并取 ACK；registry 在这里只负责强持有（阻止 GC）。
    if (_dartSubscriptionsSealed && !subscription.externallyCoordinated) {
      unawaited(subscription.beginCancel());
    }
  }

  void _forgetDartSubscription(_DartSubscription subscription) {
    if (_dartSubscriptions.remove(subscription)) _liveDartSubscriptions -= 1;
  }

  static void _trackStartupSubscription(_DartSubscription subscription) {
    _startupSubscriptions.add(subscription);
    _liveDartSubscriptions += 1;
    if (_closing) unawaited(subscription.beginCancel());
  }

  static void _forgetStartupSubscription(_DartSubscription subscription) {
    if (_startupSubscriptions.remove(subscription)) _liveDartSubscriptions -= 1;
  }

  /// 封闭创建并**并发**有界取消全部 Dart 侧 FRB 订阅（shutdown-progress 由协调器单独
  /// 收束）。先同时发出所有独立 cancel，任何一个 hang 都不会让其余 entry 拿不到 cancel；
  /// 随后按同一 absolute deadline 有界 join。未在预算内收敛的 owner 被保留并回报真实
  /// issue（含脱敏具体错因），因此 safeDispose 只有在 native 与 Dart 两侧全部收束后才可能。
  @override
  Future<List<StudioShutdownIssue>> cancelDartSubscriptions({
    required int budgetMs,
  }) async {
    _dartSubscriptionsSealed = true;
    // 通用汇总只负责产品/Thread 等自持 owner；外部协调的 owner（进度流）仍留在 registry
    // 里强持有，但其取消/回报由协调器在自身预算内单独 join，避免重复 cancel 与重复 issue。
    final entries = <_DartSubscription>[
      for (final entry in [..._startupSubscriptions, ..._dartSubscriptions])
        if (!entry.externallyCoordinated) entry,
    ];
    for (final entry in entries) {
      unawaited(entry.beginCancel());
    }
    final watch = Stopwatch()..start();
    final remaining = budgetMs - watch.elapsedMilliseconds;
    final bound = Duration(milliseconds: remaining <= 0 ? 1 : remaining);
    final results = await Future.wait([
      for (final entry in entries) entry.settle(bound),
    ]);
    final issues = <StudioShutdownIssue>[];
    for (var index = 0; index < entries.length; index += 1) {
      final entry = entries[index];
      final result = results[index];
      if (result.drained) {
        _forgetDartSubscription(entry);
        _forgetStartupSubscription(entry);
        continue;
      }
      // 同一次失败只生成一次 correlation：report issue 与同步诊断共用同一编号。
      final correlationId = newStudioCorrelationId();
      issues.add(_dartSubscriptionIssue(entry, result, correlationId));
      final error = result.error;
      if (error != null) {
        recordDartError(
          error,
          result.stackTrace,
          stage: 'subscriptions',
          correlationId: correlationId,
          elapsedMs: watch.elapsedMilliseconds,
        );
      }
    }
    return issues;
  }

  static StudioShutdownIssue _dartSubscriptionIssue(
    _DartSubscription entry,
    _DartCancelResult result,
    String correlationId,
  ) {
    return StudioShutdownIssue(
      stage: 'subscriptions',
      code: switch (result.error) {
        final StudioFailure failure => failure.code.name,
        TimeoutException() => 'timeout',
        _ => 'cancelFailed',
      },
      // 固定安全文案：绝不把任意 error.toString()（可能含正文/凭据）当 wire 安全消息。
      message:
          'the ${entry.stage} bridge subscription did not cancel within the '
          'exit budget',
      retryable: true,
      correlationId: correlationId,
    );
  }

  static bool get _dartSubscriptionsSettled => _liveDartSubscriptions == 0;

  /// 协调器在合并 Dart 侧关闭阶段问题之后调用：只要最终报告不是可靠 Clean/NotStarted，
  /// 就撤销安全 dispose，绝不在仍有 Dart 订阅或事件循环占用时释放 RustLib。
  static void revokeSafeDispose() {
    _safeToDisposeAfterShutdown = false;
  }

  static void retryInitialization() {
    // 退出闩不因启动重试复位；进入退出后不再重新初始化。
    if (_closing) return;
    if (startupProgress.value == StudioStartupPhase.failed) {
      _initFuture = null;
    }
  }

  @visibleForTesting
  static void debugOverrideInitialization(
    Future<void> Function()? initialization,
  ) {
    _initFuture = null;
    _shutdownFuture = null;
    _beginExitOutcome = null;
    _beginExitFailure = null;
    _beginExitIssue = null;
    // 仅供隔离测试复位单向闩；生产路径不得复位。
    _closing = false;
    _runtimeReady = false;
    // Test override must only be used before installing live resources.
    if (_liveDartSubscriptions != 0) {
      throw StateError('Native subscriptions remain owned');
    }
    _startupSubscriptions.clear();
    _safeToDisposeAfterShutdown = false;
    _initializationOverrideForTesting = initialization;
  }

  static Future<void> _ensureReady() {
    if (_closing) {
      return Future<void>.error(
        _studioFailure(StateError('Studio runtime is shutting down')),
      );
    }
    final existing = _initFuture;
    if (existing != null) {
      return existing;
    }
    late final Future<void> attempt;
    attempt = () async {
      try {
        final initializationOverride = _initializationOverrideForTesting;
        if (initializationOverride != null) {
          await initializationOverride();
        } else {
          if (!_rustInitialized) {
            startupProgress.value = StudioStartupPhase.loadingBridge;
            final bridgeWatch = Stopwatch()..start();
            await RustLib.init();
            _rustInitialized = true;
            debugPrint(
              'startup_stage=load_bridge elapsed_ms=${bridgeWatch.elapsedMilliseconds}',
            );
          }
          // 关闭不触发初始化：RustLib.init() 完成后必须再检查单向闩，绝不再调用
          // native startStudioRuntime，也不发布 ready。
          if (_closing) return;
          await _startStudioRuntimeAttempt();
        }
      } catch (error, stackTrace) {
        _runtimeReady = false;
        // 迟到的失败：退出已开始，绝不把 UI 拉回 failed；仍以错误完成 init future，
        // 让关闭路径据此视为未决而不是「从未安装」。
        if (!_closing) {
          startupProgress.value = StudioStartupPhase.failed;
        }
        Error.throwWithStackTrace(_studioFailure(error), stackTrace);
      }
    }();
    _initFuture = attempt;
    return attempt;
  }

  /// 尽早同步封闭退出准入：关闭协调器在收束任何 owner 之前调用。该闩一经置位，本进程
  /// 余下生命周期内不再启动/发布运行时，也不再创建新的 Dart FRB 订阅；即使随后 Dart 进度
  /// 订阅取消挂住，也不允许新的 mutation 借此进入。
  @override
  void sealForShutdown() {
    _closing = true;
    _dartSubscriptionsSealed = true;
  }

  /// 一次启动尝试：先领取 attempt token 并订阅其进度流，再安装 runtime。
  ///
  /// 进度是事件驱动的当前帧（不再 100ms 轮询），完整映射 bridge 的全部启动阶段；
  /// `Ready`/`Failed` 为终态，成功后发布主线 `ready`。订阅登记为进程级 owned
  /// cancellation：启动仍 pending 时进入退出也会由 `cancelDartSubscriptions` 在预算内取消并
  /// 如实回报；本订阅只服务本次尝试，成功/失败/退出后都异步注销，避免重试累积。
  /// 同一次尝试失败后，重试会领取新的 token（每次调用都 `prepareStartupAttempt`）。
  static Future<void> _startStudioRuntimeAttempt() async {
    frb.BridgeEventSubscription? handle;
    StreamSubscription<frb.BridgeStartupStage>? progress;
    // 创建决策是否已完成（native 句柄已产出且可取消）：据此区分「真实 create/prepare
    // 失败、未产句柄」与「start/发布失败、已持有可取消资源」，避免篡改 create-error owner。
    var creationDecisionCompleted = false;

    Future<void> release() async {
      // 只有 native cancel 真正 ACK 才丢弃强引用并 dispose；抛错/超时保留 handle，owner
      // 仍强持有 opaque 句柄直到确认或进程终止，绝不先清后 cancel 让对象被 GC/finalizer。
      final activeHandle = handle;
      if (activeHandle != null) {
        if (RustLib.instance.initialized) {
          await activeHandle.cancel();
        }
        handle = null;
        activeHandle.dispose();
      }
      // native 已 ACK 后再收束本地进度监听；抛错保留这一活动层，不二次 cancel/dispose。
      final activeProgress = progress;
      if (activeProgress != null) {
        await activeProgress.cancel();
        progress = null;
      }
    }

    final tracked = _DartSubscription(
      'startup',
      release,
      _forgetStartupSubscription,
    );
    _trackStartupSubscription(tracked);

    try {
      // 退出/封闭/已取消时不新建桥订阅；本订阅是 pre-active，不在此触发 _ensureReady
      // （否则会递归初始化）。
      if (tracked.cancelRequested || _closing || _closing) {
        return;
      }
      final attempt = await _bridgeCall(frb.prepareStartupAttempt);
      // prepare await 期间退出已开始：不再新建订阅。
      if (tracked.cancelRequested || _closing || _closing) {
        return;
      }
      final created = await _bridgeCall(
        () => frb_startup_sub.subscribeStartupProgress(attempt: attempt),
      );
      // await create 期间退出已开始：句柄交给 owner 的 release 闭包，由 registry 持有并等
      // cancel 完成，绝不在这里自行 dispose 后误报 settled。
      handle = created;
      creationDecisionCompleted = true;
      if (tracked.cancelRequested || _closing || _closing) {
        return;
      }
      progress = created.startupStream().listen(
        (stage) {
          // 关闭/已取消：拒迟到帧，绝不把 UI 拉回启动态。
          if (tracked.cancelRequested || _closing || _closing) {
            return;
          }
          startupProgress.value = switch (stage) {
            frb.BridgeStartupStage.preparing => StudioStartupPhase.preparing,
            frb.BridgeStartupStage.waitingForSteps =>
              StudioStartupPhase.waitingForSteps,
            frb.BridgeStartupStage.closingResources =>
              StudioStartupPhase.closingResources,
            frb.BridgeStartupStage.backingUp => StudioStartupPhase.backingUp,
            frb.BridgeStartupStage.resetting => StudioStartupPhase.resetting,
            frb.BridgeStartupStage.startingServices =>
              StudioStartupPhase.startingServices,
            frb.BridgeStartupStage.openingStorage =>
              StudioStartupPhase.openingStorage,
            frb.BridgeStartupStage.loadingConfiguration =>
              StudioStartupPhase.loadingConfiguration,
            frb.BridgeStartupStage.readingProjects =>
              StudioStartupPhase.readingProjects,
            frb.BridgeStartupStage.preparingResources =>
              StudioStartupPhase.preparingResources,
            frb.BridgeStartupStage.ready => StudioStartupPhase.ready,
            frb.BridgeStartupStage.failed => StudioStartupPhase.failed,
          };
        },
        onError: (Object error, StackTrace stackTrace) {
          // 进度流失败不改变启动结果：成败由 startStudioRuntime 的返回/异常决定。
          debugPrint('startup_progress_error error=$error');
        },
      );
      // 创建决策（native 句柄 + 本地监听）已完成：立即开创建闩，令退出/取消能真实 cancel
      // 已持有资源，而不必等待随后 startStudioRuntime 的长 await。
      tracked.markCreated();
      // mark 后若已进入退出或收到取消：不再安装运行时（避免退出期间新建 runtime）。
      if (tracked.cancelRequested || _closing || _closing) {
        return;
      }
      final runtime = await frb.startStudioRuntime(attempt: attempt);
      // 迟到的运行时安装：退出已开始，绝不重新发布运行时或把 UI 拉回 ready。
      if (_closing) {
        _runtimeReady = false;
        return;
      }
      _runtimeReady = true;
      final report = runtime.startupRecovery;
      startupRecovery.value = report == null
          ? null
          : StartupRecoveryReport(
              backupPath: report.backupPath,
              reason: report.reason,
              createdAt: report.createdAt.toInt(),
            );
      startupProgress.value = StudioStartupPhase.ready;
    } catch (error, stackTrace) {
      // 仅「真实 create/prepare 失败、尚未产出可取消句柄」时记录 create failure：owner 据此
      // 在取消时如实回报，绝不静默成 Clean。start/发布失败已持有可取消资源，仅原样上报
      // initFuture typed error，不篡改 create-error owner 语义，cleanup 真成功仍可 forget。
      if (!creationDecisionCompleted) {
        tracked.recordCreateFailure(_studioFailure(error), stackTrace);
      }
      rethrow;
    } finally {
      // 完成创建闩，令取消可推进（含 create 未决 / 迟到的 native 句柄）。
      tracked.markCreated();
      // 本订阅只服务本次启动尝试：无论成功、失败还是退出都启动唯一 owned cancellation，
      // 异步注销以免重试累积；退出路径由 registry 在预算内 join 并写入关闭报告。
      unawaited(tracked.beginCancel());
    }
  }

  /// 共享同一退出 future：重复调用（重复关窗、dispose、driver）复用同一次收束，绝不复位
  /// 或延长单一期限。[externalIssues] 是 Dart 拥有的外部 owner 在本次预算内收束得到的
  /// typed issue，作为关闭的最终成功前置条件随请求交给 runtime 单编排。
  static Future<StudioShutdownReport> shutdownAndDispose({
    required int remainingMs,
    List<StudioShutdownIssue> externalIssues = const [],
  }) {
    final running = _shutdownFuture;
    if (running != null) return running;
    _closing = true;
    final attempt = _runShutdownAndDispose(remainingMs, externalIssues);
    _shutdownFuture = attempt;
    return attempt;
  }

  static Future<StudioShutdownReport> _runShutdownAndDispose(
    int remainingMs,
    List<StudioShutdownIssue> externalIssues,
  ) async {
    final stopwatch = Stopwatch()..start();
    // 早期退出封闭是同一 coordinator 的第一步：直接消费者（不经 runStudioShutdown）也必须在
    // 任何后续阶段前有界封闭准入，把失败/超时保留为 external issue 继续收尾。经
    // runStudioShutdown 的路径已调用同一 one-shot，这里复用结果，绝不重复并行封闭；已由协调器
    // 并入 externalIssues 的同一 issue（同 correlation）不再重复添加。
    final beginExit = await _runBeginRuntimeExit(remainingMs);
    final beginExitIssue = beginExit.issue;
    final mergedExternalIssues = beginExitIssue == null
        ? externalIssues
        : <StudioShutdownIssue>[
            ...externalIssues,
            if (!externalIssues.any(
              (issue) => issue.correlationId == beginExitIssue.correlationId,
            ))
              beginExitIssue,
          ];
    final initialization = _initFuture;
    var initCompleted = initialization == null;
    // 初始化是否已「有结果」（成功或失败）。与 initCompleted 不同：失败完成也视为已定，
    // 据此区分「确无 owner」与「永久未决」。
    var initSettled = initialization == null;
    if (initialization != null) {
      try {
        // 等待初始化有界：初始化未在预算内完成即视为未决，不无限挂起退出。
        final wait = remainingMs <= 0 ? 0 : remainingMs.clamp(0, 5000).toInt();
        await initialization.timeout(Duration(milliseconds: wait));
        initCompleted = true;
        initSettled = true;
      } on TimeoutException {
        initCompleted = false;
        initSettled = false;
      } on Object {
        // 初始化失败完成：若 RustLib 未装载则确无 owner；若已装载则由真实报告判定。
        initCompleted = false;
        initSettled = true;
      }
    }
    // Dart 侧外部 owner（产品/Thread 流与关机进度流）的取消已在协调器的同一预算内、于
    // 本次请求之前完成（见 runStudioShutdown / cancelDartSubscriptions），此处绝不再取消，
    // 避免两份关闭事实。即使 Dart 取消已耗尽预算，仍以 remainingMs=0 立即向已初始化的
    // runtime 广播封闭，绝不整段跳过；bridge 上限 28s、原生 hard deadline 照常。
    final rustBudget = _remainingAfter(remainingMs, stopwatch);
    StudioShutdownReport report;
    var bridgeConsulted = false;
    if (_rustInitialized) {
      bridgeConsulted = true;
      try {
        // 用真实 Rust 报告判定是否安装 runtime，而不是本地标志；外部 issue 随请求交给
        // runtime 与 bridge 自身取消结果一并裁决 Clean/Stopped/实例锁释放。
        report = _shutdownReportFromFrb(
          await _bridgeCall(
            () => frb.shutdownRuntime(
              remainingMs: rustBudget,
              externalIssues: [
                for (final issue in mergedExternalIssues)
                  _bridgeIssueFromStudio(issue),
              ],
            ),
          ),
        );
      } on Object catch (error, stackTrace) {
        // 同一次失败只生成一次 correlation：report issue 与同步诊断共用同一编号。
        final correlationId = _correlationOf(error);
        // 不扔掉已有阶段问题：把桥错误本身与 Dart 订阅问题一起如实上报为 Degraded。
        recordDartError(
          error,
          stackTrace,
          stage: 'shutdown',
          correlationId: correlationId,
        );
        report = StudioShutdownReport(
          outcome: StudioShutdownOutcome.degraded,
          issues: [
            StudioShutdownIssue(
              stage: 'shutdown',
              code: _codeOf(error),
              message: 'studio runtime shutdown reported an error',
              retryable: false,
              correlationId: correlationId,
            ),
          ],
          persistence: const UnknownStudioPendingPersistence(),
        );
      }
      _runtimeReady = false;
    } else if (initSettled) {
      // 初始化已定且 RustLib 未装载：确无 owner，才视为从未安装。
      report = StudioShutdownReport.notStarted;
    } else {
      // 初始化永久未决：可能仍持有 native owner，如实为 Degraded + Unknown。
      report = _unresolvedInitReport();
    }
    // 初始化永久未决时绝不允许正常退出：即使桥报告 NotStarted，也可能仍有尚未安装完成的
    // owner；只有在初始化确有结果（成功或失败完成）时，NotStarted 才能据实判定为 exit 0。
    if (!initSettled && report.allowsCleanExit) {
      report = StudioShutdownReport(
        outcome: StudioShutdownOutcome.degraded,
        issues: [...report.issues, ..._unresolvedInitReport().issues],
        persistence: const UnknownStudioPendingPersistence(),
      );
    }
    // 外部（Dart）issue 是本次关闭的最终成功前置条件：桥未咨询（无 runtime）时在此如实
    // 并入；桥已咨询时以桥的裁决为准，但绝不静默吞掉真实 issue —— 只要事后仍能可靠
    // Clean/NotStarted，就说明这些真实问题未被承认，必须本地降级。
    report = _reflectExternalIssues(
      report,
      mergedExternalIssues,
      bridgeConsulted,
    );
    // 仅 Clean/NotStarted 且初始化已完成才允许稍后安全 dispose；Degraded 或未决初始化
    // 保留 owner 与句柄至进程终止，避免与仍在写库的 writer 竞争。native 与 Dart 订阅
    // 两侧都收敛才允许 dispose；真正的 dispose 由 [finishShutdownDiagnostics] 在调用方
    // 收束/取消 shutdown-progress 订阅之后执行。
    _safeToDisposeAfterShutdown =
        _rustInitialized &&
        initCompleted &&
        report.allowsCleanExit &&
        _dartSubscriptionsSettled;
    return report;
  }

  static int _remainingAfter(int remainingMs, Stopwatch watch) {
    if (remainingMs <= 0) return 0;
    final left = remainingMs - watch.elapsedMilliseconds;
    return left <= 0 ? 0 : left;
  }

  /// 早期退出封闭本身的有界上限：绝不无界等待桥，也绝不延长单一 30s / 28s 期限。
  static const int _beginExitBoundMs = 2000;

  /// [StudioTransport.beginRuntimeExit] 的单一 one-shot 实现。
  ///
  /// 未加载 runtime（`RustLib.init` 未完成）时确无 owner，跳过且**不**触发初始化；库已加载
  /// 即直接调用 native `begin_runtime_exit` 封闭准入并广播一次取消（令在途 start 无法再发布
  /// runtime），**不**等待 `initFuture`。自身有界（≤2s，随剩余清理预算收紧）。失败/超时保留
  /// 原始错误、堆栈与 memoized typed issue，并记录一次脱敏诊断；调用方据此作为 external
  /// issue 继续收尾，绝不伪成功或 early return。重复调用复用同一结果，绝不重复并行封闭。
  static Future<_BeginExitOutcome> _runBeginRuntimeExit(int remainingMs) {
    final existing = _beginExitOutcome;
    if (existing != null) return existing;
    final attempt = () async {
      if (!_rustInitialized) {
        // RustLib 未加载：确无 runtime owner 可封闭。迟到初始化 / start 由单向闩拒绝，
        // 这里跳过而不是触发初始化。
        return const _BeginExitOutcome.success();
      }
      final bound = remainingMs <= 0
          ? 1
          : (remainingMs > _beginExitBoundMs ? _beginExitBoundMs : remainingMs);
      try {
        await _bridgeCall(() => frb.beginRuntimeExit(remainingMs: remainingMs))
            .timeout(Duration(milliseconds: bound));
        return const _BeginExitOutcome.success();
      } on Object catch (error, stackTrace) {
        // 同一次失败只生成一次 correlation：memoized issue 与脱敏诊断共用同一编号。
        final correlationId = _correlationOf(error);
        final issue = StudioShutdownIssue(
          stage: 'exit-seal',
          code: _codeOf(error),
          // 固定安全文案：绝不把任意 error.toString()（可能含正文/凭据）当 wire 安全消息。
          message: 'studio runtime early exit seal did not confirm',
          retryable: false,
          correlationId: correlationId,
        );
        _beginExitFailure = error;
        _beginExitIssue = issue;
        recordDartError(
          error,
          stackTrace,
          stage: 'exit-seal',
          correlationId: correlationId,
        );
        return _BeginExitOutcome.failed(
          issue: issue,
          error: error,
          stackTrace: stackTrace,
        );
      }
    }();
    _beginExitOutcome = attempt;
    return attempt;
  }

  /// 当 [error] 正是早期退出封闭 core 记录的原始失败时，返回其 memoized typed issue（协调器
  /// 据此复用同一 correlation，绝不重复生成 / 重复记录）；否则（测试替身自抛的失败）返回
  /// `null`，由调用方走自身脱敏诊断路径。
  static StudioShutdownIssue? recordedBeginExitIssue(Object error) {
    final issue = _beginExitIssue;
    return issue != null && identical(error, _beginExitFailure) ? issue : null;
  }

  /// 让最终报告如实反映 Dart 外部取消 issue。
  ///
  /// 桥已被咨询（有 runtime）时，external issue 由 bridge 与自身取消结果一起裁决；只有
  /// 返回仍为可靠 `Clean`/`NotStarted`（无问题）时才说明这些真实 issue 被吞掉，必须本地
  /// 降级，绝不伪 Clean —— 正常情况下桥已并入，不会重复计数。桥未被咨询（无 runtime）时
  /// 直接并入。绝不改写 runtime 报出的待保存事实（未知保持未知）。
  static StudioShutdownReport _reflectExternalIssues(
    StudioShutdownReport report,
    List<StudioShutdownIssue> externalIssues,
    bool bridgeConsulted,
  ) {
    if (externalIssues.isEmpty) return report;
    if (bridgeConsulted && !report.allowsCleanExit) return report;
    return StudioShutdownReport(
      outcome: StudioShutdownOutcome.degraded,
      issues: [...report.issues, ...externalIssues],
      persistence: report.persistence,
    );
  }

  /// 领域 issue 到 bridge wire 的一次穷尽映射；只传允许的诊断字段，绝不携带正文/凭据。
  static frb_shutdown.BridgeShutdownIssue _bridgeIssueFromStudio(
    StudioShutdownIssue issue,
  ) {
    return frb_shutdown.BridgeShutdownIssue(
      stage: issue.stage,
      code: issue.code,
      message: issue.message,
      retryable: issue.retryable,
      correlationId: issue.correlationId,
    );
  }

  static String _correlationOf(Object error) =>
      error is StudioFailure && error.correlationId.isNotEmpty
      ? error.correlationId
      : newStudioCorrelationId();

  static String _codeOf(Object error) => switch (error) {
    StudioFailure(:final code) => code.name,
    TimeoutException() => 'timeout',
    _ => 'unexpected',
  };

  /// 初始化永久未决时的报告：不伪「从未安装」，待保存事实未知。correlation 非空，
  /// 便于与同步日志/桥诊断关联。
  static StudioShutdownReport _unresolvedInitReport() => StudioShutdownReport(
    outcome: StudioShutdownOutcome.degraded,
    issues: [
      StudioShutdownIssue(
        stage: 'initialize',
        code: 'initUnresolved',
        message: 'studio runtime initialization did not complete',
        retryable: false,
        correlationId: newStudioCorrelationId(),
      ),
    ],
    persistence: const UnknownStudioPendingPersistence(),
  );

  /// 独立诊断收尾：即使 runtime 从未安装或安装失败，也回收诊断资源。
  ///
  /// 调用方必须先收束/取消所有 Dart 订阅，之后这里才在必要时真正 `RustLib.dispose()`：
  /// dispose 早于订阅取消正是桥在退出时挂起的点。
  static Future<void> finishShutdownDiagnostics() async {
    if (_rustInitialized) {
      try {
        await _bridgeCall(frb.finishShutdownDiagnostics);
      } on Object catch (error, stackTrace) {
        // 诊断收尾失败不阻断退出；真实原因由报告与同步日志承载。
        recordDartError(
          error,
          stackTrace,
          stage: 'diagnostics',
          correlationId: _correlationOf(error),
        );
      }
    }
    // 只有 native 与 Dart 两侧订阅全部收束后才真正 dispose；否则保留 owner，绝不在
    // 仍有 bridge 回调时释放，也绝不把正常关闭拖成 watchdog 的 degraded。
    if (_rustInitialized &&
        _safeToDisposeAfterShutdown &&
        _dartSubscriptionsSettled) {
      RustLib.dispose();
      _rustInitialized = false;
      _initFuture = null;
    }
    _safeToDisposeAfterShutdown = false;
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async {
    final cached = _providerCatalogCache;
    if (cached != null) return cached;
    final inFlight = _providerCatalogFuture;
    if (inFlight != null) return inFlight;
    final future = _loadProviderCatalog();
    _providerCatalogFuture = future;
    try {
      return await future;
    } finally {
      if (identical(_providerCatalogFuture, future)) {
        _providerCatalogFuture = null;
      }
    }
  }

  Future<ProviderCatalogView> _loadProviderCatalog() async {
    await _ensureReady();
    final catalog = providerCatalogFromFrb(
      await _bridgeCall(frb.loadProviderCatalog),
    );
    if (catalog.isUsable) {
      _providerCatalogCache = catalog;
    }
    return catalog;
  }

  @override
  Future<AgentProfilesStateView> readAgentProfilesState() async {
    await _ensureReady();
    return _agentProfilesStateFromFrb(
      await _bridgeCall(frb.readAgentProfilesState),
    );
  }

  @override
  Future<List<AgentProfileView>> readAgentProfiles() async {
    return (await readAgentProfilesState()).data.profiles;
  }

  @override
  Future<SettingsCommandReply> setSystemAgentEnabled({
    required OperationId operationId,
    required int expectedSettingsRevision,
    required String profileId,
    required bool enabled,
  }) async {
    await _ensureReady();
    return _settingsCommandReplyFromFrb(
      await _bridgeCall(
        () => frb.setSystemAgentEnabled(
          operationId: operationId.value,
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          profileId: profileId,
          enabled: enabled,
        ),
      ),
    );
  }

  @override
  Future<SettingsCommandReply> saveUserAgentProfile(
    int expectedSettingsRevision,
    AgentProfileDraft draft, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    return _settingsCommandReplyFromFrb(
      await _bridgeCall(
        () => frb.saveUserAgentProfile(
          operationId: operationId.value,
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          profileId: draft.id,
          enabled: draft.enabled,
          displayName: draft.displayName,
          description: draft.description,
          whenToUse: draft.whenToUse,
          systemInstructions: draft.systemInstructions,
          providerId: draft.providerId,
          model: draft.model,
          effort: draft.effort,
          workspaceMode: switch (draft.workspaceMode) {
            AgentWorkspaceMode.unrestricted =>
              frb.BridgeAgentWorkspaceMode.unrestricted,
            AgentWorkspaceMode.directory =>
              frb.BridgeAgentWorkspaceMode.directory,
            AgentWorkspaceMode.worktree =>
              frb.BridgeAgentWorkspaceMode.worktree,
            AgentWorkspaceMode.review => frb.BridgeAgentWorkspaceMode.review,
          },
        ),
      ),
    );
  }

  @override
  Future<void> cleanupPreservedWorktree({
    required String ownerKind,
    required String ownerThreadId,
    required int expectedLeaseRevision,
  }) async {
    await _ensureReady();
    await _bridgeCall(
      () => frb.cleanupPreservedWorktree(
        ownerKind: ownerKind,
        ownerThreadId: ownerThreadId,
        expectedLeaseRevision: BigInt.from(expectedLeaseRevision),
      ),
    );
  }

  @override
  Future<StudioBootstrap> readBootstrap() async {
    await _ensureReady();
    final watch = Stopwatch()..start();
    final state = studioBootstrapFromFrbSnapshot(
      await _bridgeCall(frb.readStudioState),
    );
    debugPrint(
      'startup_stage=read_state elapsed_ms=${watch.elapsedMilliseconds}',
    );
    return state;
  }

  @override
  Future<ThreadDirectoryPage> listThreadsPage({
    String? cursor,
    int limit = 50,
  }) async {
    await _ensureReady();
    final page = await _bridgeCall(
      () => frb.listThreadsPage(
        request: frb.BridgeListThreadsPageRequest(
          cursor: cursor,
          limit: limit.clamp(1, 100),
        ),
      ),
    );
    return _threadDirectoryPageFromFrb(page);
  }

  @override
  Future<ThreadDirectoryPage> queryThreads(
    DirectoryQuery query, {
    String? cursor,
    int limit = 50,
  }) async {
    await _ensureReady();
    final page = await _bridgeCall(
      () => frb.queryThreads(
        request: frb.BridgeDirectoryQuery(
          projectId: query.projectId,
          search: query.search,
          archived: query.archived,
          filter: switch (query.filter) {
            DirectoryFilter.all => frb.BridgeDirectoryFilter.all,
            DirectoryFilter.running => frb.BridgeDirectoryFilter.running,
            DirectoryFilter.attention => frb.BridgeDirectoryFilter.attention,
          },
          cursor: cursor,
          limit: limit.clamp(1, 100),
        ),
      ),
    );
    return _threadDirectoryPageFromFrb(page);
  }

  @override
  Future<StudioThread> restoreThread(String threadId) async {
    await _ensureReady();
    return _threadFromFrb(
      await _bridgeCall(() => frb.restoreThread(threadId: threadId)),
    );
  }

  @override
  Future<StudioProject> renameProject(String projectId, String name) async {
    await _ensureReady();
    return _projectFromFrb(
      await _bridgeCall(
        () => frb.renameProject(projectId: projectId, name: name),
      ),
    );
  }

  @override
  Future<StudioProject> openProject(String path) async {
    await _ensureReady();
    return _projectFromFrb(
      await _bridgeCall(() => frb.openProject(path: path)),
    );
  }

  @override
  Future<List<SshServer>> listSshServers() async {
    await _ensureReady();
    final servers = await _bridgeCall(frb_ssh.listSshServers);
    return servers.map(_sshServerFromFrb).toList(growable: false);
  }

  @override
  Future<SshServer> saveSshServer(SaveSshServerCommand command) async {
    await _ensureReady();
    final server = await _bridgeCall(
      () => frb_ssh.saveSshServer(
        request: frb_ssh_types.SaveSshServerRequest(
          alias: command.alias,
          hostName: command.hostName,
          port: command.port,
          username: command.username,
          identityFile: command.identityFile,
          password: switch (command.password.action) {
            SshPasswordAction.preserve =>
              const frb_ssh_types.SshPasswordInput.preserve(),
            SshPasswordAction.replace => frb_ssh_types.SshPasswordInput.replace(
              value: command.password.value!,
            ),
            SshPasswordAction.clear =>
              const frb_ssh_types.SshPasswordInput.clear(),
          },
        ),
      ),
    );
    return _sshServerFromFrb(server);
  }

  @override
  Future<void> deleteSshServer(String alias) async {
    await _ensureReady();
    await _bridgeCall(() => frb_ssh.deleteSshServer(alias: alias));
  }

  @override
  Future<SshConnectionView> authenticateSshServer(
    AuthenticateSshServerCommand command,
  ) async {
    await _ensureReady();
    final snapshot = await _bridgeCall(
      () => frb_ssh.authenticateSshServer(
        request: frb_ssh_types.AuthenticateSshServerRequest(
          alias: command.alias,
          password: command.password,
          rememberPassword: command.rememberPassword,
        ),
      ),
    );
    return SshConnectionView(
      alias: snapshot.alias,
      state: snapshot.state,
      helperVersion: snapshot.helperVersion,
      architecture: snapshot.architecture,
      attempt: snapshot.attempt,
      delaySeconds: snapshot.delaySeconds?.toInt(),
      errorCode: snapshot.errorCode,
      errorMessage: snapshot.errorMessage,
    );
  }

  @override
  Future<SshConnectionView> testSshConnection(String alias) async {
    await _ensureReady();
    final snapshot = await _bridgeCall(
      () => frb_ssh.testSshConnection(alias: alias),
    );
    return SshConnectionView(
      alias: snapshot.alias,
      state: snapshot.state,
      helperVersion: snapshot.helperVersion,
      architecture: snapshot.architecture,
      attempt: snapshot.attempt,
      delaySeconds: snapshot.delaySeconds?.toInt(),
      errorCode: snapshot.errorCode,
      errorMessage: snapshot.errorMessage,
    );
  }

  @override
  Future<SshConnectionView> reconnectSshServer(String alias) async {
    await _ensureReady();
    final snapshot = await _bridgeCall(
      () => frb_ssh.reconnectSshServer(alias: alias),
    );
    return SshConnectionView(
      alias: snapshot.alias,
      state: snapshot.state,
      helperVersion: snapshot.helperVersion,
      architecture: snapshot.architecture,
      attempt: snapshot.attempt,
      delaySeconds: snapshot.delaySeconds?.toInt(),
      errorCode: snapshot.errorCode,
      errorMessage: snapshot.errorMessage,
    );
  }

  @override
  Future<RemoteDirectoryListing> browseRemoteDirectories(
    String alias, {
    String? path,
  }) async {
    await _ensureReady();
    final listing = await _bridgeCall(
      () => frb_ssh.browseRemoteDirectories(alias: alias, path: path),
    );
    return RemoteDirectoryListing(
      path: listing.path,
      parent: listing.parent,
      entries: listing.entries
          .map(
            (entry) => RemoteDirectoryEntry(name: entry.name, path: entry.path),
          )
          .toList(growable: false),
    );
  }

  @override
  Future<StudioProject> openRemoteProject(String alias, String path) async {
    await _ensureReady();
    return _projectFromFrb(
      await _bridgeCall(
        () => frb_ssh.openRemoteProject(alias: alias, path: path),
      ),
    );
  }

  @override
  Future<void> activateProject(String projectId) async {
    await _ensureReady();
    await _bridgeCall(() => frb.activateProject(projectId: projectId));
  }

  @override
  Future<StartNewThreadResult> startNewThread(
    String projectId,
    StudioPromptInput input,
    ThreadModeId mode, {
    required ModeModelRouteView route,
    String? workspaceMode,
  }) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.startNewThread(
        projectId: projectId,
        input: _bridgePromptInput(input),
        mode: mode.id,
        providerId: route.providerId,
        model: route.model,
        effort: route.effort.isEmpty ? null : route.effort,
        workspaceMode: workspaceMode,
      ),
    );
    return StartNewThreadResult(
      thread: _threadFromFrb(response.thread),
      receipt: SubmitPromptReceipt(
        threadId: response.receipt.threadId,
        inputId: response.receipt.inputId,
        cursor: response.receipt.revision.toInt(),
      ),
    );
  }

  @override
  Future<RenameThreadCommandReply> renameThread(
    String threadId,
    String title, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.renameThread(
        operationId: operationId.value,
        threadId: threadId,
        title: title,
      ),
    );
    return RenameThreadCommandReply(
      thread: _threadFromFrb(response.thread),
      receipt: _commandReceiptFromFrb(response.receipt),
    );
  }

  @override
  Future<ArchiveThreadCommandReply> archiveThread(
    String threadId, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () =>
          frb.archiveThread(operationId: operationId.value, threadId: threadId),
    );
    return ArchiveThreadCommandReply(
      result: ArchiveThreadResult(
        archivedRootId: response.result.archivedRootId,
        removedThreadIds: response.result.removedThreadIds,
        nextRoot: response.result.nextRoot == null
            ? null
            : _threadFromFrb(response.result.nextRoot!),
      ),
      receipt: _commandReceiptFromFrb(response.receipt),
    );
  }

  @override
  Future<void> archiveProject(String projectId) async {
    await _ensureReady();
    await _bridgeCall(() => frb.archiveProject(projectId: projectId));
  }

  @override
  Future<PersistenceStateSnapshot> retryPersistence() async {
    await _ensureReady();
    return _persistenceStateFromFrb(await _bridgeCall(frb.retryPersistence));
  }

  @override
  Future<PersistenceQueueSnapshot> retryThreadHistory(
    String threadId,
    int faultGeneration,
  ) async {
    await _ensureReady();
    return _persistenceQueueFromFrb(
      await _bridgeCall(
        () => frb.retryThreadHistory(
          threadId: threadId,
          faultGeneration: BigInt.from(faultGeneration),
        ),
      ),
    );
  }

  @override
  Future<PersistenceQueueStateView> readPersistenceQueueState() async {
    await _ensureReady();
    return _persistenceQueueStateFromFrb(
      await _bridgeCall(frb.readPersistenceQueue),
    );
  }

  @override
  Future<PersistenceQueueSnapshot> readPersistenceQueue() async {
    return (await readPersistenceQueueState()).queue;
  }

  @override
  Future<PersistenceQueueSnapshot> resumeThreadHistory(
    String threadId,
    int faultGeneration,
  ) async {
    await _ensureReady();
    return _persistenceQueueFromFrb(
      await _bridgeCall(
        () => frb.resumeThreadHistory(
          threadId: threadId,
          faultGeneration: BigInt.from(faultGeneration),
        ),
      ),
    );
  }

  @override
  Future<ThreadModelRouteCommandReply> setThreadModelRoute({
    required OperationId operationId,
    required String threadId,
    required int expectedModelRouteRevision,
    required int expectedSettingsRevision,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.setThreadModelRoute(
        operationId: operationId.value,
        threadId: threadId,
        providerId: providerId,
        model: model,
        effort: effort,
        expectedModelRouteRevision: BigInt.from(expectedModelRouteRevision),
        expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
      ),
    );
    return ThreadModelRouteCommandReply(
      receipt: _commandReceiptFromFrb(response.receipt),
      runtime: _threadRuntimeFromFrb(response.runtime),
      settings: _settingsStateFromFrb(response.settings),
      modeDefaultSaved: response.modeDefaultSaved,
      warning: response.warning,
    );
  }

  @override
  Future<ThreadModeCommandReply> setThreadMode({
    required OperationId operationId,
    required String threadId,
    required ThreadModeId mode,
  }) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.setThreadMode(
        operationId: operationId.value,
        threadId: threadId,
        mode: mode.id,
      ),
    );
    return ThreadModeCommandReply(
      receipt: _commandReceiptFromFrb(response.receipt),
    );
  }

  @override
  Stream<StudioShutdownProgress> subscribeShutdownProgress() {
    if (!_runtimeReady) {
      // 无 owner（未安装 / 第二实例 / 初始化失败）：不存在可观察的进度资源。返回空流，
      // 绝不触发初始化，也不产生虚假的 NotInitialized 进度失败。
      return Stream<StudioShutdownProgress>.empty();
    }
    late final StreamController<StudioShutdownProgress> controller;
    frb.BridgeEventSubscription? handle;
    StreamSubscription<frb.BridgeShutdownProgress>? subscription;

    Future<void> release() async {
      // 只有 native cancel 真正 ACK 才丢弃强引用并 dispose；抛错/超时保留 handle，owner
      // 仍强持有 opaque 句柄直到确认或进程终止，绝不先清后 cancel 让对象被 GC/finalizer。
      final activeHandle = handle;
      if (activeHandle != null) {
        if (RustLib.instance.initialized) {
          await activeHandle.cancel();
        }
        handle = null;
        activeHandle.dispose();
      }
      // native 已 ACK 后 handle 已清；若 Dart 订阅取消抛错则保留这一活动层，不二次
      // cancel/dispose，也不把已 ACK 的 native 变成伪 pending。
      final activeSubscription = subscription;
      if (activeSubscription != null) {
        await activeSubscription.cancel();
        subscription = null;
      }
    }

    // 与产品/Thread 流同一套 owned cancellation（`_DartSubscription`）：normal `onCancel`
    // 只启动这一条 future，先等完整 create 决策再释放（含迟到句柄），绝不重复
    // cancel/dispose，也绝不在创建未完成时早退伪造「已停止」。进度 owner **登记进进程级
    // registry**：取消未 ACK 时 registry 强持有它（阻止 GC 与 safeDispose 并发），直到真正
    // ACK 才删除；但标记为外部协调（`externallyCoordinated`），由关闭协调器在自身预算内
    // 单独 join / 回报，`cancelDartSubscriptions` 汇总不重复 cancel，避免重复 issue。
    final tracked = _DartSubscription(
      'shutdown-progress',
      release,
      _forgetDartSubscription,
      externallyCoordinated: true,
    );
    _trackDartSubscription(tracked);

    Future<void> start() async {
      try {
        // 已取消则不再创建。进度流是退出作用域内的本地显示流，故与产品/Thread 不同：它在
        // 退出已 seal 后仍按其本地策略创建，但 create 不设本地 timeout —— 有界等待落在
        // owned cancellation（等完整 create 决策）。因此 create 未决 / 超时 / 迟到都不会被
        // 误判为已停止，迟到句柄仍由 owner 的 release 真实 cancel 并取 ACK。也绝不触发
        // _ensureReady。
        if (tracked.cancelRequested) {
          return;
        }
        final created = await _bridgeCall(
          frb_shutdown_sub.subscribeShutdownProgress,
        );
        handle = created;
        // 取消在 create 期间到达：不派发任何事件，句柄交给 owner 的 release 真实 cancel。
        if (tracked.cancelRequested) {
          return;
        }
        subscription = created.shutdownStream().listen(
          (event) => controller.add(switch (event) {
            frb.BridgeShutdownProgress_StoppingSubscriptions() =>
              const StoppingSubscriptionsProgress(),
            frb.BridgeShutdownProgress_CancellingTurns() =>
              const CancellingTurnsProgress(),
            frb.BridgeShutdownProgress_FlushingPersistence(
              :final pendingCommits,
            ) =>
              FlushingPersistenceProgress(
                pendingCommits: pendingCommits.toInt(),
              ),
            frb.BridgeShutdownProgress_StoppingAgents() =>
              const StoppingAgentsProgress(),
            frb.BridgeShutdownProgress_StoppingMcp() =>
              const StoppingMcpProgress(),
            frb.BridgeShutdownProgress_StoppingLsp() =>
              const StoppingLspProgress(),
            frb.BridgeShutdownProgress_Stopped() => const StoppedProgress(),
          }),
          onError: (Object error, StackTrace stackTrace) =>
              controller.addError(_studioFailure(error), stackTrace),
          onDone: controller.close,
        );
      } catch (error, stackTrace) {
        if (!tracked.cancelRequested) {
          // 尚未取消：向仍活动的 listener 下发 typed 失败。
          controller.addError(_studioFailure(error), stackTrace);
          await controller.close();
        } else {
          // 取消已请求：listener / controller 多半已取消、不能 addError。把真实创建异常
          // 记入 owner，做成取消 ACK failure 让预算内的取消等待（`_cancelBounded`）如实回报
          // 为 external issue，绝不静默成 Clean；保留 typed code / correlation 与原始堆栈，
          // wire / 日志文案固定安全（不泄漏 body/credential）。
          final failure = _studioFailure(error);
          tracked.recordCreateFailure(
            failure is StudioFailure
                ? StudioFailure(
                    code: failure.code,
                    message:
                        'studio shutdown progress subscription failed to start',
                    retryable: failure.retryable,
                    correlationId: failure.correlationId,
                  )
                : failure,
            stackTrace,
          );
        }
      } finally {
        // 无论是否创建成功都完成创建闩：owned cancellation 据此释放已建立或迟到的 native
        // 句柄；create 永久未决则不完成该闩，owner 被保留并由预算内的取消等待如实回报。
        tracked.markCreated();
      }
    }

    controller = StreamController<StudioShutdownProgress>(
      onListen: () => unawaited(start()),
      // 不吞取消错误：owned cancellation 的原始 cause/stack 交给预算内的取消等待
      // （runStudioShutdown 的 `_cancelBounded`）回报为 typed issue 并记录同一 correlation。
      onCancel: () => tracked.beginCancel(),
    );
    return controller.stream;
  }

  @override
  Future<void> beginRuntimeExit({required int remainingMs}) async {
    final outcome = await _runBeginRuntimeExit(remainingMs);
    final error = outcome.error;
    if (error != null) {
      // 原始错误/堆栈抛给协调器：协调器据 memoized issue（同一 correlation）作为 external
      // issue 继续收尾，绝不用无界等待或伪成功。
      Error.throwWithStackTrace(
        error,
        outcome.stackTrace ?? StackTrace.current,
      );
    }
  }

  @override
  Future<StudioShutdownReport> shutdownRuntime({
    required int remainingMs,
    List<StudioShutdownIssue> externalIssues = const [],
  }) => shutdownAndDispose(
    remainingMs: remainingMs,
    externalIssues: externalIssues,
  );

  @override
  Future<InteractionCommandReply> respondInteraction(
    String interactionId,
    InteractionResolutionCommand resolution, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.respondInteraction(
        operationId: operationId.value,
        interactionId: interactionId,
        resolution: _interactionResolutionFromDomain(resolution),
      ),
    );
    return InteractionCommandReply(
      interaction: _interactionFromFrb(response.interaction),
      receipt: _commandReceiptFromFrb(response.receipt),
    );
  }

  static Future<T> _bridgeCall<T>(Future<T> Function() call) async {
    try {
      return await call();
    } catch (error, stackTrace) {
      Error.throwWithStackTrace(_studioFailure(error), stackTrace);
    }
  }

  @override
  Future<InterruptTurnCommandReply> interruptTurn(
    String threadId,
    String turnId, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.interruptTurn(
        operationId: operationId.value,
        threadId: threadId,
        turnId: turnId,
      ),
    );
    return InterruptTurnCommandReply(
      threadId: response.threadId,
      turnId: response.turnId,
      interrupted: response.interrupted,
      receipt: _commandReceiptFromFrb(response.receipt),
    );
  }

  @override
  Stream<ProductTopicFrame> subscribeProductTopic(ProductTopic topic) {
    late final StreamController<ProductTopicFrame> controller;
    frb.BridgeEventSubscription? handle;
    StreamSubscription<frb_topic_types.BridgeProductTopicStreamEnvelope>?
    subscription;

    Future<void> release() async {
      // 只有 native cancel 真正 ACK 才丢弃强引用并 dispose；抛错/超时保留 handle，owner
      // 仍强持有 opaque 句柄直到确认或进程终止，绝不先清后 cancel 让对象被 GC/finalizer。
      final activeHandle = handle;
      if (activeHandle != null) {
        if (RustLib.instance.initialized) {
          await activeHandle.cancel();
        }
        handle = null;
        activeHandle.dispose();
      }
      // native 已 ACK 后 handle 已清；若 Dart 订阅取消抛错则保留这一活动层，不二次
      // cancel/dispose，也不把已 ACK 的 native 变成伪 pending。
      final activeSubscription = subscription;
      if (activeSubscription != null) {
        await activeSubscription.cancel();
        subscription = null;
      }
    }

    final tracked = _DartSubscription(
      'product',
      release,
      _forgetDartSubscription,
    );
    _trackDartSubscription(tracked);

    Future<void> start() async {
      try {
        // 关闭/封闭/已取消时不创建新桥订阅；_ensureReady 也不在退出后被调用。
        if (tracked.cancelRequested || _closing || _dartSubscriptionsSealed) {
          return;
        }
        await _ensureReady();
        if (tracked.cancelRequested || _dartSubscriptionsSealed || _closing) {
          return;
        }
        final created = await _bridgeCall(
          () => frb_topic_sub.createProductTopicSubscription(
            topic: bridgeProductTopic(topic),
          ),
        );
        // await create 期间关闭：句柄交给 owner 的 release 闭包，由 registry 持有并等
        // cancel 完成，绝不在这里自行 dispose 后误报 settled。
        handle = created;
        if (tracked.cancelRequested || _dartSubscriptionsSealed || _closing) {
          return;
        }
        subscription = created.productTopicStream().listen(
          (envelope) {
            if (envelope
                is frb_topic_types.BridgeProductTopicStreamEnvelope_Closed) {
              controller.close();
              return;
            }
            final frame = productTopicFrameFromEnvelope(topic, envelope);
            if (frame != null) controller.add(frame);
          },
          onError: (Object error, StackTrace stackTrace) =>
              controller.addError(_studioFailure(error), stackTrace),
          onDone: controller.close,
        );
      } catch (error, stackTrace) {
        if (!tracked.cancelRequested) {
          controller.addError(_studioFailure(error), stackTrace);
          await controller.close();
        }
      } finally {
        // 封闭/退出中：即使本 entry 尚未收到取消请求，也启动唯一 owned cancellation，
        // 保证任何已建立（或迟到）的 native 句柄都被释放，绝不让 owner 逃逸 settle 检查。
        if (_dartSubscriptionsSealed || _closing) {
          unawaited(tracked.beginCancel());
        }
        // 无论是否创建成功都完成创建闩：取消不必永远等待一个不会发生的创建。
        tracked.markCreated();
      }
    }

    controller = StreamController<ProductTopicFrame>(
      onListen: () => unawaited(start()),
      onCancel: () async {
        try {
          await tracked.beginCancel();
        } on Object {
          // 具体（脱敏）错因与堆栈已由 owner 保留，用于关闭报告的同步诊断；onCancel
          // 不抛出，避免未 await 的 cancel 变成未处理异步错误。失败即保留 pending owner。
        }
      },
    );
    return controller.stream;
  }

  @override
  Stream<ThreadStreamFrame> subscribeThread(String threadId) {
    late final StreamController<ThreadStreamFrame> controller;
    frb.BridgeEventSubscription? handle;
    StreamSubscription<frb.BridgeThreadStreamEnvelope>? subscription;

    Future<void> release() async {
      // 只有 native cancel 真正 ACK 才丢弃强引用并 dispose；抛错/超时保留 handle，owner
      // 仍强持有 opaque 句柄直到确认或进程终止，绝不先清后 cancel 让对象被 GC/finalizer。
      final activeHandle = handle;
      if (activeHandle != null) {
        if (RustLib.instance.initialized) {
          await activeHandle.cancel();
        }
        handle = null;
        activeHandle.dispose();
      }
      // native 已 ACK 后 handle 已清；若 Dart 订阅取消抛错则保留这一活动层，不二次
      // cancel/dispose，也不把已 ACK 的 native 变成伪 pending。
      final activeSubscription = subscription;
      if (activeSubscription != null) {
        await activeSubscription.cancel();
        subscription = null;
      }
    }

    final tracked = _DartSubscription(
      'thread',
      release,
      _forgetDartSubscription,
    );
    _trackDartSubscription(tracked);

    Future<void> start() async {
      try {
        // 关闭/封闭/已取消时不创建新桥订阅；_ensureReady 也不在退出后被调用。
        if (tracked.cancelRequested || _closing || _dartSubscriptionsSealed) {
          return;
        }
        await _ensureReady();
        if (tracked.cancelRequested || _dartSubscriptionsSealed || _closing) {
          return;
        }
        final created = await _bridgeCall(
          () => frb_thread_sub.subscribeThread(threadId: threadId),
        );
        // await create 期间关闭：句柄交给 owner 的 release 闭包，由 registry 持有并等
        // cancel 完成，绝不在这里自行 dispose 后误报 settled。
        handle = created;
        if (tracked.cancelRequested || _dartSubscriptionsSealed || _closing) {
          return;
        }
        subscription = created.threadStream().listen(
          (envelope) => envelope.when(
            data: (update) => controller.add(_threadStreamFrameFromFrb(update)),
            failure: (error) => controller.addError(_studioFailure(error)),
            closed: controller.close,
          ),
          onError: (Object error, StackTrace stackTrace) =>
              controller.addError(_studioFailure(error), stackTrace),
          onDone: controller.close,
        );
      } catch (error, stackTrace) {
        if (!tracked.cancelRequested) {
          controller.addError(_studioFailure(error), stackTrace);
          await controller.close();
        }
      } finally {
        // 封闭/退出中：即使本 entry 尚未收到取消请求，也启动唯一 owned cancellation，
        // 保证任何已建立（或迟到）的 native 句柄都被释放，绝不让 owner 逃逸 settle 检查。
        if (_dartSubscriptionsSealed || _closing) {
          unawaited(tracked.beginCancel());
        }
        // 无论是否创建成功都完成创建闩：取消不必永远等待一个不会发生的创建。
        tracked.markCreated();
      }
    }

    controller = StreamController<ThreadStreamFrame>(
      onListen: () => unawaited(start()),
      onCancel: () async {
        try {
          await tracked.beginCancel();
        } on Object {
          // 具体（脱敏）错因与堆栈已由 owner 保留，用于关闭报告的同步诊断；onCancel
          // 不抛出，避免未 await 的 cancel 变成未处理异步错误。失败即保留 pending owner。
        }
      },
    );
    return controller.stream;
  }

  @override
  Future<ThreadActivityDetail> readThreadActivityDetail(
    String threadId,
    String activityId,
  ) async {
    await _ensureReady();
    return _activityDetailFromFrb(
      await _bridgeCall(
        () => frb.readThreadActivityDetail(
          threadId: threadId,
          activityId: activityId,
        ),
      ),
    );
  }

  @override
  Future<StudioChatWindow> openChatWindow(String threadId) async {
    await _ensureReady();
    final view = await _bridgeCall(
      () => frb_chat.openChatView(
        threadId: threadId,
        focus: const frb_chat_types.BridgeChatFocus.latest(),
      ),
    );
    return FrbChatWindow(view);
  }

  @override
  Future<SubmitPromptReceipt> submitPrompt(
    String threadId,
    StudioPromptInput input, {
    required int expectedModelRouteRevision,
  }) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.submitPrompt(
        threadId: threadId,
        input: _bridgePromptInput(input),
        expectedModelRouteRevision: BigInt.from(expectedModelRouteRevision),
      ),
    );
    return SubmitPromptReceipt(
      threadId: response.threadId,
      inputId: response.inputId,
      cursor: response.revision.toInt(),
    );
  }

  @override
  Future<List<AttachmentDraftView>> admitAttachmentDrafts(
    AttachmentAdmissionContext context,
    List<AttachmentDraftSource> sources,
  ) async {
    await _ensureReady();
    final drafts = await _bridgeCall(
      () => frb_attachment.admitAttachmentDrafts(
        context: switch (context) {
          ExistingThreadAttachmentAdmissionContext(:final threadId) =>
            frb_attachment_types
                .BridgeAttachmentAdmissionContext.existingThread(
              threadId: threadId,
            ),
          NewThreadAttachmentAdmissionContext(:final mode) =>
            frb_attachment_types.BridgeAttachmentAdmissionContext.newThread(
              mode: mode.id,
            ),
        },
        sources: [
          for (final source in sources)
            switch (source) {
              LocalFileAttachmentDraftSource(:final path) =>
                frb_attachment_types.BridgeAttachmentDraftSource.localFile(
                  path: path,
                ),
              RemoteUrlAttachmentDraftSource(:final url, :final filename) =>
                frb_attachment_types.BridgeAttachmentDraftSource.remoteUrl(
                  url: url,
                  filename: filename,
                ),
            },
        ],
      ),
    );
    return [for (final draft in drafts) _attachmentDraftFromFrb(draft)];
  }

  @override
  Future<bool> removeAttachmentDraft(String draftId) async {
    await _ensureReady();
    return _bridgeCall(
      () => frb_attachment.removeAttachmentDraft(draftId: draftId),
    );
  }

  @override
  Future<Uint8List> readAttachmentDraft(String draftId) async {
    await _ensureReady();
    return _bridgeCall(
      () => frb_attachment.readAttachmentDraft(draftId: draftId),
    );
  }

  @override
  Future<Uint8List> readThreadAttachment(
    String threadId,
    String attachmentId,
  ) async {
    await _ensureReady();
    return _bridgeCall(
      () => frb_attachment.readThreadAttachment(
        threadId: threadId,
        attachmentId: attachmentId,
      ),
    );
  }

  @override
  Future<SettingsCommandReply> saveRuntimePermissionMode(
    int expectedSettingsRevision,
    PermissionMode mode, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    return _settingsCommandReplyFromFrb(
      await _bridgeCall(
        () => frb.saveRuntimePermissionMode(
          operationId: operationId.value,
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          mode: _permissionModeLabel(mode),
        ),
      ),
    );
  }

  @override
  Future<SettingsCommandReply> saveProvider(
    int expectedSettingsRevision,
    ProviderCommand command, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    return _settingsCommandReplyFromFrb(
      await _bridgeCall(
        () => frb.saveProvider(
          operationId: operationId.value,
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          input: frb.ProviderSettingsInput(
            provider: _providerInputFromCommand(command),
          ),
        ),
      ),
    );
  }

  @override
  Future<SettingsCommandReply> setDefaultProvider(
    int expectedSettingsRevision,
    String providerId, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    return _settingsCommandReplyFromFrb(
      await _bridgeCall(
        () => frb.setDefaultProvider(
          operationId: operationId.value,
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          providerId: providerId,
        ),
      ),
    );
  }

  @override
  Future<SettingsCommandReply> removeProviderModel(
    int expectedSettingsRevision,
    String providerId,
    String slug,
  ) async {
    await _ensureReady();
    return _settingsCommandReplyFromFrb(
      await _bridgeCall(
        () => frb.removeProviderModel(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          providerId: providerId,
          slug: slug,
        ),
      ),
    );
  }

  @override
  Future<SettingsCommandReply> removeProvider(
    int expectedSettingsRevision,
    String providerId, {
    required OperationId operationId,
    String? replacementProviderId,
  }) async {
    await _ensureReady();
    return _settingsCommandReplyFromFrb(
      await _bridgeCall(
        () => frb.removeProvider(
          operationId: operationId.value,
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          providerId: providerId,
          input: frb.RemoveProviderInput(
            replacementProviderId: replacementProviderId,
          ),
        ),
      ),
    );
  }

  @override
  Future<SettingsCommandReply> applySettingsField(
    int expectedSettingsRevision,
    SettingsFieldCommand command, {
    required OperationId operationId,
  }) async {
    await _ensureReady();
    return _settingsCommandReplyFromFrb(
      await _bridgeCall(
        () => frb.applySettingsField(
          operationId: operationId.value,
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          input: _settingsFieldInputFromDomain(command),
        ),
      ),
    );
  }

  @override
  Future<SettingsStateSnapshot> readSettingsState() async {
    await _ensureReady();
    return _settingsStateFromFrb(await _bridgeCall(frb.readSettingsState));
  }

  @override
  Future<ProviderUsageStateSnapshot> checkProviderUsage(
    String providerId,
  ) async {
    await _ensureReady();
    return _providerUsageStateFromFrb(
      await _bridgeCall(() => frb.checkProviderUsage(providerId: providerId)),
    );
  }

  @override
  Future<SkillsStateSnapshot> readSkillsState(String projectId) async {
    await _ensureReady();
    return _skillsStateFromFrb(
      await _bridgeCall(() => frb.readSkillsState(projectId: projectId)),
    );
  }

  @override
  Future<SkillsStateSnapshot> discoverSkills(String projectId) async {
    await _ensureReady();
    return _skillsStateFromFrb(
      await _bridgeCall(() => frb.discoverSkills(projectId: projectId)),
    );
  }

  @override
  Future<SkillSearchResultView> searchSkills(
    String projectId,
    String query, {
    int limit = 50,
  }) async {
    await _ensureReady();
    return _skillSearchResultFromFrb(
      await _bridgeCall(
        () =>
            frb.searchSkills(projectId: projectId, query: query, limit: limit),
      ),
    );
  }

  @override
  Future<McpStateSnapshot> readMcpState() async {
    await _ensureReady();
    return _mcpStateFromFrb(await _bridgeCall(frb.readMcpState));
  }

  @override
  Future<McpStateSnapshot> resetMcpServer(String serverId) async {
    await _ensureReady();
    return _mcpStateFromFrb(
      await _bridgeCall(
        () => frb.resetMcp(input: frb.McpResetInput.server(serverId: serverId)),
      ),
    );
  }

  @override
  Future<McpStateSnapshot> resetAllMcp() async {
    await _ensureReady();
    return _mcpStateFromFrb(
      await _bridgeCall(
        () => frb.resetMcp(input: const frb.McpResetInput.all()),
      ),
    );
  }

  @override
  Future<LspStateSnapshot> readLspState() async {
    await _ensureReady();
    return _lspStateFromFrb(await _bridgeCall(frb.readLspState));
  }

  @override
  Future<LspStateSnapshot> probeLspServer(String projectId) async {
    await _ensureReady();
    return _lspStateFromFrb(
      await _bridgeCall(() => frb.probeLspServer(projectId: projectId)),
    );
  }

  @override
  Future<LspStateSnapshot> repairLspServer(
    String projectId,
    String serverId,
  ) async {
    await _ensureReady();
    return _lspStateFromFrb(
      await _bridgeCall(
        () => frb.repairLspServer(projectId: projectId, serverId: serverId),
      ),
    );
  }

  @override
  Future<LspStateSnapshot> resetLspServer(
    String projectId,
    String serverId,
  ) async {
    await _ensureReady();
    return _lspStateFromFrb(
      await _bridgeCall(
        () => frb.resetLsp(
          input: frb.LspScopeInput.server(
            projectId: projectId,
            serverId: serverId,
          ),
        ),
      ),
    );
  }

  @override
  Future<LspStateSnapshot> resetLspWorkspace(String projectId) async {
    await _ensureReady();
    return _lspStateFromFrb(
      await _bridgeCall(
        () => frb.resetLsp(
          input: frb.LspScopeInput.workspace(projectId: projectId),
        ),
      ),
    );
  }
}

/// DTO 到领域的一次转换：数据层消费 typed 报告，界面只读领域形状。
StudioShutdownReport _shutdownReportFromFrb(
  frb_shutdown.BridgeShutdownReport report,
) {
  return StudioShutdownReport(
    outcome: switch (report.outcome) {
      frb_shutdown.BridgeShutdownOutcome.notStarted =>
        StudioShutdownOutcome.notStarted,
      frb_shutdown.BridgeShutdownOutcome.clean => StudioShutdownOutcome.clean,
      frb_shutdown.BridgeShutdownOutcome.degraded =>
        StudioShutdownOutcome.degraded,
    },
    issues: [
      for (final issue in report.issues)
        StudioShutdownIssue(
          stage: issue.stage,
          code: issue.code,
          message: issue.message,
          retryable: issue.retryable,
          correlationId: issue.correlationId,
        ),
    ],
    persistence: switch (report.persistence) {
      frb_shutdown.BridgePendingPersistence_Unknown() =>
        const UnknownStudioPendingPersistence(),
      frb_shutdown.BridgePendingPersistence_Pending(:final count) =>
        PendingStudioPendingPersistence(count: count.toInt()),
      frb_shutdown.BridgePendingPersistence_Drained() =>
        const DrainedStudioPendingPersistence(),
    },
  );
}
