import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../data/frb/studio_api.dart' show FrbStudioApi;
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
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
  (ref) => Platform.isWindows && _isReleaseBuild && !_isDemoBuild,
);

final studioVersionProvider = Provider<String>((ref) => _compiledStudioVersion);

final studioRuntimeBusyProvider = Provider<bool>((ref) {
  final studio = ref.watch(studioControllerProvider).value;
  return studio?.isBusy == true || studio?.runtime.hasActiveTask == true;
});

enum StudioUpdatePhase {
  disabled,
  idle,
  checking,
  upToDate,
  available,
  downloading,
  verifying,
  installerLaunched,
  failed,
}

class StudioUpdateInfo {
  const StudioUpdateInfo({
    required this.revision,
    required this.version,
    required this.publishedAt,
    required this.notesUrl,
  });

  final int revision;
  final String version;
  final DateTime publishedAt;
  final String notesUrl;
}

class StudioUpdateState {
  const StudioUpdateState({
    required this.phase,
    required this.currentVersion,
    this.update,
    this.downloaded = 0,
    this.total = 0,
    this.errorCode,
    this.errorMessage,
  });

  final StudioUpdatePhase phase;
  final String currentVersion;
  final StudioUpdateInfo? update;
  final int downloaded;
  final int total;
  final String? errorCode;
  final String? errorMessage;

  bool get hasUpdate =>
      update != null && phase != StudioUpdatePhase.installerLaunched;

  double? get progress {
    if (total <= 0) return null;
    return (downloaded / total).clamp(0, 1);
  }
}

enum StudioUpdateInstallEventKind {
  started,
  progress,
  verifying,
  installerLaunched,
  failed,
}

class StudioUpdateInstallEvent {
  const StudioUpdateInstallEvent({
    required this.kind,
    this.downloaded = 0,
    this.total = 0,
    this.code,
    this.message,
  });

  final StudioUpdateInstallEventKind kind;
  final int downloaded;
  final int total;
  final String? code;
  final String? message;
}

abstract class StudioUpdateApi {
  Future<UpdaterStateSnapshot> check();

  Future<StudioUpdateOperation> startInstall({
    required int expectedRevision,
    required String version,
  });

  Future<void> openReleaseNotes(String url);
}

abstract class StudioUpdateOperation {
  Stream<StudioUpdateInstallEvent> get events;

  Future<void> cancel();

  void dispose();
}

class FrbStudioUpdateApi implements StudioUpdateApi {
  const FrbStudioUpdateApi();

  @override
  Future<UpdaterStateSnapshot> check() async {
    await FrbStudioApi.ensureReady();
    final snapshot = await frb.checkStudioUpdate();
    return UpdaterStateSnapshot(
      meta: _metaFromBridge(snapshot.meta),
      version: snapshot.update?.version,
      publishedAt: snapshot.update == null
          ? null
          : DateTime.fromMillisecondsSinceEpoch(
              snapshot.update!.publishedAt * 1000,
            ),
      notesUrl: snapshot.update?.notesUrl,
    );
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
    await Process.start('rundll32.exe', [
      'url.dll,FileProtocolHandler',
      url,
    ], mode: ProcessStartMode.detached);
  }

  static StudioUpdateInstallEvent _installEventFromBridge(
    frb.BridgeStudioUpdateEventDto event,
  ) {
    return switch (event) {
      frb.BridgeStudioUpdateEventDto_Started(:final total) =>
        StudioUpdateInstallEvent(
          kind: StudioUpdateInstallEventKind.started,
          total: total.toInt(),
        ),
      frb.BridgeStudioUpdateEventDto_Progress(
        :final downloaded,
        :final total,
      ) =>
        StudioUpdateInstallEvent(
          kind: StudioUpdateInstallEventKind.progress,
          downloaded: downloaded.toInt(),
          total: total.toInt(),
        ),
      frb.BridgeStudioUpdateEventDto_Verifying() =>
        const StudioUpdateInstallEvent(
          kind: StudioUpdateInstallEventKind.verifying,
        ),
      frb.BridgeStudioUpdateEventDto_InstallerLaunched() =>
        const StudioUpdateInstallEvent(
          kind: StudioUpdateInstallEventKind.installerLaunched,
        ),
      frb.BridgeStudioUpdateEventDto_Failed(:final code, :final message) =>
        StudioUpdateInstallEvent(
          kind: StudioUpdateInstallEventKind.failed,
          code: code,
          message: message,
        ),
    };
  }

  static ObservedStateMeta _metaFromBridge(frb.BridgeObservedStateMeta meta) {
    final phase = meta.phase.when(
      uninitialized: () => ObservedStatePhase.uninitialized,
      ready: () => ObservedStatePhase.ready,
      running: (_, _) => ObservedStatePhase.running,
      failed: (_, _) => ObservedStatePhase.failed,
      stopped: () => ObservedStatePhase.stopped,
    );
    return ObservedStateMeta(
      revision: meta.revision.toInt(),
      phase: phase,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(meta.updatedAt * 1000),
      lastCheckedAt: meta.lastCheckedAt == null
          ? null
          : DateTime.fromMillisecondsSinceEpoch(meta.lastCheckedAt! * 1000),
      stale: meta.stale,
    );
  }
}

