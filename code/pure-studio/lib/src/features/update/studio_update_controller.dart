import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../data/frb/studio_api.dart' show FrbStudioApi, updaterStateFromFrb;
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../platform/studio_platform.dart';
import '../../rust/api/studio.dart' as frb;

part 'studio_update_controller.g.dart';

const _isDemoBuild = bool.fromEnvironment('PURE_STUDIO_DEMO');
const _isReleaseBuild = bool.fromEnvironment('dart.vm.product');
const _compiledStudioVersion = String.fromEnvironment(
  'PURE_STUDIO_VERSION',
  defaultValue: '1.0.0',
);

final studioUpdateApiProvider = Provider<StudioUpdateApi>(
  (ref) => const FrbStudioUpdateApi(),
);

final studioUpdateEnabledProvider = Provider<bool>(
  (ref) => isWindowsPlatform && _isReleaseBuild && !_isDemoBuild,
);

final studioVersionProvider = Provider<String>((ref) => _compiledStudioVersion);

final studioRuntimeBusyProvider = Provider<bool>((ref) {
  final studio = ref.watch(studioControllerProvider).value;
  return studio?.isBusy == true || studio?.runtime.hasActiveWorkflow == true;
});

abstract class StudioUpdateApi {
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

  void dispose();
}

class FrbStudioUpdateApi implements StudioUpdateApi {
  const FrbStudioUpdateApi();

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
    try {
      yield* _handle.progressStream().map(updaterStateFromFrb);
    } finally {
      dispose();
    }
  }

  @override
  Future<void> cancel() => _handle.cancel();

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
    final observed = ref.watch(
      studioControllerProvider.select((value) => value.value?.updaterState),
    );
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
    if (!_enabled || state is CheckingUpdaterStateSnapshot) return;
    try {
      state = await _api.check();
    } catch (error) {
      if (state is! CheckFailedUpdaterStateSnapshot) {
        state = CheckFailedUpdaterStateSnapshot(
          revision: state.revision + 1,
          failedAt: DateTime.now(),
          error: UpdaterErrorView(
            code: 'checkFailed',
            message: error.toString(),
            retryable: true,
          ),
        );
      }
    }
  }

  Future<void> install() async {
    final update = state.update;
    if (!_enabled || update == null || _isInstalling(state)) return;
    if (ref.read(studioRuntimeBusyProvider)) {
      _installFailure(
        'runtimeBusy',
        'Studio runtime has an active turn or task',
      );
      return;
    }
    try {
      final operation = await _api.startInstall(
        expectedRevision: state.revision,
        version: update.version,
      );
      _activeOperation = operation;
      await for (final snapshot in operation.events) {
        state = snapshot;
      }
    } catch (error) {
      if (state is! InstallFailedUpdaterStateSnapshot) {
        _installFailure('installFailed', error.toString());
      }
    } finally {
      final operation = _activeOperation;
      _activeOperation = null;
      operation?.dispose();
    }
  }

  Future<void> cancelInstall() async {
    final operation = _activeOperation;
    if (operation == null) {
      return;
    }
    try {
      await operation.cancel();
    } catch (_) {}
  }

  Future<void> openReleaseNotes() async {
    final update = state.update;
    if (update == null) return;
    try {
      await _api.openReleaseNotes(update.notesUrl);
    } catch (_) {}
  }

  void _installFailure(String code, String message) {
    final update = state.update;
    if (update == null) return;
    state = InstallFailedUpdaterStateSnapshot(
      revision: state.revision + 1,
      failedAt: DateTime.now(),
      update: update,
      error: UpdaterErrorView(code: code, message: message, retryable: true),
    );
  }
}

bool _isInstalling(UpdaterStateSnapshot state) =>
    state is DownloadingUpdaterStateSnapshot ||
    state is VerifyingUpdaterStateSnapshot ||
    state is InstallerLaunchedUpdaterStateSnapshot;
