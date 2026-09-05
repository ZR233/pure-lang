import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:pure_studio/src/app/pure_studio_app.dart';
import 'package:pure_studio/src/data/frb/studio_api.dart';
import 'package:pure_studio/src/data/repositories/studio_repository.dart';
import 'package:pure_studio/src/domain/models/studio_models.dart';
import 'package:pure_studio/src/features/update/studio_update_controller.dart';
import 'package:pure_studio/src/shared/studio_driver_keys.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('demo Studio smoke flow remains driver-addressable', (
    tester,
  ) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(DemoStudioApi()),
          studioUpdateEnabledProvider.overrideWithValue(false),
        ],
        child: const PureStudioApp(),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.shell), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.sidebar), findsOneWidget);
    expect(find.byKey(StudioDriverKeys.timeline), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.settingsOpen));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.settingsPage), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.settingsBack));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.composerInput), findsOneWidget);

    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'integration smoke',
    );
    await tester.pump();
    await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
    await _pumpUntilFound(tester, find.byKey(StudioDriverKeys.composerStop));
    expect(find.byKey(StudioDriverKeys.composerStop), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.composerStop));
    await _pumpUntilFound(tester, find.byKey(StudioDriverKeys.composerSubmit));
    expect(find.byKey(StudioDriverKeys.composerSubmit), findsOneWidget);
    expect(find.text('integration smoke'), findsWidgets);
  });

  testWidgets(
    'compatible provider can be configured, selected and used without extra settings',
    (tester) async {
      final api = DemoStudioApi();
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            studioApiProvider.overrideWithValue(api),
            studioUpdateEnabledProvider.overrideWithValue(false),
          ],
          child: const PureStudioApp(),
        ),
      );
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.settingsOpen));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.settingsTab('providers')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.providerAdd));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.providerPreset));
      await tester.pumpAndSettle();
      await tester.tap(find.text('OpenAI API 兼容').last);
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<SwitchListTile>(
              find.byKey(StudioDriverKeys.providerPricing),
            )
            .value,
        isFalse,
      );
      await tester.ensureVisible(find.byKey(StudioDriverKeys.providerModelAdd));
      await tester.tap(find.byKey(StudioDriverKeys.providerModelAdd));
      await tester.pumpAndSettle();
      final id = find.descendant(
        of: find.byKey(StudioDriverKeys.customModelId(0)),
        matching: find.byType(TextFormField),
      );
      await tester.ensureVisible(id);
      await tester.enterText(id, 'local-coder');
      await tester.pumpAndSettle();
      await tester.scrollUntilVisible(
        find.byKey(StudioDriverKeys.providerSave),
        -400,
        scrollable: find
            .descendant(
              of: find.byKey(StudioDriverKeys.providerEditorScroll),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      await tester.tap(find.byKey(StudioDriverKeys.providerSave));
      await tester.pumpAndSettle();
      final state = await api.readStudioState();
      final provider = state.providers.singleWhere(
        (provider) => provider.templateKind == 'openai-compatible',
      );
      expect(provider.defaultModel, 'local-coder');
      expect(provider.pricingEnabled, isFalse);
      expect(provider.hasBearerToken, isFalse);
      await tester.tap(find.byKey(StudioDriverKeys.settingsBack));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(StudioDriverKeys.model));
      await tester.pumpAndSettle();
      await tester.tap(
        find.byKey(StudioDriverKeys.modelOption(provider.id, 'local-coder')),
      );
      await tester.pumpAndSettle();
      final selected = (await api.readStudioState()).roles.firstWhere(
        (role) => role.key == 'planner',
      );
      expect(selected.providerId, provider.id);
      expect(selected.model, 'local-coder');
      expect(selected.effort, isEmpty);
      await tester.enterText(
        find.byKey(StudioDriverKeys.composerInput),
        'Complete the compatible-provider acceptance task.',
      );
      await tester.pump();
      await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
      await _pumpUntilFound(tester, find.byKey(StudioDriverKeys.composerStop));
      await _pumpUntilFound(
        tester,
        find.byKey(StudioDriverKeys.composerSubmit),
      );
      expect(
        find.text('Complete the compatible-provider acceptance task.'),
        findsWidgets,
      );
    },
  );

  testWidgets('provider settings and typed interactions expose stable keys', (
    tester,
  ) async {
    final api = _RemoteDriverDemoStudioApi();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(api),
          studioUpdateEnabledProvider.overrideWithValue(false),
        ],
        child: const PureStudioApp(),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.toolApprove), findsOneWidget);
    await tester.tap(find.byKey(StudioDriverKeys.toolApprove));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.userInputSubmit), findsOneWidget);
    await tester.tap(find.byKey(StudioDriverKeys.userInputSubmit));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.composerInput), findsOneWidget);

    expect(
      find.byKey(StudioDriverKeys.threadRow('thread-alt')),
      findsOneWidget,
    );
    await tester.tap(find.byKey(StudioDriverKeys.threadRow('thread-alt')));
    await tester.pumpAndSettle();
    expect(
      find.text('Riverpod selector boundary is isolated.'),
      findsOneWidget,
    );

    await tester.tap(find.byKey(StudioDriverKeys.threadRow('thread-main')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.agentSwitcher));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.agentRow('thread-reviewer')),
      findsOneWidget,
    );
    await tester.tap(find.byKey(StudioDriverKeys.agentRow('thread-reviewer')));
    await tester.pumpAndSettle();
    expect(find.text('Driver agent workspace selected.'), findsOneWidget);

    await tester.tap(find.byKey(StudioDriverKeys.threadRow('thread-main')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.settingsOpen));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('ssh')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.sshTest('demo-ssh')));
    await tester.pumpAndSettle();
    expect(api.testedServerId, 'demo-ssh');
    await tester.tap(find.byKey(StudioDriverKeys.sshReconnect('demo-ssh')));
    await tester.pumpAndSettle();
    expect(api.reconnectedServerId, 'demo-ssh');
    await tester.tap(find.byKey(StudioDriverKeys.sshOpen('demo-ssh')));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);
    expect(api.browsedServerId, 'demo-ssh');
    await tester.tap(find.byKey(StudioDriverKeys.sshOpenCurrentDirectory));
    await tester.pumpAndSettle();
    expect(api.openedRemoteProject, ('demo-ssh', '/home'));
    expect(api.activatedProjectId, 'project-remote');

    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('providers')));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.providerRow('future-provider')),
      findsOneWidget,
    );
    await tester.tap(
      find.byKey(StudioDriverKeys.providerRow('future-provider')),
    );
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.providerEditor), findsOneWidget);
    await tester.tap(find.byKey(StudioDriverKeys.providerEdit));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.providerCancel));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.providerEditor), findsNothing);

    await tester.tap(
      find.byKey(StudioDriverKeys.providerRow('future-provider')),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.providerEdit));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(StudioDriverKeys.providerSave));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.providerEditor), findsNothing);

    // PureStudioApp 在同一 Driver 进程中复用路由器；显式返回工作区，
    // 避免后续场景继承当前设置页位置。
    await tester.tap(find.byKey(StudioDriverKeys.settingsBack));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.shell), findsOneWidget);
  });

  testWidgets('fallback interaction accepts Continue through stable keys', (
    tester,
  ) async {
    final api = DriverDemoStudioApi()..prepareFallbackInputScenario();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(api),
          studioUpdateEnabledProvider.overrideWithValue(false),
        ],
        child: const PureStudioApp(),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.fallbackUserInput), findsOneWidget);
    await tester.enterText(
      find.byKey(StudioDriverKeys.fallbackUserInput),
      '继续',
    );
    await tester.pump();
    await tester.tap(find.byKey(StudioDriverKeys.fallbackUserInputSubmit));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.fallbackUserInput), findsNothing);
    expect(find.byKey(StudioDriverKeys.composerInput), findsOneWidget);
  });

  testWidgets('persistence degradation banner retries back to ready', (
    tester,
  ) async {
    final api = _DegradedDemoStudioApi()..prepareSessionLifecycleScenario();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(api),
          studioUpdateEnabledProvider.overrideWithValue(false),
        ],
        child: const PureStudioApp(),
      ),
    );
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey('persistence-state-banner')),
      findsOneWidget,
    );
    expect(
      tester
          .widget<TextField>(find.byKey(StudioDriverKeys.composerInput))
          .enabled,
      isFalse,
    );
    expect(
      tester
          .widget<IconButton>(find.byKey(StudioDriverKeys.composerSubmit))
          .onPressed,
      isNull,
    );
    await tester.tap(find.byKey(const ValueKey('persistence-retry')));
    await tester.pumpAndSettle();

    expect(api.retryCount, 1);
    expect(
      find.byKey(const ValueKey('persistence-state-banner')),
      findsNothing,
    );
  });
}

