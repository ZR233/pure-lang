import 'package:flutter_driver/flutter_driver.dart';

/// A real mouse-wheel signal, including on a list with no scroll extent.
class PointerScroll extends CommandWithTarget {
  PointerScroll(super.finder, this.dy, {super.timeout});

  PointerScroll.deserialize(super.json, super.finderFactory)
    : dy = double.parse(json['dy']!),
      super.deserialize();

  final double dy;

  @override
  String get kind => 'PointerScroll';

  @override
  Map<String, String> serialize() => {...super.serialize(), 'dy': '$dy'};
}
