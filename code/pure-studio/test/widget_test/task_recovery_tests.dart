part of '../widget_test.dart';

void registerTaskRecoveryTests() {
  test('idle active Task is paused only for a settled executor failure', () {
    final base = _emptyState();
    final root = base.selectedThread!.copyWith(
      mode: StudioMode.task,
      status: 'idle',
      rootThreadId: 'session-1',
    );

    AgentWorkspaceView workspaceFor(TaskWorkUnitView workUnit) {
      final task = TaskRuntimeView(
        runId: 'run-46',
        phase: 'implementing',
        branch: 'main',
        expectedHead: 'head',
        statusMessage: null,
        stopRequestedOrigin: null,
        stopRequestedReason: null,
        taskGeneration: 1,
        workUnits: [workUnit],
        completions: const [],
        merges: const [],
        reviews: const [],
      );
      return AgentWorkspaceView(
        thread: root,
        rootThread: root,
        syncState: AgentWorkspaceSyncState.ready,
        timelineRows: const [],
        todo: null,
        runtime: _testRuntime().copyWith(task: task),
        turn: null,
        activeInteraction: null,
        composer: const ComposerThreadState.idle(),
        composerMode: AgentComposerMode.editable,
        permissionMode: PermissionMode.requestApproval,
        providers: const [],
        roles: const [],
        agents: const [],
      );
    }

    expect(
      workspaceFor(
        _taskRecoveryWorkUnit(
          status: 'awaitingCompletion',
          execution: 'failed',
        ),
      ).isTaskPaused,
      isTrue,
    );
    expect(
      workspaceFor(
        _taskRecoveryWorkUnit(status: 'running', execution: 'running'),
      ).isTaskPaused,
      isFalse,
    );
  });

  testWidgets(
    'paused Task previews recovery and applies one selected Turn exactly once',
    (tester) async {
      final preview = _taskRecoveryPreviewFixture();
      final base = _emptyState();
      final root = base.selectedThread!.copyWith(
        mode: StudioMode.task,
        status: 'interrupted',
        rootThreadId: 'session-1',
      );
      final task = TaskRuntimeView(
        runId: preview.runId,
        phase: preview.phase,
        branch: 'codex/task-46',
        expectedHead: preview.expectedHead,
        statusMessage: 'paused',
        stopRequestedOrigin: null,
        stopRequestedReason: null,
        taskGeneration: preview.taskGeneration,
        workUnits: const [],
        completions: const [],
        merges: const [],
        reviews: const [],
      );
      final runtime = _testRuntime().copyWith(task: task);
      final workspace = AgentWorkspaceView(
        thread: root,
        rootThread: root,
        syncState: AgentWorkspaceSyncState.ready,
        timelineRows: const [],
        todo: null,
        runtime: runtime,
        turn: null,
        activeInteraction: null,
        composer: const ComposerThreadState.idle(),
        composerMode: AgentComposerMode.editable,
        permissionMode: PermissionMode.requestApproval,
        providers: const [],
        roles: const [],
        agents: const [],
      );
      final initial = base.copyWith(
        threads: [root],
        workspacesByThread: {
          root.id: base.selectedWorkspace!.copyWith(
            thread: root,
            runtime: runtime,
          ),
        },
      );
      final api = _FakeStudioApi(initial)
        ..taskRecoveryPreview = preview
        ..taskRecoveryResult = TaskRecoveryResult(
          recoveryId: 'server-recovery-id',
          runId: preview.runId,
          workUnitId: 'wu-1',
          rootThreadId: root.id,
          targetThreadId: 'executor-1',
          mode: ConversationRecoveryMode.rebuildThread,
          recoveryRevision: 2,
          runtimeRevision: 8,
          threadRevision: 12,
          beforeTranscriptHash: 'before',
          afterTranscriptHash: 'after',
          removedItemCount: 3,
          removedInputCount: 1,
          stopCleared: true,
          resumeTurnId: 'resume-turn',
          gitFingerprint: preview.targets.first.gitFingerprint,
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

      expect(find.byKey(StudioDriverKeys.taskPaused), findsOneWidget);
      await tester.tap(find.byKey(StudioDriverKeys.taskResume));
      await tester.pumpAndSettle();

      expect(find.byKey(StudioDriverKeys.taskRecoveryDialog), findsOneWidget);
      expect(find.text('shell_command (exit 1)'), findsOneWidget);
      expect(find.textContaining('codex/task-46'), findsWidgets);

      await tester.tap(find.byKey(StudioDriverKeys.taskRecoveryTarget));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Planner').last);
      await tester.pumpAndSettle();
      expect(find.text('局部重建 Thread 上下文'), findsOneWidget);
      expect(find.textContaining('只清空这个 Thread'), findsOneWidget);

      await tester.tap(find.byKey(StudioDriverKeys.taskRecoveryTarget));
      await tester.pumpAndSettle();
      await tester.tap(find.textContaining('Executor').last);
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(StudioDriverKeys.taskRecoveryTailCount));
      await tester.pumpAndSettle();
      await tester.tap(find.text('1').last);
      await tester.pumpAndSettle();
      expect(
        find.byKey(StudioDriverKeys.taskRecoveryTurn('turn-3')),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.taskRecoveryTurn('turn-2')),
        findsNothing,
      );

      await tester.tap(find.byKey(StudioDriverKeys.taskRecoveryMode));
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(StudioDriverKeys.taskRecoveryModeOption('rebuildThread')),
      );
      await tester.pumpAndSettle();
      expect(find.textContaining('只清空这个 Thread'), findsOneWidget);

      await tester.tap(find.byKey(StudioDriverKeys.taskRecoveryConfirm));
      await tester.pump();
      expect(api.taskRecoveryRequest, isNull);
      expect(find.byKey(StudioDriverKeys.taskRecoveryApply), findsOneWidget);

      await tester.tap(find.byKey(StudioDriverKeys.taskRecoveryApply));
      await tester.pumpAndSettle();

      expect(api.taskRecoveryRequest, isNotNull);
      expect(api.taskRecoveryRequest!.targetThreadId, 'executor-1');
      expect(api.taskRecoveryRequest!.turnIds, ['turn-3']);
      expect(
        api.taskRecoveryRequest!.mode,
        ConversationRecoveryMode.rebuildThread,
      );
      expect(api.submittedPrompts, isEmpty);
      expect(find.byKey(StudioDriverKeys.taskRecoveryDialog), findsNothing);
    },
  );

  testWidgets(
    'rolled back Timeline rows remain visible with weak audit label',
    (tester) async {
      final item = ThreadItemView(
        id: 'rolled-item',
        threadId: 'session-1',
        turnId: 'rolled-turn',
        ordinal: 1,
        revision: 1,
        status: 'failed',
        createdAt: _fixtureDate(1),
        updatedAt: _fixtureDate(1),
        kind: ThreadItemKind.agentMessage,
        text: 'failed historical answer',
        channel: AgentMessageChannel.finalAnswer,
        contextDisposition: ThreadContextDisposition.rolledBack,
      );

      await tester.pumpWidget(
        _timelineHarness(threadId: 'session-1', items: [item]),
      );
      await tester.pumpAndSettle();

      expect(find.text('failed historical answer'), findsOneWidget);
      expect(
        find.byKey(StudioDriverKeys.timelineRolledBack('rolled-item')),
        findsOneWidget,
      );
      expect(find.text('Rolled back from active context'), findsOneWidget);
      expect(
        tester
            .widgetList<Opacity>(find.byType(Opacity))
            .any((widget) => widget.opacity == 0.52),
        isTrue,
      );
    },
  );
}

