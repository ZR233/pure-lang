import 'package:flutter_driver/flutter_driver.dart';

/// Native manual observation of expansion and nested pointer scrolling.
class ToolProbe extends CommandWithTarget {
  ToolProbe(super.finder, this.action, {this.dy = 0});
  ToolProbe.deserialize(super.json, super.finderFactory)
    : action = json['action']!,
      dy = double.parse(json['dy']!),
      super.deserialize();
  final String action;
  final double dy;
  @override
  String get kind => 'ToolProbe';
  @override
  Map<String, String> serialize() => {
    ...super.serialize(),
    'action': action,
    'dy': '$dy',
  };
}
