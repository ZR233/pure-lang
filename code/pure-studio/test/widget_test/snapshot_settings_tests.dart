part of '../widget_test.dart';

void registerSnapshotSettingsTests() {
  test('observed snapshots reject equal and older revisions uniformly', () {
    const currentMeta = ObservedStateMeta(
      revision: 2,
      phase: ObservedStatePhase.ready,
      updatedAt: null,
      stale: false,
    );
    const olderMeta = ObservedStateMeta(
      revision: 1,
      phase: ObservedStatePhase.ready,
      updatedAt: null,
      stale: false,
    );
    final current = _emptyState().copyWith(
      settingsState: const SettingsStateSnapshot(
        meta: currentMeta,
        permissionMode: PermissionMode.fullAccess,
      ),
      mcpState: const McpStateSnapshot(
        meta: currentMeta,
        activeServers: ['canonical'],
      ),
    );

    final settings = applySettingsState(
      current,
      const SettingsStateSnapshot(
        meta: currentMeta,
        permissionMode: PermissionMode.requestApproval,
      ),
    );
    final mcp = applyMcpState(
      current,
      const McpStateSnapshot(meta: olderMeta, activeServers: ['stale']),
    );

    expect(settings, same(current));
    expect(mcp, same(current));
  });

  test('settings merge does not replace canonical Thread workspace', () {
    final current = _emptyState();
    final next = const SettingsStateSnapshot(
      meta: ObservedStateMeta(
        revision: 1,
        phase: ObservedStatePhase.ready,
        updatedAt: null,
        stale: false,
      ),
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

    expect(merged.workspacesByThread, same(current.workspacesByThread));
    expect(merged.providers.single.id, 'provider-1');
    expect(merged.permissionMode, PermissionMode.fullAccess);
  });

  test('provider catalog metadata remains attached after a save', () async {
    final api = _FakeStudioApi(_stateWithPlannerModels());
    final container = ProviderContainer(
      overrides: [studioApiProvider.overrideWithValue(api)],
    );
    addTearDown(container.dispose);

    final state = await container.read(studioControllerProvider.future);

    expect(state.providerCatalog.revision, _testProviderCatalog.revision);
    expect(state.providers.single.models, isNotEmpty);
  });

  test('Thread snapshot never overwrites global settings', () {
    final current = _stateWithPlannerModels();
    final snapshot = current.selectedWorkspace!.copyWith(
      revision: 5,
      runtime: current.runtime.copyWith(model: 'runtime/model'),
    );

    final next = applyThreadSnapshot(current, snapshot);

    expect(next.providers, current.providers);
    expect(next.roles, current.roles);
    expect(next.runtime.model, 'runtime/model');
  });
}
