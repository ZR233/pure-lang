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

  test('completed single-executor task accepts equivalent delivery without integrated review', () {
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
  });

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

  test('completed task rejects a missing final summary Timeline row', () {
    final snapshot = _completedSnapshot();
    final workspace = snapshot['workspace'] as Map<String, dynamic>;
    final timeline = workspace['timelineProgress'] as Map<String, dynamic>;
    final rows = timeline['rows'] as List<dynamic>;
    rows.clear();

    expect(
      () => validateTaskCompletion(snapshot, worktreeExists: (_) => false),
      throwsA(isA<StateError>()),
    );
  });

  test('completed no-delivery task uses the canonical completed state', () {
    expect(
      () => validateTaskCompletion(
        _noDeliveryCompletedSnapshot(),
        worktreeExists: (_) => false,
      ),
      returnsNormally,
    );
  });

  test('completed task accepts a matching integrated review gate', () {
    final snapshot = _completedSnapshot();
    final task = snapshot['task'] as Map<String, dynamic>;
    task['integratedReviewGate'] = {
      'status': 'satisfiedByReview',
      'reviewRoundId': 'review-1',
      'reviewedHead': 'head-2',
    };
    final reviews = task['reviews'] as List<dynamic>;
    reviews.add({
      'id': 'review-1',
      'scope': 'integrated',
      'verdict': 'passed',
      'reviewedHead': 'head-2',
    });

    expect(
      () => validateTaskCompletion(snapshot, worktreeExists: (_) => false),
      returnsNormally,
    );
  });

  test('single-executor exemption rejects an unexpected integrated review', () {
    final snapshot = _completedSnapshot();
    final task = snapshot['task'] as Map<String, dynamic>;
    final reviews = task['reviews'] as List<dynamic>;
    reviews.add({
      'id': 'review-1',
      'scope': 'integrated',
      'verdict': 'passed',
      'reviewedHead': 'head-2',
    });

    expect(
      () => validateTaskCompletion(snapshot, worktreeExists: (_) => false),
      throwsA(isA<StateError>()),
    );
  });

  test('fatal authentication failure keeps typed redacted evidence', () {
    expect(() => validateFatalTaskFailure(_failedSnapshot()), returnsNormally);

    final invalid = _failedSnapshot();
    final task = invalid['task'] as Map<String, dynamic>;
    final failure =
        (task['issues'] as List<dynamic>).single as Map<String, dynamic>;
    failure['message'] = 'Invalid API key sk-driver-secret';
    expect(() => validateFatalTaskFailure(invalid), throwsA(isA<StateError>()));
  });

  test(
    'budget recovery preserves executor identity and returns to slice one',
    () {
      final evidence = BudgetRecoveryEvidence();
      evidence.observe(_budgetRecoverySnapshot(needsAttention: true));
      evidence.observe(_budgetRecoverySnapshot(needsAttention: false));

      final result = evidence.validate();

      expect(result['limited'], containsPair('budgetSliceCount', 1));
      expect(result['resumed'], containsPair('budgetSliceCount', 1));
      expect(
        (result['limited'] as Map<String, Object?>)['workUnitId'],
        (result['resumed'] as Map<String, Object?>)['workUnitId'],
      );
    },
  );

  test('budget recovery rejects a replacement executor identity', () {
    final evidence = BudgetRecoveryEvidence();
    evidence.observe(_budgetRecoverySnapshot(needsAttention: true));
    final resumed = _budgetRecoverySnapshot(needsAttention: false);
    final task = resumed['task'] as Map<String, dynamic>;
    final unit = (task['workUnits'] as List<dynamic>).single;
    (unit as Map<String, dynamic>)['agentId'] = 'replacement-agent';
    evidence.observe(resumed);

    expect(evidence.validate, throwsA(isA<StateError>()));
  });
}

