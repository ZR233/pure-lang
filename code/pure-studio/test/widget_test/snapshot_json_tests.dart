part of '../widget_test.dart';

void registerSnapshotJsonTests() {
  test('config snapshot restores built-in Zhipu MCP metadata', () {
    final state = studioStateFromFrbSnapshot(
      frb.BridgeStudioSnapshotResponse(
        projects: const [],
        selectedProjectId: 'project-1',
        recoveryIssues: const [],
        sessions: const [
          frb.SessionDto(
            id: 'session-1',
            projectId: 'project-1',
            title: 'Session',
            mode: 'simple',
            createdAt: 1,
            updatedAt: 1,
            visibility: 'active',
            parentSessionId: null,
            rootSessionId: 'session-1',
            sessionKind: 'root',
            ownerAgentId: 'studio:session-1',
            ownerRole: 'planner',
            agentStatus: 'idle',
            agentSummary: null,
            agentError: null,
            agentUpdatedAt: 1,
          ),
        ],
        selectedSessionId: 'session-1',
        selectedSessionTask: null,
        configJson: jsonEncode({
          'defaultProviderId': 'zhipu-coding-plan',
          'providers': {
            'zhipu-coding-plan': {
              'presetId': 'zhipu-coding-plan',
              'wireProtocol': 'chat_completions',
              'connectionMode': 'http',
              'baseUrl': 'https://open.bigmodel.cn/api/coding/paas/v4',
              'hasBearerToken': true,
              'name': 'Zhipu Coding Plan',
              'defaultModel': 'glm-5',
              'models': [],
              'customModels': [],
              'catalogId': 'zhipu',
              'capabilitySource': 'preset_defaults',
              'serviceCapabilities': {
                'webSearch': {'hostedResponses': false, 'standalone': null},
              },
            },
          },
          'roles': {},
          'runtime': {'permissionMode': 'request-approval'},
          'instructions': {},
          'skills': {},
          'mcpServers': {},
          'builtinMcpServers': {
            'zhipu_search': {'enabled': true},
            'zhipu_vision': {'enabled': false},
          },
        }),
        generalSettingsJson: '{}',
        webSearch: const frb.BridgeWebSearchSettingsDto(
          configuredMode: 'disabled',
          effectiveMode: 'disabled',
          availability: 'disabled',
          allowedDomains: [],
        ),
      ),
    );

    final search = state.mcpServers.singleWhere(
      (server) => server.id == 'zhipu_search',
    );
    final vision = state.mcpServers.singleWhere(
      (server) => server.id == 'zhipu_vision',
    );

    expect(
      search.endpoint,
      'https://open.bigmodel.cn/api/mcp/web_search_prime/mcp',
    );
    expect(search.status, 'enabled');
    expect(search.sourceKind, 'builtIn');
    expect(search.mutationPolicy, 'lockedIdentity');
    expect(vision.transport, 'stdio');
    expect(vision.endpoint, 'npx');
    expect(vision.status, 'disabled');
  });

  test('realtime synthetic lifecycle part updates are ignored', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      StudioBridgeEvent(
        sessionId: 'session-1',
        sequence: BigInt.one,
        createdAt: _fixtureDate(1),
        payload: MessagePartUpdatedPayload(
          part: _timelinePartFixture(
            id: 'turn-1',
            messageId: 'turn-1:assistant',
            sessionId: 'session-1',
            turnId: 'turn-1',
            type: TimelinePartType.turn,
            synthetic: true,
          ),
        ),
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.partSnapshotsBySession['session-1'], isNull);
    expect(state.selectedTimelineRows, isEmpty);
    expect(state.eventCursorsBySession['session-1'], 1);
  });

  test('live stale events do not advance durable cursor', () async {
    final api = _FakeStudioApi(
      _emptyState().copyWith(eventCursorsBySession: const {'session-1': 1}),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      StudioBridgeEvent(
        sessionId: 'session-1',
        sequence: BigInt.from(9),
        payload: const StalePayload(laggedEvents: 1),
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.eventCursorsBySession['session-1'], 1);
  });

  test('product sequence does not suppress canonical session events', () async {
    final api = _FakeStudioApi(
      _emptyState().copyWith(eventCursorsBySession: const {'session-1': 1}),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitGlobal(
      StudioBridgeEvent.fromProduct(
        frb.BridgeProductEventEnvelope(
          eventId: 'product-task-99',
          projectId: 'project-1',
          sequence: BigInt.from(99),
          createdAt: 1,
          payload: const frb.BridgeProductEventPayload.sessionTaskChanged(
            sessionId: 'session-1',
          ),
        ),
      ),
    );
    api.emitSession(
      StudioBridgeEvent(
        sessionId: 'session-1',
        turnId: 'turn-2',
        sequence: BigInt.two,
        createdAt: _fixtureDate(2),
        payload: TurnChangedPayload(
          turn: StudioTurnView(
            turnId: 'turn-2',
            sessionId: 'session-1',
            state: const StudioTurnState.inProgress(
              StudioTurnActivity.thinking,
            ),
            updatedAt: _fixtureDate(2),
          ),
        ),
      ),
    );
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(state.eventCursorsBySession['session-1'], 2);
    expect(state.turnsBySession['session-1']?.turnId, 'turn-2');
    expect(
      state.turnsBySession['session-1']?.state,
      const StudioTurnState.inProgress(StudioTurnActivity.thinking),
    );
  });

  test('product event projects typed task coordinator detail', () {
    final event = StudioBridgeEvent.fromProduct(
      frb.BridgeProductEventEnvelope(
        eventId: 'event-task-runtime',
        projectId: 'project-1',
        sequence: BigInt.from(10),
        createdAt: 1,
        payload: frb.BridgeProductEventPayload.sessionTaskChanged(
          sessionId: 'session-1',
          task: frb.BridgeTaskRuntimeDto(
            runId: 'run-1',
            phase: 'reviewing',
            branch: 'codex/task',
            expectedHead: 'abcdef123456',
            statusMessage: 'Review returned',
            taskGeneration: BigInt.zero,
            workUnits: [],
            agents: [],
            merges: [],
            reviews: [
              frb.BridgeTaskReviewDto(
                round: 1,
                headCommit: 'abcdef123456',
                verdict: 'pass',
                reviewerAgentId: 'reviewer-1',
                summary: 'Passed',
                designReferences: ['design/16-task-orchestration.md#UI'],
              ),
            ],
          ),
        ),
      ),
    );

    final payload = event.payload as SessionTaskChangedPayload;
    expect(payload.task?.phase, 'reviewing');
    expect(payload.task?.branch, 'codex/task');
    expect(payload.task?.reviews.single.designReferences, [
      'design/16-task-orchestration.md#UI',
    ]);
  });
}
