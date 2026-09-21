import 'dart:async';

import '../frb/studio_api.dart';

/// Product 流协调器：把流终止（bridge 的 `failure`/`closed`）当作显式 stale，
/// 先做一次全量 snapshot 重同步，再有界重订阅，避免目录/状态更新静默停止。
class ProductStreamCoordinator {
  ProductStreamCoordinator(this._api, this._onEvent, this._onStale);

  /// 单次终止允许的重订阅次数：runtime 已关闭时不会无限重试。
  static const int maxResubscribeAttempts = 3;
  static const Duration resubscribeDelay = Duration(milliseconds: 150);

  final StudioApi _api;
  final void Function(Object event) _onEvent;

  /// 流终止后的重同步入口：客户端不能证明增量连续，必须重新读取 canonical snapshot。
  final void Function() _onStale;
  StreamSubscription<Object>? _subscription;
  Timer? _resubscribeTimer;
  int _resubscribeAttempts = 0;
  bool _disposed = false;

  void start() {
    if (_disposed) return;
    _subscription ??= _api.subscribeProductEvents().listen(
      (event) {
        _resubscribeAttempts = 0;
        _onEvent(event);
      },
      onError: (Object error, StackTrace stackTrace) => _onTerminated(),
      onDone: _onTerminated,
    );
  }

  Future<void> dispose() async {
    _disposed = true;
    _resubscribeTimer?.cancel();
    _resubscribeTimer = null;
    final subscription = _subscription;
    _subscription = null;
    await subscription?.cancel();
  }

  void _onTerminated() {
    _subscription = null;
    if (_disposed) return;
    _onStale();
    if (_resubscribeAttempts >= maxResubscribeAttempts) return;
    _resubscribeAttempts += 1;
    _resubscribeTimer?.cancel();
    _resubscribeTimer = Timer(resubscribeDelay, () {
      _resubscribeTimer = null;
      if (_disposed) return;
      start();
    });
  }
}

class ThreadStreamCoordinator {
  ThreadStreamCoordinator(this._api, this._onFrame, this._onDisconnected);

  final StudioApi _api;
  final void Function(ThreadStreamFrame frame, String threadId, int generation)
  _onFrame;
  final void Function(String threadId, int generation, Object? error)
  _onDisconnected;

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
            onError: (Object error, StackTrace _) =>
                _onDisconnected(threadId, generation, error),
            onDone: () => _onDisconnected(threadId, generation, null),
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
