part of '../widget_test.dart';

void registerInteractionTests() {
  testWidgets('Composer admits, previews, and removes a remote image draft', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(_stateWithAttachmentModels())
      ..nextAdmittedDrafts = const [
        AttachmentDraftView(
          id: 'draft-image-1',
          modality: AttachmentModalityView.image,
          mediaType: 'image/png',
          filename: 'marker.png',
          byteSize: 68,
          width: 1,
          height: 1,
        ),
      ]
      ..attachmentDraftBytes['draft-image-1'] = base64Decode(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
      );

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.attachmentEntry));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.attachmentUrl));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(StudioDriverKeys.attachmentUrlInput),
      'https://cdn.example/marker.png',
    );
    await tester.tap(find.byKey(StudioDriverKeys.attachmentUrlSubmit));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.attachmentDraftRail), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.attachmentDraft('draft-image-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.attachmentModality('draft-image-1')),
      findsOneWidget,
    );
    expect(find.textContaining('Vision'), findsWidgets);
    expect(api.attachmentAdmissionRequests, hasLength(1));
    expect(
      api.attachmentAdmissionRequests.single.context,
      isA<ExistingThreadAttachmentAdmissionContext>(),
    );
    final source = api.attachmentAdmissionRequests.single.sources.single;
    expect(source, isA<RemoteUrlAttachmentDraftSource>());
    expect(
      (source as RemoteUrlAttachmentDraftSource).url,
      'https://cdn.example/marker.png',
    );

    await tester.tap(
      find.byKey(StudioDriverKeys.attachmentRemove('draft-image-1')),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.attachmentDraftRail), findsNothing);
    expect(api.removedAttachmentDraftIds, ['draft-image-1']);
  });

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

  test('only the canonical Plan question derives Plan presentation', () {
    final interaction = _planConfirmationInteraction();

    expect(interaction.planConfirmation?.title, 'Plan review layout');
    expect(
      interaction.planConfirmation?.summary,
      'Keep the timeline compact while the full plan stays readable.',
    );
    expect(
      const PendingInteraction(
        id: 'ordinary-input',
        threadId: 'session-1',
        turnId: 'turn-1',
        kind: InteractionKind.userInput,
        title: 'Question',
        body: 'Continue?',
        payload: UserInputInteractionPayload(
          questions: [
            UserQuestionView(
              id: 'ordinary_question',
              header: 'Question',
              question: 'Continue?',
              isOther: false,
              isSecret: false,
              options: [],
            ),
          ],
        ),
      ).planConfirmation,
      isNull,
    );
  });

  testWidgets(
    'Plan confirmation renders a timeline summary, detail panel, and replacement composer',
    (tester) async {
      tester.view.physicalSize = const Size(1280, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final api = _FakeStudioApi(_stateWithPlanConfirmation());

      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.planSummary), findsOneWidget);
      expect(find.byKey(StudioDriverKeys.planDetails), findsOneWidget);
      expect(find.byKey(StudioDriverKeys.planDetailsScroll), findsOneWidget);
      expect(find.byKey(StudioDriverKeys.planFeedbackInput), findsOneWidget);
      expect(find.byKey(StudioDriverKeys.planApprove), findsOneWidget);
      expect(find.byKey(StudioDriverKeys.composerInput), findsNothing);
      expect(find.byKey(StudioDriverKeys.userInputFirstOption), findsNothing);

      final timelineScrollable = find
          .descendant(
            of: find.byKey(StudioDriverKeys.timeline),
            matching: find.byType(Scrollable),
          )
          .first;
      final detailsScrollable = find
          .descendant(
            of: find.byKey(StudioDriverKeys.planDetailsScroll),
            matching: find.byType(Scrollable),
          )
          .first;
      final timelineBefore = tester
          .state<ScrollableState>(timelineScrollable)
          .position
          .pixels;
      await tester.drag(
        find.byKey(StudioDriverKeys.planDetailsScroll),
        const Offset(0, -260),
      );
      await tester.pumpAndSettle();
      final detailsBeforeClose = tester
          .state<ScrollableState>(detailsScrollable)
          .position
          .pixels;
      expect(detailsBeforeClose, greaterThan(0));
      expect(
        tester.state<ScrollableState>(timelineScrollable).position.pixels,
        timelineBefore,
      );

      await tester.tap(find.byKey(StudioDriverKeys.planDetailsClose));
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.planDetails), findsNothing);

      await tester.tap(find.byKey(StudioDriverKeys.planSummary));
      await tester.pumpAndSettle();
      expect(find.byKey(StudioDriverKeys.planDetails), findsOneWidget);
      final restoredDetailsScrollable = find
          .descendant(
            of: find.byKey(StudioDriverKeys.planDetailsScroll),
            matching: find.byType(Scrollable),
          )
          .first;
      expect(
        tester
            .state<ScrollableState>(restoredDetailsScrollable)
            .position
            .pixels,
        closeTo(detailsBeforeClose, 0.5),
      );
    },
  );

  testWidgets('Plan revision submits one unambiguous typed answer', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(1280, 800);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(_stateWithPlanConfirmation());

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(StudioDriverKeys.planFeedbackInput),
      'Keep the panel narrower and preserve its scroll position.',
    );
    await tester.pump();
    expect(
      tester
          .widget<OutlinedButton>(
            find.byKey(StudioDriverKeys.planSubmitRevision),
          )
          .onPressed,
      isNotNull,
    );
    await tester.tap(find.byKey(StudioDriverKeys.planSubmitRevision));
    await tester.pumpAndSettle();

    expect(api.resolvedInteractionId, 'plan-confirmation');
    expect(api.resolvedInteraction, {
      'type': 'userInput',
      'answers': {
        agentSessionPlanConfirmationQuestionId: {
          'answers': [
            agentSessionPlanReviseAnswer,
            'Keep the panel narrower and preserve its scroll position.',
          ],
        },
      },
    });
    expect(find.byKey(StudioDriverKeys.planFeedbackInput), findsNothing);
    expect(find.byKey(StudioDriverKeys.composerInput), findsOneWidget);
  });

  testWidgets('Plan approval submits Approve without revision text', (
    tester,
  ) async {
    final api = _FakeStudioApi(_stateWithPlanConfirmation());

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.planApprove));
    await tester.pumpAndSettle();

    expect(api.resolvedInteraction, {
      'type': 'userInput',
      'answers': {
        agentSessionPlanConfirmationQuestionId: {
          'answers': [agentSessionPlanApproveAnswer],
        },
      },
    });
  });

  testWidgets('compact Plan details overlay only the timeline', (tester) async {
    tester.view.physicalSize = const Size(760, 720);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final api = _FakeStudioApi(_stateWithPlanConfirmation());

    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    expect(tester.takeException(), isNull);
    final details = tester.getRect(find.byKey(StudioDriverKeys.planDetails));
    final response = tester.getRect(
      find.byKey(StudioDriverKeys.planFeedbackInput),
    );
    expect(details.bottom, lessThanOrEqualTo(response.top));
  });

  test('interaction selection is scoped to the selected Thread', () {
    final state = _rootAndChildState();

    expect(state.activeInteraction, isNull);
    expect(
      state.copyWith(selectedThreadId: 'child-1').activeInteraction!.id,
      'child-interaction',
    );
  });

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