Map<String, dynamic> _budgetRecoverySnapshot({required bool needsAttention}) =>
    {
      'task': {
        'phase': 'working',
        'workUnits': [
          {
            'id': 'work-unit-1',
            'agentId': 'executor-1',
            'worktreePath': r'C:\work\executor-1',
            'branch': 'pure-task-executor-1',
            'status': needsAttention ? 'needsAttention' : 'running',
            'executionStatus': needsAttention ? 'budgetLimited' : 'running',
            'executionError': needsAttention ? 'rollover timed out' : null,
            'budgetLimit': needsAttention
                ? {
                    'kind': 'wallClock',
                    'usage': {
                      'modelSteps': 0,
                      'toolCalls': 0,
                      'waitCalls': 0,
                      'elapsedMs': '0',
                    },
                  }
                : null,
            'budgetSliceCount': 1,
            'continuationState': needsAttention ? 'needsAttention' : 'none',
            'continuationSourceTurnId': needsAttention ? 'turn-budget' : null,
            'continuationRevision': needsAttention ? '2' : '3',
          },
        ],
      },
    };

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
    'phase': 'working',
    'statusMessage': null,
    'taskGeneration': 0,
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
  'workspace': {
    'isBusy': false,
    'activeInteraction': null,
    'timelineProgress': {
      'rows': [
        {'type': 'finalAnswer', 'text': 'Completed the fixture Task.'},
      ],
    },
  },
  'task': {
    'runId': 'task-run-1',
    'phase': 'completed',
    'outcome': {'kind': 'succeeded', 'summary': 'Completed the fixture Task.'},
    'integratedReviewGate': {
      'status': 'notRequiredSingleExecutorEquivalent',
      'workUnitId': 'work-unit-1',
      'completionRevision': 1,
      'mergeRecordId': 'merge-1',
    },
    'workUnits': [
      {
        'id': 'work-unit-1',
        'status': 'completed',
        'worktreePath': r'C:\work\.pure\worktrees\run\agent',
      },
    ],
    'completions': [
      {
        'id': 'completion-1',
        'workUnitId': 'work-unit-1',
        'revision': 1,
        'kind': 'delivery',
        'status': 'approved',
        'headCommit': 'head-1',
      },
    ],
    'merges': [
      {
        'id': 'merge-1',
        'workUnitId': 'work-unit-1',
        'completionId': 'completion-1',
        'completionRevision': 1,
        'deliveryHead': 'head-1',
        'cleanupStatus': 'discarded',
      },
    ],
    'reviews': [
      {
        'id': 'review-delivery-1',
        'scope': 'delivery',
        'workUnitId': 'work-unit-1',
        'completionId': 'completion-1',
        'completionRevision': 1,
        'reviewedHead': 'head-1',
        'verdict': 'passed',
      },
    ],
  },
};

Map<String, dynamic> _noDeliveryCompletedSnapshot() => {
  'workspace': {
    'isBusy': false,
    'activeInteraction': null,
    'timelineProgress': {
      'rows': [
        {'type': 'finalAnswer', 'text': 'No delivery was required.'},
      ],
    },
  },
  'task': {
    'runId': 'task-run-1',
    'phase': 'completed',
    'outcome': {'kind': 'succeeded', 'summary': 'No delivery was required.'},
    'integratedReviewGate': {'status': 'notRequiredNoDelivery'},
    'workUnits': [
      {
        'id': 'work-unit-1',
        'status': 'completed',
        'worktreePath': r'C:\work\.pure\worktrees\run\agent',
      },
    ],
    'completions': [
      {
        'id': 'completion-1',
        'workUnitId': 'work-unit-1',
        'revision': 1,
        'kind': 'noDelivery',
        'status': 'approved',
      },
    ],
    'merges': <dynamic>[],
    'reviews': [
      {
        'id': 'review-delivery-1',
        'scope': 'delivery',
        'workUnitId': 'work-unit-1',
        'completionId': 'completion-1',
        'completionRevision': 1,
        'verdict': 'passed',
      },
    ],
  },
};

Map<String, dynamic> _failedSnapshot() => {
  'task': {
    'phase': 'completed',
    'outcome': {
      'kind': 'failed',
      'failureKind': 'fatal',
      'summary': 'Provider authentication failed.',
    },
    'issues': [
      {
        'disposition': 'fatal',
        'providerKind': 'authentication',
        'code': 'invalid_api_key',
        'httpStatus': 401,
        'retryable': false,
        'message': 'Invalid API key provided: <redacted>',
      },
    ],
  },
};
