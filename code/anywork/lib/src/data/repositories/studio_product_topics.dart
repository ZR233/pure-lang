import 'dart:async';

import 'package:flutter/foundation.dart' show debugPrint;

import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';

/// Shell 常驻产品 topics：导航目录、公共配置与诊断 summary。
///
/// 这些领域在 Shell 生命周期内持续可见（侧栏、横幅、更新入口），由 controller
/// 在 bootstrap 完成后一次性持有；其余 topics 只随可见页面/面板按需租用。
const List<ProductTopic> shellProductTopics = [
  ProjectDirectoryTopic(),
  ThreadDirectoryTopic(),
  AgentDirectoryTopic(),
  SettingsConfigTopic(),
  ModelCatalogTopic(),
  RecoveryTopic(),
  PersistenceTopic(),
  UpdaterTopic(),
  ThreadModeCatalogTopic(),
];

/// 设置页可见 tab 的按需 topic 作用域。
///
/// 每个 tab 是独立职责：MCP、LSP、项目 Skills、Provider usage、模型性能、
/// 配置级 Profiles 不合并为一个含糊的 `services` 作用域；隐藏 tab 不持有租约，
/// 只保留 UI 状态。Skills 作用域始终携带当前 project identity。
enum SettingsProductScopeKind { statistics, agents, mcp, lsp, skills, usage }

/// 该设置作用域需要的 topics。
///
/// [selectedProjectId] 为空（或空白）时 Skills 作用域没有可用 topic，返回空列表；
/// 其余作用域与项目无关。
List<ProductTopic> settingsScopeTopics(
  SettingsProductScopeKind kind,
  String? selectedProjectId,
) {
  switch (kind) {
    case SettingsProductScopeKind.statistics:
      return const [ModelPerformanceTopic()];
    case SettingsProductScopeKind.agents:
      return const [AgentProfilesTopic()];
    case SettingsProductScopeKind.mcp:
      return const [McpTopic()];
    case SettingsProductScopeKind.lsp:
      return const [LspTopic()];
    case SettingsProductScopeKind.skills:
      final projectId = selectedProjectId?.trim();
      return projectId == null || projectId.isEmpty
          ? const []
          : [SkillsTopic(projectId: projectId)];
    case SettingsProductScopeKind.usage:
      return const [ProviderUsageTopic()];
  }
}

/// 单个 topic 的共享订阅租约。
///
/// 同一 topic（值相等）的多个租约共享一个底层订阅。帧的统一接线由
/// [ProductTopicRegistry] 单 owner 完成：每个有效帧先进入 canonical reducer
/// （`onFrame`），再广播给 [frames] 供外观察者做局部传输观测；页面不得据
/// [frames] 复制业务投影。[release] 只减少引用，最后一个释放才真正等待底层
/// 订阅取消完成，并清理该 topic 的局部连接状态。
abstract interface class ProductTopicLease {
  ProductTopic get topic;

  /// 本租约有效期内的 topic 帧（Baseline/Data/Lagged/Failure），仅供局部传输观测。
  Stream<ProductTopicFrame> get frames;

  /// 释放本租约；幂等。最后一个释放会等待底层取消/清理真正完成。
  Future<void> release();
}

/// 一组租约；一次性获取、一次性释放，供页面作用域使用。
class ProductTopicLeaseBundle {
  ProductTopicLeaseBundle._(this._leases);

  final List<ProductTopicLease> _leases;
  var _released = false;

  List<ProductTopic> get topics => [for (final lease in _leases) lease.topic];

  /// 逐个 topic 的帧流；消费者按需监听，不需要的 topic 可以不监听。
  Stream<ProductTopicFrame> frames(ProductTopic topic) {
    for (final lease in _leases) {
      if (lease.topic == topic) return lease.frames;
    }
    return const Stream.empty();
  }

  Future<void> release() async {
    if (_released) return;
    _released = true;
    await Future.wait([for (final lease in _leases) lease.release()]);
  }
}

