import 'dart:convert';

import 'package:flutter_driver/driver_extension.dart';
import 'package:pure_studio/src/data/frb/studio_api.dart';
import 'package:pure_studio/src/shared/studio_driver_state.dart';
import 'package:pure_studio/main.dart' as studio;

/// Starts Pure Studio with the Flutter Driver extension enabled.
///
/// This entrypoint is intended only for local GUI acceptance. Production and
/// release builds continue to use `lib/main.dart`.
void main() {
  if (const bool.fromEnvironment('dart.vm.product')) {
    throw StateError('Flutter Driver mode is unavailable in product builds');
  }
  enableFlutterDriverExtension(handler: _handleDriverData);
  studio.main();
}

Future<String> _handleDriverData(String? message) async {
  switch (message) {
    case 'snapshot':
      return StudioDriverState.snapshotJson();
    case 'shutdown':
      await FrbStudioApi.shutdownAndDispose();
      return jsonEncode({'shutdown': 'completed'});
    default:
      return jsonEncode({
        'error': 'unsupported driver request',
        'request': message,
      });
  }
}