class _RemoteDriverDemoStudioApi extends DriverDemoStudioApi {
  String? testedServerId;
  String? reconnectedServerId;
  String? browsedServerId;
  (String, String)? openedRemoteProject;
  String? activatedProjectId;

  @override
  Future<SshConnectionView> testSshConnection(String serverId) async {
    testedServerId = serverId;
    return super.testSshConnection(serverId);
  }

  @override
  Future<SshConnectionView> reconnectSshServer(String serverId) async {
    reconnectedServerId = serverId;
    return super.reconnectSshServer(serverId);
  }

  @override
  Future<RemoteDirectoryListing> browseRemoteDirectories(
    String serverId, {
    String? path,
  }) async {
    browsedServerId = serverId;
    return super.browseRemoteDirectories(serverId, path: path);
  }

  @override
  Future<StudioProject> openRemoteProject(String serverId, String path) async {
    openedRemoteProject = (serverId, path);
    return super.openRemoteProject(serverId, path);
  }

  @override
  Future<void> activateProject(String projectId) async {
    activatedProjectId = projectId;
  }
}

class _DegradedDemoStudioApi extends DriverDemoStudioApi {
  int retryCount = 0;

  @override
  Future<StudioState> readStudioState() async {
    final current = await super.readStudioState();
    return current.copyWith(
      persistenceState: const PersistenceStateSnapshot(
        revision: 1,
        state: DegradedPersistenceState(
          pendingCommits: 3,
          oldestPendingRevision: 8,
          firstFailedAt: 1,
          error: ObservedResourceError(
            code: 'storageUnavailable',
            message: 'database is temporarily unavailable',
            retryable: true,
          ),
        ),
      ),
    );
  }

  @override
  Future<PersistenceStateSnapshot> retryPersistence() async {
    retryCount += 1;
    return const PersistenceStateSnapshot(
      revision: 2,
      state: ReadyPersistenceState(pendingCommits: 0),
    );
  }
}

Future<void> _pumpUntilFound(
  WidgetTester tester,
  Finder finder, {
  Duration timeout = const Duration(seconds: 5),
}) async {
  final deadline = tester.binding.clock.now().add(timeout);
  while (finder.evaluate().isEmpty &&
      tester.binding.clock.now().isBefore(deadline)) {
    await tester.pump(const Duration(milliseconds: 50));
  }
  if (finder.evaluate().isEmpty) {
    throw TestFailure('Timed out waiting for $finder');
  }
}