class _FrbStudioUpdateOperation implements StudioUpdateOperation {
  _FrbStudioUpdateOperation(this._handle);

  final frb.BridgeStudioUpdateOperation _handle;
  bool _disposed = false;

  @override
  Stream<StudioUpdateInstallEvent> get events => _events();

  Stream<StudioUpdateInstallEvent> _events() async* {
    try {
      yield* _handle.progressStream().map(
        FrbStudioUpdateApi._installEventFromBridge,
      );
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
  StudioUpdateState build() {
    ref.onDispose(() {
      final operation = _activeOperation;
      _activeOperation = null;
      if (operation != null) {
        operation.dispose();
      }
    });
    final currentVersion = ref.watch(studioVersionProvider);
    final enabled = ref.watch(studioUpdateEnabledProvider);
    final observed = ref.watch(
      studioControllerProvider.select((value) => value.value?.updaterState),
    );
    final available = enabled && observed?.version != null;
    return StudioUpdateState(
      phase: !enabled
          ? StudioUpdatePhase.disabled
          : available
          ? StudioUpdatePhase.available
          : StudioUpdatePhase.idle,
      currentVersion: currentVersion,
      update: available
          ? StudioUpdateInfo(
              revision: observed!.meta.revision,
              version: observed.version!,
              publishedAt: observed.publishedAt!,
              notesUrl: observed.notesUrl!,
            )
          : null,
    );
  }

  Future<void> check() async {
    if (!_enabled || state.phase == StudioUpdatePhase.checking) return;
    state = StudioUpdateState(
      phase: StudioUpdatePhase.checking,
      currentVersion: state.currentVersion,
    );
    try {
      final result = await _api.check();
      final version = result.version;
      state = version == null
          ? StudioUpdateState(
              phase: StudioUpdatePhase.upToDate,
              currentVersion: state.currentVersion,
            )
          : StudioUpdateState(
              phase: StudioUpdatePhase.available,
              currentVersion: state.currentVersion,
              update: StudioUpdateInfo(
                revision: result.meta.revision,
                version: version,
                publishedAt: result.publishedAt!,
                notesUrl: result.notesUrl!,
              ),
            );
    } catch (error) {
      _fail('checkFailed', error.toString());
    }
  }

  Future<void> install() async {
    final update = state.update;
    if (!_enabled || update == null || _isInstalling(state.phase)) return;
    if (ref.read(studioRuntimeBusyProvider)) {
      _fail('runtimeBusy', 'Studio runtime has an active turn or task');
      return;
    }
    state = StudioUpdateState(
      phase: StudioUpdatePhase.downloading,
      currentVersion: state.currentVersion,
      update: update,
    );
    try {
      final operation = await _api.startInstall(
        expectedRevision: update.revision,
        version: update.version,
      );
      _activeOperation = operation;
      await for (final event in operation.events) {
        _applyInstallEvent(event);
      }
    } catch (error) {
      _fail('installFailed', error.toString());
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
      _fail('cancelled', 'Studio update installation was cancelled');
    } catch (error) {
      _fail('cancellationTooLate', error.toString());
    }
  }

  Future<void> openReleaseNotes() async {
    final update = state.update;
    if (update == null) return;
    try {
      await _api.openReleaseNotes(update.notesUrl);
    } catch (error) {
      _fail('releaseNotesFailed', error.toString());
    }
  }

  void _applyInstallEvent(StudioUpdateInstallEvent event) {
    final update = state.update;
    if (update == null) return;
    switch (event.kind) {
      case StudioUpdateInstallEventKind.started:
      case StudioUpdateInstallEventKind.progress:
        state = StudioUpdateState(
          phase: StudioUpdatePhase.downloading,
          currentVersion: state.currentVersion,
          update: update,
          downloaded: event.downloaded,
          total: event.total,
        );
      case StudioUpdateInstallEventKind.verifying:
        state = StudioUpdateState(
          phase: StudioUpdatePhase.verifying,
          currentVersion: state.currentVersion,
          update: update,
          downloaded: state.downloaded,
          total: state.total,
        );
      case StudioUpdateInstallEventKind.installerLaunched:
        state = StudioUpdateState(
          phase: StudioUpdatePhase.installerLaunched,
          currentVersion: state.currentVersion,
          update: update,
          downloaded: state.downloaded,
          total: state.total,
        );
      case StudioUpdateInstallEventKind.failed:
        _fail(event.code ?? 'installFailed', event.message ?? 'Update failed');
    }
  }

  void _fail(String code, String message) {
    state = StudioUpdateState(
      phase: StudioUpdatePhase.failed,
      currentVersion: state.currentVersion,
      update: state.update,
      downloaded: state.downloaded,
      total: state.total,
      errorCode: code,
      errorMessage: message,
    );
  }
}

bool _isInstalling(StudioUpdatePhase phase) {
  return phase == StudioUpdatePhase.downloading ||
      phase == StudioUpdatePhase.verifying ||
      phase == StudioUpdatePhase.installerLaunched;
}