TaskWorkUnitView _taskRecoveryWorkUnit({
  required String status,
  required String execution,
}) => TaskWorkUnitView(
  id: 'wu-1',
  title: 'executor',
  status: status,
  worktreePath: r'C:\workspace-wu-1',
  branch: 'task-wu-1',
  agentId: 'executor-1',
  executionStatus: execution,
  executionError: execution == 'failed' ? 'provider failed' : null,
  budgetLimit: null,
  budgetSliceCount: 1,
  budgetSliceLimit: 4,
  continuationState: 'none',
  continuationSourceTurnId: null,
  continuationRevision: BigInt.zero,
  executorProgressRevision: BigInt.zero,
);

TaskRecoveryPreview _taskRecoveryPreviewFixture() {
  const mainFingerprint = TaskGitFingerprint(
    workspaceRoot: r'C:\workspace',
    gitCommonDir: r'C:\workspace\.git',
    branch: 'codex/task-46',
    head: '0123456789abcdef',
    baseCommit: 'base',
    expectedHead: '0123456789abcdef',
    operation: 'none',
    indexDiffHash: 'index',
    workingTreeDiffHash: 'working-tree',
    untrackedContentHash: 'untracked',
  );
  return TaskRecoveryPreview(
    previewToken: 'preview-token',
    rootThreadId: 'session-1',
    runId: 'run-46',
    taskGeneration: 3,
    phase: 'implementing',
    expectedHead: '0123456789abcdef',
    stopRequested: true,
    branchLeaseId: 'lease-1',
    branchLeaseBranch: 'codex/task-46',
    branchLeaseGitCommonDir: r'C:\workspace\.git',
    branchLeaseExpectedHead: '0123456789abcdef',
    recommendedThreadId: 'executor-1',
    targets: [
      TaskRecoveryTarget(
        threadId: 'executor-1',
        kind: TaskRecoveryTargetKind.executor,
        workUnitId: 'wu-1',
        attempt: 1,
        continuationRevision: 4,
        expectedRuntimeRevision: 7,
        expectedThreadRevision: 9,
        branch: 'codex/task-46-wu-1',
        worktreePath: r'C:\workspace-wu-1',
        turns: [
          TaskRecoveryTurn(
            turnId: 'turn-1',
            status: 'completed',
            updatedAt: _fixtureDate(1),
            itemCount: 2,
            inputCount: 1,
            toolCount: 0,
            toolSummaries: const [],
          ),
          TaskRecoveryTurn(
            turnId: 'turn-2',
            status: 'failed',
            updatedAt: _fixtureDate(2),
            itemCount: 4,
            inputCount: 1,
            toolCount: 1,
            toolSummaries: const ['shell_command (exit 1)'],
          ),
          TaskRecoveryTurn(
            turnId: 'turn-3',
            status: 'interrupted',
            updatedAt: _fixtureDate(3),
            itemCount: 3,
            inputCount: 1,
            toolCount: 1,
            toolSummaries: const ['apply_patch'],
          ),
        ],
        defaultTurnIds: const ['turn-2', 'turn-3'],
        availableModes: const [
          ConversationRecoveryMode.rewindTail,
          ConversationRecoveryMode.rebuildThread,
        ],
        gitFingerprint: mainFingerprint,
      ),
      TaskRecoveryTarget(
        threadId: 'session-1',
        kind: TaskRecoveryTargetKind.planner,
        expectedRuntimeRevision: 11,
        expectedThreadRevision: 13,
        branch: 'codex/task-46',
        worktreePath: r'C:\workspace',
        turns: [
          TaskRecoveryTurn(
            turnId: 'planner-turn',
            status: 'interrupted',
            updatedAt: _fixtureDate(4),
            itemCount: 2,
            inputCount: 1,
            toolCount: 0,
            toolSummaries: const [],
          ),
        ],
        defaultTurnIds: const ['planner-turn'],
        availableModes: const [ConversationRecoveryMode.rebuildThread],
        gitFingerprint: mainFingerprint,
      ),
    ],
    mainGitFingerprint: mainFingerprint,
    completionRevisionFingerprint: 'completions',
    reviewRevisionFingerprint: 'reviews',
    mergeRevisionFingerprint: 'merges',
  );
}
