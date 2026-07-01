part of '../widget_test.dart';

void registerSessionStreamTests() {
  test('timeline snapshot wins over same tick delta batch', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _messageUpdatedEvent(
        sessionId: 'session-1',
        message: {
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'role': 'assistant',
          'status': 'streaming',
          'createdAt': 1,
          'updatedAt': 1,
        },
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'partType': 'text',
          'order': 0,
          'revision': 0,
          'status': 'streaming',
          'createdAt': 1,
          'updatedAt': 1,
          'textChannel': 'final',
          'text': '',
        },
      ),
    );
    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: {
          'sessionId': 'session-1',
          'messageId': 'turn-1:assistant',
          'partId': 'part-1',
          'revision': 1,
          'field': 'text',
          'delta': 'partial',
        },
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: {
          'partId': 'part-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'partType': 'text',
          'order': 0,
          'revision': 1,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 2,
          'textChannel': 'final',
          'text': 'authoritative',
        },
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.selectedTimelineRows.single.part!.text, 'authoritative');
    expect(state.partOverlaysBySession['session-1'], isEmpty);
  });

  test('session list stream updates only the addressed project', () async {
    final now = DateTime.fromMillisecondsSinceEpoch(1000);
    final api = _FakeStudioApi(
      _emptyState().copyWith(
        sessions: [
          StudioSession(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session 1',
            mode: CompileMode.auto,
            updatedAt: now,
          ),
          StudioSession(
            id: 'session-2',
            projectId: 'project-2',
            title: 'Session 2',
            mode: CompileMode.plan,
            updatedAt: now,
          ),
        ],
      ),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitGlobal(
      _sessionListChangedEvent(projectId: 'project-1', sessions: const []),
    );
    await pumpEventQueue();

    final sessions = container
        .read(studioControllerProvider)
        .requireValue
        .sessions;
    expect(sessions.map((session) => session.id), ['session-2']);
  });

  test(
    'session runtime stream preserves agents and refreshes active capabilities',
    () async {
      final api = _FakeStudioApi(
        _emptyState().copyWith(
          runtime: const SessionRuntimeView(
            model: 'planner/old',
            contextTokens: 1,
            contextWindow: 100,
            totalTokens: 2,
            costLabel: '',
            activeSkills: ['old-skill'],
            activeMcpServers: ['old-mcp'],
            activeLspServers: ['old-lsp'],
            agentCount: 2,
          ),
        ),
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      api.emitSession(
        _sessionRuntimeChangedEvent(
          sessionId: 'session-1',
          runtime: sessionRuntimeFromJson({
            'sessionId': 'session-1',
            'usage': {
              'model': 'planner/new',
              'latestContextTokens': 42,
              'contextWindow': 128000,
              'promptTokens': 21,
              'completionTokens': 21,
              'cachedPromptTokens': 0,
              'totalTokens': 42,
              'estimatedCosts': [
                {'currency': 'CNY', 'amount': '0.1600'},
              ],
              'hasUnpricedUsage': false,
              'updatedAt': 2,
            },
            'activeSkills': ['new-skill'],
            'activeMcpServers': ['new-mcp'],
            'activeLspServers': ['new-lsp'],
            'updatedAt': 2,
          }),
        ),
      );
      await pumpEventQueue();

      final runtime = container
          .read(studioControllerProvider)
          .requireValue
          .runtime;
      expect(runtime.model, 'planner/new');
      expect(runtime.contextTokens, 42);
      expect(runtime.costLabel, 'CNY 0.16');
      expect(runtime.activeSkills, ['new-skill']);
      expect(runtime.activeMcpServers, ['new-mcp']);
      expect(runtime.activeLspServers, ['new-lsp']);
      expect(runtime.agentCount, 2);

      api.emitSession(
        _sessionRuntimeChangedEvent(
          sessionId: 'other-session',
          runtime: sessionRuntimeFromJson({
            'sessionId': 'other-session',
            'usage': {
              'model': 'planner/other',
              'latestContextTokens': 7,
              'contextWindow': 128000,
              'promptTokens': 7,
              'completionTokens': 0,
              'cachedPromptTokens': 0,
              'totalTokens': 7,
              'estimatedCosts': <Object?>[],
              'hasUnpricedUsage': false,
              'updatedAt': 3,
            },
            'activeSkills': ['other-skill'],
            'activeMcpServers': ['other-mcp'],
            'activeLspServers': ['other-lsp'],
            'updatedAt': 3,
          }),
        ),
      );
      await pumpEventQueue();

      final unchangedRuntime = container
          .read(studioControllerProvider)
          .requireValue
          .runtime;
      expect(unchangedRuntime.model, 'planner/new');
      expect(unchangedRuntime.activeSkills, ['new-skill']);
    },
  );
}
