part of '../widget_test.dart';

void registerSnapshotSettingsTests() {
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
        settings: frb.BridgeStudioSettingsDto(
          defaultProviderId: 'zhipu-coding-plan',
          providers: [
            frb.BridgeProviderSettingsDto(
              id: 'zhipu-coding-plan',
              templateKind: 'zhipu-coding-plan',
              wireProtocol: 'chat_completions',
              connectionMode: 'http',
              name: 'Zhipu Coding Plan',
              baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
              hasBearerToken: true,
              capabilitySource: 'preset_defaults',
              hostedWebSearch: false,
              defaultModel: 'glm-5',
              models: [],
              customModels: [],
              catalogId: 'zhipu',
            ),
          ],
          roles: [],
          permissionMode: 'request-approval',
          instructions: frb.BridgeInstructionsSettingsDto(
            baseOverride: '',
            developer: '',
            user: '',
            projectDocMaxBytes: BigInt.from(65536),
            projectDocFallbackFilenames: [],
          ),
          skills: frb.BridgeSkillsSettingsDto(
            enabled: true,
            autoLearn: true,
            systemEnabled: true,
            projectDir: 'skills',
            userDir: '~/.pure/skills',
            externalDirs: [],
            disabled: [],
            autoLearnMinToolCalls: 5,
          ),
          mcpServers: [
            frb.BridgeMcpServerSettingsDto(
              id: 'zhipu_search',
              transport: 'streamableHttp',
              endpoint: 'https://open.bigmodel.cn/api/mcp/web_search_prime/mcp',
              enabled: true,
              status: 'enabled',
              sourceKind: 'builtIn',
              mutationPolicy: 'lockedIdentity',
            ),
            frb.BridgeMcpServerSettingsDto(
              id: 'zhipu_vision',
              transport: 'stdio',
              endpoint: 'npx',
              enabled: false,
              status: 'disabled',
              sourceKind: 'builtIn',
              mutationPolicy: 'lockedIdentity',
            ),
          ],
          general: frb.BridgeGeneralSettingsDto(
            followSystemTheme: true,
            followActiveTurn: true,
            compactTimeline: false,
          ),
          webSearch: frb.BridgeWebSearchSettingsDto(
            configuredMode: 'disabled',
            effectiveMode: 'disabled',
            availability: 'disabled',
            allowedDomains: [],
          ),
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
            completions: [],
            merges: [],
            reviews: [
              frb.BridgeTaskReviewDto(
                id: 'review-1',
                round: 1,
                scope: 'integrated',
                reviewedHead: 'abcdef123456',
                verdict: 'pass',
                requestedByCallId: 'call-review-1',
                reviewerAgentId: 'reviewer-1',
                summary: 'Passed',
                designReferences: const [
                  frb.BridgeTaskDesignReferenceDto(
                    path: 'design/16-task-orchestration.md',
                    section: 'UI',
                  ),
                ],
                findings: const [],
                createdAt: 1,
                updatedAt: 1,
              ),
            ],
          ),
        ),
      ),
    );

    final payload = event.payload as SessionTaskChangedPayload;
    expect(payload.task?.phase, 'reviewing');
    expect(payload.task?.branch, 'codex/task');
    expect(
      payload.task?.reviews.single.designReferences.single.path,
      'design/16-task-orchestration.md',
    );
  });
}
