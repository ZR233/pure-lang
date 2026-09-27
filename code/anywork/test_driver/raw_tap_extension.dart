// App-side handler for [RawTap]. Registered from `driver_main.dart`.
//
// The handler resolves the target's global center, waits until that center stops
// moving across consecutive frames and is hit-testable through the current
// overlays, then dispatches exactly one tap. A stationary underlying target can
// still be blocked by a departing dialog's modal barrier.
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
    var center = _targetOf(finder)?.center;
    final deadline = DateTime.now().add(
      command.timeout ?? const Duration(seconds: 30),
    );
    while (true) {
      await _nextFrame();
      if (!DateTime.now().isBefore(deadline)) {
        throw StateError(
          'RawTap target did not become stable and hit-testable',
        );
      }
      final next = _targetOf(finder);
      if (next == null) continue;
      final hit = HitTestResult();
      // Use the target's actual render view. The stock finder dispatches through
      // the test binding's view registry, which is empty on the Linux embedder.
      next.view.hitTest(hit, position: next.center);
      final hittable = hit.path.any(
        (entry) => isRenderObjectAncestorOfTarget(next.object, entry.target),
      );
      if (next.center == center && hittable) {
        await prober.tapAt(next.center, view: next.view.flutterView);
        // Focus changes are deferred; complete their frame before enterText can
        // address the previously focused dialog's text-input connection.
        await _nextFrame();
        return Result.empty;
      }
      center = next.center;
    }
  }
}

({Offset center, RenderView view, RenderBox object})? _targetOf(Finder finder) {
  final elements = finder.evaluate().toList();
  if (elements.length != 1) return null;
  final object = elements.single.renderObject;
  if (object is! RenderBox || !object.attached || !object.hasSize) return null;
  RenderObject root = object;
  while (root.parent != null) {
    root = root.parent!;
  }
  if (root is! RenderView) return null;
  return (
    center: object.localToGlobal(object.size.center(Offset.zero)),
    view: root,
    object: object,
  );
}

Future<void> _nextFrame() {
  final binding = SchedulerBinding.instance;
  binding.scheduleFrame();
  return binding.endOfFrame;
}
