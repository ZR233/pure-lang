import 'package:flutter_driver/driver_extension.dart';
import 'package:pure_studio/main.dart' as studio;

/// Starts Pure Studio with the Flutter Driver extension enabled.
///
/// This entrypoint is intended only for local GUI acceptance. Production and
/// release builds continue to use `lib/main.dart`.
void main() {
  if (const bool.fromEnvironment('dart.vm.product')) {
    throw StateError('Flutter Driver mode is unavailable in product builds');
  }
  enableFlutterDriverExtension();
  studio.main();
}
