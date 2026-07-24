part of '../widget_test.dart';

void registerSnapshotJsonTests() {
  test('canonical session snapshot filters synthetic lifecycle parts', () {
    final state = applyCanonicalSessionSnapshot(_emptyState(), {
      'sessionId': 'session-1',
      'throughSequence': 4,
      'messages': [
        {
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'role': 'assistant',
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 1,
        },
      ],
      'parts': [
        {
          'partId': 'turn-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'order': 0,
          'revision': 0,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 1,
          'content': {'type': 'turn'},
          'synthetic': true,
          'ignored': false,
        },
        {
          'partId': 'turn-1-inf-1',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'order': 1,
          'revision': 0,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 1,
          'content': {
            'type': 'inference',
            'inferenceId': 'inf-1',
            'model': 'model',
          },
          'synthetic': true,
          'ignored': false,
        },
        {
          'partId': 'turn-1-final',
          'messageId': 'turn-1:assistant',
          'sessionId': 'session-1',
          'turnId': 'turn-1',
          'order': 2,
          'revision': 0,
          'status': 'completed',
          'createdAt': 1,
          'updatedAt': 1,
          'content': {
            'type': 'text',
            'channel': 'final',
            'text': 'visible answer',
            'attachments': <Object?>[],
          },
          'synthetic': false,
          'ignored': false,
        },
      ],
    });

    expect(state.partSnapshotsBySession['session-1']!.keys, {'turn-1-final'});
    expect(state.selectedTimelineRows.single.part!.text, 'visible answer');
  });

  test('config snapshot restores built-in Zhipu MCP metadata', () {
    final state = studioStateFromFrbSnapshot(
      frb.BridgeStudioSnapshotResponse(
        projects: const [],
        selectedProjectId: 'project-1',
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
            },
          },
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

  test('timeline parser accepts internal parts and rejects unknown values', () {
    final base = {
      'partId': 'part-1',
      'messageId': 'turn-1:assistant',
      'sessionId': 'session-1',
      'turnId': 'turn-1',
      'order': 0,
      'revision': 0,
      'status': 'completed',
      'createdAt': 1,
      'updatedAt': 2,
      'text': 'hello',
    };

    final message = TimelineMessage(
      id: 'turn-1:assistant',
      sessionId: 'session-1',
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(1000),
    );
    for (final internalType in ['file', 'turn', 'inference']) {
      final internalPart = timelinePartSnapshotFromJson({
        ...base,
        'type': internalType,
      });
      expect(isInternalTimelinePartType(internalPart.type), isTrue);
      expect(
        timelineRowsFromMessages(
          [message],
          parts: [timelinePartFromSnapshot(internalPart)],
        ),
        isEmpty,
      );
    }
    expect(
      () => timelinePartSnapshotFromJson({...base, 'type': 'widget'}),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Unknown timeline part type'),
        ),
      ),
    );
    expect(
      () => timelinePartSnapshotFromJson({
        ...base,
        'type': 'text',
        'textChannel': 'draft',
      }),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Unknown text channel'),
        ),
      ),
    );
  });

  test('realtime synthetic lifecycle part updates are ignored', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    api.emitSession(
      _canonicalSessionEvent(
        sessionId: 'session-1',
        kind: {
          'type': 'partChanged',
          'part': {
            'partId': 'turn-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'order': 0,
            'revision': 0,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 1,
            'content': {'type': 'turn'},
            'synthetic': true,
            'ignored': false,
          },
        },
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

  test('studio bridge event normalizes canonical session delta', () {
    final event = _canonicalSessionEvent(
      sessionId: 'session-1',
      sequence: 9,
      kind: {
        'type': 'partDelta',
        'delta': {
          'partId': 'part-1',
          'revision': 4,
          'field': 'text',
          'delta': 'typed',
        },
      },
    );

    final payload = event.payload;
    expect(payload, isA<MessagePartDeltaPayload>());
    expect((payload as MessagePartDeltaPayload).delta.delta, 'typed');
    expect(payload.delta.revision, 4);
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
          task: const frb.BridgeTaskRuntimeDto(
            runId: 'run-1',
            phase: 'reviewing',
            branch: 'codex/task',
            expectedHead: 'abcdef123456',
            statusMessage: 'Review returned',
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

  test('canonical message part events reject unknown part types', () {
    expect(
      () => _canonicalSessionEvent(
        sessionId: 'session-1',
        kind: {
          'type': 'partChanged',
          'part': {
            'partId': 'part-1',
            'messageId': 'turn-1:assistant',
            'sessionId': 'session-1',
            'turnId': 'turn-1',
            'order': 0,
            'revision': 0,
            'status': 'completed',
            'createdAt': 1,
            'updatedAt': 1,
            'content': {'type': 'widget'},
            'synthetic': false,
            'ignored': false,
          },
        },
      ),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Unknown timeline part type'),
        ),
      ),
    );
  });

  test('timeline delta parser accepts only v2 fields', () {
    final base = {
      'sessionId': 'session-1',
      'messageId': 'message-1',
      'partId': 'part-1',
      'revision': 1,
      'delta': 'summary',
    };

    expect(
      timelinePartDeltaFromJson({...base, 'field': 'reasoning.summary'}).field,
      'reasoning.summary',
    );
    expect(
      () => timelinePartDeltaFromJson({...base, 'field': 'reasoningText'}),
      throwsA(
        isA<FormatException>().having(
          (error) => error.message,
          'message',
          contains('Unknown timeline delta field'),
        ),
      ),
    );
  });
}
