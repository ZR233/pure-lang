// A Flutter Driver command that dispatches a tap directly on the finder's
// center without the `hitTestable()` pre-filter that the stock `Tap` command
// applies.
//
// On the Linux desktop embedder used for native GUI acceptance the stock
// `hitTestable()` finder yields no candidates for every widget (its internal
// `WidgetsBinding.hitTestInView` path finds nothing), so `Tap`/`waitForTappable`
// block until their timeout. The engine still routes real pointer events to the
// hit-tested widget during normal gesture dispatch. The handler therefore checks
// the target's actual RenderView, including modal barriers, before dispatching
// one tap in that same view and waiting for its focus update.
//
// This file must stay importable from a plain `dart run` client: it only
// depends on `package:flutter_driver/flutter_driver.dart`.
import 'package:flutter_driver/flutter_driver.dart';

class RawTap extends CommandWithTarget {
  RawTap(super.finder, {super.timeout});

  RawTap.deserialize(super.json, super.finderFactory) : super.deserialize();

  @override
  String get kind => 'RawTap';
}
