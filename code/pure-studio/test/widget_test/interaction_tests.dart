part of '../widget_test.dart';

void registerInteractionTests() {
  testWidgets('user input interaction accepts freeform fallback answers', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-1',
          sessionId: 'session-1',
          kind: InteractionKind.userInput,
          title: 'Need input',
          body: 'Tell me which branch to use',
        ),
      ],
      turnPhasesBySession: const {'session-1': TurnPhase.waitingForInteraction},
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Tell me which branch to use'), findsWidgets);
    expect(find.widgetWithText(FilledButton, 'Answer'), findsOneWidget);
    await tester.enterText(find.byType(TextField).last, 'use main');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Answer'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-1');
    expect(api.resolvedInteraction?['type'], 'userInput');
    expect(api.resolvedInteraction?['answers'], {
      'answer': {
        'answers': ['use main'],
      },
    });
  });

  testWidgets('user input interaction submits paged multi-question answers', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-questions',
          sessionId: 'session-1',
          kind: InteractionKind.userInput,
          title: 'Need input',
          body: 'Choose implementation details',
          payload: UserInputInteractionPayload(
            questions: [
              UserQuestionView(
                id: 'scope',
                header: 'Scope',
                question: 'Pick the areas to update',
                isOther: false,
                isSecret: false,
                options: [
                  UserQuestionOptionView(
                    label: 'UI',
                    description: 'Polish the dock',
                  ),
                  UserQuestionOptionView(
                    label: 'Tests',
                    description: 'Add widget coverage',
                  ),
                ],
              ),
              UserQuestionView(
                id: 'notes',
                header: 'Notes',
                question: 'Anything else?',
                isOther: true,
                isSecret: false,
                options: [
                  UserQuestionOptionView(
                    label: 'Docs',
                    description: 'Update design notes',
                  ),
                ],
              ),
              UserQuestionView(
                id: 'secret',
                header: 'Secret',
                question: 'Secret value?',
                isOther: false,
                isSecret: true,
                options: [],
              ),
            ],
          ),
        ),
      ],
      turnPhasesBySession: const {'session-1': TurnPhase.waitingForInteraction},
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('A few questions'), findsOneWidget);
    expect(find.text('Question 1 / 3'), findsOneWidget);
    await tester.tap(find.text('UI'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.text('Tests'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Next'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Question 2 / 3'), findsOneWidget);
    await tester.tap(find.text('Docs'));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.enterText(find.byType(TextField).last, 'also mention risk');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Next'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Question 3 / 3'), findsOneWidget);
    await tester.enterText(find.byType(TextField).last, 'secret-value');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Submit answers'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-questions');
    expect(api.resolvedInteraction?['type'], 'userInput');
    expect(api.resolvedInteraction?['answers'], {
      'scope': {
        'answers': ['UI', 'Tests'],
      },
      'notes': {
        'answers': ['Docs', 'also mention risk'],
      },
      'secret': {
        'answers': ['secret-value'],
      },
    });
  });

  testWidgets('user input interaction resets drafts for new question payload', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final firstInteraction = const PendingInteraction(
      id: 'interaction-first',
      sessionId: 'session-1',
      kind: InteractionKind.userInput,
      title: 'Need input',
      body: 'First question',
      payload: UserInputInteractionPayload(
        questions: [
          UserQuestionView(
            id: '',
            header: 'First',
            question: 'First free text?',
            isOther: false,
            isSecret: false,
            options: [],
          ),
        ],
      ),
    );
    final secondInteraction = const PendingInteraction(
      id: 'interaction-second',
      sessionId: 'session-1',
      kind: InteractionKind.userInput,
      title: 'Need input',
      body: 'Second question',
      payload: UserInputInteractionPayload(
        questions: [
          UserQuestionView(
            id: '',
            header: 'Second',
            question: 'Second free text?',
            isOther: false,
            isSecret: false,
            options: [],
          ),
        ],
      ),
    );
    final api = _FakeStudioApi(
      _emptyState().copyWith(
        pendingInteractions: [firstInteraction],
        turnPhasesBySession: const {
          'session-1': TurnPhase.waitingForInteraction,
        },
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));
    await tester.enterText(find.byType(TextField).last, 'old draft');
    await tester.pump(const Duration(milliseconds: 50));

    api.emitSession(
      _interactionChangedEvent(
        sessionId: 'session-1',
        interaction: firstInteraction,
        status: 'resolved',
      ),
    );
    api.emitSession(
      _interactionChangedEvent(
        sessionId: 'session-1',
        interaction: secondInteraction,
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('old draft'), findsNothing);
    expect(find.text('Second free text?'), findsOneWidget);
    await tester.enterText(find.byType(TextField).last, 'new answer');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Submit answers'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-second');
    expect(api.resolvedInteraction?['answers'], {
      'answer_0': {
        'answers': ['new answer'],
      },
    });
  });

  testWidgets('plan confirmation implement keeps task mode', (tester) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      sessions: [
        StudioSession(
          id: 'session-1',
          projectId: 'project-1',
          title: 'Session',
          mode: StudioMode.task,
          updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
        ),
      ],
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-plan',
          sessionId: 'session-1',
          kind: InteractionKind.planConfirmation,
          title: 'Confirm plan',
          body: '## Plan\n- Implement',
          payload: PlanConfirmationInteractionPayload(
            planId: 'plan-1',
            content: '## Plan\n- Implement',
          ),
        ),
      ],
      turnPhasesBySession: const {'session-1': TurnPhase.completed},
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Implement this plan?'), findsOneWidget);
    expect(find.text('Implement this plan'), findsOneWidget);
    expect(find.text('Plan content'), findsNothing);
    expect(find.text('Task'), findsOneWidget);
    await tester.tap(find.widgetWithText(FilledButton, 'Implement this plan'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-plan');
    expect(api.resolvedInteraction?['type'], 'planConfirmation');
    expect(api.resolvedInteraction?['decision'], 'implementFreshContext');
    expect(api.resolvedInteraction?.containsKey('content'), isFalse);
    expect(find.text('Task'), findsOneWidget);
    expect(find.text('Simple'), findsNothing);
  });

  testWidgets('plan confirmation adjustment submits only user instruction', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-plan-adjust',
          sessionId: 'session-1',
          kind: InteractionKind.planConfirmation,
          title: 'Confirm plan',
          body: '## Plan\n- Implement',
          payload: PlanConfirmationInteractionPayload(
            planId: 'plan-1',
            content: '## Plan\n- Implement',
          ),
        ),
      ],
      turnPhasesBySession: const {'session-1': TurnPhase.completed},
    );
    final api = _FakeStudioApi(state);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('Tell Pure how to adjust'), findsNothing);
    expect(
      find.widgetWithText(TextField, 'Describe what should change...'),
      findsOneWidget,
    );
    await tester.enterText(find.byType(TextField).last, 'add tests first');
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.widgetWithText(FilledButton, 'Submit adjustment'));
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolvedInteractionId, 'interaction-plan-adjust');
    expect(api.resolvedInteraction, {
      'type': 'planConfirmation',
      'decision': 'continuePlanning',
      'content': 'add tests first',
      'reason': 'continue planning',
    });
  });

  testWidgets('plan confirmation failure stays pending and can be retried', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);

    final state = _emptyState().copyWith(
      sessions: [
        StudioSession(
          id: 'session-1',
          projectId: 'project-1',
          title: 'Session',
          mode: StudioMode.task,
          updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
        ),
      ],
      pendingInteractions: const [
        PendingInteraction(
          id: 'interaction-plan-failure',
          sessionId: 'session-1',
          kind: InteractionKind.planConfirmation,
          title: 'Confirm plan',
          body: '## Plan\n- Implement',
        ),
      ],
      turnPhasesBySession: const {'session-1': TurnPhase.completed},
    );
    final api = _FakeStudioApi(state)
      ..resolveInteractionError = StateError(
        'task mode requires a clean working tree',
      );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump(const Duration(milliseconds: 50));

    final implementButton = find.widgetWithText(
      FilledButton,
      'Implement this plan',
    );
    await tester.tap(implementButton);
    await tester.pump(const Duration(milliseconds: 50));

    expect(tester.takeException(), isNull);
    expect(api.resolveInteractionCount, 1);
    expect(find.byKey(const Key('plan-confirmation-error')), findsOneWidget);
    expect(find.textContaining('clean working tree'), findsOneWidget);
    expect(find.text('Implement this plan?'), findsOneWidget);
    expect(tester.widget<FilledButton>(implementButton).onPressed, isNotNull);

    api.resolveInteractionError = null;
    await tester.tap(implementButton);
    await tester.pump(const Duration(milliseconds: 50));

    expect(api.resolveInteractionCount, 2);
    expect(api.resolvedInteractionId, 'interaction-plan-failure');
    expect(tester.takeException(), isNull);
  });
}
