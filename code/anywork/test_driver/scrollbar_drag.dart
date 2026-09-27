import 'package:flutter_driver/flutter_driver.dart';

/// Drags the actual painted thumb with a mouse; never writes a scroll position.
class ScrollbarDrag extends CommandWithTarget {
  ScrollbarDrag(super.finder, this.dy, {super.timeout});

  ScrollbarDrag.deserialize(super.json, super.finderFactory)
    : dy = double.parse(json['dy']!),
      super.deserialize();

  final double dy;

  @override
  String get kind => 'ScrollbarDrag';

  @override
  Map<String, String> serialize() => {...super.serialize(), 'dy': '$dy'};
}
