import 'dart:convert';

import '../domain/models/studio_models.dart';

/// Read-only state exported through Flutter Driver in local acceptance builds.
abstract final class StudioDriverState {
  static StudioProject? _project;
  static TaskRuntimeView? _task;
  static AgentWorkspaceView? _workspace;
  static final Map<String, StudioTurnView> _lastTurnsByThread = {};
  static String? _planContent;
  static TaskRecoveryPreview? _taskRecoveryPreview;
  static TaskRecoveryResult? _taskRecoveryResult;
  static final List<StudioShutdownProgress> _shutdownProgress = [];
  static List<String> _sidebarDirectoryIds = const [];
  static bool _sidebarDirectoryHasMore = false;
  static String? _selectedProjectId;
  static String? _selectedThreadId;
  static StudioMode _newThreadMode = StudioMode.simple;
  static ComposerThreadState _newThreadComposer =
      const ComposerThreadState.idle();

  static void publishState(StudioState state) {
    _selectedProjectId = state.selectedProjectId;
    _selectedThreadId = state.selectedThreadId;
    _newThreadMode = state.newThreadMode;
    _newThreadComposer = state.newThreadComposer;
    _project = state.projects
        .where((project) => project.id == state.selectedProjectId)
        .firstOrNull;
    final workspace = state.selectedAgentWorkspace;
    if (workspace == null) {
      _workspace = null;
      _task = null;
    } else {
      publishWorkspace(workspace);
      _task = workspace.runtime.task;
    }
    publishSidebarDirectory([
      for (final thread in state.rootThreads) thread.id,
    ], state.threadDirectory.hasMore);
  }

  static void publishTask(TaskRuntimeView? task) {
    _task = task;
  }

  static void publishProject(StudioProject? project) {
    _project = project;
  }

  static void publishWorkspace(AgentWorkspaceView workspace) {
    _workspace = workspace;
    final turn = workspace.turn;
    if (turn != null) publishTurn(turn);
  }

  static void publishTurn(StudioTurnView turn) {
    _lastTurnsByThread[turn.threadId] = turn;
  }

  static void publishPlan(String content) {
    _planContent = content;
  }

  static void publishSidebarDirectory(List<String> threadIds, bool hasMore) {
    _sidebarDirectoryIds = List.unmodifiable(threadIds);
    _sidebarDirectoryHasMore = hasMore;
  }

  static void publishShutdownProgress(StudioShutdownProgress progress) {
    _shutdownProgress.add(progress);
  }

