import 'package:anywork/src/presentation/providers/client_provider.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:anywork/src/app/app_graph.dart';
import 'package:anywork_client/studio_client.dart';

import 'package:anywork/src/infrastructure/frb/studio_api.dart';
import 'package:anywork/src/presentation/providers/settings_store.dart';
import 'package:anywork/src/presentation/providers/thread_directory_store.dart';
import 'package:anywork/src/presentation/providers/thread_session_store.dart';
import 'package:anywork/src/features/interaction/composer_view.dart';

/// AppGraph/Riverpod 适配契约：改动“任务模式 / 思考强度”后，主界面的显示值必须**只由
/// canonical 事实流收敛**——mode 走目录增量，运行时 route 走 Thread 有序流。
///
/// 这里用 AppGraph/Client owner 与只读 Riverpod adapter，只把
/// transport 换成 Demo；它不证明 native FRB，但 Demo
/// 少发任何一条真实侧会发的帧，本文件就会失败，而不会被“命令后重新读取”这类补偿
/// 路径掩盖（design/25 §25.5：命令成功以响应 canonical 应用回 store，不额外触发
/// 显式刷新）。
void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  const threadId = 'thread-main';

  late AppGraph graph;
  late ProviderContainer container;
  late ProviderSubscription<Object?> sessionHold;
  late ProviderSubscription<Object?> composerHold;

  setUp(() async {
    final api = DemoStudioTransport(lspActivityLoop: false);
    graph = AppGraph(transport: api, clientId: 'canonical-convergence');
    container = ProviderContainer(
      overrides: [appGraphProvider.overrideWithValue(graph)],
    );
    await graph.client.bootstrap();
    await graph.client.directoryCommands.selectThread(threadId);
    sessionHold = container.listen(
      threadSessionStoreProvider(threadId),
      (_, _) {},
    );
    composerHold = container.listen(
      composerDockViewProvider(threadId),
      (_, _) {},
    );
    await graph.client.session(threadId).source.waitFor((s) => s.hasSnapshot);
  });

  tearDown(() async {
    sessionHold.close();
    composerHold.close();
    container.dispose();
    await graph.close(deadline: DateTime.now().add(const Duration(seconds: 5)));
  });

  int routeRevision() => _routeRevision(container, threadId);

  ProviderModelView? modelFor(String providerId, String model) =>
      _modelFor(container, providerId, model);

  test('任务模式由目录增量交付，运行时 route 由 Thread 帧交付', () async {
    expect(
      container.read(composerDockViewProvider(threadId)).thread?.mode,
      ThreadModeId.simple,
      reason: 'demo fixture 的根会话默认是简洁模式',
    );
    final routeRevisionBefore = routeRevision();
    final directoryRevisionBefore = container
        .read(threadDirectoryStoreProvider)
        .revision;

    await container
        .read(anyworkClientProvider)
        .threadCommands
        .setThreadMode(ThreadModeId.task);

    await graph.client
        .session(threadId)
        .source
        .waitFor(
          (s) => s.runtime?.modelRoute?.revision == routeRevisionBefore + 1,
        );
    await graph.client.threadDirectory.source.waitFor(
      (state) => state.window.threads.any(
        (thread) => thread.id == threadId && thread.mode == ThreadModeId.task,
      ),
    );
    expect(
      container.read(threadDirectoryStoreProvider).revision,
      greaterThan(directoryRevisionBefore),
      reason: 'mode 是目录事实：必须经目录增量推进水位后才能被界面看到',
    );
    expect(
      routeRevision(),
      routeRevisionBefore + 1,
      reason:
          'mode 默认 route 是 Thread 运行时事实，必须由有序流帧交付；'
          '命令后重新读取会掩盖这里缺帧',
    );
  });

  test('思考强度由 Thread 有序流收敛，命令响应只合并 settings', () async {
    final route = container.read(composerDockViewProvider(threadId)).modelRoute;
    expect(route, isNotNull, reason: '已建会话必须有 canonical route');
    final canonicalRoute = route!;

    final model = modelFor(canonicalRoute.providerId, canonicalRoute.model);
    expect(model, isNotNull, reason: '当前 route 的模型必须在 provider 目录中');
    final target = model!.reasoningEfforts
        .where((effort) => effort != canonicalRoute.effort)
        .firstOrNull;
    expect(target, isNotNull, reason: '模型需要至少两个思考强度候选');
    final routeRevisionBefore = routeRevision();

    await container
        .read(anyworkClientProvider)
        .threadCommands
        .setThreadModelRoute(
          threadId: threadId,
          providerId: canonicalRoute.providerId,
          model: canonicalRoute.model,
          effort: target,
        );

    await graph.client
        .session(threadId)
        .source
        .waitFor(
          (state) =>
              state.runtime?.modelRoute?.revision == routeRevisionBefore + 1 &&
              state.runtime?.modelRoute?.effort == target,
        );

    expect(
      container
          .read(threadSessionStoreProvider(threadId))
          .runtime
          ?.modelRoute
          ?.effort,
      target,
      reason: 'canonical 事实必须经 Thread 帧收敛，不能停留在乐观目标值',
    );
    expect(
      routeRevision(),
      routeRevisionBefore + 1,
      reason: 'route 变更必须带新的模型路由 revision 到达 store',
    );
    final settled = container.read(composerDockViewProvider(threadId));
    expect(settled.routePending, isFalse, reason: '命令完成后不得停在 pending 目标值');
    expect(
      settled.modelRoute?.effort,
      target,
      reason: '选择器显示值必须与收敛后的 canonical 一致（乐观值清除后不回落）',
    );
    expect(
      container
          .read(settingsStoreProvider)
          .canonical
          ?.modeModelRoutes
          .where((candidate) => candidate.modeId == settled.thread?.mode)
          .firstOrNull
          ?.effort,
      target,
      reason: '命令响应的 settings canonical 是同一水位下的另一半事实',
    );
  });

  test('Thread 单流水位门：同水位与更低水位丢弃，缺口走重同步', () {
    const watermark = ThreadStreamWatermark(
      hasSnapshot: true,
      revision: 4,
      epoch: 1,
    );

    expect(
      classifyThreadFrame(watermark, _notification(revision: 4)),
      ThreadFrameDecision.drop,
      reason: '同水位重复帧不得重复应用',
    );
    expect(
      classifyThreadFrame(watermark, _notification(revision: 3)),
      ThreadFrameDecision.drop,
      reason: '更低水位不得覆盖更新事实',
    );
    expect(
      classifyThreadFrame(watermark, _notification(revision: 5)),
      ThreadFrameDecision.apply,
    );
    expect(
      classifyThreadFrame(watermark, _notification(revision: 7)),
      ThreadFrameDecision.resync,
      reason: '缺口不能就地拼接',
    );
    expect(
      classifyThreadFrame(
        watermark,
        _notification(revision: 5, baseRevision: 3),
      ),
      ThreadFrameDecision.resync,
      reason: '基线水位与当前水位不一致表示缺口',
    );
    expect(
      classifyThreadFrame(watermark, _notification(revision: 5, epoch: 2)),
      ThreadFrameDecision.resync,
      reason: '跨订阅代次的通知不得应用',
    );
  });
}

int _routeRevision(ProviderContainer container, String threadId) =>
    container
        .read(threadSessionStoreProvider(threadId))
        .runtime
        ?.modelRoute
        ?.revision ??
    -1;

ProviderModelView? _modelFor(
  ProviderContainer container,
  String providerId,
  String model,
) {
  final providers =
      container.read(settingsStoreProvider).canonical?.providers ?? const [];
  for (final provider in providers) {
    if (provider.id != providerId) continue;
    for (final candidate in provider.models) {
      if (candidate.slug == model) return candidate;
    }
  }
  return null;
}

ThreadNotificationFrame _notification({
  required int revision,
  int? epoch,
  int? baseRevision,
}) {
  return ThreadNotificationFrame(
    threadId: 'thread-main',
    revision: revision,
    epoch: epoch,
    baseRevision: baseRevision,
    update: const ThreadActivityUpdate(activity: null),
  );
}
