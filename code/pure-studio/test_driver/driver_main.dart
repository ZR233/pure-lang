import 'dart:convert';

import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pure_studio/main.dart' as studio;
import 'package:pure_studio/src/app/studio_shutdown.dart';
import 'package:pure_studio/src/data/frb/studio_api.dart';
import 'package:pure_studio/src/data/repositories/studio_repository.dart';
import 'package:pure_studio/src/shared/studio_driver_state.dart';

/// Starts Pure Studio with the Flutter Driver extension enabled.
///
/// This entrypoint is intended only for local GUI acceptance. Production and
/// release builds continue to use `lib/main.dart`.
void main() {
  if (const bool.fromEnvironment('dart.vm.product')) {
    throw StateError('Flutter Driver mode is unavailable in product builds');
  }
  enableFlutterDriverExtension(handler: _handleDriverData);
  _container = ProviderContainer();
  studio.bootstrapStudio(container: _container);
}

late final ProviderContainer _container;
Future<void>? _shutdownTask;

Future<String> _handleDriverData(String? message) async {
  switch (message) {
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
    case final String lookup when lookup.startsWith('ssh-server-id:'):
      final name = lookup.substring('ssh-server-id:'.length);
      final servers = await _container.read(studioApiProvider).listSshServers();
      final matches = servers.where((server) => server.name == name).toList();
      if (matches.length != 1) {
        return jsonEncode({
          'error': 'expected exactly one SSH server named $name',
          'count': matches.length,
        });
      }
      return jsonEncode({'serverId': matches.single.id});
    case 'prepare-persistence-failure-demo':
      final api = _container.read(studioApiProvider);
      if (api is! DriverDemoStudioApi) {
        return jsonEncode({'error': 'failure scenario requires demo mode'});
      }
      api.preparePersistenceFailureScenario();
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
    case 'shutdown':
      // 兼容旧 harness：启动关机并等待阶段序列完成（含落库排空）。
      await (_shutdownTask ??= _runShutdown());
      return jsonEncode({'shutdown': 'completed'});
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
    case 'shutdown-await':
      await (_shutdownTask ??= _runShutdown());
      return jsonEncode({'shutdown': 'completed'});
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

Future<void> _runShutdown() {
  final api = _container.read(studioApiProvider);
  final progress = _container.read(
    studioShutdownProgressStateProvider.notifier,
  );
  return runStudioShutdown(api, progress.update);
}