/// 底层订阅显式终止且无错误时的占位事实；调用方据此显示局部错误并保留旧数据。
class ProductTopicClosedError implements Exception {
  const ProductTopicClosedError();

  @override
  String toString() => 'product topic subscription closed';
}

class _TopicSubscription {
  _TopicSubscription(this.topic, this.registry);

  final ProductTopic topic;
  final ProductTopicRegistry registry;

  final StreamController<ProductTopicFrame> controller =
      StreamController<ProductTopicFrame>.broadcast();
  int refs = 0;
  StreamSubscription<ProductTopicFrame>? underlying;

  /// 串行化订阅建立/重建操作；release 等待它，保证最后释放时没有在途创建。
  Future<void> _ops = Future<void>.value();

  /// 当前底层来源代号：每次建立/重建/关闭都前进，旧来源的迟到帧与终止回调全部丢弃。
  int _sourceEpoch = 0;

  /// 有界指数退避：只在建立后稳定 [stableWindow] 才重置预算，快速 Baseline→closed
  /// 不会以固定间隔无限重连；预算耗尽进入 failed 终态，等待显式 retry。
  int _reconnectAttempts = 0;
  bool _exhausted = false;
  Timer? _retryTimer;
  Timer? _stabilityTimer;
  bool _rebuildInFlight = false;
  bool _closing = false;

  ProductTopicConnectionStateView? _connection;

  static const Duration _baseRetryDelay = Duration(milliseconds: 250);
  static const Duration _maxRetryDelay = Duration(seconds: 5);
  static const int _maxReconnectAttempts = 5;
  static const Duration _stableWindow = Duration(seconds: 10);

  /// 活动身份检查：注册表中的当前 entry 且未进入关闭流程。
  /// 重建/取消竞争中被替换或关闭的旧订阅，其迟到帧全部丢弃。
  bool get isActive => registry._entries[topic] == this && !_closing;

  Future<void> start() => _enqueue(() async {
    // 同 topic 上一次 close 的 Rust 句柄取消完成前不建立新订阅：串行等待旧句柄。
    await registry._awaitPriorClose(topic);
    await _establish();
  });

  /// 显式 retry：重置退避预算并从当前订阅身份重新建立（新 Baseline）。
  Future<void> retryNow() async {
    if (!isActive) return;
    _reconnectAttempts = 0;
    _exhausted = false;
    _retryTimer?.cancel();
    _retryTimer = null;
    _stabilityTimer?.cancel();
    _stabilityTimer = null;
    _setConnection(
      const ProductTopicConnectionStateView(
        phase: ProductTopicConnectionPhase.connecting,
      ),
    );
    await _recreate();
  }

  Future<void> _enqueue(Future<void> Function() operation) {
    final chained = _ops.then(
      (_) => operation(),
      onError: (Object _, StackTrace _) {},
    );
    _ops = chained.catchError((Object _) {});
    return chained;
  }

  Future<void> _establish() async {
    if (!isActive) return;
    final epoch = ++_sourceEpoch;
    try {
      final frames = registry._api.subscribeProductTopic(topic);
      final subscription = frames.listen(
        (frame) => _onFrame(frame, epoch),
        onError: (Object error, StackTrace _) => _onTerminated(epoch, error),
        onDone: () => _onTerminated(epoch, null),
        cancelOnError: true,
      );
      if (!isActive || epoch != _sourceEpoch) {
        // 建立期间租约已全部释放或被替换：立即取消刚拿到的订阅并释放句柄。
        await subscription.cancel();
        return;
      }
      underlying = subscription;
      _armStabilityWindow();
    } catch (error) {
      debugPrint('product_topic_subscribe_failed topic=$topic error=$error');
      if (!isActive || epoch != _sourceEpoch) return;
      _setConnection(
        ProductTopicConnectionStateView(
          phase: ProductTopicConnectionPhase.reconnecting,
          errorMessage: error.toString(),
        ),
      );
      registry._dispatch(ProductTopicFailureFrame(topic: topic, error: error));
      _scheduleRetry();
    }
  }

