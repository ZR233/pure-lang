import 'dart:async';
import 'dart:ui' show AppExitType;

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../data/frb/studio_api.dart'
    show FrbStudioBridgeDataSource, updaterStateFromFrb;
import '../../app/studio_shutdown.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../platform/studio_platform.dart';
import '../../platform/error_log.dart';
import '../../rust/api/studio.dart' as frb;

part 'studio_update_controller.g.dart';

const _isDemoBuild = bool.fromEnvironment('ANYWORK_DEMO');
const _isReleaseBuild = bool.fromEnvironment('dart.vm.product');
const _isDriverBuild = bool.fromEnvironment('ANYWORK_DRIVER');
const _compiledStudioVersion = String.fromEnvironment(
  'ANYWORK_VERSION',
  defaultValue: '1.0.0',
);

final studioUpdateApiProvider = Provider<StudioUpdateApi>(
  (ref) => const FrbStudioUpdateApi(),
);

final studioUpdateEnabledProvider = Provider<bool>(
  (ref) =>
      isWindowsPlatform && (_isReleaseBuild || _isDriverBuild) && !_isDemoBuild,
);

final studioVersionProvider = Provider<String>((ref) => _compiledStudioVersion);

final studioRuntimeBusyProvider = Provider<bool>((ref) {
  final studio = ref.watch(studioControllerProvider).value;
  return studio?.isBusy == true || studio?.runtime.hasActiveWorkflow == true;
});

abstract class StudioUpdateApi {
  Future<UpdaterStateSnapshot> read();

  Future<UpdaterStateSnapshot> check();

  Future<void> startAutomaticUpdate();

  Future<void> cancelAutomaticUpdate();

  Future<StudioUpdateOperation> startInstall({
    required int expectedRevision,
    required String version,
  });

  Future<void> openReleaseNotes(String url);
}

abstract class StudioUpdateOperation {
  Stream<UpdaterStateSnapshot> get events;

  Future<void> cancel();

  Future<bool> finishHandoff();

  void dispose();
}

class FrbStudioUpdateApi implements StudioUpdateApi {
  const FrbStudioUpdateApi();

  @override
  Future<UpdaterStateSnapshot> read() async {
    await FrbStudioBridgeDataSource.ensureReady();
    return updaterStateFromFrb(await frb.readStudioUpdateState());
  }

  @override
  Future<UpdaterStateSnapshot> check() async {
    await FrbStudioBridgeDataSource.ensureReady();
    return updaterStateFromFrb(await frb.checkStudioUpdate());
  }

  @override
  Future<void> startAutomaticUpdate() async {
    await FrbStudioBridgeDataSource.ensureReady();
    await frb.startStudioBackgroundUpdate();
  }

  @override
  Future<void> cancelAutomaticUpdate() => frb.cancelStudioBackgroundUpdate();

  @override
  Future<StudioUpdateOperation> startInstall({
    required int expectedRevision,
    required String version,
  }) async {
    await FrbStudioBridgeDataSource.ensureReady();
    return _FrbStudioUpdateOperation(
      await frb.installStudioUpdate(
        expectedRevision: BigInt.from(expectedRevision),
        version: version,
      ),
    );
  }

  @override
  Future<void> openReleaseNotes(String url) async {
    await openExternalUrl(url);
  }
}

class _FrbStudioUpdateOperation implements StudioUpdateOperation {
  _FrbStudioUpdateOperation(this._handle);

  final frb.BridgeStudioUpdateOperation _handle;
  bool _disposed = false;

  @override
  Stream<UpdaterStateSnapshot> get events => _events();

  Stream<UpdaterStateSnapshot> _events() async* {
    yield* _handle.progressStream().map(updaterStateFromFrb);
  }

  @override
  Future<void> cancel() => _handle.cancel();

  @override
  Future<bool> finishHandoff() => _handle.finishHandoff();

  @override
  void dispose() {
    if (_disposed) {
      return;
    }
    _disposed = true;
    _handle.dispose();
  }
}

@Riverpod(keepAlive: true)
class StudioUpdateController extends _$StudioUpdateController {
  StudioUpdateOperation? _activeOperation;
  Future<void>? _installFuture;
  bool _checking = false;
  bool _automaticStarted = false;

  StudioUpdateApi get _api => ref.read(studioUpdateApiProvider);

  bool get _enabled => ref.read(studioUpdateEnabledProvider);

  @override
  UpdaterStateSnapshot build() {
    ref.onDispose(() {
      final operation = _activeOperation;
      _activeOperation = null;
      if (operation != null) {
        operation.dispose();
      }
    });
    final enabled = ref.watch(studioUpdateEnabledProvider);
    final observedProvider = studioControllerProvider.select(
      (value) => value.value?.updaterState,
    );
    // Listening preserves the operation owner when product progress arrives.
    ref.listen(observedProvider, (_, observed) {
      if (_enabled && observed != null && observed.revision >= state.revision) {
        state = observed;
      }
      if (observed != null) _scheduleAutomaticUpdateOnce();
    });
    final observed = ref.read(observedProvider);
    if (!enabled) {
      return DisabledUpdaterStateSnapshot(
        revision: observed?.revision ?? 0,
        updatedAt:
            observed?.updatedAt ?? DateTime.fromMillisecondsSinceEpoch(0),
      );
    }
    if (observed != null) _scheduleAutomaticUpdateOnce();
    return observed ??
        UpdaterStateSnapshot.idle(
          revision: 0,
          updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
        );
  }

