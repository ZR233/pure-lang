// Driver-only 故障注入复用既有 @visibleForTesting 初始化 override，不新增生产 fault API。
// ignore_for_file: invalid_use_of_visible_for_testing_member

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:anywork/main.dart' as studio;
import 'package:anywork/src/app/studio_host_lifecycle.dart';
import 'package:anywork/src/app/studio_shutdown.dart';
import 'package:anywork/src/data/frb/studio_api.dart';
import 'package:anywork/src/data/repositories/studio_repository.dart';
import 'package:anywork/src/domain/models/studio_models.dart';
import 'package:anywork/src/platform/error_log.dart';
import 'package:anywork/src/shared/studio_driver_state.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';
import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'raw_tap_extension.dart';
import 'pointer_scroll_extension.dart';
import 'scrollbar_drag_extension.dart';
import 'key_press_extension.dart';
import 'tool_probe_extension.dart';

/// Native-only Driver entrypoint. Product and release builds use lib/main.dart.
void main() {
  if (const bool.fromEnvironment('dart.vm.product')) {
    throw StateError('Flutter Driver mode is unavailable in product builds');
  }
  enableFlutterDriverExtension(
    handler: _handleDriverData,
    commands: <CommandExtension>[
      RawTapCommandExtension(),
      PointerScrollCommandExtension(),
      ScrollbarDragCommandExtension(),
      KeyPressCommandExtension(),
      ToolProbeExtension(),
    ],
  );
  SchedulerBinding.instance.addTimingsCallback(_recordFrameTimings);
  _container = ProviderContainer();
  _applyShutdownFaultOverride();
  studio.bootstrapStudio(container: _container);
  // 与窗口关闭共用同一退出协调器：native -> Dart 的 requestExit 也进入该协调器。
  StudioExitCoordinator.install(
    StudioExitCoordinator(
      _container.read(studioBridgeDataSourceProvider),
      _container.read(studioShutdownProgressStateProvider.notifier).update,
    ),
  );
}

/// Driver-only 初始化故障注入：`ANYWORK_DRIVER_SHUTDOWN_FAULT` 取值
/// `pending-init`（初始化永久挂住）或 `bridge-load-error`（初始化显式抛错）。
///
/// 复用既有 [FrbStudioBridgeDataSource.debugOverrideInitialization]，不新增生产 fault 接口；订阅
/// 故障由验收侧独立 Driver 入口/装饰器提供（见 ui-handoff-r3.txt）。
void _applyShutdownFaultOverride() {
  final fault = Platform.environment['ANYWORK_DRIVER_SHUTDOWN_FAULT'];
  if (fault == 'pending-init') {
    FrbStudioBridgeDataSource.debugOverrideInitialization(
      () => Completer<void>().future,
    );
  } else if (fault == 'bridge-load-error') {
    FrbStudioBridgeDataSource.debugOverrideInitialization(() async {
      throw StateError('driver-injected bridge load failure');
    });
  }
}

late final ProviderContainer _container;
Future<StudioShutdownReport>? _shutdownTask;
Future<void>? _driverFixtureClipboardImageTask;
ProviderSubscription<ProductTopicLeaseBundle?>? _driverStatisticsScope;
ProviderSubscription<ProductTopicLease?>? _driverQueueScope;
bool _recordingFrames = false;
int _frameCount = 0;
int _slowFrames = 0;
int _verySlowFrames = 0;
int _maxFrameMicros = 0;
final List<Map<String, num>> _frameSamples = [];

void _ensureDriverStatisticsScope() {
  _driverStatisticsScope ??= _container.listen<ProductTopicLeaseBundle?>(
    settingsStatisticsScopeProvider,
    (_, _) {},
    fireImmediately: true,
  );
}

void _ensureDriverQueueScope() {
  _driverQueueScope ??= _container.listen<ProductTopicLease?>(
    persistenceQueueTopicProvider,
    (_, _) {},
    fireImmediately: true,
  );
}

void _recordFrameTimings(List<FrameTiming> timings) {
  if (!_recordingFrames) return;
  final completedAt = DateTime.now().millisecondsSinceEpoch;
  for (final timing in timings) {
    final elapsed = timing.totalSpan.inMicroseconds;
    _frameCount++;
    if (elapsed > 16667) _slowFrames++;
    if (elapsed > 33333) _verySlowFrames++;
    if (elapsed > _maxFrameMicros) _maxFrameMicros = elapsed;
    _frameSamples.add({
      'completedUnixMillis': completedAt,
      'totalMillis': elapsed / 1000,
      'vsyncOverheadMillis': timing.vsyncOverhead.inMicroseconds / 1000,
      'buildMillis': timing.buildDuration.inMicroseconds / 1000,
      'rasterMillis': timing.rasterDuration.inMicroseconds / 1000,
    });
  }
  if (_frameSamples.length > 8192) _frameSamples.removeRange(0, 4096);
}

