import 'package:flutter_test/flutter_test.dart';

import '../test_driver/task_progress.dart';

void main() {
  test('transient workspace activity does not reset task progress', () {
    final thinking = _snapshot(activity: 'thinking');
    final runningTool = _snapshot(activity: 'runningTool');

    expect(
      taskProgressFingerprint(thinking),
      taskProgressFingerprint(runningTool),
    );
  });

  test('durable continuation revision advances task progress', () {
    final before = _snapshot(activity: 'thinking');
    final after = _snapshot(activity: 'thinking');
    final task = after['task'] as Map<String, dynamic>;
    final workUnit =
        (task['workUnits'] as List<dynamic>).single as Map<String, dynamic>;
    workUnit['continuationRevision'] = '2';

    expect(
      taskProgressFingerprint(before),
      isNot(taskProgressFingerprint(after)),
    );
  });

  test('durable executor revision advances progress with a static root', () {
    final before = _snapshot(activity: 'runningTool');
    final after = _snapshot(activity: 'runningTool');
    final task = after['task'] as Map<String, dynamic>;
    final workUnit =
        (task['workUnits'] as List<dynamic>).single as Map<String, dynamic>;
    workUnit['executorProgressRevision'] = '2';

    expect(
      taskProgressFingerprint(before),
      isNot(taskProgressFingerprint(after)),
    );
  });

  test('durable timeline progress prevents a false executor stall', () {
    final before = _snapshot(activity: 'runningTool');
    final after = _snapshot(activity: 'runningTool');
    final workspace = after['workspace'] as Map<String, dynamic>;
    workspace['timelineProgress'] = {
      'rowCount': 12,
      'lastSequence': 3418,
      'renderVersion': 43,
    };

    expect(
      taskProgressFingerprint(before),
      isNot(taskProgressFingerprint(after)),
    );
  });

  test(
    'completed task requires successful cleanup and exact merge revision',
    () {
      final snapshot = _completedSnapshot();

      expect(
        () => validateTaskCompletion(snapshot, worktreeExists: (_) => false),
        returnsNormally,
      );

      final task = snapshot['task'] as Map<String, dynamic>;
      final merge =
          (task['merges'] as List<dynamic>).single as Map<String, dynamic>;
      merge['cleanupStatus'] = 'failed';
      expect(
        () => validateTaskCompletion(snapshot, worktreeExists: (_) => false),
        throwsA(isA<StateError>()),
      );
    },
  );

  test('completed task rejects a residual executor worktree', () {
    expect(
      () => validateTaskCompletion(
        _completedSnapshot(),
        worktreeExists: (_) => true,
      ),
      throwsA(isA<StateError>()),
    );
  });

  test('completed task rejects a mismatched completion revision', () {
    final snapshot = _completedSnapshot();
    final task = snapshot['task'] as Map<String, dynamic>;
    final merge =
        (task['merges'] as List<dynamic>).single as Map<String, dynamic>;
    merge['completionRevision'] = 2;

    expect(
      () => validateTaskCompletion(snapshot, worktreeExists: (_) => false),
      throwsA(isA<StateError>()),
    );
  });

  test('fatal authentication failure keeps typed redacted evidence', () {
    expect(() => validateFatalTaskFailure(_failedSnapshot()), returnsNormally);

    final failure =
        (_failedSnapshot()['task'] as Map<String, dynamic>)['terminalFailure']
            as Map<String, dynamic>;
    failure['message'] = 'Invalid API key sk-driver-secret';
    expect(
      () => validateFatalTaskFailure({
        'task': {'phase': 'failed', 'terminalFailure': failure},
      }),
      throwsA(isA<StateError>()),
    );
  });
}

Map<String, dynamic> _snapshot({required String activity}) => {
  'workspace': {
    'threadId': 'thread-1',
    'turn': {
      'id': 'turn-1',
      'status': 'inProgress',
      'activity': activity,
      'updatedAt': '2026-08-06T00:00:00.000Z',
    },
    'timelineProgress': {'rowCount': 1, 'lastSequence': 1, 'renderVersion': 1},
  },
  'task': {
    'runId': 'task-run-1',
    'phase': 'implementing',
    'statusMessage': null,
    'taskGeneration': 0,
    'expectedHead': 'abc',
    'workUnits': [
      {
        'id': 'work-unit-1',
        'status': 'running',
        'executionStatus': 'running',
        'executionError': null,
        'budgetLimit': null,
        'budgetSliceCount': 1,
        'budgetSliceLimit': 4,
        'continuationState': 'none',
        'continuationSourceTurnId': null,
        'continuationRevision': '1',
        'executorProgressRevision': '1',
      },
    ],
    'completions': <dynamic>[],
    'merges': <dynamic>[],
    'reviews': <dynamic>[],
  },
};

Map<String, dynamic> _completedSnapshot() => {
  'workspace': {'isBusy': false, 'activeInteraction': null},
  'task': {
    'runId': 'task-run-1',
    'phase': 'completed',
    'expectedHead': 'head-2',
    'workUnits': [
      {
        'id': 'work-unit-1',
        'status': 'merged',
        'worktreePath': r'C:\work\.pure\worktrees\run\agent',
      },
    ],
    'completions': [
      {'id': 'completion-1', 'workUnitId': 'work-unit-1', 'revision': 1},
    ],
    'merges': [
      {
        'id': 'merge-1',
        'workUnitId': 'work-unit-1',
        'completionId': 'completion-1',
        'completionRevision': 1,
        'cleanupStatus': 'discarded',
      },
    ],
    'reviews': [
      {
        'id': 'review-1',
        'scope': 'integrated',
        'verdict': 'pass',
        'reviewedHead': 'head-2',
      },
    ],
  },
};

Map<String, dynamic> _failedSnapshot() => {
  'task': {
    'phase': 'failed',
    'terminalFailure': {
      'disposition': 'fatal',
      'providerKind': 'authentication',
      'code': 'invalid_api_key',
      'httpStatus': 401,
      'retryable': false,
      'message': 'Invalid API key provided: <redacted>',
    },
  },
};
