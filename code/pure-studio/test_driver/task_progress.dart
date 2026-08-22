import 'dart:convert';
import 'dart:io';

class BudgetRecoveryEvidence {
  Map<String, Object?>? _limited;
  Map<String, Object?>? _resumed;

  void observe(Map<String, dynamic> snapshot) {
    final task = snapshot['task'];
    if (task is! Map<String, dynamic>) return;
    final workUnits = task['workUnits'];
    if (workUnits is! List<dynamic>) return;
    for (final unit in workUnits.whereType<Map<String, dynamic>>()) {
      final budget = unit['budgetLimit'];
      if (_limited == null &&
          unit['status'] == 'needsAttention' &&
          unit['executionStatus'] == 'budgetLimited' &&
          unit['continuationState'] == 'needsAttention' &&
          budget is Map<String, dynamic> &&
          budget['kind'] == 'wallClock') {
        _limited = _identity(unit, includeBudget: true);
        continue;
      }
      final limited = _limited;
      if (limited != null &&
          _resumed == null &&
          unit['id'] == limited['workUnitId'] &&
          unit['status'] == 'running' &&
          unit['executionStatus'] == 'running' &&
          unit['executionError'] == null &&
          unit['budgetLimit'] == null &&
          unit['budgetSliceCount'] == 1) {
        _resumed = _identity(unit, includeBudget: false);
      }
    }
  }

  Map<String, Object?> validate() {
    final limited = _limited;
    final resumed = _resumed;
    if (limited == null) {
      throw StateError('budget recovery never exposed NeedsAttention');
    }
    if (resumed == null) {
      throw StateError('budget recovery never resumed at slice one');
    }
    for (final field in ['workUnitId', 'agentId', 'worktreePath', 'branch']) {
      if (limited[field] != resumed[field]) {
        throw StateError(
          'budget recovery changed executor identity field $field',
        );
      }
    }
    return {'limited': limited, 'resumed': resumed};
  }

  Map<String, Object?> _identity(
    Map<String, dynamic> unit, {
    required bool includeBudget,
  }) => {
    'workUnitId': unit['id'],
    'agentId': unit['agentId'],
    'worktreePath': unit['worktreePath'],
    'branch': unit['branch'],
    'status': unit['status'],
    'executionStatus': unit['executionStatus'],
    'executionError': unit['executionError'],
    'budgetLimit': includeBudget ? unit['budgetLimit'] : null,
    'budgetSliceCount': unit['budgetSliceCount'],
    'continuationState': unit['continuationState'],
    'continuationSourceTurnId': unit['continuationSourceTurnId'],
    'continuationRevision': unit['continuationRevision'],
  };
}

