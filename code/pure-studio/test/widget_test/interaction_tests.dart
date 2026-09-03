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