  void _onFrame(ProductTopicFrame frame, int epoch) {
    if (!isActive || epoch != _sourceEpoch) return;
    switch (frame) {
      case ProductTopicBaselineFrame():
      case ProductTopicDataFrame():
        _setConnection(
          const ProductTopicConnectionStateView(
            phase: ProductTopicConnectionPhase.connected,
          ),
        );
      case ProductTopicLaggedFrame():
        _setConnection(
          const ProductTopicConnectionStateView(
            phase: ProductTopicConnectionPhase.reconnecting,
          ),
        );
      case ProductTopicFailureFrame(:final error):
        _setConnection(
          ProductTopicConnectionStateView(
            phase: ProductTopicConnectionPhase.reconnecting,
            errorMessage: error.toString(),
          ),
        );
    }
    // 单一接线：每个有效帧先进入 canonical reducer，再广播给外观察者。
    registry._dispatch(frame);
    controller.add(frame);
    if (frame is ProductTopicLaggedFrame) {
      // Lagged 无法证明增量连续：只重建该 topic（取消旧句柄、重新接收再基线）。
      _scheduleRebuild();
    } else if (frame is ProductTopicFailureFrame) {
      _scheduleRetry();
    }
  }

  void _onTerminated(int epoch, Object? error) {
    if (!isActive || epoch != _sourceEpoch) return;
    final failure = error ?? const ProductTopicClosedError();
    registry._dispatch(ProductTopicFailureFrame(topic: topic, error: failure));
    _setConnection(
      ProductTopicConnectionStateView(
        phase: ProductTopicConnectionPhase.reconnecting,
        errorMessage: failure.toString(),
      ),
    );
    _scheduleRetry();
  }

  /// 建立成功后启动稳定窗口：只有保持稳定 [stableWindow] 才重置退避预算，
  /// 避免“Baseline→closed”快速循环反复刷新预算而无界重连。
  void _armStabilityWindow() {
    _stabilityTimer?.cancel();
    _stabilityTimer = Timer(_stableWindow, () {
      _stabilityTimer = null;
      _reconnectAttempts = 0;
      _exhausted = false;
    });
  }

  void _scheduleRetry() {
    if (!isActive || _exhausted) return;
    _stabilityTimer?.cancel();
    _stabilityTimer = null;
    if (_reconnectAttempts >= _maxReconnectAttempts) {
      _exhausted = true;
      _setConnection(
        const ProductTopicConnectionStateView(
          phase: ProductTopicConnectionPhase.failed,
          retryExhausted: true,
        ),
      );
      debugPrint(
        'product_topic_reconnect_exhausted topic=$topic '
        'attempts=$_reconnectAttempts',
      );
      return;
    }
    final delay = _backoffDelay(_reconnectAttempts);
    _reconnectAttempts += 1;
    _retryTimer?.cancel();
    _retryTimer = Timer(delay, () {
      _retryTimer = null;
      unawaited(_recreate());
    });
  }

  Duration _backoffDelay(int attempt) {
    final doubled = _baseRetryDelay.inMilliseconds * (1 << attempt.clamp(0, 6));
    return doubled > _maxRetryDelay.inMilliseconds
        ? _maxRetryDelay
        : Duration(milliseconds: doubled);
  }

  /// Lagged 触发的即时重建：取消旧句柄、重新订阅，由新的 Baseline 首帧恢复该领域。
  /// 不计数、不重建其他 topic，也不做全量 readStudioState。
  void _scheduleRebuild() {
    if (!isActive) return;
    _retryTimer?.cancel();
    _retryTimer = null;
    unawaited(_recreate());
  }

