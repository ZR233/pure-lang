part of '../widget_test.dart';

void registerAgentWorkspaceTests() {
  test(
    'agent workspace keeps runtime and composer drafts by session',
    () async {
      final initial = _agentWorkspaceState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);
      controller.updateComposer('session-1', 'Planner draft');

      await controller.selectAgentSession('agent-session-1');
      var state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedAgentSessionId, 'agent-session-1');
      expect(state.composerText, isEmpty);
      expect(state.runtime.model, isEmpty);
      expect(state.turnPhase, TurnPhase.idle);
      expect(
        state.selectedAgentWorkspace?.syncState,
        AgentWorkspaceSyncState.loading,
      );
      expect(state.selectedTimelineRows, isEmpty);

      final timestamp = DateTime.fromMillisecondsSinceEpoch(2000);
      final childSnapshot = initial.copyWith(
        selectedSessionId: 'agent-session-1',
        selectedRootSessionId: 'session-1',
        runtimesBySession: {
          'agent-session-1': const SessionRuntimeView(
            model: 'reviewer/model',
            contextTokens: 48,
            contextWindow: 1000,
            totalTokens: 72,
            costLabel: '',
            activeSkills: ['review-skill'],
            activeMcpServers: ['review-mcp'],
            activeLspServers: [],
            agentCount: 0,
          ),
        },
        messagesBySession: {
          ...initial.messagesBySession,
          'agent-session-1': [
            TimelineMessage(
              id: 'reviewer-message',
              sessionId: 'agent-session-1',
              role: 'assistant',
              createdAt: timestamp,
            ),
          ],
        },
        partSnapshotsBySession: {
          ...initial.partSnapshotsBySession,
          'agent-session-1': {
            'reviewer-part': TimelinePartSnapshot(
              id: 'reviewer-part',
              messageId: 'reviewer-message',
              sessionId: 'agent-session-1',
              turnId: 'reviewer-turn',
              type: TimelinePartType.text,
              order: 0,
              revision: 0,
              text: 'Reviewer snapshot',
              status: 'completed',
              createdAt: timestamp,
              updatedAt: timestamp,
            ),
          },
        },
      );
      api.emitSessionFrame(_sessionSnapshotFrame(childSnapshot));
      await pumpEventQueue();

      state = container.read(studioControllerProvider).requireValue;
      final workspace = state.selectedAgentWorkspace!;
      expect(workspace.syncState, AgentWorkspaceSyncState.ready);
      expect(workspace.runtime.model, 'reviewer/model');
      expect(workspace.runtime.activeMcpServers, ['review-mcp']);
      expect(workspace.timelineRows, isNotEmpty);
      expect(state.selectedMessages.single.sessionId, 'agent-session-1');

      api.emitSession(
        _interactionChangedEvent(
          sessionId: 'agent-session-1',
          interaction: const PendingInteraction(
            id: 'reviewer-input',
            sessionId: 'agent-session-1',
            kind: InteractionKind.userInput,
            title: 'Reviewer input',
            body: 'Choose an option',
          ),
        ),
      );
      await pumpEventQueue();
      state = container.read(studioControllerProvider).requireValue;
      expect(
        state.selectedAgentWorkspace?.activeInteraction?.id,
        'reviewer-input',
      );

      await controller.selectAgentSession('session-1');
      state = container.read(studioControllerProvider).requireValue;
      expect(state.composerText, 'Planner draft');
      expect(state.runtime.model, 'planner/model');
    },
  );

  test('executor workspace uses its own timeline and todo snapshot', () {
    final initial = _agentWorkspaceState(withTodo: true);
    final root = initial.selectedRootSession!;
    final executor = initial.sessions
        .where((session) => session.isAgent)
        .single
        .copyWith(ownerRole: 'executor');
    final rootMessage = TimelineMessage(
      id: 'root-message',
      sessionId: root.id,
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(2000),
    );
    final executorMessage = TimelineMessage(
      id: 'executor-message',
      sessionId: executor.id,
      role: 'assistant',
      createdAt: DateTime.fromMillisecondsSinceEpoch(3000),
    );
    final state = initial.copyWith(
      sessions: [root, executor],
      selectedSessionId: executor.id,
      messagesBySession: {
        root.id: [rootMessage],
        executor.id: [executorMessage],
      },
      agentTimelineEventsBySession: {
        ...initial.agentTimelineEventsBySession,
        executor.id: {
          'executor-todo': TimelineAgentEvent(
            eventId: 'executor-todo',
            sessionId: executor.id,
            sequence: 2,
            createdAt: DateTime.fromMillisecondsSinceEpoch(3000),
            payload: const TimelineTodoListUpdate(
              callId: 'executor-todo-call',
              explanation: 'Executor checklist',
              items: [
                TimelineTodoItem(
                  step: 'Implement the plan',
                  status: 'inProgress',
                ),
              ],
            ),
          ),
        },
      },
    );

    expect(state.selectedAgentSessionId, executor.id);
    expect(state.selectedTimelineSessionId, executor.id);
    expect(state.selectedMessages, [executorMessage]);
    expect(state.selectedTodoList?.explanation, 'Executor checklist');
  });

  test('non-executor agents keep their own timeline', () {
    final initial = _agentWorkspaceState();
    final reviewer = initial.sessions
        .where((session) => session.isAgent)
        .single;
    final state = initial.copyWith(selectedSessionId: reviewer.id);

    expect(state.selectedTimelineSessionId, reviewer.id);
  });

  test(
    'directory refresh never switches away from the selected planner',
    () async {
      final initial = _agentWorkspaceState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      final child = initial.sessions
          .where((session) => session.id == 'agent-session-1')
          .single;
      api.emitGlobal(
        _sessionListChangedEvent(
          projectId: 'project-1',
          sessions: [
            initial.selectedRootSession!,
            child.copyWith(
              agentStatus: 'running',
              agentSummary: 'Working in the background',
            ),
          ],
        ),
      );
      await pumpEventQueue();

      final state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedAgentSessionId, 'session-1');
      expect(
        state.sessions
            .where((session) => session.id == 'agent-session-1')
            .single
            .agentStatus,
        'running',
      );
    },
  );

  test(
    'late frames from the previous agent cannot pollute the new workspace',
    () async {
      final initial = _agentWorkspaceState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await container
          .read(studioControllerProvider.notifier)
          .selectAgentSession('agent-session-1');
      api.emitSession(
        _messageUpdatedEvent(
          sessionId: 'session-1',
          message: {
            'messageId': 'late-root-message',
            'sessionId': 'session-1',
            'turnId': 'late-root-turn',
            'role': 'assistant',
            'status': 'completed',
            'createdAt': 2,
            'updatedAt': 2,
          },
        ),
      );
      api.emitSession(
        _sessionRuntimeChangedEvent(
          sessionId: 'session-1',
          runtime: const SessionRuntimeView(
            model: 'late/root-model',
            contextTokens: 900,
            contextWindow: 1000,
            totalTokens: 900,
            costLabel: '',
            activeSkills: ['late-root-skill'],
            activeMcpServers: [],
            activeLspServers: [],
            agentCount: 0,
          ),
        ),
      );
      api.emitSession(
        _interactionChangedEvent(
          sessionId: 'session-1',
          interaction: const PendingInteraction(
            id: 'late-root-interaction',
            sessionId: 'session-1',
            kind: InteractionKind.toolApproval,
            title: 'Late approval',
            body: 'Ignore this frame',
          ),
        ),
      );
      await pumpEventQueue();

      final state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedAgentSessionId, 'agent-session-1');
      expect(state.selectedMessages, isEmpty);
      expect(state.runtime.model, isEmpty);
      expect(state.activeInteraction, isNull);
      expect(
        state.messagesBySession['session-1']?.any(
              (message) => message.id == 'late-root-message',
            ) ??
            false,
        isFalse,
      );
    },
  );

  testWidgets('agent switcher is the only child-agent navigation surface', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(_agentWorkspaceState(cacheChild: true));

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('agent-switcher')), findsOneWidget);
    expect(find.text('2 agents'), findsOneWidget);
    expect(find.text('Child reviewer'), findsNothing);

    await tester.tap(find.byKey(const ValueKey('agent-switcher')));
    await tester.pumpAndSettle();
    expect(find.text('Planner'), findsOneWidget);
    expect(find.text('Child reviewer'), findsOneWidget);

    await tester.tap(
      find.byKey(const ValueKey('agent-session-agent-session-1')),
    );
    await tester.pumpAndSettle();
    expect(
      find.text('This agent session is driven by the runtime'),
      findsOneWidget,
    );
    expect(find.text('reviewer/model'), findsOneWidget);
    expect(find.byTooltip('Planner model'), findsNothing);
    expect(find.byTooltip('Reasoning effort'), findsNothing);
    expect(
      find.byWidgetPredicate(
        (widget) =>
            widget is TextField &&
            widget.decoration?.hintText == 'Describe what you need...',
      ),
      findsNothing,
    );
    expect(
      tester.widgetList<Text>(find.text('Root planning session')).length,
      greaterThanOrEqualTo(1),
    );
  });

  testWidgets('agent switcher opens after the hover delay', (tester) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(_agentWorkspaceState());

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
    addTearDown(gesture.removePointer);
    await gesture.addPointer();
    await gesture.moveTo(
      tester.getCenter(find.byKey(const ValueKey('agent-switcher'))),
    );
    await tester.pump(const Duration(milliseconds: 249));
    expect(find.text('Child reviewer'), findsNothing);
    await tester.pump(const Duration(milliseconds: 1));
    await tester.pump();
    expect(find.text('Child reviewer'), findsOneWidget);
  });

  testWidgets('unfinished todo auto-opens once in the wide side panel', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(_agentWorkspaceState(withTodo: true));

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('todo-side-panel')), findsOneWidget);
    expect(find.text('Agent workspace checklist'), findsOneWidget);
    expect(find.text('Keep timeline agent-local'), findsOneWidget);
    expect(find.text('Finish snapshot refresh'), findsOneWidget);
    expect(find.text('Agent workspace checklist'), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('todo-close-button')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('todo-side-panel')), findsNothing);
    expect(find.byKey(const ValueKey('todo-open-button')), findsOneWidget);
    await tester.pump();
    expect(find.byKey(const ValueKey('todo-side-panel')), findsNothing);
  });

  testWidgets('narrow todo uses an end drawer without shrinking timeline', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(900, 700);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(_agentWorkspaceState(withTodo: true));

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const ValueKey('todo-drawer-panel')), findsOneWidget);
    expect(
      tester.getSize(find.byKey(const ValueKey('todo-drawer-panel'))).width,
      328,
    );
    await tester.tap(find.byKey(const ValueKey('todo-close-button')));
    await tester.pumpAndSettle();
    expect(find.byType(TimelineView), findsOneWidget);
    expect(tester.getSize(find.byType(TimelineView)).width, greaterThan(500));
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'agent workspace previews isolate root child and loading states',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);

      await tester.pumpWidget(agentWorkspaceRootPreview());
      await tester.pumpAndSettle();
      expect(find.text('Refine the implementation plan'), findsOneWidget);
      expect(
        find.text(
          'Planner owns this root workspace and its editable composer.',
        ),
        findsOneWidget,
      );

      await tester.pumpWidget(agentWorkspaceChildPreview());
      await tester.pumpAndSettle();
      expect(find.text('reviewer/model'), findsOneWidget);
      expect(
        find.text('This agent session is driven by the runtime'),
        findsOneWidget,
      );
      expect(
        find.text('Reviewer is checking the workspace boundary.'),
        findsOneWidget,
      );

      await tester.pumpWidget(agentWorkspaceLoadingPreview());
      await tester.pump();
      expect(
        find.byKey(const ValueKey('agent-workspace-loading')),
        findsOneWidget,
      );
      expect(find.text('reviewer/model'), findsNothing);
      expect(
        find.text('Reviewer is checking the workspace boundary.'),
        findsNothing,
      );
      expect(tester.takeException(), isNull);
    },
  );
}