Future<String> _handleDriverData(String? message) async {
  switch (message) {
    case 'frame-start':
      _frameCount = 0;
      _slowFrames = 0;
      _verySlowFrames = 0;
      _maxFrameMicros = 0;
      _frameSamples.clear();
      _recordingFrames = true;
      return jsonEncode({'recording': true});
    case 'frame-stop':
      _recordingFrames = false;
      return jsonEncode({
        'frames': _frameCount,
        'over16Millis': _slowFrames,
        'over33Millis': _verySlowFrames,
        'maxFrameMillis': _maxFrameMicros / 1000,
        'samples': _frameSamples,
      });
    case 'snapshot':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      if (state != null) StudioDriverState.publishState(state);
      return _driverSnapshotJson(state);
    case 'driver-diagnostics':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      return jsonEncode(_driverDiagnostics(state));
    case 'driver-fixture-clipboard-image':
      // This is deliberately not a system clipboard operation. There is no
      // Driver-only ClipboardImageReader override in the current app, so the
      // deterministic fixture hook starts the same controller admission path
      // without awaiting its preview continuation. The follow-up
      // `driver-fixture-clipboard-image-preview` request awaits that typed
      // Driver-only stage.
      return _startDriverFixtureClipboardImage();
    case 'driver-fixture-clipboard-image-preview':
      return _finishDriverFixtureClipboardImage();
    case 'pid':
      // Acceptance locates the X11 window by `_NET_WM_PID`, so the driver must
      // expose its own process id. Product builds use lib/main.dart, so this
      // stays inside the Driver-only entrypoint.
      return jsonEncode({'pid': pid});
    case 'statistics':
      // Driver 是显式观察者；产品会话不会因一次请求偷偷常驻全局统计租约。
      _ensureDriverStatisticsScope();
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      final performance = state?.modelPerformance;
      return jsonEncode({
        'revision': performance?.revision,
        'statisticsPending': performance?.statisticsPending,
        'statisticsGap': performance?.statisticsGap,
        'readFailed': performance?.readFailed,
        'summaries': [
          for (final summary in performance?.summaries ?? const [])
            {
              'providerInstanceId': summary.providerInstanceId,
              'model': summary.model,
              'effort': summary.reasoningEffort,
              'samples': summary.sampleCount,
              'tokens': summary.completionTokens,
              'tokensPerSecond': summary.tokensPerSecond,
            },
        ],
        'history': [
          for (final sample in performance?.history ?? const [])
            {
              'providerInstanceId': sample.providerInstanceId,
              'model': sample.model,
              'effort': sample.reasoningEffort,
              'tokens': sample.completionTokens,
              'ttftMillis': sample.ttftMillis,
              'decodeMillis': sample.decodeMillis,
              'responseMillis': sample.totalResponseMillis,
              'tokensPerSecond': sample.tokensPerSecond,
            },
        ],
      });
    case 'thread-current':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      final threadId = state?.selectedThreadId;
      if (threadId == null) return jsonEncode({'outputTokens': null});
      final snapshot = await _container
          .read(studioBridgeDataSourceProvider)
          .readThreadSnapshot(threadId);
      return jsonEncode({
        'outputTokens': snapshot.runtime.completionTokens,
        'revision': snapshot.revision,
      });
    case 'persistence-queue':
      // Driver 通过显式 topic lease 读取事件缓存，避免恢复已删除的 GUI 周期查询。
      _ensureDriverQueueScope();
      final queue = switch (_container.read(persistenceQueueStateProvider)) {
        AsyncData(:final value) => value?.queue,
        _ => null,
      };
      return jsonEncode({
        'pendingOperations': queue?.pendingOperations,
        'threads': [
          for (final thread in queue?.threads ?? const [])
            {
              'fault': thread.fault,
              'generation': thread.faultGeneration,
              'stateDirty': thread.stateDirtyRevision,
              'stateDurable': thread.stateDurableRevision,
              'historyAdmitted': thread.historyAdmittedSequence,
              'historyDurable': thread.historyDurableSequence,
              'pending': thread.pendingOperations,
              'error': thread.lastError,
            },
        ],
      });
    case 'load-older':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      final threadId = state?.selectedThreadId;
      if (threadId == null) return jsonEncode({'loaded': false});
      await _container
          .read(studioControllerProvider.notifier)
          .loadOlderHistory(threadId);
      return jsonEncode({'loaded': true});
    case 'selection-body':
      // Read-only: the rendered split of the long assistant body.
      return jsonEncode(<String, Object?>{'ok': true, ...?_renderedLongBody()});
    case 'selection-select-all':
      return _handleSelectAll();
    case 'selection-copy':
      return _handleCopySelected();
    case 'selection-state':
      return _handleSelectionState();
    case 'search-settings':
      return _handleSearchSettings();
    case 'search-mcp-state':
      return _handleSearchMcpState();
    case 'shutdown':
      try {
        final report = await (_shutdownTask ??= _runShutdown());
        // typed 清理 ack：成功才返 completed；degraded 如实上报，绝不伪 Stopped。
        return jsonEncode({
          'shutdown': report.allowsCleanExit ? 'completed' : 'degraded',
          ..._shutdownReportJson(report),
        });
      } on Object catch (error, stackTrace) {
        // 失败与堆栈不能只 debugPrint：落盘到 canonical 诊断日志。
        recordDartError(error, stackTrace);
        _shutdownTask = null;
        return jsonEncode({'shutdown': 'failed'});
      }
    case 'request-app-exit':
      // 先回复 exit_requested，再调度同一中央退出（arm deadline + 有界清理 + finishExit）。
      StudioDriverState.markExitRequested();
      Timer.run(() {
        unawaited(
          StudioExitCoordinator.requestExit().catchError((Object _) {}),
        );
      });
      return jsonEncode({'exit': 'exit_requested'});
    case 'arm-exit':
      // Driver-only：只固定 native 首次期限（beginExit），不清理、不退出。用于第一次
      // arm 后隔 5s 再 begin/repeat 再 hang，核对第一次 deadline 未被刷新。
      final armExitRemainingMs = await StudioHostLifecycle.beginExit();
      return jsonEncode({
        'armed': true,
        'remainingMs': armExitRemainingMs,
        'pid': pid,
      });
    case 'request-app-exit-twice':
      // 先武装 native 单期限并回报剩余，再间隔触发两次同一中央退出请求：第一次锚定
      // Dart 期限，第二次只复用同一 shared future；remaining 只减不增即证明不刷新。
      final twiceArmedRemainingMs = await StudioHostLifecycle.beginExit();
      StudioDriverState.markExitRequested();
      unawaited(StudioExitCoordinator.requestExit().catchError((Object _) {}));
      Timer(const Duration(milliseconds: 500), () {
        StudioDriverState.markExitRequested();
        unawaited(
          StudioExitCoordinator.requestExit().catchError((Object _) {}),
        );
      });
      return jsonEncode({
        'exit': 'exit_requested_twice',
        'armedRemainingMs': twiceArmedRemainingMs,
      });
    case 'shutdown-hang':
      // 仅 Driver 构建：先 arm deadline 并更新阶段，再阻塞 Dart isolate 并不返回 ack，
      // 验证 native 期限线程不依赖 Dart/bridge/mainloop 也能在到期强制结束本进程。
      // 验收以 OS pid 在 ~30s 内消失为证据，不依赖 requestData 是否拿到回复。
      await StudioHostLifecycle.beginExit();
      await StudioHostLifecycle.updateExitDiagnostics(stage: 'driver-hang');
      await StudioHostLifecycle.updateExitDiagnostics(
        stage: 'driver-hang-blocked',
      );
      sleep(const Duration(minutes: 5));
      return jsonEncode({'shutdown': 'hang-finished-unexpectedly'});
    case 'exit-status':
      // Driver-only 只读：暴露 native 剩余期限/清理预算/退出请求次数、typed 报告与
      // 已观察阶段、typed startup 状态/失败 code/correlation，供验收对齐 remaining/stage
      // 与 second-instance busy 判定，不触发任何退出动作。
      final diagnostics = StudioDriverState.exitDiagnosticsJson();
      final startup = diagnostics['startup'];
      return jsonEncode({
        ...diagnostics,
        'startup': {
          if (startup is Map<String, Object?>) ...startup,
          'phase': FrbStudioBridgeDataSource.startupProgress.value.name,
          'ownerPresent': FrbStudioBridgeDataSource.runtimeOwnerPresent,
        },
      });
    default:
      return jsonEncode({'error': 'unsupported driver request'});
  }
}

