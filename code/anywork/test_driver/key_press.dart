import 'package:flutter_driver/flutter_driver.dart';

/// Dispatches a key through Flutter's focused widget, without OS keyboard input.
class KeyPress extends Command {
  KeyPress(this.keyId, this.usbHidUsage, {super.timeout});

  KeyPress.deserialize(super.json)
    : keyId = int.parse(json['keyId']!),
      usbHidUsage = int.parse(json['usbHidUsage']!),
      super.deserialize();

  final int keyId;
  final int usbHidUsage;

  @override
  String get kind => 'KeyPress';

  @override
  Map<String, String> serialize() => {
    ...super.serialize(),
    'keyId': '$keyId',
    'usbHidUsage': '$usbHidUsage',
  };
}
