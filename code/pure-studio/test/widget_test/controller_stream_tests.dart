part of '../widget_test.dart';

void registerControllerStreamTests() {
  test('controller subscribes only the selected Thread', () async {
    final api = _FakeStudioApi(_emptyState());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();

    expect(api.threadSubscriptions, ['session-1']);
    expect(
      container
          .read(studioControllerProvider)
          .requireValue
          .selectedWorkspaceUi
          .subscriptionGeneration,
      greaterThan(0),
    );
  });

  test(
    'model command response cannot overwrite a newer settings event',
    () async {
      final initial = _stateWithPlannerModels();
      final api = _FakeStudioApi(initial)
        ..blockedModelRoleSave = Completer<SettingsStateSnapshot>();
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);
      final controller = container.read(studioControllerProvider.notifier);

      final save = controller.setModelRole(
        roleKey: 'planner',
        providerId: 'deepseek',
        model: 'deepseek-reasoner',
        effort: 'max',
      );
      await pumpEventQueue();
      final eventSettings = _settingsSnapshot(
        initial.settingsState,
        revision: initial.settingsRevision + 2,
        roles: [
          for (final role in initial.roles)
            role.key == 'planner'
                ? const RoleSettingsView(
                    key: 'planner',
                    providerId: 'openai',
                    model: 'gpt-5.6',
                    effort: 'high',
                  )
                : role,
        ],
      );
      api.emitGlobal(_settingsChangedEvent(eventSettings));
      await pumpEventQueue();
      api.blockedModelRoleSave!.complete(
        _settingsSnapshot(
          initial.settingsState,
          revision: initial.settingsRevision + 1,
          roles: [
            for (final role in initial.roles)
              role.key == 'planner'
                  ? const RoleSettingsView(
                      key: 'planner',
                      providerId: 'deepseek',
                      model: 'deepseek-reasoner',
                      effort: 'max',
                    )
                  : role,
          ],
        ),
      );
      await save;

      final state = container.read(studioControllerProvider).requireValue;
      expect(state.settingsRevision, initial.settingsRevision + 2);
      expect(state.role('planner')?.providerId, 'openai');
      expect(state.role('planner')?.model, 'gpt-5.6');
      expect(state.selectedWorkspace, same(initial.selectedWorkspace));
      expect(state.runtime, initial.runtime);
    },
  );

  test(
    'authoritative product snapshot removes a stale selected Task',
    () async {
      final staleTask = TaskRuntimeView(
        runId: 'task-stale',
        state: TaskStateView.facts(
          kind: TaskStateKind.implementing,
          generation: 1,
        ),
        branch: 'pure-task-stale',
        expectedHead: 'abc123',
        revision: 0,
        workUnits: [],
        completions: [],
        merges: [],
        reviews: [],
      );
      final initial = _twoProjectState(selectedProjectId: 'project-a').copyWith(
        taskDirectory: TaskDirectoryState(
          values: [
            TaskDirectoryEntryView(rootThreadId: 'session-b', task: staleTask),
          ],
        ),
      );
      final api = _FakeStudioApi(initial);
      api.selectProjectStates['project-b'] = _twoProjectState(
        selectedProjectId: 'project-b',
      ).copyWith(taskDirectory: const TaskDirectoryState());
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);

      await container.read(studioControllerProvider.future);
      await container
          .read(studioControllerProvider.notifier)
          .selectProject('project-b');

      final next = container.read(studioControllerProvider).requireValue;
      expect(next.selectedThreadId, 'session-b');
      expect(next.tasksByRootThread, isNot(contains('session-b')));
    },
  );

  test('idle composer starts a Turn and busy composer steers it', () async {
    final initial = _stateWithPlannerModels();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    final controller = container.read(studioControllerProvider.notifier);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    controller.updateComposer('session-1', 'first');
    await controller.submitComposer('session-1');
    expect(api.submittedPrompts.single.prompt, 'first');

    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: initial.selectedWorkspace!.copyWith(
          revision: 1,
          activeTurn: _testTurn(
            threadId: 'session-1',
            state: const StudioTurnState.inProgress(
              StudioTurnActivity.thinking,
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
  });

  test('TurnStarted clears the matching pending composer submission', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);
    final controller = container.read(studioControllerProvider.notifier);

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    controller.updateComposer('session-1', 'hello');
    await controller.submitComposer('session-1');
    expect(
      container.read(studioControllerProvider).requireValue.composer.phase,
      ComposerSubmissionPhase.pendingStart,
    );

    api.emitThreadFrame(
      _threadTurnFrame(
        threadId: 'session-1',
        workspaceRevision: 1,
        state: const StudioTurnState.inProgress(StudioTurnActivity.preparing),
        turnId: api.submitTurnId,
      ),
    );
    await pumpEventQueue();

    expect(
      container.read(studioControllerProvider).requireValue.composer.phase,
      ComposerSubmissionPhase.idle,
    );
  });

  test(
    'accepted Turn failure remains visible after TurnStarted cleared pending',
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
      controller.updateComposer('session-1', 'hello');
      await controller.submitComposer('session-1');
      api.emitThreadFrame(
        _threadTurnFrame(
          threadId: 'session-1',
          workspaceRevision: 1,
          state: const StudioTurnState.inProgress(StudioTurnActivity.preparing),
          turnId: api.submitTurnId,
        ),
      );
      await pumpEventQueue();
      expect(
        container.read(studioControllerProvider).requireValue.composer.phase,
        ComposerSubmissionPhase.idle,
      );

      api.emitThreadFrame(
        _threadTurnFrame(
          threadId: 'session-1',
          workspaceRevision: 2,
          state: const StudioTurnState.failed('fallback reason'),
          turnId: api.submitTurnId,
          failure: const StudioTurnFailureView(
            category: 'provider',
            providerKind: 'openaiCompatible',
            code: 'invalid_request_error',
            httpStatus: 400,
            message: 'Invalid schema for function skill_manage',
            retryable: false,
            retryAfterMs: null,
          ),
        ),
      );
      await pumpEventQueue();

      final composer = container
          .read(studioControllerProvider)
          .requireValue
          .composer;
      expect(composer.phase, ComposerSubmissionPhase.idle);
      expect(composer.error, 'Invalid schema for function skill_manage');
    },
  );

  test('interrupt uses the exact active Turn identity', () async {
    final initial = _emptyState();
    final workspace = initial.selectedWorkspace!.copyWith(
      activeTurn: _testTurn(
        threadId: 'session-1',
        state: const StudioTurnState.inProgress(StudioTurnActivity.thinking),
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
      expect(
        container.read(studioControllerProvider).requireValue.selectedWorkspace,
        same(workspaceBefore),
      );
      expect(api.bootstrapCount, productReadsBefore);
    },
  );

  test('Thread switch does not wait for old transport teardown', () async {
    final cancellation = Completer<void>();
    final api = _FakeStudioApi(_emptyState())
      ..blockedThreadCancellation = cancellation;
    final coordinator = ThreadStreamCoordinator(api, (_, _, _) {}, (_, _) {});
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

    await container.read(studioControllerProvider.future);
    await pumpEventQueue();
    api.emitThreadFrame(
      ThreadSnapshotFrame(
        workspace: initial.selectedWorkspace!.copyWith(
          revision: 1,
          items: [
            _threadItemFixture(
              id: 'live-item',
              threadId: 'session-1',
              turnId: 'turn-live',
              ordinal: 10,
              text: 'live',
            ),
          ],
        ),
        historyCursor: 'turn-live',
      ),
    );
    await pumpEventQueue();
    api.historyPagesByThread['session-1'] = {
      'turn-live': ThreadHistoryPage(
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

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.historyRequests.single.cursor, 'turn-live');
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
      mode: StudioMode.simple,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
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

      // 快照携带 400 条窗口内容与更旧回源锚点；随后一页 120 条更旧历史
      // 使窗口达到 520，超过 500 上限后从最旧方向裁剪 20 条。
      api.emitThreadFrame(
        ThreadSnapshotFrame(
          workspace: initial.selectedWorkspace!.copyWith(
            revision: 1,
            items: windowItems(0, 400),
          ),
          historyCursor: 'turn-0',
        ),
      );
      await pumpEventQueue();
      api.historyPagesByThread['session-1'] = {
        'turn-0': ThreadHistoryPage(
          items: windowItems(-120, 120),
          nextCursor: null,
        ),
      };

      await container
          .read(studioControllerProvider.notifier)
          .loadOlderHistory('session-1');

      final state = container.read(studioControllerProvider).requireValue;
      final history = state.selectedWorkspaceUi.history;
      expect(state.selectedWorkspace!.items.length, 500);
      // 裁剪后窗口首条是被保留的最旧条目；被裁内容仍可回源（hasOlder 保持）。
      expect(state.selectedWorkspace!.items.first.id, 'item--100');
      expect(history.hasOlder, isTrue);
      expect(history.isLoading, isFalse);

      // 再次回源的锚点从裁剪后的窗口首条派生。
      api.historyPagesByThread['session-1'] = {
        'turn--100': ThreadHistoryPage(
          items: windowItems(-140, 40),
          nextCursor: null,
        ),
      };
      await container
          .read(studioControllerProvider.notifier)
          .loadOlderHistory('session-1');
      expect(api.historyRequests.last.cursor, 'turn--100');
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
      mode: StudioMode.simple,
      parentThreadId: 'session-1',
      rootThreadId: 'session-1',
      updatedAt: DateTime.now(),
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
                mode: StudioMode.simple,
                updatedAt: DateTime.now(),
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
            mode: StudioMode.simple,
            updatedAt: DateTime.now(),
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
        mode: StudioMode.simple,
        updatedAt: DateTime.now(),
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
        after.workspaceUiByThread[created.id]?.composer.phase,
        ComposerSubmissionPhase.pendingStart,
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
        mode: StudioMode.simple,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(-1),
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

  test('a late first-send response never exits a reset start page', () async {
    final initial = _emptyState();
    final created = StudioThread(
      id: 'session-created',
      projectId: 'project-1',
      title: 'New Session',
      mode: StudioMode.simple,
      updatedAt: DateTime.now(),
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
        turnId: 'turn-created',
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
        mode: StudioMode.simple,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(-1),
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
}
