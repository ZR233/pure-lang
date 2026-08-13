import 'package:flutter_driver/flutter_driver.dart' as driver;
import 'package:flutter_test/flutter_test.dart';

import '../test_driver/flutter_driver_session.dart';
import '../test_driver/task_acceptance_driver.dart';

void main() {
  test('prompt postcondition accepts a fast terminal planning Turn', () {
    expect(
      hasSubmittedTaskPrompt({
        'planContent': '# Plan',
        'task': null,
        'workspace': {
          'turn': null,
          'activeInteraction': {'kind': 'planConfirmation'},
        },
      }),
      isTrue,
    );
    expect(
      hasSubmittedTaskPrompt({
        'planContent': null,
        'task': null,
        'workspace': {'turn': null, 'activeInteraction': null},
      }),
      isFalse,
    );
  });

  test('snapshot reconnects after a closed observation connection', () async {
    final first = _FakeDriverClient(
      snapshotError: StateError('connection closed'),
    );
    final second = _FakeDriverClient(snapshot: '{"task":{"phase":"running"}}');
    final clients = [first, second];
    final delays = <Duration>[];
    final events = <SnapshotReconnectEvent>[];
    var connectCount = 0;
    final session = await FlutterDriverSession.connect(
      vmServiceUrl: 'http://vm.test',
      connector: (_) async => clients[connectCount++],
      delay: (delay) async => delays.add(delay),
      onReconnect: events.add,
    );

    final snapshot = await session.readSnapshot();

    expect(snapshot['task'], {'phase': 'running'});
    expect(connectCount, 2);
    expect(first.requestCount, 1);
    expect(second.requestCount, 1);
    expect(first.closeCount, 1);
    expect(first.healthCount, 1);
    expect(second.healthCount, 1);
    expect(first.frameSyncValues, [false]);
    expect(second.frameSyncValues, [false]);
    expect(delays, [const Duration(milliseconds: 250)]);
    expect(events.map((event) => event.phase), ['starting', 'succeeded']);
  });

  test(
    'acceptance disconnect closes only the replaceable observation',
    () async {
      final first = _FakeDriverClient(failSnapshotAfterClose: true);
      final second = _FakeDriverClient(
        snapshot: '{"task":{"phase":"running"}}',
      );
      final clients = [first, second];
      var connectCount = 0;
      final session = await FlutterDriverSession.connect(
        vmServiceUrl: 'http://vm.test',
        connector: (_) async => clients[connectCount++],
        delay: (_) async {},
      );

      await session.disconnectObservationForAcceptance();
      final snapshot = await session.readSnapshot();

      expect(snapshot['task'], {'phase': 'running'});
      expect(connectCount, 2);
      expect(first.closeCount, 2);
      expect(second.requestCount, 1);
    },
  );

  test('side effect commands are never reconnected or replayed', () async {
    final client = _FakeDriverClient(tapError: StateError('connection closed'));
    var connectCount = 0;
    final session = await FlutterDriverSession.connect(
      vmServiceUrl: 'http://vm.test',
      connector: (_) async {
        connectCount += 1;
        return client;
      },
      delay: (_) async {},
    );

    await expectLater(
      session.tap(driver.find.byValueKey('side-effect')),
      throwsA(isA<StateError>()),
    );

    expect(connectCount, 1);
    expect(client.tapCount, 1);
  });

  test('non-transport snapshot errors do not reconnect', () async {
    final client = _FakeDriverClient(
      snapshotError: const FormatException('bad'),
    );
    var connectCount = 0;
    final session = await FlutterDriverSession.connect(
      vmServiceUrl: 'http://vm.test',
      connector: (_) async {
        connectCount += 1;
        return client;
      },
      delay: (_) async {},
    );

    await expectLater(session.readSnapshot(), throwsFormatException);
    expect(connectCount, 1);
  });

  test(
    'snapshot reconnect is bounded to three replacement connections',
    () async {
      final clients = List.generate(
        4,
        (_) => _FakeDriverClient(snapshotError: StateError('connection reset')),
      );
      var connectCount = 0;
      final delays = <Duration>[];
      final session = await FlutterDriverSession.connect(
        vmServiceUrl: 'http://vm.test',
        connector: (_) async => clients[connectCount++],
        delay: (delay) async => delays.add(delay),
      );

      await expectLater(session.readSnapshot(), throwsA(isA<StateError>()));

      expect(connectCount, 4);
      expect(delays, FlutterDriverSession.reconnectBackoff);
      expect(clients.map((client) => client.requestCount), [1, 1, 1, 1]);
    },
  );
}

class _FakeDriverClient implements FlutterDriverClient {
  _FakeDriverClient({
    this.snapshot = '{}',
    this.snapshotError,
    this.tapError,
    this.failSnapshotAfterClose = false,
  });

  final String snapshot;
  final Object? snapshotError;
  final Object? tapError;
  final bool failSnapshotAfterClose;
  int healthCount = 0;
  int requestCount = 0;
  int closeCount = 0;
  int tapCount = 0;
  final frameSyncValues = <bool>[];

  @override
  Future<void> checkHealth() async {
    healthCount += 1;
  }

  @override
  Future<void> setFrameSync(bool enabled) async {
    frameSyncValues.add(enabled);
  }

  @override
  Future<String> requestData(String message, {Duration? timeout}) async {
    requestCount += 1;
    if (failSnapshotAfterClose && closeCount > 0) {
      throw StateError('connection disposed');
    }
    if (snapshotError case final error?) throw error;
    return snapshot;
  }

  @override
  Future<void> tap(driver.SerializableFinder finder) async {
    tapCount += 1;
    if (tapError case final error?) throw error;
  }

  @override
  Future<void> close() async {
    closeCount += 1;
  }

  @override
  Future<void> enterText(String text) async {}

  @override
  Future<String> getText(driver.SerializableFinder finder) async => '';

  @override
  Future<String> renderTree() async => '';

  @override
  Future<List<int>> screenshot() async => const [];

  @override
  Future<void> sendTextInputAction(
    driver.TextInputAction action, {
    Duration? timeout,
  }) async {}

  @override
  Future<void> waitFor(
    driver.SerializableFinder finder, {
    Duration? timeout,
  }) async {}

  @override
  Future<void> waitUntilNoTransientCallbacks({Duration? timeout}) async {}

  @override
  Future<void> waitForNoPendingFrame({Duration? timeout}) async {}
}
