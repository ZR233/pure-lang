import 'dart:convert';

import '../domain/models/studio_models.dart';

/// Read-only state exported through Flutter Driver in local acceptance builds.
abstract final class StudioDriverState {
  static TaskRuntimeView? _task;
  static AgentWorkspaceView? _workspace;
  static String? _planContent;
  static TaskRecoveryPreview? _taskRecoveryPreview;
  static TaskRecoveryResult? _taskRecoveryResult;

  static void publishTask(TaskRuntimeView? task) {
    _task = task;
  }

  static void publishWorkspace(AgentWorkspaceView workspace) {
    _workspace = workspace;
  }

  static void publishPlan(String content) {
    _planContent = content;
  }

  static void clearTaskRecovery() {
    _taskRecoveryPreview = null;
    _taskRecoveryResult = null;
  }

  static void publishTaskRecoveryPreview(TaskRecoveryPreview preview) {
    _taskRecoveryPreview = preview;
  }

  static void publishTaskRecoveryResult(TaskRecoveryResult result) {
    _taskRecoveryResult = result;
  }

  static String snapshotJson() {
    final workspace = _workspace;
    return jsonEncode({
      'planContent': _planContent,
      'workspace': workspace == null
          ? null
          : {
              'threadId': workspace.thread.id,
              'rootThreadId': workspace.rootThread.id,
              'threadMode': workspace.thread.mode.name,
              'threadStatus': workspace.thread.status,
              'isBusy': workspace.isBusy,
              'isTaskPaused': workspace.isTaskPaused,
              'activeInteraction': workspace.activeInteraction == null
                  ? null
                  : {
                      'id': workspace.activeInteraction!.id,
                      'kind': workspace.activeInteraction!.kind.name,
                    },
              'turn': workspace.turn == null
                  ? null
                  : {
                      'id': workspace.turn!.turnId,
                      'status': workspace.turn!.state.status.name,
                      'activity': workspace.turn!.state.activity?.name,
                      'reason': workspace.turn!.state.reason,
                      'updatedAt': workspace.turn!.updatedAt
                          .toUtc()
                          .toIso8601String(),
                    },
              'timelineProgress': _timelineProgress(workspace),
            },
      'task': _task == null ? null : _taskJson(_task!),
      'taskRecovery': {
        'preview': _taskRecoveryPreview == null
            ? null
            : _taskRecoveryPreviewJson(_taskRecoveryPreview!),
        'result': _taskRecoveryResult == null
            ? null
            : _taskRecoveryResultJson(_taskRecoveryResult!),
      },
    });
  }

  static Map<String, Object?> _taskRecoveryPreviewJson(
    TaskRecoveryPreview preview,
  ) => {
    'previewToken': preview.previewToken,
    'runId': preview.runId,
    'taskGeneration': preview.taskGeneration,
    'phase': preview.phase,
    'expectedHead': preview.expectedHead,
    'stopRequested': preview.stopRequested,
    'branchLeaseId': preview.branchLeaseId,
    'branchLeaseBranch': preview.branchLeaseBranch,
    'branchLeaseGitCommonDir': preview.branchLeaseGitCommonDir,
    'branchLeaseExpectedHead': preview.branchLeaseExpectedHead,
    'recommendedThreadId': preview.recommendedThreadId,
    'mainGitFingerprint': _gitFingerprintJson(preview.mainGitFingerprint),
    'targets': [
      for (final target in preview.targets)
        {
          'threadId': target.threadId,
          'kind': target.kind.name,
          'workUnitId': target.workUnitId,
          'attempt': target.attempt,
          'continuationRevision': target.continuationRevision,
          'expectedRuntimeRevision': target.expectedRuntimeRevision,
          'expectedThreadRevision': target.expectedThreadRevision,
          'branch': target.branch,
          'worktreePath': target.worktreePath,
          'defaultTurnIds': target.defaultTurnIds,
          'availableModes': target.availableModes
              .map((mode) => mode.name)
              .toList(),
          'turns': [
            for (final turn in target.turns)
              {
                'turnId': turn.turnId,
                'status': turn.status,
                'itemCount': turn.itemCount,
                'inputCount': turn.inputCount,
                'toolCount': turn.toolCount,
                'toolSummaries': turn.toolSummaries,
              },
          ],
          'gitFingerprint': _gitFingerprintJson(target.gitFingerprint),
        },
    ],
  };

