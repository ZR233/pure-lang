import 'dart:async';

import '../frb/studio_api.dart';

/// Thread 状态流协调器：同一时刻只有一个选中会话的活动订阅。
///
/// 切换会话经串行 barrier 执行：先等待旧订阅真正取消，再建立新订阅；切换后的
/// 迟到帧按 generation 拒绝。断流由调用方按 typed 状态决定有界重订阅。
class ThreadStreamCoordinator {
  ThreadStreamCoordinator(this._api, this._onFrame, this._onDisconnected);

  final StudioBridgeDataSource _api;
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
      // 先等待旧订阅真正取消：旧会话的迟到帧不能进入新会话的 generation。
      final oldSubscription = _subscription;
      _subscription = null;
      await oldSubscription?.cancel();
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

  Future<void> get switchBarrier => _switchBarrier;

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
