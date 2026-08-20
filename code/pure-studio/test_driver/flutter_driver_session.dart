import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

typedef FlutterDriverConnector = Future<FlutterDriverClient> Function(
  String vmServiceUrl,
);
typedef SnapshotReconnectObserver = FutureOr<void> Function(
  SnapshotReconnectEvent event,
);

abstract interface class FlutterDriverClient {
  Future<void> checkHealth();

  Future<void> setFrameSync(bool enabled);

  Future<String> requestData(String message, {Duration? timeout});

  Future<void> waitFor(SerializableFinder finder, {Duration? timeout});

  Future<void> waitForAbsent(SerializableFinder finder, {Duration? timeout});

  Future<void> tap(SerializableFinder finder);

  Future<void> enterText(String text);

  Future<String> getText(SerializableFinder finder);

  Future<void> waitUntilNoTransientCallbacks({Duration? timeout});

  Future<void> waitForNoPendingFrame({Duration? timeout});

  Future<void> sendTextInputAction(TextInputAction action, {Duration? timeout});

  Future<String> renderTree();

  Future<List<int>> screenshot();

  Future<void> close();
}

class SnapshotReconnectEvent {
  const SnapshotReconnectEvent({
    required this.attempt,
    required this.phase,
    required this.error,
    required this.capturedAt,
    this.delay,
  });

  final int attempt;
  final String phase;
  final String error;
  final DateTime capturedAt;
  final Duration? delay;

  Map<String, Object?> toJson() => {
    'kind': 'driverReconnect',
    'attempt': attempt,
    'phase': phase,
    'error': error,
    'delayMs': delay?.inMilliseconds,
    'capturedAt': capturedAt.toUtc().toIso8601String(),
  };
}

class FlutterDriverSession {
  FlutterDriverSession._(
    this._vmServiceUrl,
    this._connector,
    this._client,
    this._delay,
    this._onReconnect,
  );

  static const reconnectBackoff = [
    Duration(milliseconds: 250),
    Duration(milliseconds: 500),
    Duration(seconds: 1),
  ];

  final String _vmServiceUrl;
  final FlutterDriverConnector _connector;
  final Future<void> Function(Duration) _delay;
  final SnapshotReconnectObserver? _onReconnect;
  FlutterDriverClient _client;

  static Future<FlutterDriverSession> connect({
    required String vmServiceUrl,
    FlutterDriverConnector connector = connectFlutterDriverClient,
    Future<void> Function(Duration) delay = Future<void>.delayed,
    SnapshotReconnectObserver? onReconnect,
  }) async {
    final client = await connector(vmServiceUrl)
        .timeout(const Duration(seconds: 30));
    await client.checkHealth().timeout(const Duration(seconds: 15));
    await client.setFrameSync(false).timeout(const Duration(seconds: 15));
    return FlutterDriverSession._(
      vmServiceUrl,
      connector,
      client,
      delay,
      onReconnect,
    );
  }

  Future<Map<String, dynamic>> readSnapshot() async {
    Object? lastError;
    StackTrace? lastStackTrace;
    try {
      return await _requestSnapshot();
    } catch (error, stackTrace) {
      if (!isReconnectableReadFailure(error)) rethrow;
      lastError = error;
      lastStackTrace = stackTrace;
    }

    for (var index = 0; index < reconnectBackoff.length; index++) {
      final attempt = index + 1;
      final backoff = reconnectBackoff[index];
      await _reportReconnect(
        SnapshotReconnectEvent(
          attempt: attempt,
          phase: 'starting',
          error: '$lastError',
          delay: backoff,
          capturedAt: DateTime.now(),
        ),
      );
      await _closeClientBestEffort();
      await _delay(backoff);
      try {
        final replacement = await _connector(_vmServiceUrl)
            .timeout(const Duration(seconds: 30));
        await replacement.checkHealth().timeout(const Duration(seconds: 15));
        await replacement
            .setFrameSync(false)
            .timeout(const Duration(seconds: 15));
        _client = replacement;
        final snapshot = await _requestSnapshot();
        await _reportReconnect(
          SnapshotReconnectEvent(
            attempt: attempt,
            phase: 'succeeded',
            error: '$lastError',
            capturedAt: DateTime.now(),
          ),
        );
        return snapshot;
      } catch (error, stackTrace) {
        lastError = error;
        lastStackTrace = stackTrace;
        await _reportReconnect(
          SnapshotReconnectEvent(
            attempt: attempt,
            phase: 'failed',
            error: '$error',
            capturedAt: DateTime.now(),
          ),
        );
        if (!isReconnectableReadFailure(error)) {
          Error.throwWithStackTrace(error, stackTrace);
        }
      }
    }
    Error.throwWithStackTrace(lastError!, lastStackTrace!);
  }

