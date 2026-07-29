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
  agents: const [],
  merges: const [],
  reviews: const [],
);
