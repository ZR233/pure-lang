import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../data/frb/studio_api.dart';
import '../domain/models/studio_models.dart';
import '../l10n/app_localizations.dart';
import '../l10n/studio_l10n.dart';
import '../platform/error_log.dart';
import '../shared/studio_driver_state.dart';

part 'studio_shutdown.g.dart';

class StudioShutdownView {
  const StudioShutdownView({this.progress, this.error});

  final StudioShutdownProgress? progress;
  final String? error;
}

/// 当前关机进度或失败状态；两者均为空表示未在关机。
@Riverpod(keepAlive: true)
class StudioShutdownProgressState extends _$StudioShutdownProgressState {
  @override
  StudioShutdownView build() => const StudioShutdownView();

  void update(StudioShutdownProgress progress) =>
      state = StudioShutdownView(progress: progress);

  void fail(Object error) =>
      state = StudioShutdownView(error: error.toString());
}

/// 关闭 runtime：在同一个绝对预算内**并发**收束 Dart 拥有的外部 owner（产品/Thread 流与
/// 关机进度流两组独立取消同时发起、同时观察），把各 stage 的真实 ACK / typed 失败累积为
/// issue；随后把这些 issue 作为本次关闭的最终成功前置条件，连同扣除实际耗时后的剩余预算
/// 一并交给 runtime 单编排。两组取消共用同一 Stopwatch 与 native 首次剩余期限，绝不串行
/// 占用彼此预算；也绝不在 runtime 返回后再取消进度、事后把外部 Degraded 合并进已释锁的
/// Clean。所有独立取消都在 Rust finalizer 之前完整收敛，真实失败统一并入 externalIssues。
///
/// 返回 runtime 的 typed 报告；只有真实 `Clean` 才发布 `Stopped`，degraded 与超时都不得
/// 伪造终态。订阅创建/取消都有界：同步失败、取消错误或进度流错误都只累积为独立诊断，
/// 绝不覆盖可靠的 runtime 报告，也绝不挂起退出。
Future<StudioShutdownReport> runStudioShutdown(
  StudioBridgeDataSource api,
  void Function(StudioShutdownProgress progress) onProgress, {
  required int remainingMs,
}) async {
  // 本次清理预算的本地单调时钟：所有阶段共享并随时扣除真实耗时，绝不另起 2s。
  final watch = Stopwatch()..start();
  // Dart 拥有的外部 owner 与进度通道上的独立错误：累积后作为关闭的最终成功前置条件，
  // 随请求交给 runtime 单编排，绝不事后并入已释锁的 Clean。
  final issues = <StudioShutdownIssue>[];
  void publish(StudioShutdownProgress progress) {
    if (progress.phase == StudioShutdownPhase.stopped) {
      // 缓冲终态：只有协调器在完整关闭报告（含 Dart 订阅 / 诊断收尾）确认可靠后才发布。
      return;
    }
    onProgress(progress);
    StudioDriverState.publishShutdownProgress(progress);
  }

  void recordProgressError(Object error, StackTrace stackTrace, String stage) {
    // 无 runtime（未安装 / 第二实例）：不存在可观察的进度资源，跳过而不是伪造失败，
    // 否则会把「确无 owner」误判为 second exit 1。
    if (error is StudioFailure &&
        error.code == StudioFailureCode.notInitialized) {
      return;
    }
    // 同一次错误只生成一次 correlation：report issue 与同步诊断共用同一编号。
    final correlationId = _correlationOf(error);
    issues.add(_progressIssue(error, correlationId));
    recordDartError(
      error,
      stackTrace,
      stage: stage,
      correlationId: correlationId,
    );
  }

  // 尽早同步封闭退出准入：一经进入关闭就封闭新的 mutation/订阅，绝不因随后的 Dart 进度
  // 订阅取消挂住而允许新的 mutation 借机进入。
  FrbStudioBridgeDataSource.sealForShutdown();
  // 本地受控进度：bridge 关机进度流在 runtime 收束前即被取消，故不依赖它仍存活；终态
  // `Stopped` 只由协调器在可靠 Clean 之后发布。
  publish(const StoppingSubscriptionsProgress());

  // 在任何 Dart 外部取消等待之前，先有界封闭退出准入（native `begin_runtime_exit`）：即使
  // 随后的 Dart 取消失败或长时间不返回，runtime 准入与独立取消也已在期限一开始完成，且迟到
  // start 无法再发布 runtime。传扣真实耗时后的 native 清理剩余预算；不触发初始化，未加载
  // runtime 无 owner 时跳过。失败/超时保留真实 code / 安全 cause / stack / correlation 作为
  // external issue 继续收尾，绝不 early return、绝不伪成功。
  try {
    await api.beginRuntimeExit(
      remainingMs: _remainingBudget(remainingMs, watch),
    );
  } on Object catch (error, stackTrace) {
    // Frb core 已用同一 correlation 记录脱敏诊断时复用 memoized issue，避免重复记录。
    final recorded = FrbStudioBridgeDataSource.recordedBeginExitIssue(error);
    if (recorded != null) {
      issues.add(recorded);
    } else {
      final correlationId = _correlationOf(error);
      issues.add(_beginExitIssue(error, correlationId));
      recordDartError(
        error,
        stackTrace,
        stage: 'exit-seal',
        correlationId: correlationId,
      );
    }
  }

  // 订阅创建的同步 throw 也不能阻止关窗。
  StreamSubscription<StudioShutdownProgress>? subscription;
  try {
    subscription = api.subscribeShutdownProgress().listen(
      publish,
      onError: (Object error, StackTrace stackTrace) {
        // 进度流错误是诊断，不得取消本次关窗；累积 Object 与堆栈后继续。
        recordProgressError(error, stackTrace, 'progress');
      },
    );
  } on Object catch (error, stackTrace) {
    recordProgressError(error, stackTrace, 'progress-subscribe');
  }

  // 并发收束两组 Dart 独立 owner：产品/Thread 注册表（内部同一 registry、single owned
  // cancel future、Future.wait 并发）与关机进度流（本地受控进度 owner）各自发起并观察，
  // 绝不串行占用彼此预算。两行调用都在首个 await 之前同步发起各自 cancel，因此这里只是
  // 完整等待两者收敛，等待顺序不改变并发关系；两实现内部都已把失败收敛为 typed issue
  // （不抛出），不会互相遮蔽，也不引入 unhandled error。
  final progressIssues = <StudioShutdownIssue>[];
  final dartSubscriptionCancel =
      FrbStudioBridgeDataSource.cancelDartSubscriptions(
        budgetMs: _remainingBudget(remainingMs, watch),
      );
  final progressCancel = _cancelBounded(
    subscription,
    progressIssues,
    Duration(
      milliseconds: _remainingBudget(remainingMs, watch).clamp(0, 2000).toInt(),
    ),
  );
  // 在 Rust finalizer 之前完整提交两组外部 issue：先并净产品/Thread 注册表的真实 ACK /
  // typed 失败，再并入进度流的真实失败（含正在返回的 error），绝不在 return 之后再合并。
  issues.addAll(await dartSubscriptionCancel);
  await progressCancel;
  issues.addAll(progressIssues);

  // 即使 Dart 取消已耗尽预算，也必须立即向已初始化的 runtime 广播封闭请求
  // （remainingMs=0，原生 hard deadline 照常），绝不整段跳过。全部外部 issue 作为同一
  // 编排的前置条件传入，仅消费返回的 BridgeShutdownReport。
  return api.shutdownRuntime(
    remainingMs: _remainingBudget(remainingMs, watch),
    externalIssues: List<StudioShutdownIssue>.unmodifiable(issues),
  );
}

