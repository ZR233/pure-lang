import 'dart:async';

import 'package:anywork_client/studio_client.dart';
import 'package:test/test.dart';

const _online = ProviderModelView(
  slug: 'online-model',
  displayName: 'Online',
  reasoningEfforts: [],
  origin: ProviderModelOrigin.online,
);
const _bundled = ProviderModelView(
  slug: 'bundled-model',
  displayName: 'Bundled',
  reasoningEfforts: [],
  origin: ProviderModelOrigin.bundled,
);
const _manual = ProviderModelView(
  slug: 'manual-model',
  displayName: 'Manual',
  reasoningEfforts: [],
  origin: ProviderModelOrigin.manual,
);

ProviderSettingsView _provider({
  List<ProviderModelView> models = const [_online, _bundled, _manual],
  Map<String, String> connections = const {'manual-model': 'http'},
  Map<String, ProviderModelAutoCompactView>? limits,
}) {
  return ProviderSettingsView(
    id: 'fixture',
    name: 'Fixture',
    baseUrl: 'http://fixture.invalid',
    defaultModel: 'manual-model',
    status: 'ready',
    usageLabel: '',
    models: models,
    modelConnectionModes: connections,
    autoCompactLimits:
        limits ??
        {
          'manual-model': ProviderModelAutoCompactView.forModel(_manual)
              .withOverride(111),
          'online-model': ProviderModelAutoCompactView.forModel(_online),
        },
  );
}

SettingsStateSnapshot _snapshot(
  int revision,
  ProviderSettingsView provider, {
  int modelCatalogRevision = 0,
}) => SettingsStateSnapshot(
  revision: revision,
  modelCatalogRevision: modelCatalogRevision,
  providers: [provider],
);

class _RemoveCall {
  _RemoveCall(this.revision, this.providerId, this.slug);
  final int revision;
  final String providerId;
  final String slug;
  final reply = Completer<SettingsCommandReply>();
}

class _Transport implements StudioTransport {
  final calls = StreamController<_RemoveCall>();
  SettingsStateSnapshot? refreshed;
  int refreshes = 0;

  @override
  Stream<ProductTopicFrame> subscribeProductTopic(ProductTopic topic) =>
      const Stream.empty();

  @override
  Future<SettingsStateSnapshot> readSettingsState() async {
    refreshes++;
    return refreshed!;
  }

  @override
  Future<SettingsCommandReply> removeProviderModel(
    int revision,
    String providerId,
    String slug,
  ) {
    final call = _RemoveCall(revision, providerId, slug);
    calls.add(call);
    return call.reply.future;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnsupportedError('Unused: ${invocation.memberName}');
}

AnyworkClient _client(_Transport transport, ProviderSettingsView provider) {
  final client = AnyworkClient(transport: transport, clientId: 'origin-client');
  client.applyBootstrap(
    StudioBootstrap(
      ownerEpoch: 'origin-owner',
      projectDirectory: ProjectDirectoryState(values: const [], revision: 1),
      threadDirectory: ThreadDirectoryWindow(threads: const [], revision: 1),
      agentDirectory: AgentDirectoryState(revision: 1),
      settingsState: _snapshot(7, provider, modelCatalogRevision: 1),
      recoveryState: RecoveryStateSnapshot(revision: 1),
      mcpState: McpStateSnapshot(revision: 1),
      lspState: LspStateSnapshot(revision: 1),
      skillsByProject: const {},
      threadModeCatalog: const ThreadModeCatalogView(),
      providerUsageState: ProviderUsageStateSnapshot(revision: 1),
      modelPerformance: const ModelPerformanceSnapshotView(),
      updaterState: DisabledUpdaterStateSnapshot(
        revision: 1,
        updatedAt: DateTime.utc(2026),
      ),
      persistenceState: const PersistenceStateSnapshot.ready(),
    ),
  );
  addTearDown(client.close);
  addTearDown(transport.calls.close);
  return client;
}

void main() {
  test(
    'providerWithCatalogMetadata projects origin groups without slug diffs',
    () {
      final joined = providerWithCatalogMetadata(
        _provider(),
        const ProviderCatalogView.empty(),
      );
      expect(joined.onlineModels.map((model) => model.slug), ['online-model']);
      expect(joined.bundledModels.map((model) => model.slug), [
        'bundled-model',
      ]);
      expect(joined.customModels.map((model) => model.slug), ['manual-model']);
      expect(joined.models.map((model) => model.origin), [
        ProviderModelOrigin.online,
        ProviderModelOrigin.bundled,
        ProviderModelOrigin.manual,
      ]);
    },
  );

  test(
    'removeProviderModel uses canonical revision and merges returned snapshot',
    () async {
      final transport = _Transport();
      final client = _client(transport, _provider());
      final pending = client.settingsCommands.removeProviderModel(
        'fixture',
        'manual-model',
      );
      final call = await transport.calls.stream.first;
      expect(call.revision, 7);
      expect(call.providerId, 'fixture');
      expect(call.slug, 'manual-model');
      final remaining = _provider(
        models: const [_online, _bundled],
        connections: const {},
        limits: {
          'online-model': ProviderModelAutoCompactView.forModel(_online),
        },
      );
      call.reply.complete(
        SettingsCommandReply(
          snapshot: _snapshot(8, remaining, modelCatalogRevision: 2),
          receipt: StudioCommandReceipt(
            operationId: client.mutations
                .latest('settings:provider-model:7:fixture:manual-model')!
                .id,
            disposition: CommitDisposition.applied,
            targets: [
              ResourceStamp(
                resourceKey: 'settings',
                ownerEpoch: 'origin-owner',
                revision: 8,
              ),
            ],
            effect: EffectPoint.immediate,
            effective: true,
            durable: true,
          ),
        ),
      );
      final snapshot = await pending;
      expect(snapshot.revision, 8);
      final provider = client.settings.state.canonical!.providers.single;
      expect(provider.models.map((model) => model.slug), [
        'online-model',
        'bundled-model',
      ]);
      expect(provider.customModels, isEmpty);
      expect(
        provider.modelConnectionModes.containsKey('manual-model'),
        isFalse,
      );
      expect(provider.autoCompactLimits.containsKey('manual-model'), isFalse);
    },
  );
}
