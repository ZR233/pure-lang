import 'dart:ui' show AppExitType;

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../data/frb/studio_api.dart' show FrbStudioApi, updaterStateFromFrb;
import '../../app/studio_shutdown.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../platform/studio_platform.dart';
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
    await FrbStudioApi.ensureReady();
    return updaterStateFromFrb(await frb.readStudioUpdateState());
  }

  @override
  Future<UpdaterStateSnapshot> check() async {
    await FrbStudioApi.ensureReady();
    return updaterStateFromFrb(await frb.checkStudioUpdate());
  }

  @override
  Future<StudioUpdateOperation> startInstall({
    required int expectedRevision,
    required String version,
  }) async {
    await FrbStudioApi.ensureReady();
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
    });
    final observed = ref.read(observedProvider);
    if (!enabled) {
      return DisabledUpdaterStateSnapshot(
        revision: observed?.revision ?? 0,
        updatedAt:
            observed?.updatedAt ?? DateTime.fromMillisecondsSinceEpoch(0),
      );
    }
    return observed ??
        UpdaterStateSnapshot.idle(
          revision: 0,
          updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
        );
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
      await for (final snapshot in operation.events) {
        if (ref.mounted && snapshot.revision >= state.revision) {
          state = snapshot;
        }
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
    if (ref.mounted && state is InstallerLaunchedUpdaterStateSnapshot) {
      await ServicesBinding.instance.exitApplication(AppExitType.required);
    }
  }

  Future<void> cancelInstall() async {
    final operation = _activeOperation;
    if (operation == null) {
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
