// App-side handler for [RawTap]. Registered from `driver_main.dart`.
//
// The handler resolves the target's global center, waits until that center stops
// moving across consecutive frames (menu and route animations), and only then
// dispatches the pointer events. Without that settle step a tap issued while the
// popup menu is still animating lands on the route barrier and is dropped.
import 'package:flutter/rendering.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_driver/flutter_driver.dart';
import 'package:flutter_test/flutter_test.dart';

import 'raw_tap.dart';

class RawTapCommandExtension extends CommandExtension {
  @override
  String get commandKind => 'RawTap';

  @override
  Command deserialize(
    Map<String, String> params,
    DeserializeFinderFactory finderFactory,
    DeserializeCommandFactory commandFactory,
  ) => RawTap.deserialize(params, finderFactory);

  @override
  Future<Result> call(
    Command command,
    WidgetController prober,
    CreateFinderFactory finderFactory,
    CommandHandlerFactory handlerFactory,
  ) async {
    final finder = finderFactory.createFinder(
      (command as CommandWithTarget).finder,
    );
    var center = _centerOf(finder);
    for (var poll = 0; poll < 60; poll++) {
      await _nextFrame();
      final next = _centerOf(finder);
      if (next == null) continue;
      if (next == center) break;
      center = next;
    }
    final target = center ?? _centerOf(finder);
    if (target == null) {
      throw StateError('RawTap target is never laid out');
    }
    await prober.tapAt(target);
    return Result.empty;
  }
}

Offset? _centerOf(Finder finder) {
  try {
    final renderObject = finder.evaluate().first.renderObject;
    if (renderObject is! RenderBox) return null;
    return renderObject.localToGlobal(renderObject.size.center(Offset.zero));
  } on Object {
    return null;
  }
}

Future<void> _nextFrame() {
  final binding = SchedulerBinding.instance;
  binding.scheduleFrame();
  return binding.endOfFrame;
}
