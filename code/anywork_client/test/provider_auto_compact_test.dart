import 'dart:async';

import 'package:anywork_client/studio_client.dart';
import 'package:test/test.dart';

const _alpha = ProviderModelView(
  slug: 'alpha',
  displayName: 'Alpha',
  reasoningEfforts: [],
  contextWindow: 1000,
  maxContextWindow: 4000,
  autoCompactTokenLimit: 200,
);
const _beta = ProviderModelView(
  slug: 'beta',
  displayName: 'Beta',
  reasoningEfforts: [],
  maxContextWindow: 2000,
);
const _orphan = ProviderModelAutoCompactView(
  slug: 'temporarily-unlisted',
  defaultLimit: null,
  overrideLimit: 777,
);
final _provider = ProviderSettingsView(
  id: 'fixture',
  name: 'Fixture',
  baseUrl: 'http://fixture.invalid',
  defaultModel: 'alpha',
  status: 'ready',
  usageLabel: '',
  models: const [_alpha, _beta],
  autoCompactLimits: {
    'alpha': ProviderModelAutoCompactView.forModel(_alpha),
    'beta': ProviderModelAutoCompactView.forModel(_beta).withOverride(250),
    _orphan.slug: _orphan,
  },
);

SettingsStateSnapshot _snapshot(int revision, ProviderSettingsView provider) =>
    SettingsStateSnapshot(revision: revision, providers: [provider]);

class _SaveCall {
  _SaveCall(this.revision, this.command, this.id);
  final int revision;
  final ProviderCommand command;
  final OperationId id;
  final reply = Completer<SettingsCommandReply>();

  void complete(int revision, ProviderSettingsView provider) {
    reply.complete(
      SettingsCommandReply(
        snapshot: _snapshot(revision, provider),
        receipt: StudioCommandReceipt(
          operationId: id,
          disposition: CommitDisposition.applied,
          targets: [
            ResourceStamp(
              resourceKey: 'settings',
              ownerEpoch: 'fixture-owner',
              revision: revision,
            ),
          ],
          effect: EffectPoint.immediate,
          effective: true,
          durable: true,
        ),
      ),
    );
  }
}

/// 只脚本化远端 CAS/响应，不实现命令构造、解析、lane 或 reducer。
class _Transport implements StudioTransport {
  final calls = StreamController<_SaveCall>();
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
  Future<SettingsCommandReply> saveProvider(
    int revision,
    ProviderCommand command, {
    required OperationId operationId,
  }) {
    final call = _SaveCall(revision, command, operationId);
    calls.add(call);
    return call.reply.future;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) =>
      throw UnsupportedError('Unused: ${invocation.memberName}');
}

