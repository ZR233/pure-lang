import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import '../data/frb/studio_api.dart';
import '../domain/models/studio_models.dart';
import '../platform/error_log.dart';
import '../shared/studio_driver_state.dart';
import 'studio_shutdown.dart';

/// 原生宿主退出期限的默认总长（native 尚未应答或通道缺失时的兜底）。
const int _defaultDeadlineMs = 30000;

/// 保留给诊断退出的时间；清理预算 = 剩余期限 - 本保留量。
const int _diagnosticsReserveMs = 2000;

/// 清理预算上限（28 秒）。
const int _maxCleanupMs = 28000;

/// 唯一 native <-> Dart 退出契约通道。
///
/// 责任边界（见 design/18 §18.5、design/19）：
///  - `beginExit` 首次武装 native 硬期限并返回剩余毫秒；重复调用不刷新，因此重复关窗
///    与同一协调器共享同一条总期限。
///  - `updateExitDiagnostics` 只传小脱敏字段（stage / code / correlationId /
///    pendingCommits），不传正文或凭据。
///  - `finishExit` 结束本实例并带真实退出码。
///  - `configureDiagnostics` 提前把 canonical 日志目录交给 native 兜底写盘。
///  - native -> Dart 的 `requestExit` 回调把窗口关闭交回同一协调器。
abstract final class StudioHostLifecycle {
  static const MethodChannel channel = MethodChannel(
    'io.github.zr233.anywork/host_lifecycle',
  );

  /// 首次武装硬期限并返回剩余毫秒；通道缺失时回落到默认总期限。
  static Future<int> beginExit() async {
    try {
      final remaining = await channel
          .invokeMethod<int>('beginExit')
          .timeout(const Duration(seconds: 2));
      return remaining ?? _defaultDeadlineMs;
    } on Object catch (error, stackTrace) {
      debugPrint('host_lifecycle_begin_exit_failed=$error\n$stackTrace');
      recordDartError(
        error,
        stackTrace,
        stage: 'host-begin-exit',
        correlationId: newStudioCorrelationId(),
      );
      return _defaultDeadlineMs;
    }
  }

  /// 上报一小段脱敏诊断；失败只记录，不阻断退出。
  static Future<void> updateExitDiagnostics({
    required String stage,
    String? code,
    String? correlationId,
    int? pendingCommits,
    Duration timeout = const Duration(seconds: 2),
  }) async {
    final call = channel.invokeMethod<void>(
      'updateExitDiagnostics',
      <String, Object?>{
        'stage': stage,
        'code': ?code,
        'correlationId': ?correlationId,
        'pendingCommits': ?pendingCommits,
      },
    );
    if (timeout <= Duration.zero) {
      // 期限已尽：仍把最后一小段脱敏诊断发给 native，但不再等待应答。
      unawaited(call.catchError((Object _) {}));
      return;
    }
    try {
      await call.timeout(timeout);
    } on Object catch (error, stackTrace) {
      debugPrint('host_lifecycle_update_failed=$error\n$stackTrace');
      recordDartError(
        error,
        stackTrace,
        stage: 'host-update-diagnostics',
        correlationId: newStudioCorrelationId(),
      );
    }
  }

  /// 结束本实例：native 会以给定退出码终止进程；通道缺失时仅记录。
  static Future<void> finishExit({
    required int exitCode,
    Duration timeout = const Duration(seconds: 2),
  }) async {
    final call = channel.invokeMethod<void>('finishExit', <String, Object?>{
      'exitCode': exitCode,
    });
    if (timeout <= Duration.zero) {
      // 期限已尽：仍发出收尾消息，但不再等待应答；native 硬期限兜底终止本进程。
      unawaited(call.catchError((Object _) {}));
      return;
    }
    try {
      // native 正常会终止本进程；超时只记录，仍由 native 硬期限兜底。
      await call.timeout(timeout);
    } on Object catch (error, stackTrace) {
      debugPrint('host_lifecycle_finish_failed=$error\n$stackTrace');
      recordDartError(
        error,
        stackTrace,
        stage: 'host-finish-exit',
        correlationId: newStudioCorrelationId(),
      );
    }
  }

