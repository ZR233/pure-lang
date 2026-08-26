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
              interaction.id,
              const PlanConfirmationResolutionCommand(
                decision: PlanConfirmationDecision.revisePlan,
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
          interaction.id,
          const PlanConfirmationResolutionCommand(
            decision: PlanConfirmationDecision.revisePlan,
            reason: 'expand risks',
          ),
        );

    expect(api.resolveInteractionCount, 1);
    expect(api.resolvedInteraction, {
      'type': 'planConfirmation',
      'decision': 'revisePlan',
      'reason': 'expand risks',
    });
  });

  testWidgets('plan confirmation button submits the displayed interaction id', (
    tester,
  ) async {
    final initial = _emptyState();
    const interaction = PendingInteraction(
      id: 'displayed-plan',
      threadId: 'session-1',
      turnId: 'turn-1',
      kind: InteractionKind.planConfirmation,
      title: 'Confirm plan',
      body: 'Plan body',
    );
    final state = initial.copyWith(
      workspacesByThread: {
        'session-1': initial.selectedWorkspace!.copyWith(
          interactions: const [interaction],
        ),
      },
    );
    final api = _FakeStudioApi(state);
    final root = state.selectedThread!;
    final workspace = AgentWorkspaceView(
      thread: root,
      rootThread: root,
      syncState: AgentWorkspaceSyncState.ready,
      timelineRows: const [],
      todo: null,
      runtime: _testRuntime(),
      turn: null,
      activeInteraction: interaction,
      composer: const ComposerThreadState.idle(),
      composerMode: AgentComposerMode.editable,
      permissionMode: PermissionMode.requestApproval,
      providers: const [],
      roles: const [],
      agents: const [],
    );

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(
          locale: const Locale('zh'),
          home: Scaffold(body: ComposerDock(workspace: workspace)),
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.planConfirm));
    await tester.pumpAndSettle();

    expect(api.resolvedInteractionId, interaction.id);
    expect(api.resolveInteractionCount, 1);
  });

  testWidgets('terminal cleanup snapshot removes the plan confirmation dock', (
    tester,
  ) async {
    final initial = _emptyState();
    const interaction = PendingInteraction(
      id: 'terminal-plan',
      threadId: 'session-1',
      turnId: 'turn-1',
      kind: InteractionKind.planConfirmation,
      title: 'Confirm plan',
      body: 'Plan body',
    );
    final state = initial.copyWith(
      workspacesByThread: {
        'session-1': initial.selectedWorkspace!.copyWith(
          interactions: const [interaction],
        ),
      },
    );
    final api = _FakeStudioApi(state);

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(
          locale: const Locale('zh'),
          home: Consumer(
            builder: (context, ref, child) {
              final workspace = ref
                  .watch(studioControllerProvider)
                  .value
                  ?.selectedAgentWorkspace;
              return Scaffold(
                body: workspace == null
                    ? const SizedBox.shrink()
                    : ComposerDock(workspace: workspace),
              );
            },
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.planConfirm), findsOneWidget);

    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: state.selectedWorkspace!.copyWith(
          revision: state.selectedWorkspace!.revision + 1,
          interactions: const [],
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.planConfirm), findsNothing);
  });

  test('stale displayed interaction is rejected as a conflict', () async {
    final initial = _emptyState();
    const current = PendingInteraction(
      id: 'current-plan',
      threadId: 'session-1',
      turnId: 'turn-2',
      kind: InteractionKind.planConfirmation,
      title: 'Confirm plan',
      body: 'Current plan',
    );
    final api = _FakeStudioApi(
      initial.copyWith(
        workspacesByThread: {
          'session-1': initial.selectedWorkspace!.copyWith(
            interactions: const [current],
          ),
        },
      ),
    );
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
            'stale-plan',
            const PlanConfirmationResolutionCommand(
              decision: PlanConfirmationDecision.confirm,
            ),
          ),
      throwsA(
        isA<StudioFailure>().having(
          (error) => error.code,
          'code',
          StudioFailureCode.conflict,
        ),
      ),
    );
    expect(api.resolveInteractionCount, 0);
  });

  test(
    'completed task rejects a remaining plan interaction as a conflict',
    () async {
      final initial = _emptyState();
      const interaction = PendingInteraction(
        id: 'completed-task-plan',
        threadId: 'session-1',
        turnId: 'turn-1',
        kind: InteractionKind.planConfirmation,
        title: 'Confirm plan',
        body: 'Plan body',
      );
      final completedTask = TaskRuntimeView(
        runId: 'task-1',
        state: CompletedTaskStateView(
          outcome: FailedTaskOutcomeView(
            kind: TaskFailureKindView.fatal,
            summary: 'Planner failed',
            evidence: 'test evidence',
            cause: 'test failure',
            completedAt: DateTime.fromMillisecondsSinceEpoch(0),
          ),
        ),
        revision: 2,
        generation: 0,
        workUnits: const [],
        completions: const [],
        merges: const [],
        reviews: const [],
      );
      final state = initial.copyWith(
        workspacesByThread: {
          'session-1': initial.selectedWorkspace!.copyWith(
            interactions: const [interaction],
          ),
        },
        taskDirectory: TaskDirectoryState(
          values: [
            TaskDirectoryEntryView(
              rootThreadId: 'session-1',
              task: completedTask,
            ),
          ],
        ),
      );
      final api = _FakeStudioApi(state);
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
              interaction.id,
              const PlanConfirmationResolutionCommand(
                decision: PlanConfirmationDecision.confirm,
              ),
            ),
        throwsA(
          isA<StudioFailure>().having(
            (error) => error.code,
            'code',
            StudioFailureCode.conflict,
          ),
        ),
      );
      expect(api.resolveInteractionCount, 0);
    },
  );

  test(
    'interaction replacement during a request preserves the replacement',
    () async {
      final initial = _emptyState();
      const original = PendingInteraction(
        id: 'original-plan',
        threadId: 'session-1',
        turnId: 'turn-1',
        kind: InteractionKind.planConfirmation,
        title: 'Confirm plan',
        body: 'Original plan',
      );
      const replacement = PendingInteraction(
        id: 'replacement-plan',
        threadId: 'session-1',
        turnId: 'turn-2',
        kind: InteractionKind.planConfirmation,
        title: 'Confirm plan',
        body: 'Replacement plan',
      );
      final state = initial.copyWith(
        workspacesByThread: {
          'session-1': initial.selectedWorkspace!.copyWith(
            interactions: const [original],
          ),
        },
      );
      final api = _FakeStudioApi(state)
        ..blockedInteractionResponse = Completer<PendingInteraction>();
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      await pumpEventQueue();

      final response = container
          .read(studioControllerProvider.notifier)
          .resolveActiveInteraction(
            'session-1',
            original.id,
            const PlanConfirmationResolutionCommand(
              decision: PlanConfirmationDecision.confirm,
            ),
          );
      await pumpEventQueue();
      api.emitThreadFrame(
        ThreadSnapshotFrame(
          workspace: state.selectedWorkspace!.copyWith(
            revision: state.selectedWorkspace!.revision + 1,
            interactions: const [replacement],
          ),
        ),
      );
      await pumpEventQueue();
      api.blockedInteractionResponse!.complete(original);

      await expectLater(
        response,
        throwsA(
          isA<StudioFailure>().having(
            (error) => error.code,
            'code',
            StudioFailureCode.conflict,
          ),
        ),
      );
      expect(
        container.read(studioControllerProvider).requireValue.activeInteraction,
        replacement,
      );
    },
  );

  test('successful response reveals an already pending interaction', () async {
    final initial = _emptyState();
    const original = PendingInteraction(
      id: 'original-approval',
      threadId: 'session-1',
      turnId: 'turn-1',
      kind: InteractionKind.toolApproval,
      title: 'Approve tool',
      body: 'Tool request',
    );
    const following = PendingInteraction(
      id: 'following-input',
      threadId: 'session-1',
      turnId: 'turn-2',
      kind: InteractionKind.userInput,
      title: 'Provide input',
      body: 'Question',
    );
    final state = initial.copyWith(
      workspacesByThread: {
        'session-1': initial.selectedWorkspace!.copyWith(
          interactions: const [original, following],
        ),
      },
    );
    final api = _FakeStudioApi(state)
      ..blockedInteractionResponse = Completer<PendingInteraction>();
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    final response = container
        .read(studioControllerProvider.notifier)
        .resolveActiveInteraction(
          'session-1',
          original.id,
          const ToolApprovalResolutionCommand(
            decision: ToolApprovalDecision.approved,
          ),
        );
    await pumpEventQueue();
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: state.selectedWorkspace!.copyWith(
          revision: state.selectedWorkspace!.revision + 1,
          interactions: const [following],
        ),
      ),
    );
    await pumpEventQueue();
    api.blockedInteractionResponse!.complete(original);

    await response;
    expect(
      container.read(studioControllerProvider).requireValue.activeInteraction,
      following,
    );
    expect(api.resolveInteractionCount, 1);
  });
}
