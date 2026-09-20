import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:anywork/src/domain/models/studio_models.dart';
import 'package:anywork/src/shared/studio_driver_state.dart';

void main() {
  test('snapshot publishes the canonical settings revision', () {
    StudioDriverState.publishState(
      _studioState(
        settingsRevision: 37,
        roles: const [
          RoleSettingsView(
            key: 'executor',
            providerId: 'openai',
            model: 'gpt-5',
            effort: 'high',
          ),
        ],
      ),
    );

    final snapshot =
        jsonDecode(StudioDriverState.snapshotJson()) as Map<String, dynamic>;

    expect(snapshot['settings'], {
      'revision': 37,
      'providers': [],
      'modeModelRoutes': [],
      'roles': [
        {
          'key': 'executor',
          'providerId': 'openai',
          'model': 'gpt-5',
          'effort': 'high',
        },
      ],
    });
    expect(snapshot['persistence'], containsPair('revision', 0));
  });

  test('snapshot publishes the workspace-mode draft and per-thread modes', () {
    StudioDriverState.publishState(
      _studioState(
        settingsRevision: 1,
        projectId: 'project-1',
        threads: [
          StudioThread(
            id: 'session-worktree',
            projectId: 'project-1',
            title: 'Worktree session',
            mode: ThreadModeId.simple,
            updatedAt: DateTime.fromMillisecondsSinceEpoch(1),
            workspaceMode: ThreadWorkspaceMode.worktree,
            workspacePath: 'worktrees/session-worktree',
          ),
          StudioThread(
            id: 'session-local',
            projectId: 'project-1',
            title: 'Local session',
            mode: ThreadModeId.simple,
            updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
            workspacePath: '.',
          ),
        ],
        workspaceModeDraft: ThreadWorkspaceMode.worktree,
      ),
    );

    final snapshot =
        jsonDecode(StudioDriverState.snapshotJson()) as Map<String, dynamic>;

    final navigation = snapshot['navigation'] as Map<String, dynamic>;
    expect(navigation['newThreadWorkspaceMode'], 'worktree');
    final directory = snapshot['sidebarDirectory'] as Map<String, dynamic>;
    expect(directory['workspaceModes'], {
      'session-worktree': 'worktree',
      'session-local': 'local',
    });
    expect(directory['workspacePaths'], {
      'session-worktree': 'worktrees/session-worktree',
      'session-local': '.',
    });
  });
}

StudioState _studioState({
  required int settingsRevision,
  List<RoleSettingsView> roles = const [],
  String? projectId,
  List<StudioThread> threads = const [],
  ThreadWorkspaceMode workspaceModeDraft = ThreadWorkspaceMode.local,
}) => StudioState(
  projectDirectory: ProjectDirectoryState(),
  threadDirectory: ThreadDirectoryWindow(threads: threads),
  agentDirectory: AgentDirectoryState(),
  settingsState: SettingsStateSnapshot(
    revision: settingsRevision,
    roles: roles,
  ),
  newThreadWorkspaceModeByProject: projectId == null
      ? const {}
      : {projectId: workspaceModeDraft},
  recoveryState: RecoveryStateSnapshot(),
  mcpState: McpStateSnapshot(),
  lspState: LspStateSnapshot(),
  skillsByProject: const {},
  providerUsageState: ProviderUsageStateSnapshot(),
  updaterState: UpdaterStateSnapshot.idle(
    revision: 0,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  ),
  selectedProjectId: projectId,
  selectedThreadId: null,
);
