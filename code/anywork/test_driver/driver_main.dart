import 'dart:convert';

import 'package:anywork/src/domain/models/studio_models.dart';

import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:anywork/main.dart' as studio;
import 'package:anywork/src/app/studio_shutdown.dart';
import 'package:anywork/src/data/frb/studio_api.dart';
import 'package:anywork/src/data/repositories/studio_repository.dart';
import 'package:anywork/src/shared/studio_driver_state.dart';
import 'package:anywork/src/rust/api/studio/types/error.dart';

import 'raw_tap_extension.dart';

part 'timeline_native_fixture.dart';

/// Starts anywork with the Flutter Driver extension enabled.
///
/// This entrypoint is intended only for local GUI acceptance. Production and
/// release builds continue to use `lib/main.dart`.
void main() {
  if (const bool.fromEnvironment('dart.vm.product')) {
    throw StateError('Flutter Driver mode is unavailable in product builds');
  }
  enableFlutterDriverExtension(
    handler: _handleDriverData,
    commands: <CommandExtension>[RawTapCommandExtension()],
  );
  _container = ProviderContainer(
    overrides: [
      if (const bool.fromEnvironment('ANYWORK_DEMO'))
        shellChromeProvider.overrideWith(
          (ref) => _previewStartup ? const AsyncLoading() : shellChrome(ref),
        ),
    ],
  );
  studio.bootstrapStudio(container: _container);
}

late final ProviderContainer _container;
Future<void>? _shutdownTask;
bool _previewStartup = false;