  /// 把 canonical 日志目录提前交给 native，失败只记录。
  static Future<void> configureDiagnostics() async {
    final directory = studioLogDirectory();
    if (directory == null) return;
    try {
      await channel
          .invokeMethod<void>('configureDiagnostics', <String, Object?>{
            'directory': directory,
          })
          .timeout(const Duration(seconds: 2));
    } on Object catch (error, stackTrace) {
      debugPrint('host_lifecycle_configure_failed=$error\n$stackTrace');
      recordDartError(
        error,
        stackTrace,
        stage: 'host-configure-diagnostics',
        correlationId: newStudioCorrelationId(),
      );
    }
  }

  /// 安装 native -> Dart 的退出回调。重复安装会替换旧回调。
  static void onRequestExit(Future<void> Function() handler) {
    channel.setMethodCallHandler((call) async {
      if (call.method == 'requestExit') {
        await handler();
      }
    });
  }
}

/// 进程内唯一的退出协调器：把窗口关闭、ServicesBinding、Driver 与 dispose 归一到
/// 同一条总期限与同一个收束 future，绝不伪 `Stopped`、不进入 retry 循环。
class StudioExitCoordinator {
  StudioExitCoordinator(
    this._api,
    this._onProgress, {
    this.onFailure,
    this.shutdownOverride,
  });

  final StudioApi _api;
  final void Function(StudioShutdownProgress progress) _onProgress;

  /// 失败时呈现「正在结束 / 必要诊断」；不得向已销毁的 notifier 写状态。
  final void Function(Object error)? onFailure;

  /// 注入式清理（测试/自定义），成功即视为 Clean；默认走 [runStudioShutdown]。
  final Future<void> Function()? shutdownOverride;

  static StudioExitCoordinator? _active;
  static Future<void>? _exitFuture;
  // 单一绝对 Dart 期限：首次武装时锚定，与 native 同一条 30s 总期限。用单调
  // Stopwatch 而非墙钟，系统时间调整不会延长 duration；重复关窗 / dispose / driver
  // 只复用该期限，绝不复位、绝不延长。
  static Stopwatch? _exitClock;
  static int _exitAnchorRemainingMs = 0;

  /// 绑定当前进程的协调器并安装 native 退出回调。
  static void install(StudioExitCoordinator coordinator) {
    _active = coordinator;
    StudioHostLifecycle.onRequestExit(coordinator._requestExit);
    unawaited(StudioHostLifecycle.configureDiagnostics());
  }

  /// 请求退出：重复调用共享同一条 future 与同一条 native 期限。
  static Future<void> requestExit() {
    final active = _active;
    if (active == null) return Future<void>.value();
    return active._requestExit();
  }

  /// Driver 使用的 typed 清理：arm native deadline 并返回报告，不结束进程。
  Future<StudioShutdownReport> cleanupOnly() async {
    final budget = await _armAndComputeBudget();
    // 清理必须有总上限：即使桥不返回，也按同一绝对期限截断为 Degraded。
    StudioShutdownReport report;
    try {
      report = await _runCleanupBounded(budget);
    } on Object catch (error, stackTrace) {
      // 同一次失败只在报出 issue 的地方生成一次 correlation，并配套脱敏同步诊断。
      final correlationId = _correlationFor(error);
      debugPrint('studio_exit_cleanup_failed=$error\n$stackTrace');
      recordDartError(
        error,
        stackTrace,
        stage: 'cleanup',
        correlationId: correlationId,
        elapsedMs: _elapsedMs(),
      );
      report = StudioShutdownReport(
        outcome: StudioShutdownOutcome.degraded,
        issues: [
          StudioShutdownIssue(
            stage: 'cleanup',
            code: _codeFor(error),
            message: 'studio shutdown reported an error',
            retryable: false,
            correlationId: correlationId,
          ),
        ],
        persistence: const UnknownStudioPendingPersistence(),
      );
    }
    StudioDriverState.publishShutdownReport(report);
    // typed 清理只在可靠 Clean 时才投影终态；degraded 绝不伪 Stopped。
    if (report.isClean) {
      StudioDriverState.publishShutdownProgress(const StoppedProgress());
    }
    return report;
  }

  Future<void> _requestExit() {
    // 单一 shared future：重复关窗 / dispose / driver 复用同一次收束，期限不刷新。
    StudioDriverState.markExitRequested();
    final running = _exitFuture;
    if (running != null) return running;
    final attempt = _drive();
    _exitFuture = attempt;
    return attempt;
  }

