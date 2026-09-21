part of '../widget_test.dart';

void registerControllerStreamTests() {
  test(
    'attachment-only submission preserves drafts on failure and sends IDs',
    () async {
      final api = _FakeStudioApi(_stateWithAttachmentModels())
        ..nextAdmittedDrafts = const [
          AttachmentDraftView(
            id: 'draft-local-1',
            modality: AttachmentModalityView.image,
            mediaType: 'image/png',
            filename: 'PURE-7429.png',
            byteSize: 128,
            width: 20,
            height: 10,
          ),
        ]
        ..attachmentDraftBytes['draft-local-1'] = Uint8List.fromList([1, 2, 3]);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      await controller.addLocalAttachments([
        '/tmp/PURE-7429.png',
      ], threadId: 'session-1');

      var composer = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi
          .composer;
      expect(composer.attachments.map((item) => item.id), ['draft-local-1']);
      expect(
        api.attachmentAdmissionRequests.single.context,
        isA<ExistingThreadAttachmentAdmissionContext>(),
      );

      api.submitPromptError = Exception('provider unavailable');
      await controller.submitComposer('session-1');
      composer = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi
          .composer;
      expect(composer.error, contains('provider unavailable'));
      expect(composer.attachments.map((item) => item.id), ['draft-local-1']);
      expect(api.submittedInputs.last.input.text, isEmpty);
      expect(api.submittedInputs.last.input.attachmentDraftIds, [
        'draft-local-1',
      ]);

      api.submitPromptError = null;
      await controller.submitComposer('session-1');
      composer = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi
          .composer;
      expect(composer.attachments, isEmpty);
      expect(api.submittedInputs.last.input.attachmentDraftIds, [
        'draft-local-1',
      ]);
    },
  );

  test('model switch rejects drafts unsupported by the target model', () async {
    final api = _FakeStudioApi(_stateWithAttachmentModels())
      ..nextAdmittedDrafts = const [
        AttachmentDraftView(
          id: 'draft-conflict',
          modality: AttachmentModalityView.image,
          mediaType: 'image/png',
          filename: 'conflict.png',
          byteSize: 12,
        ),
      ]
      ..attachmentDraftBytes['draft-conflict'] = Uint8List.fromList([1]);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);
    await controller.addLocalAttachments([
      '/tmp/conflict.png',
    ], threadId: 'session-1');

    await controller.setThreadModelRoute(providerId: 'zhipu', model: 'glm-5.3');

    expect(api.threadModelRouteUpdate, isNull);
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi
          .composer
          .error,
      contains('conflict.png'),
    );
  });

  test(
    'controller opens only the selected Thread on explicit interaction',
    () async {
      final api = _FakeStudioApi(_emptyState());
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await pumpEventQueue();

      // §6.1：首屏只恢复“选择”，不打开会话——没有订阅、没有打开数据库、没有历史加载。
      expect(api.threadSubscriptions, isEmpty);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .selectedWorkspaceUi
            .syncState,
        AgentWorkspaceSyncState.idle,
      );

      await _openSelectedThread(container);

      expect(api.threadSubscriptions, ['session-1']);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .selectedWorkspaceUi
            .subscriptionGeneration,
        greaterThan(0),
      );
    },
  );

  test(
    'Mode model command response cannot overwrite a newer settings event',
    () async {
      final initial = _stateWithPlannerModels();
      final api = _FakeStudioApi(initial)
        ..blockedModeRouteSave = Completer<SettingsStateSnapshot>();
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      final save = controller.setModeModelRoute(
        mode: ThreadModeId.simple,
        providerId: 'deepseek',
        model: 'deepseek-flash',
        effort: 'max',
      );
      await pumpEventQueue();
      final eventSettings = _settingsSnapshot(
        initial.settingsState,
        revision: initial.settingsRevision + 2,
        modeModelRoutes: [
          for (final route in initial.modeModelRoutes)
            route.modeId == ThreadModeId.simple
                ? const ModeModelRouteView(
                    modeId: ThreadModeId.simple,
                    providerId: 'openai',
                    model: 'gpt-5.6',
                    effort: 'high',
                  )
                : route,
        ],
      );
      api.emitGlobal(_settingsChangedEvent(eventSettings));
      await pumpEventQueue();
      api.blockedModeRouteSave!.complete(
        _settingsSnapshot(
          initial.settingsState,
          revision: initial.settingsRevision + 1,
          modeModelRoutes: [
            for (final route in initial.modeModelRoutes)
              route.modeId == ThreadModeId.simple
                  ? const ModeModelRouteView(
                      modeId: ThreadModeId.simple,
                      providerId: 'deepseek',
                      model: 'deepseek-flash',
                      effort: 'max',
                    )
                  : route,
          ],
        ),
      );
      await save;

      final state = container.read(studioControllerProvider).requireValue;
      expect(state.settingsRevision, initial.settingsRevision + 2);
      final route = state.modeModelRoutes
          .where((route) => route.modeId == ThreadModeId.simple)
          .first;
      expect(route.providerId, 'openai');
      expect(route.model, 'gpt-5.6');
      expect(state.selectedThreadId, initial.selectedThreadId);
      expect(state.selectedWorkspace?.items, initial.selectedWorkspace?.items);
      expect(state.runtime, initial.runtime);
    },
  );

  test(
    'idle and running composers submit through the same prompt command',
    () async {
      final initial = _stateWithPlannerModels();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      final controller = container.read(studioControllerProvider.notifier);

      await container.read(studioControllerProvider.future);
      await pumpEventQueue();
      await _openSelectedThread(container);
      controller.updateComposer('session-1', 'first');
      await controller.submitComposer('session-1');
      expect(api.submittedPrompts.single.prompt, 'first');

      api.emitThreadFrame(
        ThreadSnapshotFrame(
          workspace: initial.selectedWorkspace!.copyWith(
            revision: 1,
            activeTurn: _testTurn(
              threadId: 'session-1',
              state: const RunningStudioTurnState(
                startedAt: 1,
                activity: StudioTurnActivity.thinking,
              ),
              turnId: api.submitTurnId,
            ),
          ),
        ),
      );
      await pumpEventQueue();
      controller.updateComposer('session-1', 'steer');
      await controller.submitComposer('session-1');
      expect(api.submittedPrompts.last.prompt, 'steer');
      expect(api.submitPromptCount, 2);
    },
  );

  test('failed prompt retry retains its identity and accepted input unlocks the next draft', () async {
    final api = _FakeStudioApi(_emptyState())
      ..submitPromptError = Exception('connection lost');
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    final controller = container.read(studioControllerProvider.notifier);
    controller.updateComposer('session-1', 'same request');
    await controller.submitComposer('session-1');
    final firstId = api.submittedInputs.last.input.inputId;
    expect(
      container.read(studioControllerProvider).requireValue.composer.draft,
      'same request',
    );
    api.submitPromptError = null;
    await controller.submitComposer('session-1');
    expect(api.submittedInputs.last.input.inputId, firstId);
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .composer
          .isSubmissionPending,
      isFalse,
    );
    controller.updateComposer('session-1', 'next request');
    await controller.submitComposer('session-1');
    expect(api.submittedInputs.last.input.inputId, isNot(firstId));
    expect(api.submittedInputs.last.input.text, 'next request');
  });

  test('terminal reconnect snapshot releases pending submission and allows another send', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    await _openSelectedThread(container);
    final controller = container.read(studioControllerProvider.notifier);
    controller.updateComposer('session-1', 'first');
    await controller.submitComposer('session-1');
    final timestamp = DateTime.fromMillisecondsSinceEpoch(2000, isUtc: true);
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: initial.selectedWorkspace!.copyWith(
          revision: 2,
          activeTurn: null,
        ),
      ),
    );
    // 终态 Turn 通过 Item 通知进入窗口：数据库正文是时间线的事实源。
    api.emitThreadFrame(
      _threadItemFrame(
        threadId: 'session-1',
        workspaceRevision: 3,
        item: ThreadItemView(
          id: 'terminal-item',
          threadId: 'session-1',
          turnId: api.submitTurnId,
          ordinal: 2,
          revision: 2,
          createdAt: timestamp,
          updatedAt: timestamp,
          state: const ThreadTurnItemStateView(
            FailedStudioTurnState(
              startedAt: 1,
              completedAt: 2,
              failure: StudioTurnFailureView(
                category: 'protocol',
                providerKind: null,
                code: null,
                httpStatus: null,
                message: 'Invalid event revision',
                retryable: false,
                retryAfterMs: null,
              ),
            ),
          ),
        ),
      ),
    );
    await pumpEventQueue();
    final composer = container
        .read(studioControllerProvider)
        .requireValue
        .composer;
    expect(composer.isSubmissionPending, isFalse);
    expect(composer.error, isNull);
    controller.updateComposer('session-1', 'second');
    await controller.submitComposer('session-1');
    expect(api.submitPromptCount, 2);
    expect(api.submittedPrompts.last.prompt, 'second');
  });

  test('admission clears the composer before TurnStarted', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    final controller = container.read(studioControllerProvider.notifier);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    await _openSelectedThread(container);
    controller.updateComposer('session-1', 'hello');
    await controller.submitComposer('session-1');
    expect(
      container.read(studioControllerProvider).requireValue.composer,
      isA<IdleComposerThreadState>(),
    );

    api.emitThreadFrame(
      _threadItemFrame(
        threadId: 'session-1',
        workspaceRevision: 1,
        item: _submittedInputItem(
          threadId: 'session-1',
          turnId: api.submitTurnId,
          inputId: api.submitInputId,
        ),
      ),
    );
    api.emitThreadFrame(
      _threadTurnFrame(
        threadId: 'session-1',
        workspaceRevision: 2,
        state: const RunningStudioTurnState(
          startedAt: 1,
          activity: StudioTurnActivity.preparing,
        ),
        turnId: api.submitTurnId,
      ),
    );
    await pumpEventQueue();

    expect(
      container.read(studioControllerProvider).requireValue.composer,
      isA<IdleComposerThreadState>(),
    );
  });

  test(
    'accepted input failures stay in timeline without locking the composer',
    () async {
      final initial = _emptyState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      final controller = container.read(studioControllerProvider.notifier);

      await container.read(studioControllerProvider.future);
      await pumpEventQueue();
      await _openSelectedThread(container);
      controller.updateComposer('session-1', 'hello');
      await controller.submitComposer('session-1');
      api.emitThreadFrame(
        _threadItemFrame(
          threadId: 'session-1',
          workspaceRevision: 1,
          item: _submittedInputItem(
            threadId: 'session-1',
            turnId: api.submitTurnId,
            inputId: api.submitInputId,
          ),
        ),
      );
      api.emitThreadFrame(
        _threadTurnFrame(
          threadId: 'session-1',
          workspaceRevision: 2,
          state: const RunningStudioTurnState(
            startedAt: 1,
            activity: StudioTurnActivity.preparing,
          ),
          turnId: api.submitTurnId,
        ),
      );
      await pumpEventQueue();
      expect(
        container.read(studioControllerProvider).requireValue.composer,
        isA<IdleComposerThreadState>(),
      );

      api.emitThreadFrame(
        _threadTurnFrame(
          threadId: 'session-1',
          workspaceRevision: 3,
          state: const FailedStudioTurnState(
            startedAt: 1,
            completedAt: 2,
            failure: StudioTurnFailureView(
              category: 'provider',
              providerKind: 'openaiCompatible',
              code: 'invalid_request_error',
              httpStatus: 400,
              message: 'Invalid schema for function skill_manage',
              retryable: false,
              retryAfterMs: null,
            ),
          ),
          turnId: api.submitTurnId,
        ),
      );
      await pumpEventQueue();

      final composer = container
          .read(studioControllerProvider)
          .requireValue
          .composer;
      expect(composer, isA<IdleComposerThreadState>());
      expect(composer.error, isNull);
    },
  );

  test('interrupt uses the exact active Turn identity', () async {
    final initial = _emptyState();
    final workspace = initial.selectedWorkspace!.copyWith(
      activeTurn: _testTurn(
        threadId: 'session-1',
        state: const RunningStudioTurnState(
          startedAt: 1,
          activity: StudioTurnActivity.thinking,
        ),
        turnId: 'turn-active',
      ),
    );
    final api = _FakeStudioApi(
      initial.copyWith(workspacesByThread: {'session-1': workspace}),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container.read(studioControllerProvider.notifier).stop('session-1');
    expect(api.interruptedTurn, (threadId: 'session-1', turnId: 'turn-active'));
  });

  test(
    'Lagged marks reconnecting and establishes a fresh generation',
    () async {
      final initial = _emptyState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await pumpEventQueue();
      await _openSelectedThread(container);
      final before = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi
          .subscriptionGeneration;
      final workspaceBefore = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace;
      final productReadsBefore = api.bootstrapCount;
      api.emitThreadFrame(
        const ThreadResyncRequiredFrame(threadId: 'session-1', dropped: 3),
      );
      await Future<void>.delayed(const Duration(milliseconds: 220));
      await pumpEventQueue();

      final after = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi;
      expect(after.subscriptionGeneration, greaterThan(before));
      expect(api.threadSubscriptions.length, 2);
      // Resync 只恢复当前状态并从数据库刷新窗口，不重读 product snapshot。
      final afterWorkspace = container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace;
      expect(afterWorkspace!.revision, workspaceBefore!.revision);
      expect(afterWorkspace.items, workspaceBefore.items);
      expect(api.bootstrapCount, productReadsBefore);
    },
  );

  test('Thread switch does not wait for old transport teardown', () async {
    final cancellation = Completer<void>();
    final api = _FakeStudioApi(_emptyState())
      ..blockedThreadCancellation = cancellation;
    final coordinator = ThreadStreamCoordinator(
      api,
      (_, _, _) {},
      (_, _, _) {},
    );
    addTearDown(() async {
      if (!cancellation.isCompleted) cancellation.complete();
      await coordinator.dispose();
    });

    coordinator.switchThread('session-1');
    await pumpEventQueue();
    coordinator.switchThread('session-2');
    await Future<void>.delayed(const Duration(milliseconds: 200));

    expect(api.threadSubscriptions.last, 'session-2');
    cancellation.complete();
  });

  test('history paging derives its anchor from the loaded window', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    // 订阅建立后的权威窗口由历史 API 提供；快照只更新当前状态。
    api.historyPagesByThread['session-1'] = {
      null: ThreadHistoryPage(
        items: [
          _threadItemFixture(
            id: 'live-item',
            threadId: 'session-1',
            turnId: 'turn-live',
            ordinal: 10,
            text: 'live',
          ),
        ],
        nextCursor: 'live-item',
      ),
    };
    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    // 显式打开会话：首个权威帧之后读取首窗（§6.1）。
    await _openSelectedThread(container);
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspace!
          .items
          .map((item) => item.id),
      ['live-item'],
    );

    // 更旧一页：分页锚点来自已加载窗口的首条身份。
    api.historyPagesByThread['session-1'] = {
      'live-item': ThreadHistoryPage(
        items: [
          _threadItemFixture(
            id: 'history-item',
            threadId: 'session-1',
            turnId: 'turn-old',
            ordinal: -1,
            text: 'older',
          ),
        ],
        nextCursor: null,
      ),
    };

    await container
        .read(studioControllerProvider.notifier)
        .loadOlderHistory('session-1');
    await pumpEventQueue();

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.historyRequests.last.cursor, 'live-item');
    expect(state.selectedWorkspace!.items.map((item) => item.id), [
      'history-item',
      'live-item',
    ]);
    expect(state.selectedWorkspaceUi.history.hasOlder, isFalse);
  });

  test(
    'interaction response removes only the selected Thread request',
    () async {
      const interaction = PendingInteraction(
        id: 'interaction-1',
        threadId: 'session-1',
        turnId: 'turn-1',
        kind: InteractionKind.userInput,
        title: 'Question',
        body: 'Continue?',
      );
      final initial = _emptyState();
      final workspace = initial.selectedWorkspace!.copyWith(
        interactions: const [interaction],
      );
      final api = _FakeStudioApi(
        initial.copyWith(workspacesByThread: {'session-1': workspace}),
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
            const UserInputResolutionCommand(answers: []),
          );

      expect(api.resolvedInteractionId, interaction.id);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .selectedWorkspace!
            .interactions,
        isEmpty,
      );
    },
  );

  test('sidebar loadMoreThreads appends the next directory page', () async {
    final bootstrap = _emptyState();
    final initial = bootstrap.copyWith(
      threadDirectory: bootstrap.threadDirectory.copyWith(
        nextCursor: 'opaque-dir',
        hasMore: true,
      ),
    );
    final api = _FakeStudioApi(initial);
    final older = StudioThread(
      id: 'session-old',
      projectId: 'project-1',
      title: 'Older session',
      mode: ThreadModeId.simple,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      workspacePath: '.',
    );
    api.directoryPages['opaque-dir'] = ThreadDirectoryPage(
      threads: [older],
      nextCursor: null,
    );
    // 初始窗口标记还有更多页。
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    await container.read(studioControllerProvider.notifier).loadMoreThreads();

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.directoryPageRequests, ['opaque-dir']);
    expect(
      state.threads.map((thread) => thread.id),
      containsAll(['session-1', 'session-old']),
    );
    expect(state.threadDirectory.hasMore, isFalse);
    expect(state.threadDirectory.isLoading, isFalse);
  });

  test(
    'history window trims to the limit and keeps older history reachable',
    () async {
      final initial = _emptyState();
      final api = _FakeStudioApi(initial);
      List<ThreadItemView> windowItems(int base, int count) => List.generate(
        count,
        (index) => _threadItemFixture(
          id: 'item-${base + index}',
          threadId: 'session-1',
          turnId: 'turn-${base + index}',
          ordinal: base + index,
          text: 'message ${base + index}',
        ),
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      await pumpEventQueue();

      // 权威窗口来自历史 API：首窗 400 条、还有更旧内容；随后一页 120 条更旧
      // 历史使窗口达到 520，向旧翻页应淘汰远端最新的 20 条。
      api.historyPagesByThread['session-1'] = {
        null: ThreadHistoryPage(
          items: windowItems(0, 400),
          nextCursor: 'item-0',
        ),
      };
      // 显式打开会话：首窗由订阅建立后的权威读取提供。
      await _openSelectedThread(container);
      expect(api.historyRequests.last.cursor, isNull);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .selectedWorkspace!
            .items
            .length,
        400,
      );
      api.historyPagesByThread['session-1'] = {
        'item-0': ThreadHistoryPage(
          items: windowItems(-120, 120),
          nextCursor: null,
        ),
      };

      await container
          .read(studioControllerProvider.notifier)
          .loadOlderHistory('session-1');
      await pumpEventQueue();

      final state = container.read(studioControllerProvider).requireValue;
      final history = state.selectedWorkspaceUi.history;
      expect(state.selectedWorkspace!.items.length, 500);
      expect(state.selectedWorkspace!.items.first.id, 'item--120');
      expect(state.selectedWorkspace!.items.last.id, 'item-379');
      expect(history.hasOlder, isFalse);
      expect(history.hasNewer, isTrue);
      expect(history.isLoading, isFalse);
      expect(state.selectedWorkspace!.cachedItems.length, 500);

      api.historyPagesByThread['session-1'] = {
        'item-379': ThreadHistoryPage(
          items: windowItems(380, 20),
          nextCursor: null,
        ),
      };
      await container
          .read(studioControllerProvider.notifier)
          .loadNewerHistory('session-1');
      final newer = container.read(studioControllerProvider).requireValue;
      expect(api.historyRequests.last.cursor, 'item-379');
      expect(newer.selectedWorkspace!.items.map((item) => item.id), [
        for (var n = -100; n < 400; n++) 'item-$n',
      ]);
      expect(newer.selectedWorkspaceUi.history.hasNewer, isFalse);
      expect(newer.selectedWorkspaceUi.history.hasOlder, isTrue);
    },
  );
  test('selection survives directory delta that does not remove it', () async {
    // 分页窗口语义：选中线程不在窗口/增量中不得触发选择回退。
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    final before = container.read(studioControllerProvider).requireValue;
    expect(before.selectedThreadId, 'session-1');

    final other = StudioThread(
      id: 'session-busy-child',
      projectId: 'project-1',
      title: 'Busy child',
      mode: ThreadModeId.simple,
      parentThreadId: 'session-1',
      rootThreadId: 'session-1',
      updatedAt: DateTime.now(),
      workspacePath: '.',
    );
    api.emitGlobal(
      _threadDirectoryChangedEvent(projectId: 'project-1', threads: [other]),
    );
    await pumpEventQueue();

    final after = container.read(studioControllerProvider).requireValue;
    expect(after.selectedThreadId, 'session-1');
    expect(after.threads.map((thread) => thread.id), contains(other.id));
  });

  test('selection falls back only on explicit removal', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    api.emitGlobal(
      _threadDirectoryChangedEvent(
        projectId: 'project-1',
        threads: const [],
        removed: const ['session-1'],
      ),
    );
    await pumpEventQueue();

    final after = container.read(studioControllerProvider).requireValue;
    expect(after.selectedThreadId, isNull);
    expect(after.threads, isEmpty);
    expect(after.workspaceUiByThread.containsKey('session-1'), isFalse);
  });
  test(
    'resync reload keeps selection when incoming window drops the thread',
    () async {
      // 窗口化目录：resync 快照首页不含选中线程（被更新更活跃的线程挤出首页）
      // 时不得切换选择（选择是显式状态，仅 removal 增量可回退）。
      final initial = _emptyState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      // 模拟 resync：replace 快照不含 session-1（目录窗口被替换）后触发 Stale。
      api.debugReplaceCurrentState(
        _emptyState().copyWith(
          threadDirectory: _emptyState().threadDirectory.copyWith(
            threads: [
              StudioThread(
                id: 'session-other',
                projectId: 'project-1',
                title: 'Other',
                mode: ThreadModeId.simple,
                updatedAt: DateTime.now(),
                workspacePath: '.',
              ),
            ],
          ),
        ),
      );
      api.emitGlobal(
        const StudioBridgeEvent(payload: StalePayload(laggedEvents: 1)),
      );
      await pumpEventQueue();
      await controller.debugReloadForTest();

      final after = container.read(studioControllerProvider).requireValue;
      expect(after.selectedThreadId, 'session-1');
      // 关键回归：窗口被替换后不订阅其他线程（选择未被顶掉）。
      expect(
        api.threadSubscriptions.every((id) => id == 'session-1'),
        isTrue,
        reason: 'subscriptions: ${api.threadSubscriptions}',
      );
    },
  );

  test('resync reload merges newer model performance snapshot', () async {
    final initial = _emptyState().copyWith(
      modelPerformance: _modelPerformanceFixture(revision: 3),
    );
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);

    api.debugReplaceCurrentState(
      initial.copyWith(modelPerformance: _modelPerformanceFixture(revision: 4)),
    );
    api.emitGlobal(
      const StudioBridgeEvent(payload: StalePayload(laggedEvents: 1)),
    );
    await pumpEventQueue();
    await controller.debugReloadForTest();

    final after = container.read(studioControllerProvider).requireValue;
    expect(after.modelPerformance.revision, 4);
  });

  test('explicit start page survives reload and directory upserts', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);

    await controller.beginNewThread();
    controller.updateNewThreadComposer('project-local draft');
    await controller.debugReloadForTest();
    api.emitGlobal(
      _threadDirectoryChangedEvent(
        projectId: 'project-1',
        threads: [
          StudioThread(
            id: 'session-late',
            projectId: 'project-1',
            title: 'Late directory entry',
            mode: ThreadModeId.simple,
            updatedAt: DateTime.now(),
            workspacePath: '.',
          ),
        ],
      ),
    );
    await pumpEventQueue();

    final after = container.read(studioControllerProvider).requireValue;
    expect(after.selectedThreadId, isNull);
    expect(after.newThreadComposer.draft, 'project-local draft');
    expect(after.threads.map((thread) => thread.id), contains('session-late'));
    expect(api.threadSubscriptions, isNot(contains('session-late')));
  });

  test('failed first send keeps the start page draft and error', () async {
    final api = _FakeStudioApi(_emptyState())
      ..submitPromptError = Exception('first send rejected');
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);

    await controller.beginNewThread();
    controller.updateNewThreadComposer('keep this draft');
    await controller.submitNewThreadComposer();

    final after = container.read(studioControllerProvider).requireValue;
    expect(after.selectedThreadId, isNull);
    expect(after.newThreadComposer.draft, 'keep this draft');
    expect(after.newThreadComposer.error, contains('first send rejected'));
    expect(after.threads.map((thread) => thread.id), ['session-1']);
  });

  test(
    'first send inserts selects and subscribes the returned Thread',
    () async {
      final initial = _emptyState();
      final created = StudioThread(
        id: 'session-created',
        projectId: 'project-1',
        title: 'New Session',
        mode: ThreadModeId.simple,
        updatedAt: DateTime.now(),
        workspacePath: '.',
      );
      final api = _FakeStudioApi(initial)
        ..createThreadState = initial.copyWith(
          threadDirectory: ThreadDirectoryWindow(
            threads: [created, ...initial.threads],
          ),
        );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      await controller.beginNewThread();
      controller.updateNewThreadComposer('start here');
      await controller.submitNewThreadComposer();

      final after = container.read(studioControllerProvider).requireValue;
      expect(after.selectedThreadId, created.id);
      expect(after.threads.map((thread) => thread.id), contains(created.id));
      expect(
        after.workspaceUiByThread[created.id]?.composer,
        isA<IdleComposerThreadState>(),
      );
      expect(api.threadSubscriptions.last, created.id);
    },
  );

  test(
    'archive result can select a neighbor outside the loaded page',
    () async {
      final initial = _emptyState();
      final outside = StudioThread(
        id: 'session-outside-page',
        projectId: 'project-1',
        title: 'Outside page',
        mode: ThreadModeId.simple,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(-1),
        workspacePath: '.',
      );
      final api = _FakeStudioApi(initial)
        ..archiveThreadResult = ArchiveThreadResult(
          archivedRootId: 'session-1',
          removedThreadIds: const ['session-1'],
          nextRoot: outside,
        );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      await container
          .read(studioControllerProvider.notifier)
          .archiveThread('session-1');

      final after = container.read(studioControllerProvider).requireValue;
      expect(after.selectedThreadId, outside.id);
      expect(after.threads.map((thread) => thread.id), [outside.id]);
      expect(api.threadSubscriptions.last, outside.id);
    },
  );

  test('new Thread drafts stay isolated by Project', () async {
    final initial = _twoProjectState(selectedProjectId: 'project-a');
    final api = _FakeStudioApi(initial);
    api.selectProjectStates['project-b'] = _twoProjectState(
      selectedProjectId: 'project-b',
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);

    await controller.beginNewThread();
    controller.updateNewThreadComposer('draft A');
    await controller.selectProject('project-b');
    await controller.beginNewThread();
    controller.updateNewThreadComposer('draft B');

    final after = container.read(studioControllerProvider).requireValue;
    expect(after.selectedProjectId, 'project-b');
    expect(after.selectedThreadId, isNull);
    expect(after.newThreadComposerByProject['project-a']?.draft, 'draft A');
    expect(after.newThreadComposerByProject['project-b']?.draft, 'draft B');
  });

  test(
    'new Thread workspace mode defaults to local and stays isolated by Project',
    () async {
      final initial = _twoProjectState(selectedProjectId: 'project-a');
      final api = _FakeStudioApi(initial);
      api.selectProjectStates['project-b'] = _twoProjectState(
        selectedProjectId: 'project-b',
      );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      await controller.beginNewThread();
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .newThreadWorkspaceMode,
        ThreadWorkspaceMode.local,
        reason: 'the start page must default to the local workspace',
      );

      controller.setNewThreadWorkspaceMode(ThreadWorkspaceMode.worktree);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .newThreadWorkspaceMode,
        ThreadWorkspaceMode.worktree,
      );

      await controller.selectProject('project-b');
      await controller.beginNewThread();
      final other = container.read(studioControllerProvider).requireValue;
      expect(other.selectedProjectId, 'project-b');
      expect(
        other.newThreadWorkspaceMode,
        ThreadWorkspaceMode.local,
        reason: 'each Project keeps its own workspace-mode draft',
      );

      await controller.selectProject('project-a');
      await controller.beginNewThread();
      final restored = container.read(studioControllerProvider).requireValue;
      expect(restored.selectedProjectId, 'project-a');
      expect(
        restored.newThreadWorkspaceMode,
        ThreadWorkspaceMode.worktree,
        reason: 'the first Project keeps the draft it was given',
      );
    },
  );

  test(
    'first send forwards the selected workspace mode to the command',
    () async {
      final initial = _emptyState();
      final api = _FakeStudioApi(initial);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      await controller.beginNewThread();
      controller.setNewThreadWorkspaceMode(ThreadWorkspaceMode.worktree);
      controller.updateNewThreadComposer('worktree session');
      await controller.submitNewThreadComposer();

      expect(api.createdThreadProjectId, 'project-1');
      expect(api.createdThreadWorkspaceMode, 'worktree');
    },
  );

  test(
    'a rejected first send keeps the workspace mode draft and error',
    () async {
      final api = _FakeStudioApi(_emptyState())
        ..submitPromptError = Exception('worktree creation rejected');
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      await controller.beginNewThread();
      controller.setNewThreadWorkspaceMode(ThreadWorkspaceMode.worktree);
      controller.updateNewThreadComposer('keep this worktree draft');
      await controller.submitNewThreadComposer();

      final after = container.read(studioControllerProvider).requireValue;
      expect(after.selectedThreadId, isNull);
      expect(after.newThreadWorkspaceMode, ThreadWorkspaceMode.worktree);
      expect(after.newThreadComposer.draft, 'keep this worktree draft');
      expect(
        after.newThreadComposer.error,
        contains('worktree creation rejected'),
      );
    },
  );

  test('a late first-send response never exits a reset start page', () async {
    final initial = _emptyState();
    final created = StudioThread(
      id: 'session-created',
      projectId: 'project-1',
      title: 'New Session',
      mode: ThreadModeId.simple,
      updatedAt: DateTime.now(),
      workspacePath: '.',
    );
    final gate = Completer<SubmitPromptReceipt>();
    final api = _FakeStudioApi(initial)
      ..blockedPromptSubmit = gate
      ..createThreadState = initial.copyWith(
        threadDirectory: ThreadDirectoryWindow(
          threads: [created, ...initial.threads],
        ),
      );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);

    await controller.beginNewThread();
    controller.updateNewThreadComposer('slow first send');
    final sending = controller.submitNewThreadComposer();
    await pumpEventQueue();
    await controller.beginNewThread();
    gate.complete(
      const SubmitPromptReceipt(
        threadId: 'session-created',
        inputId: 'input-created',
        cursor: 1,
      ),
    );
    await sending;

    final after = container.read(studioControllerProvider).requireValue;
    expect(after.selectedThreadId, isNull);
    expect(after.newThreadComposer.draft, isEmpty);
    expect(after.threads.map((thread) => thread.id), contains(created.id));
  });

  test(
    'archiving a non-selected root preserves selection then last clears it',
    () async {
      final initial = _emptyState();
      final second = StudioThread(
        id: 'session-2',
        projectId: 'project-1',
        title: 'Second',
        mode: ThreadModeId.simple,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(-1),
        workspacePath: '.',
      );
      final state = initial.copyWith(
        threadDirectory: ThreadDirectoryWindow(
          threads: [...initial.threads, second],
        ),
      );
      final api = _FakeStudioApi(state)
        ..archiveThreadResult = const ArchiveThreadResult(
          archivedRootId: 'session-2',
          removedThreadIds: ['session-2'],
        );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      await controller.archiveThread(second.id);
      expect(
        container.read(studioControllerProvider).requireValue.selectedThreadId,
        'session-1',
      );
      api.archiveThreadResult = const ArchiveThreadResult(
        archivedRootId: 'session-1',
        removedThreadIds: ['session-1'],
      );
      await controller.archiveThread('session-1');

      final after = container.read(studioControllerProvider).requireValue;
      expect(after.selectedThreadId, isNull);
      expect(after.threads, isEmpty);
    },
  );

  test(
    'overlapping archive commands submit the same Thread only once',
    () async {
      final gate = Completer<ArchiveThreadResult>();
      final api = _FakeStudioApi(_emptyState())..blockedArchiveThread = gate;
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      final first = controller.archiveThread('session-1');
      await pumpEventQueue();
      final duplicate = controller.archiveThread('session-1');
      await pumpEventQueue();

      expect(api.archiveThreadCallCount, 1);
      gate.complete(
        const ArchiveThreadResult(
          archivedRootId: 'session-1',
          removedThreadIds: ['session-1'],
        ),
      );
      await Future.wait([first, duplicate]);
      expect(api.archiveThreadCallCount, 1);
      expect(
        container.read(studioControllerProvider).requireValue.threads,
        isEmpty,
      );
    },
  );

  test('persistence events apply only increasing revisions', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    api.emitGlobal(
      const StudioBridgeEvent(
        payload: PersistenceStateChangedPayload(
          PersistenceStateSnapshot(
            revision: 2,
            state: DegradedPersistenceState(
              pendingCommits: 4,
              oldestPendingRevision: 7,
              firstFailedAt: 10,
              error: ObservedResourceError(
                code: 'sqliteBusy',
                message: 'database is locked',
                retryable: true,
              ),
            ),
          ),
        ),
      ),
    );
    api.emitGlobal(
      const StudioBridgeEvent(
        payload: PersistenceStateChangedPayload(
          PersistenceStateSnapshot(
            revision: 1,
            state: RecoveringPersistenceState(
              pendingCommits: 2,
              oldestPendingRevision: 8,
              firstFailedAt: 10,
            ),
          ),
        ),
      ),
    );
    await pumpEventQueue();

    final persistence = container
        .read(studioControllerProvider)
        .requireValue
        .persistenceState;
    expect(persistence.revision, 2);
    expect(persistence.state, isA<DegradedPersistenceState>());
    expect(persistence.state.pendingCommits, 4);
  });

  test('degraded persistence keeps submit and stop available', () async {
    final initial = _emptyState();
    final workspace = initial.selectedWorkspace!.copyWith(
      activeTurn: _testTurn(
        threadId: 'session-1',
        state: const RunningStudioTurnState(
          startedAt: 1,
          activity: StudioTurnActivity.thinking,
        ),
        turnId: 'turn-active',
      ),
    );
    final api = _FakeStudioApi(
      initial.copyWith(
        workspacesByThread: {'session-1': workspace},
        persistenceState: const PersistenceStateSnapshot(
          revision: 3,
          state: DegradedPersistenceState(
            pendingCommits: 1,
            oldestPendingRevision: 2,
            firstFailedAt: 1,
            error: ObservedResourceError(
              code: 'sqliteBusy',
              message: 'database is locked',
              retryable: true,
            ),
          ),
        ),
      ),
    );
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);
    final controller = container.read(studioControllerProvider.notifier);

    controller.updateComposer('session-1', 'continue in memory');
    await controller.submitComposer('session-1');
    await controller.stop('session-1');

    expect(api.submitPromptCount, 1);
    expect(api.interruptedTurn, (threadId: 'session-1', turnId: 'turn-active'));
  });

  test(
    'degraded persistence still allows the current interaction to settle',
    () async {
      const interaction = PendingInteraction(
        id: 'interaction-degraded',
        threadId: 'session-1',
        turnId: 'turn-active',
        kind: InteractionKind.userInput,
        title: 'Question',
        body: 'Continue?',
      );
      final initial = _emptyState();
      final workspace = initial.selectedWorkspace!.copyWith(
        interactions: const [interaction],
      );
      final api = _FakeStudioApi(
        initial.copyWith(
          workspacesByThread: {'session-1': workspace},
          persistenceState: const PersistenceStateSnapshot(
            revision: 3,
            state: DegradedPersistenceState(
              pendingCommits: 1,
              oldestPendingRevision: 2,
              firstFailedAt: 1,
              error: ObservedResourceError(
                code: 'sqliteBusy',
                message: 'database is locked',
                retryable: true,
              ),
            ),
          ),
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
            const UserInputResolutionCommand(answers: []),
          );

      expect(api.resolvedInteractionId, interaction.id);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .selectedWorkspace!
            .interactions,
        isEmpty,
      );
    },
  );

  test(
    'manual persistence retry invokes the backend and accepts newer state',
    () async {
      final initial = _emptyState().copyWith(
        persistenceState: const PersistenceStateSnapshot(
          revision: 2,
          state: DegradedPersistenceState(
            pendingCommits: 2,
            oldestPendingRevision: 1,
            firstFailedAt: 1,
            error: ObservedResourceError(
              code: 'sqliteBusy',
              message: 'database is locked',
              retryable: true,
            ),
          ),
        ),
      );
      final api = _FakeStudioApi(initial)
        ..retryPersistenceState = const PersistenceStateSnapshot(
          revision: 3,
          state: RecoveringPersistenceState(
            pendingCommits: 1,
            oldestPendingRevision: 2,
            firstFailedAt: 1,
          ),
        );
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      await container
          .read(studioControllerProvider.notifier)
          .retryPersistence();

      expect(api.retryPersistenceCallCount, 1);
      expect(
        container
            .read(studioControllerProvider)
            .requireValue
            .persistenceState
            .state,
        isA<RecoveringPersistenceState>(),
      );
    },
  );

  test(
    'openRemoteProject refuses work before controller initialization',
    () async {
      final initializationGate = Completer<void>();
      final api = _FakeStudioApi(_emptyState())
        ..blockedStudioStateLoad = initializationGate;
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      final initialization = container.read(studioControllerProvider.future);
      await pumpEventQueue();

      final opened = await container
          .read(studioControllerProvider.notifier)
          .openRemoteProject('ssh-arm', '/workspace');

      expect(opened, isFalse);
      expect(api.openRemoteProjectCallCount, 0);
      initializationGate.complete();
      await initialization;
    },
  );

  test(
    'openRemoteProject reports failure when backend rejects the request',
    () async {
      final api = _FakeStudioApi(_emptyState())
        ..openRemoteProjectError = StateError('remote host refused');
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      final opened = await container
          .read(studioControllerProvider.notifier)
          .openRemoteProject('ssh-arm', '/workspace');

      expect(opened, isFalse);
      expect(api.openRemoteProjectCallCount, 1);
    },
  );

  test('openRemoteProject reports failure when canonical snapshot omits the project', () async {
    final api = _FakeStudioApi(_emptyState())
      ..selectProjectStates['remote-project'] = _emptyState();
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    await container.read(studioControllerProvider.future);

    final opened = await container
        .read(studioControllerProvider.notifier)
        .openRemoteProject('ssh-arm', '/workspace');

    expect(opened, isFalse);
    expect(api.openRemoteProjectCallCount, 1);
    expect(
      container.read(studioControllerProvider).requireValue.selectedProjectId,
      'project-1',
    );
  });

  test(
    'openRemoteProject adopts canonical project without backend selection',
    () async {
      final api = _FakeStudioApi(_emptyState())
        ..selectProjectStates['remote-project'] = _remoteProjectAdoptedState()
            .copyWith(selectedProjectId: null);
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      final opened = await container
          .read(studioControllerProvider.notifier)
          .openRemoteProject('ssh-arm', '/workspace');

      expect(opened, isTrue);
      expect(api.openRemoteProjectCallCount, 1);
      // 采用后实际状态中的选中项目必须是请求的远端项目：
      // 与 API 返回同 id、同 server、同 canonical path。
      final adopted = container.read(studioControllerProvider).requireValue;
      expect(adopted.selectedProjectId, 'remote-project');
      final selected = adopted.projects.firstWhere(
        (project) => project.id == 'remote-project',
      );
      expect(selected.sshAlias, 'ssh-arm');
      expect(selected.path, '/workspace');
    },
  );

  test('openRemoteProject rejects adoption when the project is local or on another server', () async {
    Future<bool> openWith(StudioState adopted) async {
      final api = _FakeStudioApi(_emptyState())
        ..selectProjectStates['remote-project'] = adopted;
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      return container
          .read(studioControllerProvider.notifier)
          .openRemoteProject('ssh-arm', '/workspace');
    }

    // 同 id 但 local（无 sshAlias）→ 拒绝。
    expect(await openWith(_remoteProjectAdoptedState(sshAlias: null)), isFalse);
    // 同 id 但归属另一台 server → 拒绝。
    expect(
      await openWith(_remoteProjectAdoptedState(sshAlias: 'other-server')),
      isFalse,
    );
  });
}
