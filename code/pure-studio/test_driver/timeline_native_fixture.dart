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
      const StudioPromptInput(
        text: 'timeline-create-child',
        attachmentDraftIds: [],
      ),
      ThreadModeId.simple,
    );
    await _waitTimelineTurn(api, root.thread.id, root.receipt.inputId);
    for (var index = 0; index < 160; index++) {
      final receipt = await api.startTurn(
        root.thread.id,
        StudioPromptInput(
          text: 'timeline-seed-$index',
          attachmentDraftIds: const [],
        ),
      );
      await _waitTimelineTurn(api, root.thread.id, receipt.inputId);
    }
    final controller = _container.read(studioControllerProvider.notifier);
    await controller.debugReloadForTest();
    await controller.selectProject(project.id);
    await controller.selectThread(root.thread.id);
    return jsonEncode({'rootId': root.thread.id});
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
    final receipt = await api.startTurn(
      threadId,
      StudioPromptInput(
        text:
            'timeline-stream${command['ending'] == null ? '' : '-${command['ending']}'}',
        attachmentDraftIds: const [],
      ),
    );
    return jsonEncode({'inputId': receipt.inputId});
  }
  if (action == 'interrupt') {
    final threadId = command['threadId']! as String;
    final turn = (await api.readThreadSnapshot(threadId)).workspace.activeTurn;
    if (turn == null || turn.inputId != command['inputId']) {
      throw StateError('fixture interrupt does not match the running input');
    }
    await api.interruptTurn(threadId, turn.turnId);
    return jsonEncode({'interrupted': turn.turnId});
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
    final snapshot = await api.readThreadSnapshot(threadId);
    final turn = snapshot.workspace.timelineTurns.values
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