  Future<void> _drive() async {
    final budget = await _armAndComputeBudget();
    StudioShutdownReport report;
    Object? failure;
    StackTrace? stackTrace;
    String? cleanupCorrelationId;
    try {
      report = await _runCleanupBounded(budget);
    } on Object catch (error, stack) {
      failure = error;
      stackTrace = stack;
      // 同一次失败只生成一次 correlation：report issue 与同步诊断共用同一编号。
      cleanupCorrelationId = _correlationFor(error);
      // 不丢弃已有阶段问题：把桥错误本身与关闭阶段的真实事实一起如实上报为 Degraded。
      report = StudioShutdownReport(
        outcome: StudioShutdownOutcome.degraded,
        issues: [
          StudioShutdownIssue(
            stage: 'cleanup',
            code: _codeFor(error),
            message: 'studio shutdown reported an error',
            retryable: false,
            correlationId: cleanupCorrelationId,
          ),
        ],
        persistence: const UnknownStudioPendingPersistence(),
      );
    }
    var reportedFailure = false;
    if (failure != null) {
      final correlationId = cleanupCorrelationId ?? _correlationFor(failure);
      debugPrint('studio_exit_cleanup_failed=$failure\n$stackTrace');
      recordDartError(
        failure,
        stackTrace,
        stage: 'cleanup',
        correlationId: correlationId,
        elapsedMs: _elapsedMs(),
      );
      // 展示「正在结束 / 必要诊断」，但不进入 retry 循环或确认对话框。
      onFailure?.call(failure);
      reportedFailure = true;
    }
    StudioDriverState.publishShutdownReport(report);
    await _reportTerminalDiagnostics(report);
    // 最终报告（含 Dart 订阅/stream 收束）不是可靠 Clean 时撤销安全 dispose，绝不
    // 在仍有订阅或事件循环占用时释放 RustLib 把正常关闭拖成 watchdog degraded。
    if (!report.allowsCleanExit) {
      FrbStudioApi.revokeSafeDispose();
    }
    // 独立诊断收尾：不依赖 runtime 是否安装，也不依赖上面的清理是否成功。
    await _runBounded(
      FrbStudioApi.finishShutdownDiagnostics,
      _remainingDiagnosticsDuration(),
    );
    // 只有全部关闭报告（含 Dart 订阅 / 诊断清理）确认可靠后才发布终态 `Stopped`；
    // 早于 Dart 侧收束发布正是「伪 Stopped」的来源。
    if (report.isClean) {
      _onProgress(const StoppedProgress());
      StudioDriverState.publishShutdownProgress(const StoppedProgress());
    } else if (!report.allowsCleanExit && !reportedFailure) {
      onFailure?.call(_reportFailure(report));
    }
    await StudioHostLifecycle.finishExit(
      exitCode: report.allowsCleanExit ? 0 : 1,
      timeout: _remainingDiagnosticsDuration(),
    );
  }

  Future<int> _armAndComputeBudget() async {
    if (_exitClock == null) {
      // 首次：native 武装唯一绝对期限并返回剩余毫秒；用单调 Stopwatch 锚定。
      // Stopwatch 在 beginExit 之前启动，因此往返耗时被保守地从预算中扣除。
      final clock = Stopwatch()..start();
      final remaining = await StudioHostLifecycle.beginExit();
      _exitClock = clock;
      _exitAnchorRemainingMs = remaining;
    } else {
      // 重复请求：native 只返回缩短后的剩余，绝不刷新；Dart 侧期限已锚定。
      await StudioHostLifecycle.beginExit();
    }
    await StudioHostLifecycle.updateExitDiagnostics(stage: 'cleanup');
    // 用同一绝对期限扣除包括诊断握手在内的全部实际耗时，剩下的才是清理预算。
    final budget = _remainingCleanupBudget();
    StudioDriverState.publishNativeExitBudget(
      remainingMs: _remainingUntilDeadlineMs(),
      cleanupBudgetMs: budget,
    );
    return budget;
  }

  /// 距离锚定绝对期限的剩余毫秒；未武装时回落到默认总期限。
  int _remainingUntilDeadlineMs() {
    final clock = _exitClock;
    if (clock == null) return _defaultDeadlineMs;
    final remaining = _exitAnchorRemainingMs - clock.elapsedMilliseconds;
    return remaining > 0 ? remaining : 0;
  }

  /// 距首次武装已消耗的毫秒；未武装时为 null（无可关联的期限进度）。
  int? _elapsedMs() => _exitClock?.elapsedMilliseconds;

