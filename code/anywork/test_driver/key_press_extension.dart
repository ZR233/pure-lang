import 'package:flutter/services.dart';
import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_driver/flutter_driver.dart';
import 'package:flutter_test/flutter_test.dart';

import 'key_press.dart';

class KeyPressCommandExtension extends CommandExtension {
  @override
  String get commandKind => 'KeyPress';

  @override
  Command deserialize(
    Map<String, String> params,
    DeserializeFinderFactory finderFactory,
    DeserializeCommandFactory commandFactory,
  ) => KeyPress.deserialize(params);

  @override
  Future<Result> call(
    Command command,
    WidgetController prober,
    CreateFinderFactory finderFactory,
    CommandHandlerFactory handlerFactory,
  ) async {
    final press = command as KeyPress;
    // Profile builds strip debug names, so the simulator cannot infer the
    // physical key from LogicalKeyboardKey.debugName.
    await prober.sendKeyEvent(
      LogicalKeyboardKey(press.keyId),
      physicalKey: PhysicalKeyboardKey(press.usbHidUsage),
    );
    return Result.empty;
  }
}
