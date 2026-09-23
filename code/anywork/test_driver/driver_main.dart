import 'dart:convert';

import 'package:anywork/main.dart' as studio;
import 'package:anywork/src/app/studio_shutdown.dart';
import 'package:anywork/src/data/repositories/studio_repository.dart';
import 'package:anywork/src/shared/studio_driver_state.dart';
import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'raw_tap_extension.dart';

/// Native-only Driver entrypoint. Product and release builds use lib/main.dart.
void main() {
  if (const bool.fromEnvironment('dart.vm.product')) {
    throw StateError('Flutter Driver mode is unavailable in product builds');
  }
  enableFlutterDriverExtension(
    handler: _handleDriverData,
    commands: <CommandExtension>[RawTapCommandExtension()],
  );
  _container = ProviderContainer();
  studio.bootstrapStudio(container: _container);
}

late final ProviderContainer _container;
Future<void>? _shutdownTask;

Future<String> _handleDriverData(String? message) async {
  switch (message) {
    case 'snapshot':
      final state = switch (_container.read(studioControllerProvider)) {
        AsyncData(:final value) => value,
        _ => null,
      };
      if (state != null) StudioDriverState.publishState(state);
      return StudioDriverState.snapshotJson();
    case 'shutdown':
      try {
        await (_shutdownTask ??= _runShutdown());
        return jsonEncode({'shutdown': 'completed'});
      } on Object {
        _shutdownTask = null;
        return jsonEncode({'shutdown': 'failed'});
      }
    default:
      return jsonEncode({'error': 'unsupported driver request'});
  }
}

Future<void> _runShutdown() async {
  final api = _container.read(studioApiProvider);
  final progress = _container.read(
    studioShutdownProgressStateProvider.notifier,
  );
  await runStudioShutdown(api, progress.update);
}
