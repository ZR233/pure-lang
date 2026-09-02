import 'dart:convert';

import '../domain/models/studio_models.dart';

/// Read-only state exported through Flutter Driver in local acceptance builds.
abstract final class StudioDriverState {
  static StudioProject? _project;
  static AgentWorkspaceView? _workspace;
  static final Map<String, StudioTurnView> _lastTurnsByThread = {};
  static final List<StudioShutdownProgress> _shutdownProgress = [];
  static List<String> _sidebarDirectoryIds = const [];
  static List<StudioThread> _currentRootThreads = const [];
  static bool _sidebarDirectoryHasMore = false;
  static String? _selectedProjectId;
  static String? _selectedThreadId;
  static StudioMode _newThreadMode = StudioMode.simple;
  static ComposerThreadState _newThreadComposer =
      const ComposerThreadState.idle();
  static int _settingsRevision = 0;
  static List<RoleSettingsView> _roles = const [];
  static PersistenceStateSnapshot _persistenceState =
      const PersistenceStateSnapshot.ready();

  static void publishState(StudioState state) {
    _selectedProjectId = state.selectedProjectId;
    _selectedThreadId = state.selectedThreadId;
    _currentRootThreads = List.unmodifiable(state.rootThreads);
    _newThreadMode = state.newThreadMode;
    _newThreadComposer = state.newThreadComposer;
    _settingsRevision = state.settingsRevision;
    _roles = List.unmodifiable([
      for (final role in state.roles)
        RoleSettingsView(
          key: role.key,
          providerId: role.providerId,
          model: role.model,
          effort: role.effort,
        ),
    ]);
    _persistenceState = state.persistenceState;
    _project = state.projects
        .where((project) => project.id == state.selectedProjectId)
        .firstOrNull;
    final workspace = state.selectedAgentWorkspace;
    if (workspace == null) {
      _workspace = null;
    } else {
      publishWorkspace(workspace);
    }
    publishSidebarDirectory([
      for (final thread in state.rootThreads) thread.id,
    ], state.threadDirectory.hasMore);
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

  static void publishSidebarDirectory(List<String> threadIds, bool hasMore) {
    _sidebarDirectoryIds = List.unmodifiable(threadIds);
    _sidebarDirectoryHasMore = hasMore;
  }

  static void publishShutdownProgress(StudioShutdownProgress progress) {
    _shutdownProgress.add(progress);
  }

  static List<StudioShutdownProgress> get shutdownProgress =>
      List.unmodifiable(_shutdownProgress);

  static String snapshotJson() {
    final workspace = _workspace;
    final lastTurn = workspace == null
        ? null
        : _lastTurnsByThread[workspace.threadId];
    return jsonEncode({
      'sidebarDirectory': {
        'count': _sidebarDirectoryIds.length,
        'hasMore': _sidebarDirectoryHasMore,
        'ids': _sidebarDirectoryIds,
        'titles': {
          for (final thread in _currentRootThreads) thread.id: thread.title,
        },
      },
      'navigation': {
        'selectedProjectId': _selectedProjectId,
        'selectedThreadId': _selectedThreadId,
        'isStartPage': _selectedThreadId == null,
        'newThreadMode': _newThreadMode.name,
        'newThreadComposer': {
          'draft': _newThreadComposer.draft,
          'phase': switch (_newThreadComposer) {
            IdleComposerThreadState() => 'idle',
            SubmittingComposerThreadState() => 'submitting',
            PendingStartComposerThreadState() => 'pendingStart',
            FailedComposerThreadState() => 'failed',
          },
          'submissionRevision': _newThreadComposer.submissionRevision,
          'error': _newThreadComposer.error,
          'attachments': [
            for (final attachment in _newThreadComposer.attachments)
              {
                'id': attachment.id,
                'modality': attachment.modality.name,
                'filename': attachment.filename,
                'byteSize': attachment.byteSize,
              },
          ],
        },
      },
      'shutdownPhases': <String>[
        for (final progress in _shutdownProgress)
          '${progress.phase.name}:${switch (progress) {
            FlushingPersistenceProgress(:final pendingCommits) => pendingCommits,
            StoppingSubscriptionsProgress() || CancellingTurnsProgress() || StoppingAgentsProgress() || StoppingMcpProgress() || StoppingLspProgress() || StoppedProgress() => 0,
          }}',
      ],
      'settings': {
        'revision': _settingsRevision,
        'roles': [
          for (final role in _roles)
            {
              'key': role.key,
              'providerId': role.providerId,
              'model': role.model,
              'effort': role.effort,
            },
        ],
      },
      'persistence': {
        'revision': _persistenceState.revision,
        'kind': switch (_persistenceState.state) {
          ReadyPersistenceState() => 'ready',
          FlushingPersistenceState() => 'flushing',
          DegradedPersistenceState() => 'degraded',
          RecoveringPersistenceState() => 'recovering',
          BlockedPersistenceState() => 'blocked',
        },
        'pendingCommits': _persistenceState.state.pendingCommits,
        'oldestPendingRevision': _persistenceState.state.oldestPendingRevision,
        'firstFailedAt': _persistenceState.state.firstFailedAt,
        'acceptsNewWork': _persistenceState.acceptsNewWork,
      },
      'project': _project == null
          ? null
          : {'id': _project!.id, 'path': _project!.path},
      'workspace': workspace == null
          ? null
          : {
              'threadId': workspace.thread.id,
              'title': workspace.rootThread.title,
              'projectId': workspace.thread.projectId,
              'rootThreadId': workspace.rootThread.id,
              'threadMode': workspace.thread.mode.name,
              'threadStatus': workspace.thread.status.name,
              'isBusy': workspace.isBusy,
              'model': workspace.runtime.model,
              'modelCapabilities': _modelCapabilities(workspace),
              'modelProvider': _modelProvider(workspace),
              'composer': {
                'mode': workspace.composerMode.name,
                'lockedByInteraction': workspace.activeInteraction != null,
                'attachments': [
                  for (final attachment in workspace.composer.attachments)
                    {
                      'id': attachment.id,
                      'modality': _attachmentModalityName(attachment.modality),
                      'filename': attachment.filename,
                      'byteSize': attachment.byteSize,
                    },
                ],
              },
              'agents': [
                for (final agent in workspace.agents)
                  {
                    'id': agent.id,
                    'threadId': agent.threadId,
                    'rootThreadId': agent.rootThreadId,
                    'path': agent.path,
                    'parentPath': agent.parentPath,
                    'role': agent.role,
                    'task': agent.task,
                    'status': agent.status,
                    'error': agent.error,
                  },
              ],
              'historyAttachments': [
                for (final row in workspace.timelineRows)
                  for (final attachment in row.part?.attachments ?? const [])
                    {
                      'id': attachment.id,
                      'modality': _attachmentModalityName(attachment.modality),
                      'filename': attachment.filename,
                      'byteSize': attachment.byteSize,
                    },
                for (final row in workspace.timelineRows)
                  for (final item in row.toolGroup?.items ?? const [])
                    for (final attachment in item.tool?.attachments ?? const [])
                      {
                        'id': attachment.id,
                        'modality': _attachmentModalityName(
                          attachment.modality,
                        ),
                        'filename': attachment.filename,
                        'byteSize': attachment.byteSize,
                        'source': 'tool',
                      },
              ],
              'timeline': [
                for (final row in workspace.timelineRows)
                  {
                    'id': row.id,
                    'type': row.type.name,
                    'text': row.part?.text,
                    'tools': [
                      for (final item in row.toolGroup?.items ?? const [])
                        if (item.tool case final tool?)
                          {
                            'name': tool.name,
                            'callId': tool.callId,
                            'status': item.status,
                            'arguments': tool.arguments,
                            'result': tool.result,
                            'denialReason': tool.denialReason,
                            'workingDirectory': tool.workingDirectory,
                            'exitCode': tool.exitCode,
                            'attachments': [
                              for (final attachment in tool.attachments)
                                {
                                  'id': attachment.id,
                                  'modality': _attachmentModalityName(
                                    attachment.modality,
                                  ),
                                  'filename': attachment.filename,
                                  'byteSize': attachment.byteSize,
                                },
                            ],
                          },
                    ],
                    'attachments': [
                      for (final attachment
                          in row.part?.attachments ?? const [])
                        {
                          'id': attachment.id,
                          'modality': _attachmentModalityName(
                            attachment.modality,
                          ),
                          'filename': attachment.filename,
                          'byteSize': attachment.byteSize,
                        },
                    ],
                  },
              ],
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
      // Workflow is the only lifecycle projection exposed to acceptance
      // drivers. Product-specific orchestration payloads are intentionally not
      // serialized.
      'workflow': workspace?.runtime.workflow == null
          ? null
          : _workflowJson(workspace!.runtime.workflow!),
    });
  }

