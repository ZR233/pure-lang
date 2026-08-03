part of '../widget_test.dart';

void registerTaskRuntimeDetailTests() {
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
    expect(find.text('task-run-stop-origin'), findsOneWidget);
    expect(find.text('Stop · generation 3'), findsOneWidget);
    expect(find.text('UserRequest: 用户点击停止'), findsOneWidget);
    expect(find.textContaining('PlannerDecision'), findsNothing);
    expect(
      find.byKey(StudioDriverKeys.taskAgent('executor-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskAgentStatus('executor-1')),
      findsOneWidget,
    );
    expect(
      find.byKey(StudioDriverKeys.taskAgentSummaryAge('executor-1')),
      findsOneWidget,
    );
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
}

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
  workUnits: const [],
  agents: [
    TaskAgentOutcomeView(
      agentId: 'executor-1',
      role: 'executor',
      status: 'completed',
      initiatedBy: 'planner',
      requestedByCallId: 'spawn-1',
      summary: null,
      error: null,
      headCommit: 'abcdef123456',
      lifecycle: 'active',
      activity: 'idle',
      progress: AgentProgressView(
        stage: 'readyForReview',
        summary: '实现完成',
        nextStep: '等待审查',
        revision: 2,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true),
      ),
      updatedAt: DateTime.fromMillisecondsSinceEpoch(1000, isUtc: true),
      summaryAgeSeconds: 12,
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
  merges: const [],
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