StudioState _agentWorkspaceState({
  bool withTodo = false,
  bool cacheChild = false,
}) {
  final base = _emptyState();
  final timestamp = DateTime.fromMillisecondsSinceEpoch(1000);
  final root = base.sessions.single.copyWith(
    title: 'Root planning session',
    createdAt: timestamp,
    updatedAt: timestamp,
    ownerAgentId: 'planner-agent',
    ownerRole: 'planner',
    agentStatus: 'idle',
  );
  final child = StudioSession(
    id: 'agent-session-1',
    projectId: root.projectId,
    title: 'Child reviewer',
    mode: root.mode,
    createdAt: timestamp.add(const Duration(seconds: 1)),
    updatedAt: timestamp.add(const Duration(seconds: 1)),
    parentSessionId: root.id,
    rootSessionId: root.id,
    sessionKind: StudioSessionKind.agent,
    ownerAgentId: 'reviewer-agent',
    ownerRole: 'reviewer',
    agentStatus: 'waiting',
  );
  return base.copyWith(
    sessions: [root, child],
    selectedRootSessionId: root.id,
    runtimesBySession: {
      root.id: const SessionRuntimeView(
        model: 'planner/model',
        contextTokens: 120,
        contextWindow: 1000,
        totalTokens: 180,
        costLabel: '',
        activeSkills: ['session-skill'],
        activeMcpServers: [],
        activeLspServers: [],
        agentCount: 1,
      ),
      if (cacheChild)
        child.id: const SessionRuntimeView(
          model: 'reviewer/model',
          contextTokens: 42,
          contextWindow: 1000,
          totalTokens: 64,
          costLabel: '',
          activeSkills: ['review-skill'],
          activeMcpServers: [],
          activeLspServers: [],
          agentCount: 0,
        ),
    },
    turnPhasesBySession: cacheChild
        ? {child.id: TurnPhase.waitingForModel}
        : const {},
    workspaceSyncBySession: cacheChild
        ? {child.id: AgentWorkspaceSyncState.ready}
        : const {},
    agentTimelineEventsBySession: withTodo
        ? {
            root.id: {
              'todo-1': TimelineAgentEvent(
                eventId: 'todo-1',
                sessionId: root.id,
                sequence: 1,
                createdAt: timestamp,
                payload: const TimelineTodoListUpdate(
                  callId: 'todo-call-1',
                  explanation: 'Agent workspace checklist',
                  items: [
                    TimelineTodoItem(
                      step: 'Keep timeline agent-local',
                      status: 'completed',
                    ),
                    TimelineTodoItem(
                      step: 'Finish snapshot refresh',
                      status: 'inProgress',
                    ),
                  ],
                ),
              ),
            },
          }
        : const {},
  );
}