/// 从本次预算里扣除某阶段已消耗的真实单调耗时；预算耗尽返回 0。
int _remainingBudget(int remainingMs, Stopwatch watch) {
  final left = remainingMs - watch.elapsedMilliseconds;
  return left <= 0 ? 0 : left;
}

StudioShutdownIssue _progressIssue(Object error, String correlationId) {
  return StudioShutdownIssue(
    stage: 'progress',
    code: switch (error) {
      StudioFailure(:final code) => code.name,
      _ => 'progressError',
    },
    // 只保留允许诊断字段：typed 失败沿用桥消息，其它一律用固定安全文案，绝不把
    // 任意 error.toString() 当作 wire 安全 message。
    message: switch (error) {
      StudioFailure(:final message) => message,
      _ => 'studio shutdown progress stream failed',
    },
    retryable: true,
    correlationId: correlationId,
  );
}

/// 早期退出封闭失败的 typed external issue：真实 code，固定安全文案（绝不把任意
/// error.toString() 当 wire 消息），保留非空 correlation 以便与同步诊断 / 桥诊断关联。
StudioShutdownIssue _beginExitIssue(Object error, String correlationId) {
  return StudioShutdownIssue(
    stage: 'exit-seal',
    code: switch (error) {
      StudioFailure(:final code) => code.name,
      TimeoutException() => 'timeout',
      _ => 'exitSealFailed',
    },
    message: 'studio runtime early exit seal did not confirm',
    retryable: false,
    correlationId: correlationId,
  );
}

String _correlationOf(Object error) =>
    error is StudioFailure && error.correlationId.isNotEmpty
    ? error.correlationId
    : newStudioCorrelationId();