Future<StudioShutdownReport> _runShutdown() async {
  _driverStatisticsScope?.close();
  _driverStatisticsScope = null;
  _driverQueueScope?.close();
  _driverQueueScope = null;
  final api = _container.read(studioBridgeDataSourceProvider);
  final progress = _container.read(
    studioShutdownProgressStateProvider.notifier,
  );
  final coordinator = StudioExitCoordinator(api, progress.update);
  // arm native 首次期限并执行 typed 清理，返回真实报告；不结束进程。
  return coordinator.cleanupOnly();
}

Map<String, Object?> _shutdownReportJson(StudioShutdownReport report) {
  return {
    'outcome': report.outcome.name,
    'persistence': switch (report.persistence) {
      UnknownStudioPendingPersistence() => 'unknown',
      PendingStudioPendingPersistence(:final count) => 'pending:$count',
      DrainedStudioPendingPersistence() => 'drained',
    },
    'issues': [
      for (final issue in report.issues)
        {
          'stage': issue.stage,
          'code': issue.code,
          'message': issue.message,
          'retryable': issue.retryable,
          'correlationId': issue.correlationId,
        },
    ],
  };
}

/// Extends the existing Driver snapshot without changing any of its legacy
/// keys. The legacy snapshot shape is a deliberate backward-compatibility
/// exception for existing manual-gui consumers. `driverDiagnostics` is the new
/// acceptance-only surface and exposes only typed, de-identified fields; it is
/// assembled in this Driver entrypoint rather than in the production snapshot
/// owner and is not part of the production protocol.
String _driverSnapshotJson(StudioState? state) {
  final decoded = jsonDecode(StudioDriverState.snapshotJson());
  if (decoded is! Map) {
    throw const FormatException('driver snapshot must be a JSON object');
  }
  final snapshot = decoded.cast<String, dynamic>();
  snapshot['driverDiagnostics'] = _driverDiagnostics(state);
  return jsonEncode(snapshot);
}

