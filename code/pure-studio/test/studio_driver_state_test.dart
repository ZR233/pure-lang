import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:pure_studio/src/domain/models/studio_models.dart';
import 'package:pure_studio/src/shared/studio_driver_state.dart';

void main() {
  test('snapshot publishes the canonical settings revision', () {
    StudioDriverState.publishState(_studioState(settingsRevision: 37));

    final snapshot =
        jsonDecode(StudioDriverState.snapshotJson()) as Map<String, dynamic>;

    expect(snapshot['settings'], {'revision': 37});
    expect(snapshot['persistence'], containsPair('revision', 0));
  });
}

StudioState _studioState({required int settingsRevision}) => StudioState(
  projectDirectory: ProjectDirectoryState(),
  threadDirectory: const ThreadDirectoryWindow(),
  agentDirectory: AgentDirectoryState(),
  settingsState: SettingsStateSnapshot(revision: settingsRevision),
  recoveryState: RecoveryStateSnapshot(),
  mcpState: McpStateSnapshot(),
  lspState: LspStateSnapshot(),
  skillsByProject: const {},
  providerUsageState: ProviderUsageStateSnapshot(),
  updaterState: UpdaterStateSnapshot.idle(
    revision: 0,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  ),
  selectedProjectId: null,
  selectedThreadId: null,
);
