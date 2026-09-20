part of '../widget_test.dart';

void registerInteractionTests() {
  testWidgets('running root can send and stop while keeping the next draft', (
    tester,
  ) async {
    final initial = _emptyState();
    final workspace = initial.selectedWorkspace!;
    final api = _FakeStudioApi(
      initial.copyWith(
        workspacesByThread: {
          ...initial.workspacesByThread,
          workspace.thread.id: workspace.copyWith(
            activeTurn: _testTurn(
              threadId: workspace.thread.id,
              turnId: 'running-turn',
              state: const RunningStudioTurnState(
                startedAt: 1,
                activity: StudioTurnActivity.thinking,
              ),
            ),
          ),
        },
      ),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'change direction',
    );
    await tester.pump();
    expect(find.byKey(StudioDriverKeys.composerStop), findsNothing);
    final send = tester.widget<IconButton>(
      find.byKey(StudioDriverKeys.composerSubmit),
    );
    expect(send.onPressed, isNotNull);
    expect(send.tooltip, 'Send and continue');
    await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(api.submittedInputs.single.input.text, 'change direction');
    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'next draft',
    );
    await tester.pump();
    expect(
      tester
          .widget<TextField>(find.byKey(StudioDriverKeys.composerInput))
          .controller!
          .text,
      'next draft',
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pump();
    expect(api.interruptedTurn, (
      threadId: 'session-1',
      turnId: 'running-turn',
    ));
    expect(api.submitPromptCount, 1);
  });

  testWidgets('Plan summary remains scrollable across short viewport heights', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    tester.platformDispatcher.textScaleFactorTestValue = 1.5;
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    final api = _FakeStudioApi(_stateWithPlanConfirmation());
    for (final size in [
      const Size(1280, 720),
      const Size(980, 600),
      const Size(1280, 520),
      const Size(760, 620),
    ]) {
      tester.view.physicalSize = size;
      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull, reason: 'Plan layout at $size');
      expect(find.byKey(StudioDriverKeys.planApprove), findsOneWidget);
      await tester.tap(find.byKey(StudioDriverKeys.planDetailsClose));
      await tester.pumpAndSettle();
      await tester.ensureVisible(find.byKey(StudioDriverKeys.planSummary));
      await tester.tap(find.byKey(StudioDriverKeys.planSummary));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull, reason: 'Plan reopened at $size');
    }
  });

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

  testWidgets(
    'focused Composer pastes an image through the canonical attachment rail',
    (tester) async {
      _configureResponsiveView(tester, const Size(1280, 800));
      final clipboard = _FakeClipboardImageReader(
        image: base64Decode(
          'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
        ),
      );
      final stager = _FakeClipboardImageStager();
      final api = _FakeStudioApi(_stateWithAttachmentModels())
        ..nextAdmittedDrafts = const [
          AttachmentDraftView(
            id: 'clipboard-image-1',
            modality: AttachmentModalityView.image,
            mediaType: 'image/png',
            filename: 'clipboard-image.png',
            byteSize: 68,
            width: 1,
            height: 1,
          ),
        ]
        ..attachmentDraftBytes['clipboard-image-1'] = clipboard.image!;

      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            studioApiProvider.overrideWithValue(api),
            clipboardImageReaderProvider.overrideWithValue(clipboard),
            clipboardImageStagerProvider.overrideWithValue(stager),
          ],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      await tester.showKeyboard(find.byKey(StudioDriverKeys.composerInput));
      await _sendControlPaste(tester);
      await tester.pumpAndSettle();

      expect(clipboard.imageReadCount, 1);
      expect(clipboard.textReadCount, 0);
      expect(api.attachmentAdmissionRequests, hasLength(1));
      final source =
          api.attachmentAdmissionRequests.single.sources.single
              as LocalFileAttachmentDraftSource;
      expect(source.path, stager.path);
      expect(stager.disposeCount, 1);
      expect(find.byKey(StudioDriverKeys.attachmentDraftRail), findsOneWidget);
      expect(
        find.byKey(StudioDriverKeys.attachmentDraft('clipboard-image-1')),
        findsOneWidget,
      );

      await tester.tap(
        find.byKey(StudioDriverKeys.attachmentRemove('clipboard-image-1')),
      );
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.attachmentDraftRail), findsNothing);
      expect(api.removedAttachmentDraftIds, ['clipboard-image-1']);
    },
  );

  testWidgets(
    'image-free clipboard paste replaces the current text selection',
    (tester) async {
      _configureResponsiveView(tester, const Size(1280, 800));
      final clipboard = _FakeClipboardImageReader(text: 'pasted');
      final api = _FakeStudioApi(_stateWithAttachmentModels());
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            studioApiProvider.overrideWithValue(api),
            clipboardImageReaderProvider.overrideWithValue(clipboard),
          ],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      final input = find.byKey(StudioDriverKeys.composerInput);
      await tester.enterText(input, 'hello world');
      final field = tester.widget<TextField>(input);
      field.controller!.selection = const TextSelection(
        baseOffset: 6,
        extentOffset: 11,
      );

      await _sendControlPaste(tester);
      await tester.pumpAndSettle();

      expect(field.controller!.text, 'hello pasted');
      expect(
        field.controller!.selection,
        const TextSelection.collapsed(offset: 12),
      );
      expect(clipboard.imageReadCount, 1);
      expect(clipboard.textReadCount, 1);
      expect(api.attachmentAdmissionRequests, isEmpty);
    },
  );

  testWidgets('unfocused Composer does not intercept paste shortcuts', (
    tester,
  ) async {
    final clipboard = _FakeClipboardImageReader(
      image: Uint8List.fromList([1, 2, 3]),
    );
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(
            _FakeStudioApi(_stateWithAttachmentModels()),
          ),
          clipboardImageReaderProvider.overrideWithValue(clipboard),
        ],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await _sendControlPaste(tester);
    await tester.pump();

    expect(clipboard.imageReadCount, 0);
    expect(clipboard.textReadCount, 0);
  });

  testWidgets('Composer rejects pasted images for a text-only model', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(1280, 800));
    final clipboard = _FakeClipboardImageReader(
      image: Uint8List.fromList([1, 2, 3]),
    );
    final api = _FakeStudioApi(_stateWithAttachmentModels(visualModel: false));
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(api),
          clipboardImageReaderProvider.overrideWithValue(clipboard),
        ],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();
    await tester.showKeyboard(find.byKey(StudioDriverKeys.composerInput));

    await _sendControlPaste(tester);
    await tester.pumpAndSettle();

    expect(api.attachmentAdmissionRequests, isEmpty);
    expect(find.byKey(StudioDriverKeys.composerError), findsOneWidget);
    expect(
      find.textContaining('does not support pasted images'),
      findsOneWidget,
    );
  });

  testWidgets(
    'Composer admits only one clipboard image while paste is pending',
    (tester) async {
      _configureResponsiveView(tester, const Size(1280, 800));
      final gate = Completer<Uint8List?>();
      final clipboard = _FakeClipboardImageReader(imageGate: gate);
      final stager = _FakeClipboardImageStager();
      final api = _FakeStudioApi(_stateWithAttachmentModels());
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            studioApiProvider.overrideWithValue(api),
            clipboardImageReaderProvider.overrideWithValue(clipboard),
            clipboardImageStagerProvider.overrideWithValue(stager),
          ],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      await tester.showKeyboard(find.byKey(StudioDriverKeys.composerInput));

      await _sendControlPaste(tester);
      await _sendControlPaste(tester);
      await tester.pump();
      expect(clipboard.imageReadCount, 1);

      gate.complete(Uint8List.fromList([1, 2, 3]));
      await tester.pumpAndSettle();
      expect(api.attachmentAdmissionRequests, hasLength(1));
      expect(stager.disposeCount, 1);
    },
  );

  test(
    'system clipboard image stager removes its temporary directory',
    () async {
      final staged = await const SystemClipboardImageStager().stage(
        Uint8List.fromList([1, 2, 3]),
      );
      final file = File(staged.path);

      expect(file.path, endsWith('/clipboard-image.png'));
      expect(await file.readAsBytes(), [1, 2, 3]);

      await staged.dispose();
      expect(await file.exists(), isFalse);
    },
  );

  testWidgets(
    'clipboard failure preserves the draft and reports Composer error',
    (tester) async {
      _configureResponsiveView(tester, const Size(1280, 800));
      final clipboard = _FakeClipboardImageReader(
        imageError: const FormatException('invalid clipboard image'),
      );
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            studioApiProvider.overrideWithValue(
              _FakeStudioApi(_stateWithAttachmentModels()),
            ),
            clipboardImageReaderProvider.overrideWithValue(clipboard),
          ],
          child: _localizedApp(home: const StudioShell()),
        ),
      );
      await tester.pumpAndSettle();
      final input = find.byKey(StudioDriverKeys.composerInput);
      await tester.enterText(input, 'keep this draft');
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.insert);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pumpAndSettle();

      expect(
        tester.widget<TextField>(input).controller!.text,
        'keep this draft',
      );
      expect(find.byKey(StudioDriverKeys.composerError), findsOneWidget);
      expect(
        find.textContaining('Unable to read the clipboard image.'),
        findsOneWidget,
      );
    },
  );

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
      tester.view.physicalSize = const Size(1600, 900);
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
      final status = tester.getRect(find.byType(ThreadStatusBar));
      final panel = tester.getRect(find.byKey(StudioDriverKeys.planDetails));
      final feedback = tester.getRect(
        find.byKey(StudioDriverKeys.planFeedbackInput),
      );
      expect(status.right, lessThanOrEqualTo(panel.left));
      expect(status.top, greaterThanOrEqualTo(feedback.bottom));
      expect(
        status.left,
        greaterThanOrEqualTo(
          tester.getRect(find.byKey(StudioDriverKeys.sidebar)).right,
        ),
      );

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

  testWidgets('composer submits the current draft on Enter like the button', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(1280, 800));

    final api = _FakeStudioApi(_emptyState());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'Enter submits this draft',
    );
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();

    expect(api.submitPromptCount, 1);
    expect(api.submittedInputs.single.threadId, 'session-1');
    expect(api.submittedInputs.single.input.text, 'Enter submits this draft');
  });

  testWidgets('composer keeps Shift+Enter as a newline without submitting', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(1280, 800));

    final api = _FakeStudioApi(_emptyState());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'line one',
    );
    await tester.pump();
    await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
    await tester.pump();

    expect(api.submitPromptCount, 0);
    expect(
      tester
          .widget<TextField>(find.byKey(StudioDriverKeys.composerInput))
          .controller!
          .text,
      'line one',
    );
  });

  testWidgets('composer ignores Enter when the draft is empty', (tester) async {
    _configureResponsiveView(tester, const Size(1280, 800));

    final api = _FakeStudioApi(_emptyState());
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.showKeyboard(find.byKey(StudioDriverKeys.composerInput));
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();

    expect(api.submitPromptCount, 0);
  });

  testWidgets('composer does not resubmit while a submission is pending', (
    tester,
  ) async {
    _configureResponsiveView(tester, const Size(1280, 800));

    final gate = Completer<SubmitPromptReceipt>();
    final api = _FakeStudioApi(_emptyState())..blockedPromptSubmit = gate;
    await tester.pumpWidget(
      ProviderScope(
        overrides: [studioApiProvider.overrideWithValue(api)],
        child: _localizedApp(home: const StudioShell()),
      ),
    );
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'blocked draft',
    );
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(api.submitPromptCount, 1);

    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump();
    expect(api.submitPromptCount, 1);

    gate.complete(
      const SubmitPromptReceipt(
        threadId: 'session-1',
        inputId: 'input-blocked',
        cursor: 1,
      ),
    );
    await tester.pumpAndSettle();
  });
}