Map<String, Object?> _driverDiagnostics(StudioState? state) {
  final workspace = state?.selectedWorkspace;
  final runtime = workspace?.runtime;
  final liveUsage = runtime?.liveUsage;
  final observedUsage =
      liveUsage != null &&
          liveUsage.turnId.isNotEmpty &&
          liveUsage.attemptId.isNotEmpty &&
          liveUsage.observationSequence >= 0
      ? liveUsage
      : null;
  final usageState = observedUsage?.state.name ?? 'unknown';
  final metricsAvailable =
      observedUsage != null &&
      (usageState == 'live' || usageState == 'authoritative');

  // ThreadRuntimeView stores these counters as non-nullable ints, while its
  // default constructor and FRB's unavailable context window both use zero.
  // That schema cannot distinguish an explicit zero from the default, so a
  // zero from those fields is conservatively unknown/null. The nullable live
  // usage fields do carry presence provenance; their explicit zero is kept.
  int? defaultProneCount(int? value) =>
      !metricsAvailable || value == null || value <= 0 ? null : value;
  int? explicitUsageCount(int? value) => metricsAvailable ? value : null;
  final contextTokens =
      explicitUsageCount(observedUsage?.latestContextTokens) ??
      defaultProneCount(runtime?.contextTokens);
  final contextWindow = defaultProneCount(runtime?.contextWindow);
  final totalTokens = defaultProneCount(runtime?.totalTokens);
  final inputTokens = defaultProneCount(runtime?.promptTokens);
  final outputTokens = defaultProneCount(runtime?.completionTokens);
  final reasoningTokens = defaultProneCount(runtime?.reasoningTokens);
  final liveOutputTokens = explicitUsageCount(observedUsage?.completionTokens);
  final turnOutputTokens =
      explicitUsageCount(observedUsage?.completionTokens) ??
      defaultProneCount(runtime?.turnCompletionTokens);
  final decodeMillis =
      explicitUsageCount(observedUsage?.decodeMillis) ??
      defaultProneCount(runtime?.turnDecodeMillis);
  // Preserve an explicitly observed decodeMillis == 0 in the source field,
  // but never derive a non-finite throughput value from it.
  final tokensPerSecond =
      turnOutputTokens != null && decodeMillis != null && decodeMillis > 0
      ? turnOutputTokens * 1000 / decodeMillis
      : null;
  return <String, Object?>{
    'schemaVersion': 1,
    'runtime': <String, Object?>{
      'availability': workspace == null ? 'unavailable' : 'available',
      // Null is an unknown/unavailable value. It is never replaced with a
      // zero-valued ThreadRuntimeView default merely to make the JSON shape
      // convenient. A usage state without a complete identity is downgraded
      // to unknown and all usage counters remain null.
      'usageState': usageState,
      'turnId': metricsAvailable ? observedUsage.turnId : null,
      'attemptId': metricsAvailable ? observedUsage.attemptId : null,
      'observationSequence': metricsAvailable
          ? observedUsage.observationSequence
          : null,
      'revision': workspace?.revision,
      'contextTokens': contextTokens,
      'contextWindow': contextWindow,
      'totalTokens': totalTokens,
      'inputTokens': inputTokens,
      'outputTokens': outputTokens,
      'reasoningTokens': reasoningTokens,
      'liveOutputTokens': liveOutputTokens,
      'turnOutputTokens': turnOutputTokens,
      'decodeMillis': decodeMillis,
      'tokensPerSecond': tokensPerSecond,
      // Keep throughput as an explicit typed group only when both source
      // counters are known; an unknown group is represented by null.
      'throughput': tokensPerSecond == null
          ? null
          : <String, Object?>{
              'outputTokens': turnOutputTokens,
              'decodeMillis': decodeMillis,
              'tokensPerSecond': tokensPerSecond,
            },
    },
    'composer': <String, Object?>{
      'newThread': _driverComposerDiagnostics(state?.newThreadComposer),
      'thread': _driverComposerDiagnostics(state?.composer),
    },
    'providers': _driverProviderDiagnostics(state),
    'topics': _driverTopicDiagnostics(state),
    'seams': <String, Object?>{
      // No production ClipboardImageReader override/fixture is present in this
      // Driver entrypoint. The fixture request below therefore does not claim
      // to prove a real OS paste operation.
      'clipboardImageReaderOverride': false,
      'pasteObservation': 'unavailable',
      'fixtureHook': 'driver-fixture-clipboard-image',
      'fixtureAdmissionHook': 'driver-fixture-clipboard-image',
      'fixturePreviewHook': 'driver-fixture-clipboard-image-preview',
      'fixturePreviewStage': 'typed-driver-only-follow-up',
      'fixtureAwaitsPreviewInAdmissionHook': false,
      'runtimeEventEndpoint': 'unsupported',
      'systemClipboardTouched': false,
    },
  };
}

Map<String, Object?>? _driverComposerDiagnostics(
  ComposerThreadState? composer,
) {
  if (composer == null) return null;
  final attachments = composer.attachments;
  return <String, Object?>{
    'present': attachments.isNotEmpty,
    'previewReady':
        attachments.isNotEmpty &&
        attachments.every(
          (attachment) => attachment.previewBytes?.isNotEmpty == true,
        ),
    'attachmentGeneration': composer.attachmentGeneration,
    'submissionPending': composer.isSubmissionPending,
    // Do not expose the error text: it can contain provider/path details. The
    // typed failure fact is sufficient for a Driver regression assertion.
    'failed': composer.error != null,
    'attachments': [
      for (final attachment in attachments)
        <String, Object?>{
          'id': attachment.id,
          'name': attachment.filename,
          'filename': attachment.filename,
          'modality': attachment.modality.name,
          'byteSize': attachment.byteSize,
          'previewReady': attachment.previewBytes?.isNotEmpty == true,
        },
    ],
  };
}

