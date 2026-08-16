import 'dart:async';
import 'dart:ui';

import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pure_studio/src/app/pure_studio_app.dart';
import 'package:pure_studio/src/data/frb/studio_api.dart';
import 'package:pure_studio/src/data/repositories/studio_repository.dart';

void main() {
  testWidgets('desktop exit waits for one shared runtime shutdown', (
    tester,
  ) async {
    final shutdown = Completer<void>();
    var shutdownCalls = 0;
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(DemoStudioApi())],
        child: StudioLifecycleCoordinator(
          shutdown: () {
            shutdownCalls += 1;
            return shutdown.future;
          },
          child: const SizedBox(),
        ),
      ),
    );

    var exitCompleted = false;
    final exit = tester.binding.handleRequestAppExit().then((response) {
      exitCompleted = true;
      return response;
    });
    await tester.pump();

    expect(shutdownCalls, 1);
    expect(exitCompleted, isFalse);

    shutdown.complete();
    await tester.pump();
    expect(await exit, AppExitResponse.exit);

    await tester.pumpWidget(const SizedBox());
    expect(shutdownCalls, 1);
  });
}
