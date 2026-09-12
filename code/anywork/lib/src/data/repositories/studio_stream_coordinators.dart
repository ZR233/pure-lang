import 'dart:async';

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

class ThreadStreamCoordinator {
  ThreadStreamCoordinator(this._api, this._onFrame, this._onDisconnected);

  final StudioApi _api;
  final void Function(ThreadStreamFrame frame, String threadId, int generation)
  _onFrame;
  final void Function(String threadId, int generation) _onDisconnected;

  StreamSubscription<ThreadStreamFrame>? _subscription;
  Timer? _resubscribeTimer;
  Future<void> _switchBarrier = Future<void>.value();
  int _generation = 0;
  bool _disposed = false;

  int get generation => _generation;

  int switchThread(String? threadId) {
    _resubscribeTimer?.cancel();
    _resubscribeTimer = null;
    final generation = ++_generation;
    final operation = _switchBarrier.then((_) async {
      final oldSubscription = _subscription;
      _subscription = null;
      unawaited(oldSubscription?.cancel());
      if (_disposed || generation != _generation || threadId == null) return;
      _subscription = _api
          .subscribeThread(threadId)
          .listen(
            (frame) => _onFrame(frame, threadId, generation),
            onError: (_, _) => _onDisconnected(threadId, generation),
            onDone: () => _onDisconnected(threadId, generation),
          );
    });
    _switchBarrier = operation.then<void>((_) {}, onError: (_, _) {});
    return generation;
  }

  void scheduleResubscribe({
    required String threadId,
    required int generation,
    required bool Function() isCurrent,
    required void Function() resubscribe,
  }) {
    if (_disposed || generation != _generation || !isCurrent()) return;
    _resubscribeTimer?.cancel();
    _resubscribeTimer = Timer(const Duration(milliseconds: 150), () {
      if (_disposed || generation != _generation || !isCurrent()) return;
      resubscribe();
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
