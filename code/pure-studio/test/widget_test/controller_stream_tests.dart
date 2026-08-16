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
      const staleTask = TaskRuntimeView(
        runId: 'task-stale',
        phase: 'implementing',
        branch: 'pure-task-stale',
        expectedHead: 'abc123',
        statusMessage: null,
        stopRequestedOrigin: null,
        stopRequestedReason: null,
        taskGeneration: 1,
        workUnits: [],
        completions: [],
        merges: [],
        reviews: [],
      );
      final initial = _twoProjectState(selectedProjectId: 'project-a').copyWith(
        taskDirectory: const TaskDirectoryState(
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

  test('history uses opaque cursor and merges by Item identity', () async {
    final initial = _emptyState();
    final api = _FakeStudioApi(initial);
    api.historyPagesByThread['session-1'] = {
      null: ThreadHistoryPage(
        items: [
          _threadItemFixture(
            id: 'history-item',
            threadId: 'session-1',
            turnId: 'turn-old',
            ordinal: -1,
            text: 'older',
          ),
        ],
        nextCursor: 'opaque-next',
      ),
    };
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    await container.read(studioControllerProvider.future);
    await container
        .read(studioControllerProvider.notifier)
        .loadOlderHistory('session-1');

    final state = container.read(studioControllerProvider).requireValue;
    expect(api.historyRequests.single.cursor, isNull);
    expect(state.selectedWorkspace!.items.single.text, 'older');
    expect(state.selectedWorkspaceUi.history.nextCursor, 'opaque-next');
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
    'history eviction rolls the cursor back to the evicted page boundary',
    () async {
      final initial = _emptyState();
      final api = _FakeStudioApi(initial);
      List<ThreadItemView> pageItems(int base, int count) => List.generate(
        count,
        (index) => _threadItemFixture(
          id: 'history-${base + index}',
          threadId: 'session-1',
          turnId: 'turn-${base + index}',
          ordinal: base + index,
          text: 'message ${base + index}',
        ),
      );
      // 两页共 520 > maxLoadedHistoryItems(500)：第二页加载后最旧一页被驱逐。
      api.historyPagesByThread['session-1'] = {
        // 第一页是较新历史（ordinal 更大），第二页是更旧历史。
        null: ThreadHistoryPage(
          items: pageItems(260, 260),
          nextCursor: 'cursor-2',
        ),
        'cursor-2': ThreadHistoryPage(
          items: pageItems(0, 260),
          nextCursor: 'cursor-3',
        ),
      };
      final container = ProviderContainer(
        overrides: [studioApiProvider.overrideWithValue(api)],
      );
      addTearDown(container.dispose);
      await container.read(studioControllerProvider.future);

      await container
          .read(studioControllerProvider.notifier)
          .loadOlderHistory('session-1');
      await container
          .read(studioControllerProvider.notifier)
          .loadOlderHistory('session-1');

      final state = container.read(studioControllerProvider).requireValue;
      final history = state.selectedWorkspaceUi.history;
      expect(history.loadedItems, 260);
      expect(history.pageSizes.length, 1);
      // load-older cursor 回退到被驱逐页的请求 cursor；向上滚动时按它回源重取。
      expect(history.nextCursor, 'cursor-2');
      expect(history.hasMore, isTrue);
      // 工作区只保留较新一页的 item。
      expect(
        state.selectedWorkspace!.items.map((item) => item.id).first,
        'history-260',
      );
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
  test('resync reload keeps selection when incoming window drops the thread', () async {
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
  });
}
