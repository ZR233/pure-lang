import 'package:flutter/gestures.dart';
import 'package:flutter/scheduler.dart';
import 'package:flutter_driver/driver_extension.dart';
import 'package:flutter_driver/flutter_driver.dart';
import 'package:flutter_test/flutter_test.dart';

import 'pointer_scroll.dart';

class PointerScrollCommandExtension extends CommandExtension {
  @override
  String get commandKind => 'PointerScroll';

  @override
  Command deserialize(
    Map<String, String> params,
    DeserializeFinderFactory finderFactory,
    DeserializeCommandFactory commandFactory,
  ) => PointerScroll.deserialize(params, finderFactory);

  @override
  Future<Result> call(
    Command command,
    WidgetController prober,
    CreateFinderFactory finderFactory,
    CommandHandlerFactory handlerFactory,
  ) async {
    final scroll = command as PointerScroll;
    final finder = finderFactory.createFinder(scroll.finder);
    GestureBinding.instance.handlePointerEvent(
      PointerScrollEvent(
        position: prober.getCenter(finder),
        scrollDelta: Offset(0, scroll.dy),
        kind: PointerDeviceKind.mouse,
      ),
    );
    SchedulerBinding.instance.scheduleFrame();
    await SchedulerBinding.instance.endOfFrame;
    return Result.empty;
  }
}