/// 取消订阅有界；取消失败只累积为诊断，不抛错覆盖报告。
Future<void> _cancelBounded(
  StreamSubscription<StudioShutdownProgress>? subscription,
  List<StudioShutdownIssue> issues,
  Duration bound,
) async {
  if (subscription == null) return;
  // 取消有界。绝不使用 `onTimeout: () => timedOut = true` 这类返回 `bool` 的箭头闭包：
  // `Future<void>` 的 `onTimeout` 运行时被具体化为 `(() => Future<Null>?)?`，返回 `bool`
  // 的闭包会在真实运行时抛 `TypeError`（静态 analyze 不报），从而把成功/超时的取消都变成
  // 伪故障。改为让 `.timeout` 超时时抛 `TimeoutException` 并单独捕获：成功 cancel 不伪报
  // Degraded，真实超时保留强 owner（进度 owner 仍在 registry 直到真正 ACK）并如实上报
  // timeout issue，绝不吞故障或退化成无界等待。
  final effectiveBound = bound <= Duration.zero
      ? const Duration(milliseconds: 1)
      : bound;
  var timedOut = false;
  try {
    // Dart 侧 stream 取消挂住时 `.timeout` 抛 TimeoutException，单独识别为真实超时。
    await subscription.cancel().timeout(effectiveBound);
  } on TimeoutException {
    timedOut = true;
  } on Object catch (error, stackTrace) {
    // 同一次错误只生成一次 correlation：report issue 与同步诊断共用同一编号。
    final correlationId = _correlationOf(error);
    issues.add(
      StudioShutdownIssue(
        stage: 'progress',
        code: 'cancelFailed',
        message: 'studio shutdown progress subscription did not cancel',
        retryable: true,
        correlationId: correlationId,
      ),
    );
    recordDartError(
      error,
      stackTrace,
      stage: 'progress-cancel',
      correlationId: correlationId,
    );
  }
  if (timedOut) {
    // 隐藏超时会把关闭误报 Clean；这里是真实 issue，保存事实保持原样。
    final correlationId = newStudioCorrelationId();
    issues.add(
      StudioShutdownIssue(
        stage: 'progress',
        code: 'timeout',
        message: 'studio shutdown progress subscription cancel timed out',
        retryable: false,
        correlationId: correlationId,
      ),
    );
    // 同一 correlation 的脱敏同步诊断：报告侧用固定安全文案，绝不泄露正文。
    recordDartError(
      TimeoutException(
        'studio shutdown progress subscription cancel timed out',
      ),
      null,
      stage: 'progress-cancel',
      correlationId: correlationId,
    );
  }
}

/// 关机阶段 overlay：不可关闭，展示阶段文案与落库进度，等待数据库存完。
class StudioShutdownOverlay extends ConsumerWidget {
  const StudioShutdownOverlay({required this.child, super.key});

  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final shutdown = ref.watch(studioShutdownProgressStateProvider);
    if (shutdown.progress == null && shutdown.error == null) return child;
    return Stack(
      children: [
        child,
        Positioned.fill(
          child: ColoredBox(
            color: Theme.of(context).colorScheme.scrim.withValues(alpha: 0.45),
            child: Center(
              child: shutdown.error != null
                  ? _ShutdownFailureCard(error: shutdown.error!)
                  : _ShutdownProgressCard(progress: shutdown.progress!),
            ),
          ),
        ),
      ],
    );
  }
}

class _ShutdownFailureCard extends StatelessWidget {
  const _ShutdownFailureCard({required this.error});

  final String error;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              l10n.shutdownFinalizing,
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 12),
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 440),
              child: Text(error, maxLines: 5, overflow: TextOverflow.ellipsis),
            ),
            const SizedBox(height: 12),
            Text(
              l10n.shutdownFinalizingHint,
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ],
        ),
      ),
    );
  }
}

class _ShutdownProgressCard extends StatelessWidget {
  const _ShutdownProgressCard({required this.progress});

  final StudioShutdownProgress progress;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    return Card(
      key: const ValueKey('studio-shutdown-overlay'),
      elevation: 6,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 28, vertical: 22),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                const SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(strokeWidth: 2.4),
                ),
                const SizedBox(width: 12),
                Text(
                  l10n.shutdownTitle,
                  style: Theme.of(context).textTheme.titleMedium
                      ?.copyWith(fontWeight: FontWeight.w700),
                ),
              ],
            ),
            const SizedBox(height: 14),
            Text(
              shutdownPhaseLabel(l10n, progress),
              style: Theme.of(context).textTheme.bodyMedium,
            ),
            // 关闭是并发收束，进度事件不再按串行阶段 ordinal 单调递增；不显示内部阶段
            // 序号（既不真实也不代表完成比例），只展示当前正在收束的真实内容。
          ],
        ),
      ),
    );
  }
}

String shutdownPhaseLabel(
  AppLocalizations l10n,
  StudioShutdownProgress progress,
) {
  final String label = switch (progress.phase) {
    StudioShutdownPhase.stoppingSubscriptions =>
      l10n.shutdownPhaseStoppingSubscriptions,
    StudioShutdownPhase.cancellingTurns => l10n.shutdownPhaseCancellingTurns,
    StudioShutdownPhase.flushingPersistence =>
      l10n.shutdownPhaseFlushingPersistence,
    StudioShutdownPhase.stoppingAgents => l10n.shutdownPhaseStoppingAgents,
    StudioShutdownPhase.stoppingMcp => l10n.shutdownPhaseStoppingMcp,
    StudioShutdownPhase.stoppingLsp => l10n.shutdownPhaseStoppingLsp,
    StudioShutdownPhase.stopped => l10n.shutdownPhaseStopped,
  };
  if (progress case FlushingPersistenceProgress(:final pendingCommits)
      when pendingCommits > 0) {
    return '$label（${l10n.shutdownPendingCommits(pendingCommits)}）';
  }
  return label;
}