Map<String, Object?> _driverProviderDiagnostics(StudioState? state) {
  final settingsAvailable = state?.settingsState.state.value != null;
  if (!settingsAvailable) {
    return <String, Object?>{
      'availability': 'unavailable',
      'list': null,
      'details': null,
    };
  }
  final providers = state!.providers;
  return <String, Object?>{
    'availability': 'available',
    // The list projection deliberately contains count/status metadata only.
    // Model slugs are retained in the detail projection below.
    'list': [
      for (final provider in providers)
        <String, Object?>{
          'id': provider.id,
          'status': provider.status,
          'modelCount': provider.allModels.length,
          'defaultModelConfigured': provider.defaultModel.isNotEmpty,
        },
    ],
    'details': [
      for (final provider in providers)
        <String, Object?>{
          'id': provider.id,
          'models': [
            for (final model in provider.allModels)
              <String, Object?>{
                'slug': model.slug,
                'name': model.displayName,
                'contextWindow': model.contextWindow,
                'maxOutputTokens': model.maxOutputTokens,
                'reasoningEfforts': model.reasoningEfforts,
                'inputCapabilities': [
                  for (final capability in model.inputCapabilities)
                    capability.modality.name,
                ],
              },
          ],
        },
    ],
  };
}

Map<String, Object?> _driverTopicDiagnostics(StudioState? state) {
  if (state == null) {
    return <String, Object?>{
      'availability': 'unavailable',
      'registryAvailability': 'unavailable',
      'leaseRefs': null,
      'connections': null,
      'projectionRevisions': null,
    };
  }

  Map<String, int>? leaseRefs;
  try {
    leaseRefs = _container
        .read(studioControllerProvider.notifier)
        .activeTopicRefCountViews();
  } on Object {
    // During startup the controller can expose a canonical state before its
    // topic registry is ready. Keep it explicitly unavailable rather than
    // inventing zero refs for every topic.
  }
  final registryAvailable = leaseRefs != null;

  return <String, Object?>{
    'availability': registryAvailable ? 'available' : 'unavailable',
    'registryAvailability': registryAvailable ? 'available' : 'unavailable',
    'leaseRefs': registryAvailable ? _aggregateLeaseRefs(leaseRefs) : null,
    'connections': registryAvailable
        ? _aggregateTopicConnections(state.topicConnections)
        : null,
    'projectionRevisions': registryAvailable
        ? <String, Object?>{
            'settings': state.settingsState.revision,
            'modelCatalog': state.settingsState.modelCatalogRevision,
            'recovery': state.recoveryState.revision,
            'mcp': state.mcpState.revision,
            'lsp': state.lspState.revision,
            'threadModeCatalog': state.threadModeCatalog.revision,
            'providerUsage': state.providerUsageState.revision,
            'modelPerformance': state.modelPerformance.revision,
            'persistence': state.persistenceState.revision,
            'persistenceQueue': state.persistenceQueueState?.revision,
            'agentProfiles': state.agentProfilesState?.revision,
            'threadDirectory': state.threadDirectory.revision,
            'selectedThread': state.selectedWorkspace?.revision,
            // New diagnostics must not expose project/root/path identifiers.
            // Keep only aggregate scope facts; the legacy snapshot's
            // compatibility exception does not apply to this surface.
            'skills': {
              'scopeCount': state.skillsByProject.length,
              'maxRevision': _maxRevision(
                state.skillsByProject.values.map((value) => value.revision),
              ),
            },
            'sessionCosts': {
              'scopeCount': state.sessionCostsByRoot.length,
              'maxRevision': _maxRevision(
                state.sessionCostsByRoot.values.map((value) => value.revision),
              ),
            },
          }
        : null,
  };
}

Map<String, int> _aggregateLeaseRefs(Map<String, int> leaseRefs) {
  var totalRefs = 0;
  for (final refs in leaseRefs.values) {
    totalRefs += refs;
  }
  return <String, int>{'topicCount': leaseRefs.length, 'totalRefs': totalRefs};
}

Map<String, int> _aggregateTopicConnections(
  Map<ProductTopic, ProductTopicConnectionStateView> connections,
) {
  var connectedCount = 0;
  var reconnectingCount = 0;
  var failedCount = 0;
  var errorCount = 0;
  for (final connection in connections.values) {
    switch (connection.phase) {
      case ProductTopicConnectionPhase.connecting:
        break;
      case ProductTopicConnectionPhase.connected:
        connectedCount++;
      case ProductTopicConnectionPhase.reconnecting:
        reconnectingCount++;
      case ProductTopicConnectionPhase.failed:
        failedCount++;
    }
    if (connection.errorMessage != null) errorCount++;
  }
  return <String, int>{
    'topicCount': connections.length,
    'connectedCount': connectedCount,
    'reconnectingCount': reconnectingCount,
    'failedCount': failedCount,
    'errorCount': errorCount,
  };
}

