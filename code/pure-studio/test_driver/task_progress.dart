import 'dart:convert';
import 'dart:io';

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
    'expectedHead': task['expectedHead'],
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
  if (workUnits.isEmpty || completions.length < workUnits.length) {
    throw StateError('not every WorkUnit has a completion');
  }
  if (merges.length != workUnits.length) {
    throw StateError('merge records are incomplete or duplicated');
  }
  final pathExists = worktreeExists ?? (path) => Directory(path).existsSync();
  for (final unit in workUnits) {
    final workUnitId = unit['id'];
    if (unit['status'] != 'merged') {
      throw StateError('WorkUnit $workUnitId is not merged: ${unit['status']}');
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
    final worktreePath = unit['worktreePath'] as String?;
    if (worktreePath == null ||
        worktreePath.isEmpty ||
        pathExists(worktreePath)) {
      throw StateError('WorkUnit $workUnitId left a worktree behind');
    }
  }
  if (!reviews.any(
    (review) =>
        review['scope'] == 'integrated' &&
        review['verdict'] == 'pass' &&
        review['reviewedHead'] == task['expectedHead'],
  )) {
    throw StateError('integrated pass review does not match expected HEAD');
  }
  final workspace = snapshot['workspace'] as Map<String, dynamic>?;
  if (workspace == null ||
      workspace['isBusy'] != false ||
      workspace['activeInteraction'] != null) {
    throw StateError('Task completed with an active turn or interaction');
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