const _planMarkdown = '''
# Plan review layout

Keep the timeline compact while the full plan stays readable.

## Implementation

1. Project a compact summary into the timeline.
2. Open the full Markdown in an independently scrolling side panel.
3. Replace the normal composer with Plan revision and approval actions.

## Interaction details

- Keep the Plan body read-only.
- Preserve the timeline scroll position when the detail panel opens.
- Preserve the Plan detail scroll position when the panel closes.
- Keep the Plan feedback composer visible below the timeline.
- Do not expose the normal message composer while confirmation is pending.

## Responsive behavior

- Use a side-by-side panel when both panes remain readable.
- Overlay only the timeline at compact widths.
- Keep the Plan feedback composer outside the overlay.

## Verification

- Cover wide and compact layouts.
- Preserve the durable UserInput resolution contract.
''';

PendingInteraction _planConfirmationInteraction() {
  return const PendingInteraction(
    id: 'plan-confirmation',
    threadId: 'session-1',
    turnId: 'turn-plan',
    kind: InteractionKind.userInput,
    title: 'Plan',
    body: _planMarkdown,
    payload: UserInputInteractionPayload(
      questions: [
        UserQuestionView(
          id: agentSessionPlanConfirmationQuestionId,
          header: 'Plan',
          question: _planMarkdown,
          isOther: true,
          isSecret: false,
          options: [
            UserQuestionOptionView(
              label: agentSessionPlanApproveAnswer,
              description: 'Approve this exact Plan.',
            ),
            UserQuestionOptionView(
              label: agentSessionPlanReviseAnswer,
              description: 'Request a revised Plan.',
            ),
          ],
        ),
      ],
    ),
  );
}

StudioState _stateWithPlanConfirmation() {
  final initial = _emptyState();
  final threadId = initial.selectedThreadId!;
  return initial.copyWith(
    workspacesByThread: {
      ...initial.workspacesByThread,
      threadId: initial.selectedWorkspace!.copyWith(
        interactions: [_planConfirmationInteraction()],
      ),
    },
  );
}
