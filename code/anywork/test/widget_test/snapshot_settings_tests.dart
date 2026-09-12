part of '../widget_test.dart';

void registerSnapshotSettingsTests() {
  test('observed snapshots reject equal and older revisions uniformly', () {
    final current = _emptyState().copyWith(
      settingsState: SettingsStateSnapshot(
        revision: 2,
        permissionMode: PermissionMode.fullAccess,
      ),
      mcpState: McpStateSnapshot(revision: 2, activeServers: ['canonical']),
    );

    final settings = applySettingsState(
      current,
      SettingsStateSnapshot(
        revision: 2,
        permissionMode: PermissionMode.requestApproval,
      ),
    );
    final mcp = applyMcpState(
      current,
      McpStateSnapshot(revision: 1, activeServers: ['stale']),
    );

    expect(settings.settingsRevision, 2);
    expect(settings.permissionMode, PermissionMode.fullAccess);
    expect(mcp.mcpState.activeServers, ['canonical']);
  });

  test('settings merge does not replace canonical Thread workspace', () {
    final current = _emptyState();
    final next = SettingsStateSnapshot(
      revision: 2,
      providers: [
        ProviderSettingsView(
          id: 'provider-1',
          name: 'Provider',
          baseUrl: '',
          defaultModel: 'model-1',
          models: [],
          status: 'ready',
          usageLabel: '',
        ),
      ],
      permissionMode: PermissionMode.fullAccess,
    );

    final merged = applySettingsState(current, next);

    expect(merged.selectedThreadId, current.selectedThreadId);
    expect(merged.selectedWorkspace?.items, current.selectedWorkspace?.items);
    expect(merged.providers.single.id, 'provider-1');
    expect(merged.permissionMode, PermissionMode.fullAccess);
  });

  test('Thread snapshot never overwrites global settings', () {
    final current = _stateWithPlannerModels();
    final snapshot = current.selectedWorkspace!.copyWith(
      revision: 5,
      runtime: current.runtime.copyWith(model: 'runtime/model'),
    );

    final next = applyThreadSnapshot(current, snapshot);

    expect(next.settingsRevision, current.settingsRevision);
    expect(next.permissionMode, current.permissionMode);
    expect(next.roles, current.roles);
    expect(next.runtime.model, 'runtime/model');
  });
}
