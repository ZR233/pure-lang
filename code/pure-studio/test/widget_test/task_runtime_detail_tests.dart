part of '../widget_test.dart';

void registerTaskRuntimeDetailTests() {
  testWidgets('fatal and recoverable task failures have distinct UI states', (
    tester,
  ) async {
    final fatal = _taskFailure(
      id: 'fatal-1',
      disposition: 'fatal',
      message: 'Invalid API key',
    );
    final recoverable = _taskFailure(
      id: 'recoverable-1',
      disposition: 'recoverable',
      message: 'Provider capacity unavailable',
    );

    await tester.pumpWidget(
      ProviderScope(
        child: _localizedApp(
          home: Scaffold(
            body: Column(
              children: [
                ThreadStatusBar(
                  view: StatusBarView(
                    thread: StudioThread(
                      id: 'session-fatal',
                      projectId: 'project-1',
                      title: 'Fatal session',
                      mode: StudioMode.task,
                      updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
                    ),
                    runtime: _testRuntime().copyWith(
                      task: _failureTask(fatal, phase: 'failed'),
                    ),
                    permissionMode: PermissionMode.requestApproval,
                    providers: const [],
                    roles: const [],
                    isBusy: false,
                  ),
                ),
                TaskRuntimeDetail(task: _failureTask(fatal, phase: 'failed')),
                TaskRuntimeDetail(
                  task: _failureTask(recoverable, phase: 'reviewing'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.byIcon(Icons.error_outline), findsOneWidget);
    expect(find.text('Task failed'), findsWidgets);
    expect(find.text('Invalid API key'), findsWidgets);
    expect(find.text('Provider capacity unavailable'), findsWidgets);
    expect(find.text('Can continue'), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.taskFailure('fatal-1')), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.taskFailure('recoverable-1')),
      findsOneWidget,
    );
  });

  testWidgets('task stop detail renders the durable origin and generation', (
    tester,
  ) async {
    await tester.pumpWidget(
      _localizedApp(
        home: Scaffold(
          body: TaskRuntimeDetail(
            task: _stoppedTask(
              origin: 'userRequest',
              reason: '用户点击停止',
              generation: 3,
            ),
          ),
        ),
      ),
    );

    expect(find.text('Task ID'), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.taskRuntime('task-run-stop-origin')),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.taskPhase('task-run-stop-origin', 'stopping'),
      ),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskStatus('task-run-stop-origin', '正在停止')),
      findsOneWidget,
    );
    expect(find.text('task-run-stop-origin'), findsOneWidget);
    expect(find.text('Stop · generation 3'), findsOneWidget);
    expect(find.text('UserRequest: 用户点击停止'), findsOneWidget);
    expect(find.textContaining('PlannerDecision'), findsNothing);
    expect(find.text('executor-1'), findsWidgets);
    expect(find.text('abcdef1234'), findsWidgets);
    expect(find.byKey(StudioDriverKeys.taskWorkUnit('unit-1')), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.taskWorkUnitExecution('unit-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskWorkUnitBudgetSlice('unit-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskWorkUnitContinuation('unit-1')),
      findsOneWidget,
    );
    expect(find.text('4/4'), findsOneWidget);
    expect(find.textContaining('Wall-clock limit'), findsOneWidget);
    expect(find.text('Needs attention'), findsWidgets);
    expect(find.text('rollover compaction failed'), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.taskCompletion('completion-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskCompletionExecutor('completion-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskCompletionStatus('completion-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskCompletionRevision('completion-1', 2)),
      findsOneWidget,
    );
    expect(find.text('MERGE RECORDS'), findsOneWidget);
    expect(find.text('merge'), findsOneWidget);
    expect(find.text('merge summary'), findsOneWidget);
    expect(find.text('alreadyAbsent'), findsOneWidget);
    expect(find.text('1111111111'), findsOneWidget);
    expect(find.text('2222222222'), findsOneWidget);
    expect(find.text('3333333333'), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.taskReview('review-1')), findsOneWidget);
    expect(
      find.byKey(StudioDriverKeys.taskReviewReviewer('review-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskReviewVerdict('review-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskFinding('review-1', 0)),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskFindingSeverity('review-1', 0)),
      findsOneWidget,
    );

    await tester.pumpWidget(
      _localizedApp(
        home: Scaffold(
          body: TaskRuntimeDetail(
            task: _stoppedTask(
              origin: 'plannerDecision',
              reason: '计划无法继续',
              generation: 4,
            ),
          ),
        ),
      ),
    );
    await tester.pump();

    expect(find.text('Stop · generation 4'), findsOneWidget);
    expect(find.text('PlannerDecision: 计划无法继续'), findsOneWidget);
    expect(find.textContaining('UserRequest'), findsNothing);
  });

  testWidgets('production status bar exposes durable task driver keys', (
    tester,
  ) async {
    final task = _stoppedTask(
      origin: 'runtimeFailure',
      reason: 'continuation failed',
      generation: 5,
    );
    final thread = StudioThread(
      id: 'session-1',
      projectId: 'project-1',
      title: 'Session',
      mode: StudioMode.task,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
    );

    await tester.pumpWidget(
      ProviderScope(
        child: _localizedApp(
          home: Scaffold(
            body: ThreadStatusBar(
              view: StatusBarView(
                thread: thread,
                runtime: _testRuntime().copyWith(task: task),
                permissionMode: PermissionMode.requestApproval,
                providers: const [],
                roles: const [],
                isBusy: true,
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(StudioDriverKeys.taskRuntime(task.runId)),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskPhase(task.runId, task.phase)),
      findsOneWidget,
    );
    expect(
      find.byKey(
        StudioDriverKeys.taskStatus(task.runId, task.statusMessage ?? ''),
      ),
      findsOneWidget,
    );
    final snapshot =
        jsonDecode(StudioDriverState.snapshotJson()) as Map<String, dynamic>;
    final taskSnapshot = snapshot['task'] as Map<String, dynamic>;
    expect(taskSnapshot['runId'], task.runId);
    expect(taskSnapshot['phase'], task.phase);
    expect(taskSnapshot['statusMessage'], task.statusMessage);
    expect(taskSnapshot['workUnits'], hasLength(1));
    final workUnitSnapshot =
        (taskSnapshot['workUnits'] as List<dynamic>).single
            as Map<String, dynamic>;
    expect(workUnitSnapshot['budgetSliceCount'], 4);
    expect(workUnitSnapshot['budgetSliceLimit'], 4);
    expect(workUnitSnapshot['continuationRevision'], '7');
    expect(workUnitSnapshot['budgetLimit'], isNotNull);
    expect(taskSnapshot['completions'], hasLength(1));
    expect(taskSnapshot['merges'], hasLength(1));
    expect(taskSnapshot['reviews'], hasLength(1));
  });
}

TaskFailureView _taskFailure({
  required String id,
  required String disposition,
  required String message,
}) => TaskFailureView(
  id: id,
  sourceThreadId: 'executor-thread-1',
  sourceTurnId: 'turn-1',
  sourceAgentId: 'executor-1',
  sourceRole: 'executor',
  workUnitId: 'work-unit-1',
  reviewRoundId: null,
  disposition: disposition,
  category: 'provider',
  providerKind: disposition == 'fatal' ? 'authentication' : 'capacity',
  code: disposition == 'fatal' ? 'invalid_api_key' : 'server_is_overloaded',
  httpStatus: disposition == 'fatal' ? 401 : 503,
  message: message,
  retryable: disposition != 'fatal',
  resolvedAt: null,
  createdAt: DateTime.fromMillisecondsSinceEpoch(0),
);

TaskRuntimeView _failureTask(
  TaskFailureView failure, {
  required String phase,
}) => TaskRuntimeView(
  runId: 'task-${failure.id}',
  phase: phase,
  branch: 'main',
  expectedHead: '0123456789abcdef',
  statusMessage: failure.message,
  stopRequestedOrigin: null,
  stopRequestedReason: null,
  taskGeneration: 1,
  failures: [failure],
  terminalFailure: failure.isFatal ? failure : null,
  workUnits: const [],
  completions: const [],
  merges: const [],
  reviews: const [],
);

TaskRuntimeView _stoppedTask({
  required String origin,
  required String reason,
  required int generation,
}) => TaskRuntimeView(
  runId: 'task-run-stop-origin',
  phase: 'stopping',
  branch: 'main',
  expectedHead: '0123456789abcdef',
  statusMessage: '正在停止',
  stopRequestedOrigin: origin,
  stopRequestedReason: reason,
  taskGeneration: generation,
  workUnits: [
    TaskWorkUnitView(
      id: 'unit-1',
      title: '实现任务',
      status: 'needsAttention',
      worktreePath: '.pure/worktrees/unit-1',
      branch: 'pure-task-unit-1',
      agentId: 'executor-1',
      executionStatus: 'budgetLimited',
      executionError: 'rollover compaction failed',
      budgetLimit: TaskBudgetLimitView(
        kind: 'wallClock',
        usage: TaskBudgetUsageView(
          modelSteps: 12,
          toolCalls: 34,
          waitCalls: 2,
          elapsedMs: BigInt.from(1800000),
        ),
      ),
      budgetSliceCount: 4,
      budgetSliceLimit: 4,
      continuationState: 'needsAttention',
      continuationSourceTurnId: 'turn-4',
      continuationRevision: BigInt.from(7),
      executorProgressRevision: BigInt.from(42),
    ),
  ],
  completions: [
    TaskCompletionView(
      id: 'completion-1',
      workUnitId: 'unit-1',
      executorAgentId: 'executor-1',
      revision: 2,
      kind: 'delivery',
      status: 'readyForReview',
      baseCommit: '0123456789abcdef',
      headCommit: 'abcdef123456',
      changedFiles: const ['lib/app.dart'],
      verificationSummary: 'flutter test passed',
      worktreePath: '.pure/worktrees/unit-1',
      branch: 'pure-task-unit-1',
      createdAt: DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true),
      updatedAt: DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true),
    ),
  ],
  merges: [
    TaskMergeView(
      id: 'merge-1',
      workUnitId: 'unit-1',
      completionId: 'completion-1',
      completionRevision: 2,
      executorAgentId: 'executor-1',
      expectedPreviousHead: '1111111111abcdef',
      resultingHead: '3333333333abcdef',
      deliveryHead: '2222222222abcdef',
      method: 'merge',
      summary: 'merge summary',
      cleanupStatus: 'alreadyAbsent',
      cleanupDetail: null,
      createdAt: DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true),
      updatedAt: DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true),
    ),
  ],
  reviews: [
    TaskReviewView(
      id: 'review-1',
      round: 1,
      scope: 'delivery',
      workUnitId: 'unit-1',
      completionId: 'completion-1',
      completionRevision: 2,
      reviewedHead: 'abcdef123456',
      verdict: 'changesRequired',
      requestedByCallId: 'review-1',
      reviewerAgentId: 'reviewer-1',
      summary: '需要修复',
      designReferences: const [],
      findings: const [
        TaskReviewFindingView(
          severity: 'major',
          title: '缺少状态投影',
          body: '补齐 completion revision。',
          path: 'lib/app.dart',
          line: 42,
          designReferences: [],
        ),
      ],
      createdAt: DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true),
      updatedAt: DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true),
    ),
  ],
);
