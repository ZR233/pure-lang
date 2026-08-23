part of '../widget_test.dart';

void registerTaskRecoveryTests() {
  testWidgets(
    'explicit conversation recovery applies one selected Turn exactly once',
    (tester) async {
      final preview = _taskRecoveryPreviewFixture();
      final base = _emptyState();
      final api = _FakeStudioApi(base)
        ..taskRecoveryPreview = preview
        ..taskRecoveryResult = TaskRecoveryResult(
          recoveryId: 'server-recovery-id',
          runId: preview.runId,
          workUnitId: 'wu-1',
          rootThreadId: 'session-1',
          targetThreadId: 'executor-1',
          mode: ConversationRecoveryMode.rebuildThread,
          recoveryRevision: 2,
          runtimeRevision: 8,
          threadRevision: 12,
          beforeTranscriptHash: 'before',
          afterTranscriptHash: 'after',
          removedItemCount: 3,
          removedInputCount: 1,
          resumeTurnId: 'resume-turn',
        );

      await tester.pumpWidget(
        ProviderScope(
          overrides: [studioApiProvider.overrideWithValue(api)],
          child: _localizedApp(
            locale: const Locale('zh'),
            home: Builder(
              builder: (context) => Scaffold(
                body: FilledButton(
                  onPressed: () =>
                      unawaited(showTaskRecoveryDialog(context, 'session-1')),
                  child: const Text('recover'),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.text('recover'));
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
      final item = _threadItemFixture(
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

TaskRecoveryPreview _taskRecoveryPreviewFixture() {
  return TaskRecoveryPreview(
    previewToken: 'preview-token',
    rootThreadId: 'session-1',
    runId: 'run-46',
    revision: 8,
    taskGeneration: 3,
    state: TaskStateKind.working,
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
        baseCommit: '0123456789abcdef',
        turns: [
          TaskRecoveryTurn(
            turnId: 'turn-1',
            state: TaskRecoveryTurnState.completed,
            updatedAt: _fixtureDate(1),
            itemCount: 2,
            inputCount: 1,
            toolCount: 0,
            toolSummaries: const [],
          ),
          TaskRecoveryTurn(
            turnId: 'turn-2',
            state: TaskRecoveryTurnState.failed,
            updatedAt: _fixtureDate(2),
            itemCount: 4,
            inputCount: 1,
            toolCount: 1,
            toolSummaries: const ['shell_command (exit 1)'],
          ),
          TaskRecoveryTurn(
            turnId: 'turn-3',
            state: TaskRecoveryTurnState.cancelled,
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
      ),
      TaskRecoveryTarget(
        threadId: 'session-1',
        kind: TaskRecoveryTargetKind.planner,
        expectedRuntimeRevision: 11,
        expectedThreadRevision: 13,
        branch: 'codex/task-46',
        worktreePath: r'C:\workspace',
        baseCommit: null,
        turns: [
          TaskRecoveryTurn(
            turnId: 'planner-turn',
            state: TaskRecoveryTurnState.cancelled,
            updatedAt: _fixtureDate(4),
            itemCount: 2,
            inputCount: 1,
            toolCount: 0,
            toolSummaries: const [],
          ),
        ],
        defaultTurnIds: const ['planner-turn'],
        availableModes: const [ConversationRecoveryMode.rebuildThread],
      ),
    ],
    completionRevisionFingerprint: 'completions',
    reviewRevisionFingerprint: 'reviews',
    mergeRevisionFingerprint: 'merges',
  );
}