int? _maxRevision(Iterable<int> revisions) {
  int? maximum;
  for (final revision in revisions) {
    if (maximum == null || revision > maximum) maximum = revision;
  }
  return maximum;
}

Future<String> _startDriverFixtureClipboardImage() async {
  final fixtureCandidates = [
    File('test_driver/fixtures/tool-image.png'),
    File('code/anywork/test_driver/fixtures/tool-image.png'),
  ];
  File? fixture;
  for (final candidate in fixtureCandidates) {
    if (await candidate.exists()) {
      fixture = candidate;
      break;
    }
  }
  if (fixture == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'hook': 'driver-fixture-clipboard-image',
      'pasteSeam': 'unavailable',
      'systemClipboardTouched': false,
      'reason': 'fixture-not-found',
    });
  }

  final state = _readyStudioState();
  if (state == null || state.selectedProjectId == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'hook': 'driver-fixture-clipboard-image',
      'pasteSeam': 'unavailable',
      'systemClipboardTouched': false,
      'reason': 'composer-not-ready',
    });
  }
  final task = _container
      .read(studioControllerProvider.notifier)
      .addClipboardImage(await fixture.readAsBytes());
  _driverFixtureClipboardImageTask = task;
  // The admission hook is intentionally fire-and-observe: it does not await
  // the controller's preview Future.wait. The journey observes the admitted
  // metadata first, then invokes the typed preview hook below.
  unawaited(
    task.then<void>((_) {}, onError: (Object _, StackTrace stackTrace) {}),
  );
  return jsonEncode(<String, Object?>{
    'ok': true,
    'hook': 'driver-fixture-clipboard-image',
    'stage': 'admission-started',
    'pasteSeam': 'unavailable',
    'systemClipboardTouched': false,
    'previewPending': true,
  });
}

Future<String> _finishDriverFixtureClipboardImage() async {
  final task = _driverFixtureClipboardImageTask;
  if (task == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'hook': 'driver-fixture-clipboard-image-preview',
      'stage': 'preview-unavailable',
      'pasteSeam': 'unavailable',
      'systemClipboardTouched': false,
      'reason': 'admission-not-started',
    });
  }
  try {
    await task;
    return jsonEncode(<String, Object?>{
      'ok': true,
      'hook': 'driver-fixture-clipboard-image-preview',
      'stage': 'preview-complete',
      'pasteSeam': 'unavailable',
      'systemClipboardTouched': false,
    });
  } catch (_) {
    // Do not expose raw provider/path errors through the Driver endpoint.
    return jsonEncode(<String, Object?>{
      'ok': false,
      'hook': 'driver-fixture-clipboard-image-preview',
      'stage': 'preview-failed',
      'pasteSeam': 'unavailable',
      'systemClipboardTouched': false,
    });
  }
}

// ---------------------------------------------------------------------------
// Web-search + MCP acceptance projection.
//
// Driver-only read-only view over the canonical settings/MCP state, so the
// web-search acceptance can prove the two search cards and the MCP health from
// typed fields instead of scraping widget text or the provider-only snapshot.
// Nothing here mutates product state and no credential is read: the endpoint is
// reduced to scheme/host/path.

StudioState? _readyStudioState() =>
    switch (_container.read(studioControllerProvider)) {
      AsyncData(:final value) => value,
      _ => null,
    };

Future<String> _handleSearchSettings() async {
  final state = _readyStudioState();
  if (state == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'studio state not ready',
    });
  }
  final web = state.settingsState.webSearch;
  final deep = state.settingsState.deepSeekWebSearch;
  return jsonEncode(<String, Object?>{
    'ok': true,
    'revision': state.settingsState.revision,
    'webSearch': <String, Object?>{
      'configuredMode': web.configuredMode,
      'effectiveMode': web.effectiveMode,
      'availability': web.availability,
      'contextSize': web.contextSize,
      'allowedDomains': web.allowedDomains,
      'country': web.country,
      'region': web.region,
      'city': web.city,
      'timezone': web.timezone,
      'providerId': web.providerId,
      'model': web.model,
    },
    'deepSeekWebSearch': <String, Object?>{
      'configuredEnabled': deep.configuredEnabled,
      'effectiveEnabled': deep.effectiveEnabled,
      'availability': deep.availability,
      'providerId': deep.providerId,
      'model': deep.model,
    },
  });
}

Future<String> _handleSearchMcpState() async {
  final mcp = _readyStudioState()?.mcpState;
  if (mcp == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'studio state not ready',
    });
  }
  return jsonEncode(<String, Object?>{
    'ok': true,
    'revision': mcp.revision,
    'desiredConfigFingerprint': mcp.desiredConfigFingerprint,
    'appliedConfigFingerprint': mcp.appliedConfigFingerprint,
    'activeServers': mcp.activeServers,
    'servers': [
      for (final server in mcp.servers)
        <String, Object?>{
          'id': server.id,
          'transport': server.transport,
          'endpoint': _redactEndpoint(server.endpoint),
          'sourceKind': server.sourceKind,
          'mutationPolicy': server.mutationPolicy,
          'state': _mcpServerStateJson(server.state),
        },
    ],
  });
}

