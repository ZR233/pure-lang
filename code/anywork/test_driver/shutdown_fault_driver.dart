// Driver-only subscription-fault entrypoint for the shutdown acceptance.
//
// It reuses the frozen [driver_main] entrypoint (same Driver requests, same real
// bridge bootstrap) and, after the first frame is mounted, re-installs the very
// same [StudioExitCoordinator] implementation over a [FrbStudioBridgeDataSource] subclass that
// overrides *only* `subscribeShutdownProgress`. Every other bridge member is
// inherited unchanged, so the real runtime is still shut down; only the progress
// subscription fails (and its cancel hangs), which must surface as a typed
// Degraded report and a non-zero exit rather than a fabricated `Stopped`.
//
// This is a Driver-only entrypoint; product and release builds use lib/main.dart.
// ignore_for_file: invalid_use_of_visible_for_testing_member

import 'dart:async';

import 'package:anywork/src/app/studio_host_lifecycle.dart';
import 'package:anywork/src/data/frb/studio_api.dart';
import 'package:anywork/src/domain/models/studio_models.dart';
import 'package:flutter/scheduler.dart';

import 'driver_main.dart' as base;

void main() {
  base.main();
  // Install the fault coordinator after the app is actually mounted, so it owns
  // the window-close request path as well as the Driver exit request.
  SchedulerBinding.instance.addPostFrameCallback((_) {
    StudioExitCoordinator.install(
      StudioExitCoordinator(_FaultSubscriptionApi(), (_) {}),
    );
  });
}

/// A real `FrbStudioBridgeDataSource` whose shutdown-progress subscription is faulty.
///
/// `shutdownRuntime` and every other member are inherited from the real bridge,
/// so the observable difference is exactly the broken progress channel: the
/// subscription emits an error (a typed `progress` issue) and never completes
/// its cancel (a typed `timeout` issue on the bounded cancel).
class _FaultSubscriptionApi extends FrbStudioBridgeDataSource {
  @override
  Stream<StudioShutdownProgress> subscribeShutdownProgress() {
    late final StreamController<StudioShutdownProgress> controller;
    controller = StreamController<StudioShutdownProgress>(
      onListen: () {
        controller.addError(
          StateError('driver-injected shutdown progress subscription failure'),
          StackTrace.current,
        );
      },
      // A hung cancel: the coordinator's bounded cancel must record a typed
      // timeout issue instead of swallowing it or faking a clean shutdown.
      onCancel: () => Completer<void>().future,
    );
    return controller.stream;
  }
}
