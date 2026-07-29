import 'dart:async';

import 'package:flutter/scheduler.dart';

import '../frb/studio_api.dart';

class ProductStreamCoordinator {
  ProductStreamCoordinator(this._api, this._onEvent);

  final StudioApi _api;
  final void Function(Object event) _onEvent;
  StreamSubscription<Object>? _subscription;

  void start() {
    _subscription ??= _api.subscribeProductEvents().listen(_onEvent);
  }

  Future<void> dispose() async {
    final subscription = _subscription;
    _subscription = null;
    await subscription?.cancel();
  }
}

class SessionStreamCoordinator {
  SessionStreamCoordinator(this._api, this._onFrame, this._onDisconnected);

  final StudioApi _api;
  final void Function(
    SessionStreamFrame frame,
    String sessionId,
    int generation,
  )
  _onFrame;
  final void Function(String sessionId, int generation) _onDisconnected;

  StreamSubscription<SessionStreamFrame>? _subscription;
  Timer? _resubscribeTimer;
  Future<void> _switchBarrier = Future<void>.value();
  int _generation = 0;
  bool _disposed = false;

  int get generation => _generation;

  Future<void> switchSession(String? sessionId, {int? afterSequence}) {
    _resubscribeTimer?.cancel();
    _resubscribeTimer = null;
    final generation = ++_generation;
    final operation = _switchBarrier.then((_) async {
      final oldSubscription = _subscription;
      _subscription = null;
      await oldSubscription?.cancel();
      if (_disposed || generation != _generation || sessionId == null) {
        return;
      }
      _subscription = _api
          .subscribeSessionEvents(sessionId, afterSequence: afterSequence)
          .listen(
            (frame) => _onFrame(frame, sessionId, generation),
            onError: (_, _) => _onDisconnected(sessionId, generation),
            onDone: () => _onDisconnected(sessionId, generation),
          );
    });
    _switchBarrier = operation.then<void>((_) {}, onError: (_, _) {});
    return operation;
  }

  void scheduleResync({
    required String sessionId,
    required int generation,
    required bool Function() isCurrent,
  }) {
    if (_disposed || generation != _generation || !isCurrent()) {
      return;
    }
    _resubscribeTimer?.cancel();
    _resubscribeTimer = Timer(const Duration(milliseconds: 150), () {
      if (_disposed || generation != _generation || !isCurrent()) {
        return;
      }
      unawaited(switchSession(sessionId));
    });
  }

  Future<void> dispose() async {
    _disposed = true;
    _generation += 1;
    _resubscribeTimer?.cancel();
    _resubscribeTimer = null;
    await _switchBarrier;
    final subscription = _subscription;
    _subscription = null;
    await subscription?.cancel();
  }
}

class PartDeltaBatcher {
  PartDeltaBatcher(
    this._onFlush, {
    void Function(FrameCallback callback)? scheduleFrame,
  }) : _scheduleFrame =
           scheduleFrame ?? SchedulerBinding.instance.scheduleFrameCallback;

  final void Function(List<StudioBridgeEvent> events) _onFlush;
  final void Function(FrameCallback callback) _scheduleFrame;
  final List<StudioBridgeEvent> _pending = [];
  bool _frameScheduled = false;

  void add(StudioBridgeEvent event) {
    _pending.add(event);
    if (_frameScheduled) {
      return;
    }
    _frameScheduled = true;
    _scheduleFrame((_) {
      _frameScheduled = false;
      flush();
    });
  }

  void flush() {
    if (_pending.isEmpty) {
      _frameScheduled = false;
      return;
    }
    final events = List<StudioBridgeEvent>.of(_pending);
    _pending.clear();
    _frameScheduled = false;
    _onFlush(events);
  }

  void dispose() {
    _pending.clear();
    _frameScheduled = false;
  }
}