Map<String, Object?> _mcpServerStateJson(
  McpServerState state,
) => switch (state) {
  McpDisabledState(:final message) => {'kind': 'disabled', 'message': message},
  McpMissingCredentialState(:final message) => {
    'kind': 'missingCredential',
    'message': message,
  },
  McpCheckingState(:final message) => {'kind': 'checking', 'message': message},
  McpAvailableState(:final checkedAt, :final toolCount) => {
    'kind': 'available',
    'checkedAt': checkedAt,
    'toolCount': toolCount,
  },
  McpUnavailableState(
    :final checkedAt,
    :final code,
    :final message,
    :final retryable,
  ) =>
    {
      'kind': 'unavailable',
      'checkedAt': checkedAt,
      'code': code,
      'message': message,
      'retryable': retryable,
    },
};

/// Drops userinfo, query and fragment so only scheme/host/path leave the VM.
String _redactEndpoint(String endpoint) {
  final uri = Uri.tryParse(endpoint);
  if (uri == null) return endpoint;
  return uri.replace(userInfo: '', query: '', fragment: '').toString();
}

// ---------------------------------------------------------------------------
// Cross-block text selection acceptance.
//
// The production timeline renders a long plain-text reply through
// `_PlainBodyText`, which seals the body into bounded chunks that each become a
// real `RenderParagraph`. This Driver-only bridge lets the acceptance journey
// drive the *real* `SelectionArea` selection and the product's own context-menu
// copy callback, then read the platform clipboard back. It never writes the
// domain text to the clipboard itself: every observation comes from the live
// element/render tree or from `Clipboard.getData`.
//
// These helpers live only in the Driver entrypoint; the product timeline widget
// and model are not touched.

/// The key prefixes the production `_PlainBodyText` gives its sealed chunks and
/// its trailing open chunk.
const _plainChunkKeyPrefix = 'plain-chunk-';
const _plainOpenKeyPrefix = 'plain-open-';

void _visitElements(Element element, void Function(Element) visit) {
  visit(element);
  element.visitChildren((Element child) => _visitElements(child, visit));
}

/// Chunks of the longest mounted plain-text body, in paint order. Scope the
/// observation to one body: the timeline can also mount the user's prompt.
List<Element> _longBodyTextElements() {
  final root = WidgetsBinding.instance.rootElement;
  if (root == null) return const <Element>[];
  final bodies = <Element, List<Element>>{};
  _visitElements(root, (Element element) {
    final widget = element.widget;
    if (widget is! Text) return;
    final key = widget.key;
    if (key is! ValueKey<String>) return;
    final value = key.value;
    if (value.startsWith(_plainChunkKeyPrefix) ||
        value.startsWith(_plainOpenKeyPrefix)) {
      element.visitAncestorElements((ancestor) {
        final key = ancestor.widget.key;
        if (key is ValueKey<String> && key.value.startsWith('plain-')) {
          bodies.putIfAbsent(ancestor, () => <Element>[]).add(element);
          return false;
        }
        return true;
      });
    }
  });
  var longest = const <Element>[];
  var longestLength = 0;
  for (final chunks in bodies.values) {
    final length = chunks.fold<int>(0, (length, element) {
      final text = element.widget as Text;
      return length + (text.data ?? text.textSpan?.toPlainText() ?? '').length;
    });
    if (length > longestLength) {
      longest = chunks;
      longestLength = length;
    }
  }
  return longest;
}

/// The `RenderParagraph` a chunk `Text` actually painted into.
RenderParagraph? _renderParagraphOf(Element element) {
  RenderParagraph? result;
  void visit(Element current) {
    if (result != null) return;
    if (current is RenderObjectElement) {
      final renderObject = current.renderObject;
      if (renderObject is RenderParagraph) {
        result = renderObject;
        return;
      }
    }
    current.visitChildren(visit);
  }

  visit(element);
  return result;
}

/// The `SelectableRegionState` (the real `SelectionArea`) above the body chunks.
SelectableRegionState? _longBodySelectableRegion() {
  final elements = _longBodyTextElements();
  if (elements.isEmpty) return null;
  SelectableRegionState? region;
  elements.first.visitAncestorElements((Element ancestor) {
    if (ancestor is StatefulElement &&
        ancestor.state is SelectableRegionState) {
      region = ancestor.state as SelectableRegionState;
      return false;
    }
    return true;
  });
  return region;
}

/// The rendered body split: how many real paragraphs carry it and what they
/// concatenate to. Reading the `RenderParagraph` text keeps the evidence at the
/// render layer instead of the domain model.
Map<String, Object?>? _renderedLongBody() {
  final elements = _longBodyTextElements();
  if (elements.isEmpty) return null;
  final paragraphs = <RenderParagraph>[];
  final buffer = StringBuffer();
  for (final element in elements) {
    final paragraph = _renderParagraphOf(element);
    if (paragraph != null) {
      paragraphs.add(paragraph);
      buffer.write(paragraph.text.toPlainText());
      continue;
    }
    final widget = element.widget as Text;
    buffer.write(widget.data ?? widget.textSpan?.toPlainText() ?? '');
  }
  final rendered = buffer.toString();
  return <String, Object?>{
    'paragraphCount': paragraphs.length,
    'chunkCount': elements.length,
    'characters': rendered.length,
    'head': _headOf(rendered),
    'tail': _tailOf(rendered),
    'hash': _fnv1a64Hex(rendered),
  };
}

