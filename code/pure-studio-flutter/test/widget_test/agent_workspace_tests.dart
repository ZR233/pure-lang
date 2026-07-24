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
      controller.updateComposer('Planner draft');

      await controller.selectAgentSession('agent-session-1');
      var state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedAgentSessionId, 'agent-session-1');
      expect(state.composerText, isEmpty);
      expect(state.runtime.model, isEmpty);
      expect(state.turnPhase, TurnPhase.idle);

      await controller.selectAgentSession('session-1');
      state = container.read(studioControllerProvider).requireValue;
      expect(state.composerText, 'Planner draft');
      expect(state.runtime.model, 'planner/model');
    },
  );

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
      await pumpEventQueue();

      final state = container.read(studioControllerProvider).requireValue;
      expect(state.selectedAgentSessionId, 'agent-session-1');
      expect(state.selectedMessages, isEmpty);
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
    final api = _FakeStudioApi(_agentWorkspaceState());

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
}

StudioState _agentWorkspaceState({bool withTodo = false}) {
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
    runtime: const SessionRuntimeView(
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