  static List<StudioShutdownProgress> get shutdownProgress =>
      List.unmodifiable(_shutdownProgress);

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
    final lastTurn = workspace == null
        ? null
        : _lastTurnsByThread[workspace.threadId];
    return jsonEncode({
      'planContent': _planContent,
      'sidebarDirectory': {
        'count': _sidebarDirectoryIds.length,
        'hasMore': _sidebarDirectoryHasMore,
        'ids': _sidebarDirectoryIds,
      },
      'navigation': {
        'selectedProjectId': _selectedProjectId,
        'selectedThreadId': _selectedThreadId,
        'isStartPage': _selectedThreadId == null,
        'newThreadMode': _newThreadMode.name,
        'newThreadComposer': {
          'draft': _newThreadComposer.draft,
          'phase': _newThreadComposer.phase.name,
          'submissionRevision': _newThreadComposer.submissionRevision,
          'error': _newThreadComposer.error,
        },
      },
      'shutdownPhases': <String>[
        for (final progress in _shutdownProgress)
          '${progress.phase.name}:${progress.pendingCommits}',
      ],
      'project': _project == null
          ? null
          : {'id': _project!.id, 'path': _project!.path},
      'workspace': workspace == null
          ? null
          : {
              'threadId': workspace.thread.id,
              'projectId': workspace.thread.projectId,
              'rootThreadId': workspace.rootThread.id,
              'threadMode': workspace.thread.mode.name,
              'threadStatus': workspace.thread.status,
              'isBusy': workspace.isBusy,
              'isTaskPaused': workspace.isTaskPaused,
              'composer': {
                'mode': workspace.composerMode.name,
                'lockedByInteraction': workspace.activeInteraction != null,
              },
              'interactionCount': workspace.activeInteraction == null ? 0 : 1,
              'activeInteraction': workspace.activeInteraction == null
                  ? null
                  : {
                      'id': workspace.activeInteraction!.id,
                      'turnId': workspace.activeInteraction!.turnId,
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
              'lastTurn': lastTurn == null ? null : _turnJson(lastTurn),
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
    'phase': preview.state.name,
    'stopRequested': preview.stopRequested,
    'projectLeaseId': preview.projectLeaseId,
    'recommendedThreadId': preview.recommendedThreadId,
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
          'baseCommit': target.baseCommit,
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
  };

  static Map<String, Object?> _taskJson(TaskRuntimeView task) => {
    'runId': task.runId,
    'phase': task.state.kind.name,
    'statusMessage': task.statusMessage,
    'taskGeneration': task.taskGeneration,
    'integratedReviewGate': _integratedReviewGateJson(
      task.integratedReviewGate,
    ),
    'failures': [
      for (final failure in task.failures) _taskFailureJson(failure),
    ],
    'terminalFailure': task.terminalFailure == null
        ? null
        : _taskFailureJson(task.terminalFailure!),
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
          'blueprintFingerprint': unit.blueprintFingerprint,
          'objective': unit.objective,
          'implementationStepCount': unit.implementationStepCount,
          'acceptanceCriterionCount': unit.acceptanceCriterionCount,
          'verificationCount': unit.verificationCount,
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

  static Map<String, Object?> _integratedReviewGateJson(
    IntegratedReviewGateView gate,
  ) => switch (gate.kind) {
    IntegratedReviewGateKind.required => {
      'status': 'required',
      'reason': gate.reason,
    },
    IntegratedReviewGateKind.satisfiedByReview => {
      'status': 'satisfiedByReview',
      'reviewRoundId': gate.reviewRoundId,
      'reviewedHead': gate.reviewedHead,
    },
    IntegratedReviewGateKind.notRequiredNoDelivery => {
      'status': 'notRequiredNoDelivery',
    },
    IntegratedReviewGateKind.notRequiredSingleExecutorEquivalent => {
      'status': 'notRequiredSingleExecutorEquivalent',
      'workUnitId': gate.workUnitId,
      'completionRevision': gate.completionRevision,
      'mergeRecordId': gate.mergeRecordId,
    },
  };

  static Map<String, Object?> _taskFailureJson(TaskFailureView failure) => {
    'id': failure.id,
    'sourceThreadId': failure.sourceThreadId,
    'sourceTurnId': failure.sourceTurnId,
    'sourceAgentId': failure.sourceAgentId,
    'sourceRole': failure.sourceRole,
    'workUnitId': failure.workUnitId,
    'reviewRoundId': failure.reviewRoundId,
    'disposition': failure.disposition,
    'category': failure.category,
    'providerKind': failure.providerKind,
    'code': failure.code,
    'httpStatus': failure.httpStatus,
    'message': failure.message,
    'retryable': failure.retryable,
    'resolvedAt': failure.resolvedAt?.toUtc().toIso8601String(),
    'createdAt': failure.createdAt.toUtc().toIso8601String(),
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
      // Driver 顺序验收：完整行序列（id/type/text/sequence）。
      'rows': [
        for (final row in rows)
          {
            'id': row.id,
            'type': row.type.name,
            'text': row.part?.text,
            'sequence': row.sequence,
          },
      ],
    };
  }

  static Map<String, Object?> _turnJson(StudioTurnView turn) => {
    'id': turn.turnId,
    'threadId': turn.threadId,
    'status': turn.state.status.name,
    'activity': turn.state.activity?.name,
    'reason': turn.state.reason,
    'updatedAt': turn.updatedAt.toUtc().toIso8601String(),
  };
}