  Future<void> _recreate() async {
    if (_rebuildInFlight || !isActive) return;
    _rebuildInFlight = true;
    try {
      await _enqueue(() async {
        // 先让当前来源失效：取消期间到达的旧帧不得写入。
        _sourceEpoch += 1;
        final old = underlying;
        underlying = null;
        try {
          await old?.cancel();
        } catch (error) {
          debugPrint('product_topic_cancel_failed topic=$topic error=$error');
        }
        if (_closing || !isActive) return;
        await _establish();
      });
    } catch (error) {
      debugPrint('product_topic_rebuild_failed topic=$topic error=$error');
    } finally {
      _rebuildInFlight = false;
    }
  }

  void _setConnection(ProductTopicConnectionStateView view) {
    if (_connection == view) return;
    _connection = view;
    registry._publishConnection(topic, view);
  }

  /// 最后释放：推进关闭身份（迟到帧被拒）、串行等待在途操作与真正取消。
  Future<void> close() async {
    _closing = true;
    _sourceEpoch += 1;
    _retryTimer?.cancel();
    _retryTimer = null;
    _stabilityTimer?.cancel();
    _stabilityTimer = null;
    final old = underlying;
    underlying = null;
    try {
      await old?.cancel();
    } catch (error) {
      debugPrint('product_topic_cancel_failed topic=$topic error=$error');
    }
    await _ops;
    try {
      await controller.close();
    } on StateError {
      // 已关闭（重复收尾）。
    }
  }
}

/// typed topic 订阅注册表：同 topic 值相等共享一个活动订阅，最后释放等待取消。
///
/// 单 owner 接线：每个有效帧经 [onFrame] 一次进入 canonical reducer；每个 topic 的
/// 局部传输连接状态经 [onTopicConnection]（`null` 表示释放）写入。外观察者可通过
/// 租约的 [ProductTopicLease.frames] 观测同一帧，但业务投影只来自 canonical state。
class ProductTopicRegistry {
  ProductTopicRegistry(
    this._api, {
    required this.onFrame,
    required this.onTopicConnection,
  });

  final StudioBridgeDataSource _api;

  /// 每有效帧一次的统一 reducer 接线（Baseline/Data/Lagged/Failure 都进入）。
  final void Function(ProductTopicFrame frame) onFrame;

  /// 局部传输连接状态更新；`null` 表示该 topic 租约已全部释放。
  final void Function(ProductTopic topic, ProductTopicConnectionStateView? view)
  onTopicConnection;

  final Map<ProductTopic, _TopicSubscription> _entries = {};

  /// 正在等待底层取消完成的旧 entry；同 topic 的新 acquire 建立前会等待它。
  final Map<ProductTopic, Future<void>> _closing = {};
  bool _disposed = false;

  /// 只读诊断：当前活动 topic 与引用计数（不含凭据或内容），供人工核对释放。
  Map<String, int> activeTopicRefCountViews() => {
    for (final entry in _entries.entries)
      describeProductTopic(entry.key): entry.value.refs,
  };

  ProductTopicLease acquire(ProductTopic topic) {
    // disposed 在 release build 也必须拒绝，不能只依赖 assert。
    if (_disposed) {
      throw StateError('ProductTopicRegistry is disposed');
    }
    var entry = _entries[topic];
    if (entry != null && entry._closing) {
      entry = null;
    }
    if (entry == null) {
      entry = _TopicSubscription(topic, this);
      _entries[topic] = entry;
      // Provider evaluation can happen during a widget build (for example when
      // a visible Settings tab first watches its scope). Publishing the
      // connection synchronously would write the controller state while that
      // build is in progress, which Riverpod correctly rejects. Defer the
      // initial transport projection until the current build stack has
      // completed; the lease itself remains established immediately.
      scheduleMicrotask(() {
        if (identical(_entries[topic], entry) &&
            entry != null &&
            !entry._closing) {
          _publishConnection(
            topic,
            const ProductTopicConnectionStateView(
              phase: ProductTopicConnectionPhase.connecting,
            ),
          );
        }
      });
      unawaited(entry.start());
    }
    entry.refs += 1;
    return _Lease(entry);
  }

