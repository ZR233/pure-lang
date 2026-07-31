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
        message: _timelineMessageFixture(
          id: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          status: 'streaming',
        ),
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: _timelinePartFixture(
          id: 'part-1',
          messageId: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          status: 'streaming',
          textChannel: TimelineTextChannel.finalAnswer,
        ),
      ),
    );
    api.emitSession(
      _partDeltaEvent(
        sessionId: 'session-1',
        delta: _timelineDeltaFixture(
          partId: 'part-1',
          revision: 1,
          field: 'text',
          delta: 'partial',
        ),
      ),
    );
    api.emitSession(
      _partUpdatedEvent(
        sessionId: 'session-1',
        part: _timelinePartFixture(
          id: 'part-1',
          messageId: 'turn-1:assistant',
          sessionId: 'session-1',
          turnId: 'turn-1',
          type: TimelinePartType.text,
          revision: 1,
          updatedAt: 2,
          textChannel: TimelineTextChannel.finalAnswer,
          text: 'authoritative',
        ),
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
            mode: StudioMode.simple,
            updatedAt: now,
          ),
          StudioSession(
            id: 'session-2',
            projectId: 'project-2',
            title: 'Session 2',
            mode: StudioMode.task,
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
          runtimesBySession: const {
            'session-1': SessionRuntimeView(
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
          },
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
          runtime: const SessionRuntimeView(
            model: 'planner/new',
            contextTokens: 42,
            contextWindow: 128000,
            totalTokens: 42,
            costLabel: 'CNY 0.16',
            activeSkills: ['new-skill'],
            activeMcpServers: ['new-mcp'],
            activeLspServers: ['new-lsp'],
            agentCount: 0,
          ),
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
          runtime: const SessionRuntimeView(
            model: 'planner/other',
            contextTokens: 7,
            contextWindow: 128000,
            totalTokens: 7,
            costLabel: '',
            activeSkills: ['other-skill'],
            activeMcpServers: ['other-mcp'],
            activeLspServers: ['other-lsp'],
            agentCount: 0,
          ),
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
