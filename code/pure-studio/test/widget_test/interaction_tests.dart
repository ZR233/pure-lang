part of '../widget_test.dart';

void registerInteractionTests() {
  testWidgets('Composer exposes an accepted Turn failure by driver key', (
    tester,
  ) async {
    final initial = _emptyState();
    final root = initial.selectedThread!;
    final workspace = AgentWorkspaceView(
      thread: root,
      rootThread: root,
      syncState: AgentWorkspaceSyncState.ready,
      timelineRows: const [],
      todo: null,
      runtime: _testRuntime(),
      turn: null,
      activeInteraction: null,
      composer: const ComposerThreadState.failure(
        error: 'Invalid schema for function skill_manage',
      ),
      composerMode: AgentComposerMode.editable,
      permissionMode: PermissionMode.requestApproval,
      providers: const [],
      roles: const [],
      agents: const [],
    );

    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(_FakeStudioApi(initial)),
        ],
        child: _localizedApp(
          locale: const Locale('zh'),
          home: Scaffold(body: ComposerDock(workspace: workspace)),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.composerError), findsOneWidget);
    expect(
      find.text('Invalid schema for function skill_manage'),
      findsOneWidget,
    );
  });

  test('interaction selection is scoped to the selected Thread', () {
    final state = _rootAndChildState();

    expect(state.activeInteraction, isNull);
    expect(
      state.copyWith(selectedThreadId: 'child-1').activeInteraction!.id,
      'child-interaction',
    );
  });

  test(
    'failed interaction response keeps the durable request visible',
    () async {
      final initial = _emptyState();
      const interaction = PendingInteraction(
        id: 'interaction-1',
        threadId: 'session-1',
        turnId: 'turn-1',
        kind: InteractionKind.planConfirmation,
        title: 'Confirm plan',
        body: 'Plan body',
      );
      final api = _FakeStudioApi(
        initial.copyWith(
          workspacesByThread: {
            'session-1': initial.selectedWorkspace!.copyWith(
              interactions: const [interaction],
            ),
          },
        ),
      )..resolveInteractionError = StateError('bridge unavailable');
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await expectLater(
        container
            .read(studioControllerProvider.notifier)
            .resolveActiveInteraction(
              'session-1',
              const PlanConfirmationResolutionCommand(
                decision: PlanConfirmationDecision.dismiss,
              ),
            ),
        throwsStateError,
      );

      expect(
        container.read(studioControllerProvider).requireValue.activeInteraction,
        interaction,
      );
    },
  );

  test('respondInteraction sends a typed plan decision exactly once', () async {
    final initial = _emptyState();
    const interaction = PendingInteraction(
      id: 'plan-1',
      threadId: 'session-1',
      turnId: 'turn-1',
      kind: InteractionKind.planConfirmation,
      title: 'Confirm plan',
      body: 'Plan body',
    );
    final api = _FakeStudioApi(
      initial.copyWith(
        workspacesByThread: {
          'session-1': initial.selectedWorkspace!.copyWith(
            interactions: const [interaction],
          ),
        },
      ),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container
        .read(studioControllerProvider.notifier)
        .resolveActiveInteraction(
          'session-1',
          const PlanConfirmationResolutionCommand(
            decision: PlanConfirmationDecision.continuePlanning,
            reason: 'expand risks',
          ),
        );

    expect(api.resolveInteractionCount, 1);
    expect(api.resolvedInteraction, {
      'type': 'planConfirmation',
      'decision': 'continuePlanning',
      'reason': 'expand risks',
    });
  });
}