  static Map<String, Object?> _taskRecoveryResultJson(
    TaskRecoveryResult result,
  ) => {
    'recoveryId': result.recoveryId,
    'runId': result.runId,
    'workUnitId': result.workUnitId,
    'rootThreadId': result.rootThreadId,
    'targetThreadId': result.targetThreadId,
    'mode': result.mode.name,
    'recoveryRevision': result.recoveryRevision,
    'runtimeRevision': result.runtimeRevision,
    'threadRevision': result.threadRevision,
    'beforeTranscriptHash': result.beforeTranscriptHash,
    'afterTranscriptHash': result.afterTranscriptHash,
    'removedItemCount': result.removedItemCount,
    'removedInputCount': result.removedInputCount,
    'stopCleared': result.stopCleared,
    'resumeTurnId': result.resumeTurnId,
    'gitFingerprint': _gitFingerprintJson(result.gitFingerprint),
  };

  static Map<String, Object?> _gitFingerprintJson(
    TaskGitFingerprint fingerprint,
  ) => {
    'workspaceRoot': fingerprint.workspaceRoot,
    'gitCommonDir': fingerprint.gitCommonDir,
    'branch': fingerprint.branch,
    'head': fingerprint.head,
    'baseCommit': fingerprint.baseCommit,
    'expectedHead': fingerprint.expectedHead,
    'operation': fingerprint.operation,
    'indexDiffHash': fingerprint.indexDiffHash,
    'workingTreeDiffHash': fingerprint.workingTreeDiffHash,
    'untrackedContentHash': fingerprint.untrackedContentHash,
  };

  static Map<String, Object?> _taskJson(TaskRuntimeView task) => {
    'runId': task.runId,
    'phase': task.phase,
    'branch': task.branch,
    'expectedHead': task.expectedHead,
    'statusMessage': task.statusMessage,
    'taskGeneration': task.taskGeneration,
    'workUnits': [
      for (final unit in task.workUnits)
        {
          'id': unit.id,
          'title': unit.title,
          'status': unit.status,
          'worktreePath': unit.worktreePath,
          'branch': unit.branch,
          'agentId': unit.agentId,
          'executionStatus': unit.executionStatus,
          'executionError': unit.executionError,
          'budgetLimit': unit.budgetLimit == null
              ? null
              : {
                  'kind': unit.budgetLimit!.kind,
                  'usage': {
                    'modelSteps': unit.budgetLimit!.usage.modelSteps,
                    'toolCalls': unit.budgetLimit!.usage.toolCalls,
                    'waitCalls': unit.budgetLimit!.usage.waitCalls,
                    'elapsedMs': unit.budgetLimit!.usage.elapsedMs.toString(),
                  },
                },
          'budgetSliceCount': unit.budgetSliceCount,
          'budgetSliceLimit': unit.budgetSliceLimit,
          'continuationState': unit.continuationState,
          'continuationSourceTurnId': unit.continuationSourceTurnId,
          'continuationRevision': unit.continuationRevision.toString(),
          'executorProgressRevision': unit.executorProgressRevision.toString(),
        },
    ],
    'completions': [
      for (final completion in task.completions)
        {
          'id': completion.id,
          'workUnitId': completion.workUnitId,
          'executorAgentId': completion.executorAgentId,
          'revision': completion.revision,
          'kind': completion.kind,
          'status': completion.status,
          'baseCommit': completion.baseCommit,
          'headCommit': completion.headCommit,
          'verificationSummary': completion.verificationSummary,
          'updatedAt': completion.updatedAt.toUtc().toIso8601String(),
        },
    ],
    'merges': [
      for (final merge in task.merges)
        {
          'id': merge.id,
          'workUnitId': merge.workUnitId,
          'completionId': merge.completionId,
          'completionRevision': merge.completionRevision,
          'executorAgentId': merge.executorAgentId,
          'expectedPreviousHead': merge.expectedPreviousHead,
          'deliveryHead': merge.deliveryHead,
          'resultingHead': merge.resultingHead,
          'method': merge.method,
          'cleanupStatus': merge.cleanupStatus,
          'cleanupDetail': merge.cleanupDetail,
          'updatedAt': merge.updatedAt.toUtc().toIso8601String(),
        },
    ],
    'reviews': [
      for (final review in task.reviews)
        {
          'id': review.id,
          'round': review.round,
          'scope': review.scope,
          'workUnitId': review.workUnitId,
          'completionId': review.completionId,
          'completionRevision': review.completionRevision,
          'reviewedHead': review.reviewedHead,
          'verdict': review.verdict,
          'reviewerAgentId': review.reviewerAgentId,
          'summary': review.summary,
          'findingCount': review.findings.length,
          'updatedAt': review.updatedAt.toUtc().toIso8601String(),
        },
    ],
  };

  static Map<String, Object?> _timelineProgress(AgentWorkspaceView workspace) {
    final rows = workspace.timelineRows;
    return {
      'rowCount': rows.length,
      'lastSequence': rows.fold<int>(
        0,
        (latest, row) => row.sequence > latest ? row.sequence : latest,
      ),
      'renderVersion': Object.hashAll([
        for (final row in rows) row.id,
        for (final row in rows) row.sequence,
        for (final row in rows) row.renderVersion,
      ]),
    };
  }
}