  /// 清理预算 = 剩余期限 - 诊断预留，并按上限收紧；随时反映已消耗时间。
  int _remainingCleanupBudget() {
    final remaining = _remainingUntilDeadlineMs();
    if (remaining <= _diagnosticsReserveMs) return 0;
    final budget = remaining - _diagnosticsReserveMs;
    return budget > _maxCleanupMs ? _maxCleanupMs : budget;
  }

  /// 收尾诊断可用时间：以锚定绝对期限为准逐项收紧，绝不累加超过预留与总期限。
  Duration _remainingDiagnosticsDuration() {
    final remaining = _remainingUntilDeadlineMs();
    return remaining <= 0 ? Duration.zero : Duration(milliseconds: remaining);
  }

  /// 有界清理：按剩余预算截断；超时是真实的 [StudioShutdownOutcome.degraded]
  /// （保存事实未知），绝不抛出异常覆盖可靠的 runtime 报告。
  Future<StudioShutdownReport> _runCleanupBounded(int budget) {
    final timeout = Duration(milliseconds: budget <= 0 ? 1 : budget);
    return _runCleanup(budget).timeout(
      timeout,
      onTimeout: () => StudioShutdownReport(
        outcome: StudioShutdownOutcome.degraded,
        issues: [
          StudioShutdownIssue(
            stage: 'cleanup',
            code: 'shutdownTimeout',
            message: 'shutdown exceeded its cleanup budget',
            retryable: false,
            correlationId: newStudioCorrelationId(),
          ),
        ],
        persistence: const UnknownStudioPendingPersistence(),
      ),
    );
  }

  Future<StudioShutdownReport> _runCleanup(int budget) async {
    // 关闭路径绝不 ensureReady / retryInitialization：只收束既有 owner。
    final override = shutdownOverride;
    if (override != null) {
      await override();
      return const StudioShutdownReport(
        outcome: StudioShutdownOutcome.clean,
        issues: [],
        persistence: DrainedStudioPendingPersistence(),
      );
    }
    return runStudioShutdown(_api, _onProgress, remainingMs: budget);
  }

  Future<void> _reportTerminalDiagnostics(StudioShutdownReport report) async {
    final pending = switch (report.persistence) {
      UnknownStudioPendingPersistence() => null,
      PendingStudioPendingPersistence(:final count) => count,
      DrainedStudioPendingPersistence() => 0,
    };
    final issue = report.issues.isEmpty ? null : report.issues.first;
    await StudioHostLifecycle.updateExitDiagnostics(
      stage: 'finalizing',
      code: issue?.code,
      correlationId: issue?.correlationId,
      pendingCommits: pending,
      timeout: _remainingDiagnosticsDuration(),
    );
  }

  /// 稳定诊断 code：typed 失败用其领域 code，其余归为 `unexpected`。
  String _codeFor(Object error) => switch (error) {
    StudioFailure(:final code) => code.name,
    TimeoutException() => 'timeout',
    _ => 'unexpected',
  };

  /// 非空 correlation：优先保留桥错误自带的 id，否则生成本地 id。
  String _correlationFor(Object error) =>
      error is StudioFailure && error.correlationId.isNotEmpty
      ? error.correlationId
      : newStudioCorrelationId();

  /// 给失败卡一个只含允许诊断字段的 typed 呈现（不塞正文/凭据）。
  StudioFailure _reportFailure(StudioShutdownReport report) {
    final issue = report.issues.isEmpty ? null : report.issues.first;
    return StudioFailure(
      code: StudioFailureCode.internal,
      message: issue?.message ?? 'studio shutdown did not complete cleanly',
      retryable: false,
      correlationId: issue?.correlationId ?? newStudioCorrelationId(),
    );
  }
}

/// 有界执行一个 best-effort 收尾动作：任何错误都只记录，不阻塞退出。
Future<void> _runBounded(
  Future<void> Function() action,
  Duration budget,
) async {
  if (budget <= Duration.zero) {
    // 期限已尽：只发出动作，不再等待；native 硬期限保证进程终止。
    unawaited(action().catchError((Object _) {}));
    return;
  }
  try {
    await action().timeout(budget);
  } on Object catch (error, stackTrace) {
    debugPrint('studio_exit_bounded_step_failed=$error\n$stackTrace');
    recordDartError(
      error,
      stackTrace,
      stage: 'exit-bounded-step',
      correlationId: newStudioCorrelationId(),
    );
  }
}