  Future<void> waitFor(SerializableFinder finder, {Duration? timeout}) {
    return _client.waitFor(finder, timeout: timeout);
  }

  Future<void> waitForAbsent(SerializableFinder finder, {Duration? timeout}) {
    return _client.waitForAbsent(finder, timeout: timeout);
  }

  Future<void> tap(SerializableFinder finder) => _client.tap(finder);

  Future<void> enterText(String text) => _client.enterText(text);

  Future<String> getText(SerializableFinder finder) => _client.getText(finder);

  Future<void> waitUntilNoTransientCallbacks({Duration? timeout}) {
    return _client.waitUntilNoTransientCallbacks(timeout: timeout);
  }

  Future<void> waitForNoPendingFrame({Duration? timeout}) {
    return _client.waitForNoPendingFrame(timeout: timeout);
  }

  Future<void> sendTextInputAction(
    TextInputAction action, {
    Duration? timeout,
  }) {
    return _client.sendTextInputAction(action, timeout: timeout);
  }

  Future<String> requestData(String message, {Duration? timeout}) {
    return _client.requestData(message, timeout: timeout);
  }

  Future<String> renderTree() => _client.renderTree();

  Future<List<int>> screenshot() => _client.screenshot();

  Future<void> close() => _client.close();

  /// Closes only the current observation connection for acceptance testing.
  ///
  /// The next read-only snapshot must reconnect; no product action is replayed.
  Future<void> disconnectObservationForAcceptance() => _closeClientBestEffort();

  Future<Map<String, dynamic>> _requestSnapshot() async {
    final raw = await _client
        .requestData('snapshot', timeout: const Duration(seconds: 15))
        .timeout(const Duration(seconds: 15));
    final decoded = jsonDecode(raw);
    if (decoded is! Map<String, dynamic>) {
      throw const FormatException('snapshot response must be a JSON object');
    }
    return decoded;
  }

  Future<void> _reportReconnect(SnapshotReconnectEvent event) async {
    final observer = _onReconnect;
    if (observer != null) await observer(event);
  }

  Future<void> _closeClientBestEffort() async {
    try {
      await _client.close().timeout(const Duration(seconds: 2));
    } on Object {
      // A broken observation connection may already be disposed.
    }
  }
}

bool isReconnectableReadFailure(Object error) {
  if (error is SocketException) return true;
  final message = error.toString().toLowerCase();
  return const [
    'connection closed',
    'connection reset',
    'connection disposed',
    'already closed',
    'websocket is not open',
    'websocketchannel',
    'vm service connection',
  ].any(message.contains);
}

Future<FlutterDriverClient> connectFlutterDriverClient(
  String vmServiceUrl,
) async {
  final driver = await FlutterDriver.connect(
    dartVmServiceUrl: vmServiceUrl,
    printCommunication: false,
    logCommunicationToFile: false,
  );
  return _RealFlutterDriverClient(driver);
}

class _RealFlutterDriverClient implements FlutterDriverClient {
  const _RealFlutterDriverClient(this._driver);

  final FlutterDriver _driver;

  @override
  Future<void> checkHealth() async {
    await _driver.checkHealth();
  }

  @override
  Future<void> setFrameSync(bool enabled) async {
    await _driver.sendCommand(SetFrameSync(enabled));
  }

  @override
  Future<String> requestData(String message, {Duration? timeout}) {
    return _driver.requestData(message, timeout: timeout);
  }

  @override
  Future<void> waitFor(SerializableFinder finder, {Duration? timeout}) {
    return _driver.waitFor(finder, timeout: timeout);
  }

  @override
  Future<void> waitForAbsent(SerializableFinder finder, {Duration? timeout}) {
    return _driver.waitForAbsent(finder, timeout: timeout);
  }

  @override
  Future<void> tap(SerializableFinder finder) => _driver.tap(finder);

  @override
  Future<void> enterText(String text) => _driver.enterText(text);

  @override
  Future<String> getText(SerializableFinder finder) => _driver.getText(finder);

  @override
  Future<void> waitUntilNoTransientCallbacks({Duration? timeout}) {
    return _driver.waitUntilNoTransientCallbacks(timeout: timeout);
  }

  @override
  Future<void> waitForNoPendingFrame({Duration? timeout}) {
    return _driver.waitForCondition(const NoPendingFrame(), timeout: timeout);
  }

  @override
  Future<void> sendTextInputAction(
    TextInputAction action, {
    Duration? timeout,
  }) {
    return _driver.sendTextInputAction(action, timeout: timeout);
  }

  @override
  Future<String> renderTree() async {
    final tree = await _driver.getRenderTree();
    return tree.tree ?? '';
  }

  @override
  Future<List<int>> screenshot() => _driver.screenshot();

  @override
  Future<void> close() => _driver.close();
}