Future<void> _sendControlPaste(WidgetTester tester) async {
  await tester.sendKeyDownEvent(LogicalKeyboardKey.controlLeft);
  await tester.sendKeyEvent(LogicalKeyboardKey.keyV);
  await tester.sendKeyUpEvent(LogicalKeyboardKey.controlLeft);
}

class _FakeClipboardImageReader implements ClipboardImageReader {
  _FakeClipboardImageReader({
    this.image,
    this.text,
    this.imageError,
    this.imageGate,
  });

  final Uint8List? image;
  final String? text;
  final Object? imageError;
  final Completer<Uint8List?>? imageGate;
  int imageReadCount = 0;
  int textReadCount = 0;

  @override
  Future<Uint8List?> readImage() async {
    imageReadCount += 1;
    if (imageError case final error?) throw error;
    return imageGate?.future ?? image;
  }

  @override
  Future<String?> readText() async {
    textReadCount += 1;
    return text;
  }
}

class _FakeClipboardImageStager implements ClipboardImageStager {
  _FakeClipboardImageStager();

  final String path = '/tmp/anywork-clipboard-test/clipboard-image.png';
  int disposeCount = 0;

  @override
  Future<StagedClipboardImage> stage(Uint8List pngBytes) async {
    return StagedClipboardImage(
      path: path,
      dispose: () async {
        disposeCount += 1;
      },
    );
  }
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
