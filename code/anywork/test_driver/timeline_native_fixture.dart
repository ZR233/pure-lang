part of 'driver_main.dart';

/// Seeds only through production FRB commands. The provider must be a local
/// scripted server; no demo repository or alternate history database is used.
Future<String> _timelineNativeFixture(String request) async {
  final command = jsonDecode(request) as Map<String, Object?>;
  final api = _container.read(studioApiProvider);
  if (api is DemoStudioApi) {
    throw StateError('native fixture requires Rust/FRB');
  }
  final state = await api.readStudioState();
  if (state.providers.isEmpty ||
      state.providers.any(
        (provider) => Uri.parse(provider.baseUrl).host != '127.0.0.1',
      )) {
    throw StateError('native fixture requires isolated loopback providers');
  }
  final action = command['action'];
  if (action == 'seed') {
    final project = await api.openProject(command['path']! as String);
    final root = await api.startNewThread(
      project.id,
      StudioPromptInput(
        inputId: newPromptInputId(),
        text: 'timeline-create-child',
        attachmentDraftIds: [],
      ),
      ThreadModeId.simple,
    );
    await _waitTimelineTurn(api, root.thread.id, root.receipt.inputId);
    final childAgent = (await api.readStudioState()).agentDirectory.values
        .where(
          (agent) =>
              agent.rootThreadId == root.thread.id &&
              agent.threadId != root.thread.id,
        )
        .firstOrNull;
    if (childAgent == null) {
      throw StateError('scripted provider did not create a child Thread');
    }
    final childThread = (await api.readThreadSnapshot(childAgent.threadId))
        .thread;
    for (var index = 0; index < 96; index++) {
      late final SubmitPromptReceipt receipt;
      try {
        receipt = await api.submitPrompt(
          root.thread.id,
          StudioPromptInput(
            inputId: newPromptInputId(),
            text: 'timeline-seed-$index',
            attachmentDraftIds: const [],
          ),
        );
      } catch (error) {
        final queue = await (api as FrbStudioApi).readPersistenceQueue();
        throw StateError(
          'seed $index failed: $error; persistence: ${queue.lastError}; '
          'threads: ${[for (final thread in queue.threads) '${thread.threadId}: ${thread.lastError} '
                '(admitted ${thread.historyAdmittedSequence}, '
                'durable ${thread.historyDurableSequence})']}',
        );
      }
      await _waitTimelineTurn(api, root.thread.id, receipt.inputId);
    }
    final controller = _container.read(studioControllerProvider.notifier);
    await controller.debugReloadForTest();
    await controller.selectProject(project.id);
    controller.includeDirectoryThreads([childThread]);
    await controller.selectThread(root.thread.id);
    return jsonEncode({'rootId': root.thread.id, 'childId': childThread.id});
  }
  if (action == 'benchmark') {
    final threadId = command['threadId']! as String;
    final oldMicros = <int>[];
    final itemMicros = <int>[];
    for (var index = 0; index < 20; index++) {
      final watch = Stopwatch()..start();
      await api.listThreadTurns(threadId, limit: 25);
      oldMicros.add(watch.elapsedMicroseconds);
      watch.reset();
      await api.listTimelineItems(threadId);
      itemMicros.add(watch.elapsedMicroseconds);
    }
    return jsonEncode({
      'canonicalItemIds': (await api.listThreadTurns(
        threadId,
        limit: 200,
      )).items.map((item) => item.id).toList(),
      'completeTurnQueryMicros': oldMicros,
      'indexedItemQueryMicros': itemMicros,
    });
  }
  if (action == 'start') {
    final threadId = command['threadId']! as String;
    final receipt = await api.submitPrompt(
      threadId,
      StudioPromptInput(
        inputId: newPromptInputId(),
        text:
            'timeline-stream${command['ending'] == null ? '' : '-${command['ending']}'}',
        attachmentDraftIds: const [],
      ),
    );
    return jsonEncode({'inputId': receipt.inputId});
  }
  throw StateError('unknown native fixture action');
}

Future<void> _waitTimelineTurn(
  StudioApi api,
  String threadId,
  String inputId,
) async {
  final deadline = DateTime.now().add(const Duration(seconds: 30));
  while (DateTime.now().isBefore(deadline)) {
    // 当前状态快照只携带活动 Turn；终态 Turn 事实从 canonical 历史窗口的 Turn 摘要读取，
    // 与 GUI 用 `listTimelineItems` 维护最近 Turn 是同一条事实源。
    final page = await api.listTimelineItems(threadId);
    final turn = page.turns
        .map((entry) => entry.turn)
        .where((turn) => turn.inputId == inputId)
        .firstOrNull;
    if (turn?.inputId == inputId && turn!.state.isTerminal) {
      if (turn.state is! CompletedStudioTurnState) {
        throw StateError('fixture turn failed: ${turn.state.reason}');
      }
      return;
    }
    await Future<void>.delayed(const Duration(milliseconds: 10));
  }
  throw StateError('fixture turn did not complete');
}