  void _scheduleAutomaticUpdateOnce() {
    if (!_enabled || _automaticStarted) return;
    // canonical 更新状态只在运行时成功就绪后出现；启动失败不消耗本进程的一次检查。
    _automaticStarted = true;
    scheduleMicrotask(() {
      if (ref.mounted) unawaited(_startAutomaticUpdate());
    });
  }

  Future<void> _startAutomaticUpdate() async {
    try {
      await _api.startAutomaticUpdate();
    } on Object catch (error, stackTrace) {
      recordDartError(error, stackTrace, stage: 'automatic-update-start');
    }
  }

  Future<void> check() async {
    if (!_enabled || _checking || _isInstalling(state)) return;
    _checking = true;
    try {
      final snapshot = await _api.check();
      if (ref.mounted) state = snapshot;
    } finally {
      _checking = false;
    }
  }

  Future<void> install() =>
      _installFuture ??= _install().whenComplete(() => _installFuture = null);

  Future<void> _install() async {
    final update = state.update;
    if (!_enabled || update == null || _isInstalling(state)) return;
    if (ref.read(studioRuntimeBusyProvider)) {
      throw StateError('Studio runtime has an active turn or task');
    }
    final api = _api;
    // 本次交接是否经 finishHandoff 的真实完成 ACK 确认需要退出旧实例。只在仍强持有
    // operation（dispose 之前）时消费，绝不依赖事件流结束的调度时间窗。
    var handoffConfirmed = false;
    try {
      final operation = await api.startInstall(
        expectedRevision: state.revision,
        version: update.version,
      );
      if (!ref.mounted) {
        operation.dispose();
        return;
      }
      _activeOperation = operation;
      UpdaterStateSnapshot? terminalSnapshot;
      await for (final snapshot in operation.events) {
        terminalSnapshot = snapshot;
        if (ref.mounted && snapshot.revision >= state.revision) {
          state = snapshot;
        }
      }
      // 正常更新交接：终态是 installer launched。事件流结束只表示 Dart 侧 StreamSink
      // 结束，与 Rust owned observer 发布「安装任务 / 进度 sink 真实完成」是不同调度顺序。
      // 必须在仍强持有 operation 时消费同一个 finishHandoff 的实际完成 ACK：它等待该操作
      // 所有 owner 真正结束，并优先依据真实 installer_launched() 返回 true；没有派生且真实
      // Stopped 时才安全恢复当前程序，任务异常如实保留。绝不用极短时间窗 / 延时 / 调度概率
      // 代替 ACK，也绝不在 dispose 之后才失去等待能力。真正的退出仍进入现有
      // ServicesBinding / native 同一 30 秒协调器。
      if (terminalSnapshot is InstallerLaunchedUpdaterStateSnapshot) {
        handoffConfirmed = await operation.finishHandoff();
      }
    } catch (error, stackTrace) {
      if (!ref.mounted) Error.throwWithStackTrace(error, stackTrace);
      final snapshot = await api.read();
      if (ref.mounted) state = snapshot;
      if (!ref.mounted) Error.throwWithStackTrace(error, stackTrace);
      if (snapshot is InstallFailedUpdaterStateSnapshot &&
          snapshot.error.code == 'runtimeShutdownFailed') {
        ref
            .read(studioShutdownProgressStateProvider.notifier)
            .fail(snapshot.error.message);
        return;
      }
      final operation = _activeOperation;
      if (operation != null && await operation.finishHandoff()) {
        await ServicesBinding.instance.exitApplication(AppExitType.required);
        return;
      }
      if (snapshot is! InstallFailedUpdaterStateSnapshot) {
        Error.throwWithStackTrace(error, stackTrace);
      }
    } finally {
      final operation = _activeOperation;
      _activeOperation = null;
      operation?.dispose();
    }
    // 只有在 finishHandoff 真实确认（派生的安装器事实优先，或失败已安全恢复进程）之后才
    // 退出旧实例：未成功的严格关闭不得启动安装器 / 恢复程序，finishHandoff 的真实错误也
    // 绝不被伪造成成功退出。
    if (handoffConfirmed) {
      await ServicesBinding.instance.exitApplication(AppExitType.required);
    }
  }

  Future<void> cancelInstall() async {
    final operation = _activeOperation;
    if (operation == null) {
      await _api.cancelAutomaticUpdate();
      return;
    }
    await operation.cancel();
  }

  Future<void> openReleaseNotes() async {
    final update = state.update;
    if (update == null) return;
    try {
      await _api.openReleaseNotes(update.notesUrl);
    } catch (_) {}
  }
}

bool _isInstalling(UpdaterStateSnapshot state) =>
    state is DownloadingUpdaterStateSnapshot ||
    state is VerifyingUpdaterStateSnapshot ||
    state is InstallerLaunchedUpdaterStateSnapshot;