List<String> _menuTypes(SelectableRegionState region) {
  try {
    return <String>[
      for (final item in region.contextMenuButtonItems) item.type.name,
    ];
  } on Object {
    return const <String>[];
  }
}

ContextMenuButtonItem? _copyItemOf(SelectableRegionState region) {
  try {
    for (final item in region.contextMenuButtonItems) {
      if (item.type == ContextMenuButtonType.copy) return item;
    }
  } on Object {
    return null;
  }
  return null;
}

/// Pumps one frame so the selection geometry can settle. Bounded so a frame the
/// acceptance window cannot produce never wedges the Driver request; the copy
/// availability poll below is the real observation.
Future<void> _pumpFrame() async {
  final binding = SchedulerBinding.instance;
  binding.scheduleFrame();
  try {
    await binding.endOfFrame.timeout(const Duration(milliseconds: 120));
  } on Object {
    // See above: a missing frame is not a selection result.
  }
}

/// Selects the whole timeline region and waits (bounded) until the selection is
/// actually copyable, so the real context-menu copy callback becomes available.
Future<bool> _ensureCopyable(SelectableRegionState region) async {
  for (var attempt = 0; attempt < 60; attempt += 1) {
    if (_copyItemOf(region) != null) return true;
    if (attempt == 0 || attempt == 20 || attempt == 40) region.selectAll();
    await _pumpFrame();
  }
  return _copyItemOf(region) != null;
}

Future<String> _handleSelectAll() async {
  final region = _longBodySelectableRegion();
  if (region == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'no production body chunk paragraphs are rendered',
    });
  }
  region.selectAll();
  final copyable = await _ensureCopyable(region);
  return jsonEncode(<String, Object?>{
    'ok': true,
    'copyable': copyable,
    'menuTypes': _menuTypes(region),
    ...?_renderedLongBody(),
  });
}

Future<String> _handleCopySelected() async {
  final region = _longBodySelectableRegion();
  if (region == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'no production body chunk paragraphs are rendered',
    });
  }
  final copyable = await _ensureCopyable(region);
  final item = _copyItemOf(region);
  if (!copyable || item == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'the timeline selection never became copyable',
      'menuTypes': _menuTypes(region),
      ...?_renderedLongBody(),
    });
  }
  // Reset the clipboard first so the readback can only come from this copy.
  await Clipboard.setData(const ClipboardData(text: ''));
  // Re-run the real select-all with no frame in between and copy immediately:
  // the product `_copy()` reads the selected content synchronously, so the copy
  // can never race a rebuild that seals another chunk and leaves it unselected.
  region.selectAll();
  final copyItem = _copyItemOf(region);
  if (copyItem == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'the re-selected timeline region is not copyable',
      'menuTypes': _menuTypes(region),
      ...?_renderedLongBody(),
    });
  }
  final startedAt = DateTime.now();
  copyItem.onPressed?.call();
  String? copied;
  for (var attempt = 0; attempt < 400; attempt += 1) {
    await Future<void>.delayed(const Duration(milliseconds: 25));
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text;
    if (text != null && text.isNotEmpty) {
      copied = text;
      break;
    }
  }
  final copyMillis = DateTime.now().difference(startedAt).inMilliseconds;
  // Hide only the toolbar; the acceptance journey cancels the selection with a
  // real click and re-reads the region state afterwards.
  region.hideToolbar();
  await _pumpFrame();
  final copyableAfterCopy = _copyItemOf(region) != null;
  return jsonEncode(<String, Object?>{
    'ok': copied != null,
    'copied': copied,
    'copiedLength': copied?.length,
    'copiedHead': copied == null ? null : _headOf(copied),
    'copiedTail': copied == null ? null : _tailOf(copied),
    'copiedHash': copied == null ? null : _fnv1a64Hex(copied),
    'copyMillis': copyMillis,
    'copyableAfterCopy': copyableAfterCopy,
    ...?_renderedLongBody(),
  });
}

/// Whether the region still holds a copyable (uncollapsed) selection.
Future<String> _handleSelectionState() async {
  final region = _longBodySelectableRegion();
  if (region == null) {
    return jsonEncode(<String, Object?>{
      'ok': false,
      'reason': 'no production body chunk paragraphs are rendered',
    });
  }
  return jsonEncode(<String, Object?>{
    'ok': true,
    'copyable': _copyItemOf(region) != null,
    'menuTypes': _menuTypes(region),
  });
}

String _headOf(String text) =>
    text.substring(0, text.length < 64 ? text.length : 64);

String _tailOf(String text) =>
    text.substring(text.length < 64 ? 0 : text.length - 64);

/// FNV-1a over the code units, formatted as hex.
///
/// Deterministic across runs and isolates, so the acceptance script can compare
/// the clipboard readback against the canonical fixture body without shipping
/// the whole payload twice.
String _fnv1a64Hex(String text) {
  var hash = 0xcbf29ce484222325;
  for (var index = 0; index < text.length; index += 1) {
    hash ^= text.codeUnitAt(index);
    hash = hash * 0x100000001b3;
  }
  return hash.toRadixString(16);
}
