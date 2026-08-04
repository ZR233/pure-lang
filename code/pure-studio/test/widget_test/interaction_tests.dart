part of '../widget_test.dart';

void registerInteractionTests() {
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
