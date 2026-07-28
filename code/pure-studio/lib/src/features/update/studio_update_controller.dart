import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../data/frb/studio_api.dart' show FrbStudioApi;
import '../../data/repositories/studio_repository.dart';
import '../../rust/api/studio.dart' as frb;

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

final studioUpdateControllerProvider =
    NotifierProvider<StudioUpdateController, StudioUpdateState>(
      StudioUpdateController.new,
    );

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
    required this.version,
    required this.publishedAt,
    required this.notesUrl,
    required this.installerUrl,
    required this.installerSize,
    required this.installerSha256,
    required this.installerSignatureUrl,
  });

  final String version;
  final int publishedAt;
  final String notesUrl;
  final String installerUrl;
  final int installerSize;
  final String installerSha256;
  final String installerSignatureUrl;
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

sealed class StudioUpdateCheckResult {
  const StudioUpdateCheckResult();
}

class StudioUpdateUpToDate extends StudioUpdateCheckResult {
  const StudioUpdateUpToDate();
}

class StudioUpdateAvailable extends StudioUpdateCheckResult {
  const StudioUpdateAvailable(this.update);

  final StudioUpdateInfo update;
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
  Future<StudioUpdateCheckResult> check(String currentVersion);

  Stream<StudioUpdateInstallEvent> install(StudioUpdateInfo update);

  Future<void> openReleaseNotes(String url);
}

class FrbStudioUpdateApi implements StudioUpdateApi {
  const FrbStudioUpdateApi();

  @override
  Future<StudioUpdateCheckResult> check(String currentVersion) async {
    await FrbStudioApi.ensureReady();
    final result = await frb.checkStudioUpdate(currentVersion: currentVersion);
    return switch (result) {
      frb.BridgeStudioUpdateCheckDto_UpToDate() => const StudioUpdateUpToDate(),
      frb.BridgeStudioUpdateCheckDto_Available(:final update) =>
        StudioUpdateAvailable(_fromBridge(update)),
    };
  }

  @override
  Stream<StudioUpdateInstallEvent> install(StudioUpdateInfo update) async* {
    await FrbStudioApi.ensureReady();
    yield* frb
        .installStudioUpdate(update: _toBridge(update))
        .map(_installEventFromBridge);
  }

  @override
  Future<void> openReleaseNotes(String url) async {
    await Process.start('rundll32.exe', [
      'url.dll,FileProtocolHandler',
      url,
    ], mode: ProcessStartMode.detached);
  }

  static StudioUpdateInfo _fromBridge(frb.BridgeStudioUpdateDto update) {
    return StudioUpdateInfo(
      version: update.version,
      publishedAt: update.publishedAt,
      notesUrl: update.notesUrl,
      installerUrl: update.installerUrl,
      installerSize: update.installerSize.toInt(),
      installerSha256: update.installerSha256,
      installerSignatureUrl: update.installerSignatureUrl,
    );
  }

  static frb.BridgeStudioUpdateDto _toBridge(StudioUpdateInfo update) {
    return frb.BridgeStudioUpdateDto(
      version: update.version,
      publishedAt: update.publishedAt,
      notesUrl: update.notesUrl,
      installerUrl: update.installerUrl,
      installerSize: BigInt.from(update.installerSize),
      installerSha256: update.installerSha256,
      installerSignatureUrl: update.installerSignatureUrl,
    );
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
}

class StudioUpdateController extends Notifier<StudioUpdateState> {
  StudioUpdateApi get _api => ref.read(studioUpdateApiProvider);

  bool get _enabled => ref.read(studioUpdateEnabledProvider);

  @override
  StudioUpdateState build() {
    final currentVersion = ref.watch(studioVersionProvider);
    final enabled = ref.watch(studioUpdateEnabledProvider);
    if (enabled) {
      Future<void>.microtask(check);
    }
    return StudioUpdateState(
      phase: enabled ? StudioUpdatePhase.idle : StudioUpdatePhase.disabled,
      currentVersion: currentVersion,
    );
  }

  Future<void> check() async {
    if (!_enabled || state.phase == StudioUpdatePhase.checking) return;
    state = StudioUpdateState(
      phase: StudioUpdatePhase.checking,
      currentVersion: state.currentVersion,
    );
    try {
      final result = await _api.check(state.currentVersion);
      state = switch (result) {
        StudioUpdateUpToDate() => StudioUpdateState(
          phase: StudioUpdatePhase.upToDate,
          currentVersion: state.currentVersion,
        ),
        StudioUpdateAvailable(:final update) => StudioUpdateState(
          phase: StudioUpdatePhase.available,
          currentVersion: state.currentVersion,
          update: update,
        ),
      };
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
      total: update.installerSize,
    );
    try {
      await for (final event in _api.install(update)) {
        _applyInstallEvent(event);
      }
    } catch (error) {
      _fail('installFailed', error.toString());
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