Future<String> _handleDriverData(String? message) async {
  switch (message) {
    case 'preview-startup-demo' || 'finish-startup-demo':
      if (_container.read(studioApiProvider) is! DriverDemoStudioApi) {
        return jsonEncode({'error': 'startup preview requires demo mode'});
      }
      _previewStartup = message == 'preview-startup-demo';
      _container.invalidate(shellChromeProvider);
      return jsonEncode({'startupPreview': _previewStartup});
    case final String request when request.startsWith('timeline-native:'):
      return _timelineNativeFixture(
        request.substring('timeline-native:'.length),
      );
    case 'snapshot':
      _publishSidebarDirectory();
      return StudioDriverState.snapshotJson();
    case 'sidebar-load-more':
      // 等价于侧栏触底：加载下一页目录并回报窗口状态。
      await _container
          .read(studioControllerProvider.notifier)
          .loadMoreThreads();
      _publishSidebarDirectory();
      return jsonEncode({'loaded': true});
    case final String lookup when lookup.startsWith('ssh-server-alias:'):
      final name = lookup.substring('ssh-server-alias:'.length);
      final servers = await _container.read(studioApiProvider).listSshServers();
      final matches = servers.where((server) => server.alias == name).toList();
      if (matches.length != 1) {
        return jsonEncode({
          'error': 'expected exactly one SSH server named $name',
          'count': matches.length,
        });
      }
      return jsonEncode({'alias': matches.single.alias});
    case 'prepare-theme-interactions-demo' || 'prepare-theme-plan-demo':
      final api = _container.read(studioApiProvider);
      if (api is! DriverDemoStudioApi) {
        return jsonEncode({'error': 'theme scenarios require demo mode'});
      }
      api.prepareThemeScenario(plan: message == 'prepare-theme-plan-demo');
      _container.invalidate(studioControllerProvider);
      await _container.read(studioControllerProvider.future);
      return jsonEncode({'prepared': true});
    case 'prepare-connection-retry-demo':
      final api = _container.read(studioApiProvider);
      if (api is! DriverDemoStudioApi) {
        return jsonEncode({
          'error': 'connection retry scenario requires demo mode',
        });
      }
      api.prepareConnectionRetryScenario();
      return jsonEncode({'prepared': true});
    case 'prepare-persistence-failure-demo':
      final api = _container.read(studioApiProvider);
      if (api is! DriverDemoStudioApi) {
        return jsonEncode({'error': 'failure scenario requires demo mode'});
      }
      api.preparePersistenceFailureScenario();
      _container.invalidate(studioControllerProvider);
      await _container.read(studioControllerProvider.future);
      return jsonEncode({'prepared': true});
    case 'prepare-retired-planner-demo':
      final api = _container.read(studioApiProvider);
      if (api is! DriverDemoStudioApi) {
        return jsonEncode({
          'error': 'retired planner scenario requires demo mode',
        });
      }
      api.prepareRetiredPlannerScenario();
      _container.invalidate(studioControllerProvider);
      await _container.read(studioControllerProvider.future);
      return jsonEncode({'prepared': true});
    case 'prepare-session-lifecycle-demo':
      final api = _container.read(studioApiProvider);
      if (api is! DriverDemoStudioApi) {
        return jsonEncode({
          'error': 'session lifecycle demo requires demo mode',
        });
      }
      api.prepareSessionLifecycleScenario();
      _container.invalidate(studioControllerProvider);
      await _container.read(studioControllerProvider.future);
      _publishSidebarDirectory();
      return jsonEncode({'prepared': true});
    case 'shutdown' || 'shutdown-await':
      try {
        await (_shutdownTask ??= _runShutdown());
        return jsonEncode({'shutdown': 'completed'});
      } on Object catch (error) {
        return jsonEncode({
          'shutdown': 'failed',
          'error': error is BridgeError
              ? {
                  'code': error.code.name,
                  'message': error.message,
                  'correlationId': error.correlationId,
                  'details': error.detailsJson,
                }
              : error.toString(),
        });
      }
    case final String mode
        when mode.startsWith('set-new-thread-workspace-mode:'):
      // Acceptance-only entry: the start-page workspace-mode popup menu is
      // covered by the native integration test
      // (`remote Project start page offers worktree and drives a worktree
      // marker`). When the popup item cannot be activated through the driver
      // transport, this command sets the same canonical draft fact so the
      // following real remote worktree creation and prompt stay observable.
      final requested = ThreadWorkspaceMode.fromId(
        mode.substring('set-new-thread-workspace-mode:'.length),
      );
      _container
          .read(studioControllerProvider.notifier)
          .setNewThreadWorkspaceMode(requested);
      _publishSidebarDirectory();
      return jsonEncode({'newThreadWorkspaceMode': requested.id});
    case final String seed when seed.startsWith('seed-threads:'):
      // Fixture 只属于专用 Driver demo harness，不穿过生产 FRB API。
      final count = int.tryParse(seed.substring('seed-threads:'.length)) ?? 0;
      final api = _container.read(studioApiProvider);
      if (api is DriverDemoStudioApi) {
        api.preparePagingScenario(count);
        _container.invalidate(studioControllerProvider);
        await _container.read(studioControllerProvider.future);
        _publishSidebarDirectory();
        return jsonEncode({'seeded': count});
      }
      return jsonEncode({'error': 'seeding requires the Driver demo harness'});
    case 'shutdown-begin':
      // 触发关机但不等待；验收脚本可在阶段界面显示期间截图/快照。
      _shutdownTask ??= _runShutdown();
      return jsonEncode({'shutdown': 'started'});
    default:
      return jsonEncode({
        'error': 'unsupported driver request',
        'request': message,
      });
  }
}

void _publishSidebarDirectory() {
  final state = switch (_container.read(studioControllerProvider)) {
    AsyncData(:final value) => value,
    _ => null,
  };
  if (state == null) return;
  StudioDriverState.publishState(state);
}

Future<void> _runShutdown() async {
  final api = _container.read(studioApiProvider);
  final progress = _container.read(
    studioShutdownProgressStateProvider.notifier,
  );
  try {
    await runStudioShutdown(api, progress.update);
  } on Object {
    _shutdownTask = null;
    rethrow;
  }
}