String taskProgressFingerprint(Map<String, dynamic> snapshot) {
  final task = snapshot['task'];
  if (task is! Map<String, dynamic>) {
    return 'no-task';
  }
  final workspace = snapshot['workspace'];
  return jsonEncode({
    'runId': task['runId'],
    'phase': task['phase'],
    'statusMessage': task['statusMessage'],
    'taskGeneration': task['taskGeneration'],
    'integratedReviewGate': task['integratedReviewGate'],
    'workUnits': _normalized(
      task['workUnits'],
      (unit) => {
        'id': unit['id'],
        'status': unit['status'],
        'executionStatus': unit['executionStatus'],
        'executionError': unit['executionError'],
        'budgetLimit': unit['budgetLimit'],
        'budgetSliceCount': unit['budgetSliceCount'],
        'budgetSliceLimit': unit['budgetSliceLimit'],
        'continuationState': unit['continuationState'],
        'continuationSourceTurnId': unit['continuationSourceTurnId'],
        'continuationRevision': unit['continuationRevision'],
        'executorProgressRevision': unit['executorProgressRevision'],
        'blueprintFingerprint': unit['blueprintFingerprint'],
        'objective': unit['objective'],
        'implementationStepCount': unit['implementationStepCount'],
        'acceptanceCriterionCount': unit['acceptanceCriterionCount'],
        'verificationCount': unit['verificationCount'],
      },
    ),
    'completions': _normalized(
      task['completions'],
      (completion) => {
        'id': completion['id'],
        'workUnitId': completion['workUnitId'],
        'revision': completion['revision'],
        'status': completion['status'],
        'headCommit': completion['headCommit'],
        'updatedAt': completion['updatedAt'],
      },
    ),
    'merges': _normalized(
      task['merges'],
      (merge) => {
        'id': merge['id'],
        'workUnitId': merge['workUnitId'],
        'completionRevision': merge['completionRevision'],
        'resultingHead': merge['resultingHead'],
        'cleanupStatus': merge['cleanupStatus'],
        'updatedAt': merge['updatedAt'],
      },
    ),
    'reviews': _normalized(
      task['reviews'],
      (review) => {
        'id': review['id'],
        'round': review['round'],
        'scope': review['scope'],
        'workUnitId': review['workUnitId'],
        'completionRevision': review['completionRevision'],
        'reviewedHead': review['reviewedHead'],
        'verdict': review['verdict'],
        'findingCount': review['findingCount'],
        'updatedAt': review['updatedAt'],
      },
    ),
    'workspaceProgress': workspace is Map<String, dynamic>
        ? {
            'threadId': workspace['threadId'],
            'timelineProgress': workspace['timelineProgress'],
            'turn': workspace['turn'] is Map<String, dynamic>
                ? {
                    'id': workspace['turn']['id'],
                    'status': workspace['turn']['status'],
                    'updatedAt': workspace['turn']['updatedAt'],
                  }
                : null,
          }
        : null,
  });
}