  ProductTopicLeaseBundle acquireBundle(Iterable<ProductTopic> topics) {
    final leases = <ProductTopicLease>[];
    try {
      for (final topic in topics) {
        leases.add(acquire(topic));
      }
    } catch (_) {
      // 中途失败（例如 disposed）：释放已取得的租约，不留下半开作用域。
      for (final lease in leases) {
        unawaited(lease.release());
      }
      rethrow;
    }
    return ProductTopicLeaseBundle._(leases);
  }

  /// 显式重试某个 topic 的订阅：取消当前句柄、重置退避预算并重新接收基线。
  /// disposed 或该 topic 未租用时为空操作。
  void retry(ProductTopic topic) {
    if (_disposed) return;
    final entry = _entries[topic];
    if (entry == null || entry._closing) return;
    unawaited(entry.retryNow());
  }

  Future<void> releaseAll() async {
    _disposed = true;
    final entries = List.of(_entries.values);
    _entries.clear();
    await Future.wait([for (final entry in entries) entry.close()]);
  }

  Future<void> _awaitPriorClose(ProductTopic topic) async {
    final pending = _closing[topic];
    if (pending == null) return;
    try {
      await pending;
    } catch (_) {
      // 旧 entry 的关闭失败不影响新订阅建立。
    }
  }

  void _dispatch(ProductTopicFrame frame) => onFrame(frame);

  void _publishConnection(
    ProductTopic topic,
    ProductTopicConnectionStateView? view,
  ) => onTopicConnection(topic, view);

  Future<void> _releaseEntry(_TopicSubscription entry) async {
    final topic = entry.topic;
    if (_entries[topic] == entry) {
      _entries.remove(topic);
    }
    final closing = entry.close();
    _closing[topic] = closing;
    unawaited(
      closing.whenComplete(() {
        if (identical(_closing[topic], closing)) {
          _closing.remove(topic);
        }
      }),
    );
    // 仅当没有同 topic 的新 entry 接替时才清理连接状态（避免释放/重租竞争清掉新状态）。
    if (!_entries.containsKey(topic)) {
      _publishConnection(topic, null);
    }
    await closing;
  }
}

class _Lease implements ProductTopicLease {
  _Lease(this._subscription);

  final _TopicSubscription _subscription;
  var _released = false;

  @override
  ProductTopic get topic => _subscription.topic;

  @override
  Stream<ProductTopicFrame> get frames => _subscription.controller.stream;

  @override
  Future<void> release() async {
    if (_released) return;
    _released = true;
    final entry = _subscription;
    entry.refs -= 1;
    if (entry.refs > 0) return;
    await entry.registry._releaseEntry(entry);
  }
}

/// topic 的诊断标签（不含作用域凭据，仅身份描述）。
String describeProductTopic(ProductTopic topic) {
  return switch (topic) {
    ProjectDirectoryTopic() => 'projectDirectory',
    ThreadDirectoryTopic() => 'threadDirectory',
    AgentDirectoryTopic() => 'agentDirectory',
    SettingsConfigTopic() => 'settingsConfig',
    ModelCatalogTopic() => 'modelCatalog',
    RecoveryTopic() => 'recovery',
    McpTopic() => 'mcp',
    LspTopic() => 'lsp',
    SkillsTopic(:final projectId) => 'skills:$projectId',
    ThreadModeCatalogTopic() => 'threadModeCatalog',
    ProviderUsageTopic() => 'providerUsage',
    ModelPerformanceTopic() => 'modelPerformance',
    SessionCostsTopic(:final rootThreadId) => 'sessionCosts:$rootThreadId',
    UpdaterTopic() => 'updater',
    PersistenceTopic() => 'persistence',
    PersistenceQueueTopic() => 'persistenceQueue',
    AgentProfilesTopic() => 'agentProfiles',
  };
}