  static Map<String, Object?> _workflowJson(WorkflowRuntimeView workflow) {
    final run = workflow.currentRun;
    return {
      'revision': workflow.revision,
      'currentRun': run == null
          ? null
          : {
              'lineageId': run.lineageId,
              'runId': run.runId,
              'title': run.title,
              'goal': run.goal,
              'definitionHash': run.definitionHash,
              'modeId': run.modeId,
              'modeDisplayName': run.modeDisplayName,
              'currentStageId': run.currentStageId,
              'terminal': run.terminal,
              'stages': [
                for (final stage in run.stages)
                  {
                    'id': stage.id,
                    'title': stage.title,
                    'terminal': stage.terminal,
                  },
              ],
              'transitions': [
                for (final transition in run.transitions)
                  {
                    'fromStageId': transition.fromStageId,
                    'toStageId': transition.toStageId,
                    'when': transition.when,
                  },
              ],
              'history': [
                for (final entry in run.history)
                  {
                    'revision': entry.revision,
                    'fromStageId': entry.fromStageId,
                    'toStageId': entry.toStageId,
                    'summary': entry.summary,
                    'evidence': entry.evidence,
                    'turnId': entry.turnId,
                    'callId': entry.callId,
                    'transitionedAt': entry.transitionedAt
                        .toUtc()
                        .toIso8601String(),
                  },
              ],
            },
    };
  }

  static List<String> _modelCapabilities(AgentWorkspaceView workspace) {
    for (final provider in workspace.providers) {
      for (final model in provider.allModels) {
        if (model.slug == workspace.runtime.model) {
          return [
            for (final capability in model.inputCapabilities)
              capability.modality.name,
          ];
        }
      }
    }
    return const [];
  }

  static Map<String, Object?>? _modelProvider(AgentWorkspaceView workspace) {
    for (final provider in workspace.providers) {
      if (provider.allModels.any(
        (model) => model.slug == workspace.runtime.model,
      )) {
        return {
          'id': provider.id,
          'hasBearerToken': provider.hasBearerToken,
          'credentialEnv': provider.credentialEnv,
        };
      }
    }
    return null;
  }

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

  static String _attachmentModalityName(AttachmentModalityView modality) =>
      switch (modality) {
        AttachmentModalityView.image => 'image',
        AttachmentModalityView.video => 'video',
        AttachmentModalityView.file => 'file',
      };
}