void validateTaskCompletion(
  Map<String, dynamic> snapshot, {
  bool Function(String path)? worktreeExists,
}) {
  final task = snapshot['task'] as Map<String, dynamic>?;
  if (task == null || task['phase'] != 'completed') {
    throw StateError('Task snapshot is not completed');
  }
  final workUnits = (task['workUnits'] as List<dynamic>? ?? const [])
      .cast<Map<String, dynamic>>();
  final completions = (task['completions'] as List<dynamic>? ?? const [])
      .cast<Map<String, dynamic>>();
  final merges = (task['merges'] as List<dynamic>? ?? const [])
      .cast<Map<String, dynamic>>();
  final reviews = (task['reviews'] as List<dynamic>? ?? const [])
      .cast<Map<String, dynamic>>();
  if (completions.length < workUnits.length) {
    throw StateError('not every WorkUnit has a completion');
  }
  final gate = task['integratedReviewGate'] as Map<String, dynamic>?;
  if (gate == null) {
    throw StateError('completed task has no integrated review gate');
  }
  final gateStatus = gate['status'];
  final noDelivery = gateStatus == 'notRequiredNoDelivery';
  if (noDelivery ? merges.isNotEmpty : merges.length != workUnits.length) {
    throw StateError('merge records do not match the integrated review gate');
  }
  final pathExists = worktreeExists ?? (path) => Directory(path).existsSync();
  for (final unit in workUnits) {
    final workUnitId = unit['id'];
    final expectedStatus = noDelivery ? 'noDelivery' : 'merged';
    if (unit['status'] != expectedStatus) {
      throw StateError(
        'WorkUnit $workUnitId is not $expectedStatus: ${unit['status']}',
      );
    }
    final worktreePath = unit['worktreePath'] as String?;
    if (worktreePath == null ||
        worktreePath.isEmpty ||
        pathExists(worktreePath)) {
      throw StateError('WorkUnit $workUnitId left a worktree behind');
    }
    if (noDelivery) {
      final unitCompletions = completions
          .where((completion) => completion['workUnitId'] == workUnitId)
          .toList();
      if (unitCompletions.isEmpty ||
          unitCompletions.any(
            (completion) => completion['kind'] != 'noDelivery',
          )) {
        throw StateError('WorkUnit $workUnitId has no noDelivery completion');
      }
      continue;
    }
    final matchingMerges = merges
        .where((merge) => merge['workUnitId'] == workUnitId)
        .toList();
    if (matchingMerges.length != 1) {
      throw StateError('WorkUnit $workUnitId does not have one merge record');
    }
    final merge = matchingMerges.single;
    if (!const {
      'discarded',
      'alreadyAbsent',
    }.contains(merge['cleanupStatus'])) {
      throw StateError(
        'WorkUnit $workUnitId cleanup failed: ${merge['cleanupStatus']}',
      );
    }
    final matchingCompletions = completions
        .where(
          (completion) =>
              completion['id'] == merge['completionId'] &&
              completion['workUnitId'] == workUnitId,
        )
        .toList();
    if (matchingCompletions.length != 1 ||
        matchingCompletions.single['revision'] != merge['completionRevision']) {
      throw StateError(
        'WorkUnit $workUnitId merge does not match its completion revision',
      );
    }
  }
  switch (gateStatus) {
    case 'required':
      throw StateError('completed task still requires integrated review');
    case 'satisfiedByReview':
      final matchingReviews = reviews
          .where(
            (review) =>
                review['id'] == gate['reviewRoundId'] &&
                review['scope'] == 'integrated' &&
                review['verdict'] == 'pass' &&
                review['reviewedHead'] == gate['reviewedHead'],
          )
          .toList();
      if (matchingReviews.length != 1) {
        throw StateError('integrated review gate does not match a pass round');
      }
      break;
    case 'notRequiredNoDelivery':
      if (reviews.any((review) => review['scope'] == 'integrated')) {
        throw StateError('no-delivery task unexpectedly has integrated review');
      }
      break;
    case 'notRequiredSingleExecutorEquivalent':
      if (workUnits.length != 1 || merges.length != 1) {
        throw StateError('single-executor gate has multiple deliveries');
      }
      final merge = merges.single;
      if (merge['workUnitId'] != gate['workUnitId'] ||
          merge['completionRevision'] != gate['completionRevision'] ||
          merge['id'] != gate['mergeRecordId']) {
        throw StateError('single-executor gate does not match its merge');
      }
      if (reviews.any((review) => review['scope'] == 'integrated')) {
        throw StateError(
          'single-executor task unexpectedly has integrated review',
        );
      }
      break;
    default:
      throw StateError('unknown integrated review gate: $gateStatus');
  }
  final workspace = snapshot['workspace'] as Map<String, dynamic>?;
  if (workspace == null ||
      workspace['isBusy'] != false ||
      workspace['activeInteraction'] != null) {
    throw StateError('Task completed with an active turn or interaction');
  }
}

void validateFatalTaskFailure(Map<String, dynamic> snapshot) {
  final task = snapshot['task'] as Map<String, dynamic>?;
  if (task == null || task['phase'] != 'failed') {
    throw StateError('Task snapshot is not failed');
  }
  final failure = task['terminalFailure'] as Map<String, dynamic>?;
  if (failure == null || failure['disposition'] != 'fatal') {
    throw StateError('Task snapshot has no fatal terminal failure');
  }
  if (failure['providerKind'] != 'authentication' ||
      failure['code'] != 'invalid_api_key' ||
      failure['httpStatus'] != 401 ||
      failure['retryable'] != false) {
    throw StateError('Task terminal failure lost typed authentication detail');
  }
  final message = failure['message'] as String? ?? '';
  if (message.isEmpty || message.contains('sk-driver-secret')) {
    throw StateError('Task terminal failure message is empty or not redacted');
  }
}

List<Map<String, Object?>> _normalized(
  Object? value,
  Map<String, Object?> Function(Map<String, dynamic>) select,
) {
  final selected = <Map<String, Object?>>[
    for (final item in value is List<dynamic> ? value : const <dynamic>[])
      if (item is Map<String, dynamic>) select(item),
  ];
  selected.sort((left, right) {
    return '${left['id']}'.compareTo('${right['id']}');
  });
  return selected;
}