AnyworkClient _client(_Transport transport) {
  final client = AnyworkClient(
    transport: transport,
    clientId: 'fixture-client',
  );
  client.applyBootstrap(
    StudioBootstrap(
      ownerEpoch: 'fixture-owner',
      projectDirectory: ProjectDirectoryState(values: const [], revision: 1),
      threadDirectory: ThreadDirectoryWindow(threads: const [], revision: 1),
      agentDirectory: AgentDirectoryState(revision: 1),
      settingsState: _snapshot(1, _provider),
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

Map<String, int> _limits(ProviderCommand command) => {
  for (final item in command.modelAutoCompactLimits) item.slug: item.limit,
};

void main() {
  test(
    'catalog revisions reproject limits while preserving stored overrides',
    () {
      final configured = _provider.withAutoCompactLimit(
        ProviderModelAutoCompactView.forModel(_alpha).withOverride(150),
      );
      var state = mergeSettingsProjection(
        const SettingsProjectionState(),
        _snapshot(1, configured),
      );

      ProviderSettingsView currentProvider() =>
          state.canonical!.providers.single;
      ProviderModelAutoCompactView alphaLimit() =>
          currentProvider().autoCompactLimits['alpha']!;
      SettingsStateSnapshot catalogFrame(
        int revision,
        List<ProviderModelView> models,
      ) {
        return SettingsStateSnapshot(
          revision: 999,
          modelCatalogRevision: revision,
          providers: [
            _provider.copyWith(
              name: 'Catalog metadata',
              models: models,
              autoCompactLimits: {
                'alpha': ProviderModelAutoCompactView.forModel(_alpha)
                    .withOverride(999),
              },
            ),
          ],
        );
      }

      const enlarged = ProviderModelView(
        slug: 'alpha',
        displayName: 'Alpha',
        reasoningEfforts: [],
        contextWindow: 2000,
        maxContextWindow: 4000,
      );
      state = mergeSettingsProjection(
        state,
        catalogFrame(2, [enlarged, _beta]),
        scope: SettingsMergeScope.catalogOnly,
      );
      expect(state.settingsRevision, 1);
      expect(currentProvider().name, 'Fixture');
      expect(alphaLimit().defaultLimit, 1800);
      expect(alphaLimit().safeLimit, 1800);
      expect(alphaLimit().overrideLimit, 150);
      expect(alphaLimit().effectiveLimit, 150);

      const unknownWithPreset = ProviderModelView(
        slug: 'alpha',
        displayName: 'Alpha',
        reasoningEfforts: [],
        autoCompactTokenLimit: 150,
      );
      state = mergeSettingsProjection(
        state,
        catalogFrame(3, [unknownWithPreset, _beta]),
        scope: SettingsMergeScope.catalogOnly,
      );
      expect(alphaLimit().defaultLimit, 150);
      expect(
        alphaLimit().overrideLimit,
        150,
        reason:
            'read-only projection must not consume an equal stored override',
      );
      expect(alphaLimit().safeLimit, isNull);
      expect(alphaLimit().effectiveLimit, isNull);

      state = mergeSettingsProjection(
        state,
        catalogFrame(4, [_beta]),
        scope: SettingsMergeScope.catalogOnly,
      );
      expect(alphaLimit().overrideLimit, 150);
      expect(alphaLimit().defaultLimit, isNull);
      expect(alphaLimit().safeLimit, isNull);
      expect(alphaLimit().effectiveLimit, isNull);
      expect(
        currentProvider().autoCompactLimits[_orphan.slug]?.overrideLimit,
        777,
      );

      const restored = ProviderModelView(
        slug: 'alpha',
        displayName: 'Alpha',
        reasoningEfforts: [],
        contextWindow: 1000,
      );
      state = mergeSettingsProjection(
        state,
        catalogFrame(5, [restored, _beta]),
        scope: SettingsMergeScope.catalogOnly,
      );
      expect(alphaLimit().defaultLimit, 900);
      expect(alphaLimit().overrideLimit, 150);
      expect(alphaLimit().effectiveLimit, 150);
      expect(_limits(ProviderCommandBuilder.buildProvider(currentProvider())), {
        'alpha': 150,
        'beta': 250,
        _orphan.slug: 777,
      });

      final configReply = _snapshot(
        2,
        configured.withAutoCompactLimit(
          ProviderModelAutoCompactView.forModel(_alpha).withOverride(300),
        ),
      );
      state = mergeSettingsProjection(
        state,
        configReply,
        scope: SettingsMergeScope.configOnly,
      );
      expect(state.modelCatalogRevision, 5);
      expect(
        alphaLimit().defaultLimit,
        900,
        reason: 'config replies retain the newer catalog metadata',
      );
      expect(alphaLimit().overrideLimit, 300);
      expect(alphaLimit().effectiveLimit, 300);
      expect(
        mergeSettingsProjection(state, _snapshot(1, configured)),
        same(state),
        reason: 'older combined snapshots cannot roll either watermark back',
      );
    },
  );

  test('model metadata derives nullable defaults and payload stores only complete overrides', () {
    final preset = ProviderModelAutoCompactView.forModel(_alpha);
    final derived = ProviderModelAutoCompactView.forModel(_beta);
    expect(preset.defaultLimit, 200);
    expect(preset.safeLimit, 900, reason: 'current window precedes maximum');
    expect(derived.defaultLimit, 1800, reason: 'maximum is a fallback');
    expect(preset.withOverride(1500).effectiveLimit, 900);
    expect(preset.withOverride(200).overrideLimit, isNull);
    expect(derived.withOverride(null).effectiveLimit, 1800);
    for (final capacity in [null, 0, 1]) {
      final model = ProviderModelView(
        slug: 'unknown',
        displayName: '',
        reasoningEfforts: const [],
        contextWindow: capacity,
      );
      final unknown = ProviderModelAutoCompactView.forModel(model);
      expect(unknown.defaultLimit, isNull);
      expect(unknown.withOverride(15).effectiveLimit, isNull);
      final declared = ProviderModelAutoCompactView.forModel(
        model.copyWith(autoCompactTokenLimit: 20),
      );
      expect(declared.defaultLimit, 20);
      expect(declared.effectiveLimit, isNull);
    }
    expect(
      ProviderModelAutoCompactView.safeLimitForContext(9000000000000000001),
      8100000000000000000,
    );
    final command = ProviderCommandBuilder.buildProvider(_provider);
    expect(_limits(command), {'beta': 250, _orphan.slug: 777});
    expect(_provider.autoCompactLimits['alpha']?.overrideLimit, isNull);
    final joined = providerWithCatalogMetadata(
      _provider.copyWith(
        models: [
          _alpha.copyWith(
            origin: ProviderModelOrigin.manual,
            contextWindow: 1000,
          ),
          _beta,
        ],
      ),
      const ProviderCatalogView.empty(),
    );
    expect(joined.customModels.map((model) => model.slug), ['alpha']);
    expect(joined.onlineModels, isEmpty);
    expect(
      joined.customModels.single.autoCompactTokenLimit,
      _alpha.autoCompactTokenLimit,
      reason: 'origin grouping does not drop catalog token presets',
    );
  });

  test('inline intents rebuild at lane entry and every CAS retry without dropping siblings or unavailable overrides', () async {
    final transport = _Transport();
    final client = _client(transport);
    final calls = StreamIterator(transport.calls.stream);
    addTearDown(calls.cancel);
    final commands = client.settingsCommands;
    final first = commands.setModelAutoCompactLimit('fixture', 'alpha', 100);
    await calls.moveNext();
    final initial = calls.current;
    expect(initial.revision, 1);
    expect(initial.command.secret.action, ProviderSecretAction.preserve);
    expect(initial.command.originalId, 'fixture');
    final second = commands.setModelAutoCompactLimit('fixture', 'alpha', 150);
    final reset = commands.setModelAutoCompactLimit('fixture', 'beta', null);
    final alphaKey = 'model-auto-compact:7:fixture:alpha';
    final latest = client.mutations.latest('settings:$alphaKey')!;
    final external = _provider
        .copyWith(
          name: 'External name',
          baseUrl: 'http://changed.invalid',
          pricingEnabled: true,
        )
        .withAutoCompactLimit(
          ProviderModelAutoCompactView.forModel(_beta).withOverride(400),
        );
    transport.refreshed = _snapshot(2, external);
    initial.reply.completeError(
      const StudioFailure(
        code: StudioFailureCode.conflict,
        message: 'External write',
        retryable: true,
        correlationId: 'fixture',
      ),
    );
    await calls.moveNext();
    final retried = calls.current;
    expect(retried.id, initial.id);
    expect(retried.revision, 2);
    expect(retried.command.name, 'External name');
    expect(retried.command.baseUrl, 'http://changed.invalid');
    expect(retried.command.pricingEnabled, isTrue);
    expect(_limits(retried.command), {
      'alpha': 100,
      'beta': 400,
      _orphan.slug: 777,
    });
    final afterFirst = external.withAutoCompactLimit(
      ProviderModelAutoCompactView.forModel(_alpha).withOverride(100),
    );
    retried.complete(3, afterFirst);
    await first;
    await calls.moveNext();
    expect(client.mutations.latest('settings:$alphaKey')?.id, latest.id);
    expect(
      client.settings.state.commands[alphaKey]?.isPending,
      isTrue,
      reason: 'old ACK cannot clear newer intent',
    );
    expect(
      client
          .settings
          .state
          .canonical!
          .providers
          .single
          .autoCompactLimits['alpha']!
          .overrideLimit,
      100,
      reason: 'no optimistic canonical',
    );
    final next = calls.current;
    expect(next.revision, 3);
    expect(_limits(next.command), {
      'alpha': 150,
      'beta': 400,
      _orphan.slug: 777,
    });
    final afterSecond = external.withAutoCompactLimit(
      ProviderModelAutoCompactView.forModel(_alpha).withOverride(150),
    );
    next.complete(4, afterSecond);
    await second;
    await calls.moveNext();
    final restoring = calls.current;
    expect(restoring.revision, 4);
    expect(_limits(restoring.command), {'alpha': 150, _orphan.slug: 777});
    var joined = false;
    final closing = commands.close().then((_) => joined = true);
    await Future<void>.delayed(Duration.zero);
    expect(
      joined,
      isFalse,
      reason: 'settings owner waits for the in-flight response',
    );
    restoring.complete(
      5,
      afterSecond.withAutoCompactLimit(
        ProviderModelAutoCompactView.forModel(_beta),
      ),
    );
    await reset;
    await closing;
    expect(joined, isTrue, reason: 'Client settings owner joins queued writes');
    expect(transport.refreshes, 1);
  });

  test('old failed intent cannot erase newer pending edit; latest failure can be explicitly retried', () async {
    final transport = _Transport();
    final client = _client(transport);
    final calls = StreamIterator(transport.calls.stream);
    addTearDown(calls.cancel);
    final commands = client.settingsCommands;
    final first = commands.setModelAutoCompactLimit('fixture', 'alpha', 100);
    final firstFailure = expectLater(first, throwsA(isA<StudioFailure>()));
    await calls.moveNext();
    final older = calls.current;
    final second = commands.setModelAutoCompactLimit('fixture', 'alpha', 150);
    final secondFailure = expectLater(second, throwsA(isA<StudioFailure>()));
    const key = 'model-auto-compact:7:fixture:alpha';
    const error = StudioFailure(
      code: StudioFailureCode.invalidArgument,
      message: 'Rejected by owner',
      retryable: false,
      correlationId: 'fixture',
    );
    older.reply.completeError(error);
    await firstFailure;
    await calls.moveNext();
    expect(client.settings.state.commands[key]?.isPending, isTrue);
    calls.current.reply.completeError(error);
    await secondFailure;
    expect(client.settings.state.commands[key]?.phase, CommandPhase.failed);
    final retry = commands.setModelAutoCompactLimit('fixture', 'alpha', 150);
    await calls.moveNext();
    expect(_limits(calls.current.command)['alpha'], 150);
    calls.current.complete(
      2,
      _provider.withAutoCompactLimit(
        ProviderModelAutoCompactView.forModel(_alpha).withOverride(150),
      ),
    );
    await retry;
    expect(client.settings.state.commands.containsKey(key), isFalse);
  });
}
