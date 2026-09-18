import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';
import 'package:anywork/src/app/anywork_app.dart';
import 'package:anywork/src/data/frb/studio_api.dart';
import 'package:anywork/src/data/repositories/studio_repository.dart';
import 'package:anywork/src/domain/models/studio_models.dart';
import 'package:anywork/src/features/update/studio_update_controller.dart';
import 'package:anywork/src/shared/studio_driver_keys.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  testWidgets('background checks keep navigation usable and expose retry', (
    tester,
  ) async {
    final api = _RecoveryLoadingDemo();
    addTearDown(api.events.close);
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(api),
          studioUpdateEnabledProvider.overrideWithValue(false),
        ],
        child: const AnyworkApp(),
      ),
    );
    await _pumpUntilFound(tester, find.byKey(StudioDriverKeys.shell));
    expect(find.byKey(const ValueKey('recovery-check-status')), findsOneWidget);
    await tester.tap(find.byKey(StudioDriverKeys.settingsOpen));
    await _pumpUntilFound(tester, find.byKey(StudioDriverKeys.settingsPage));
    await tester.tap(find.byKey(StudioDriverKeys.settingsTab('agents')));
    await _pumpUntilFound(
      tester,
      find.byKey(const ValueKey('agent-profile-add')),
    );
    // Both routes expose the status banner while navigation is animating. The
    // recovery spinner stays active, so wait for the route rather than all frames.
    await _pumpUntilFound(
      tester,
      find.byElementPredicate(
        (element) =>
            element.widget.key == StudioDriverKeys.settingsPage &&
            ModalRoute.of(element)?.animation?.isCompleted == true,
      ),
    );
    expect(find.byKey(const ValueKey('recovery-check-status')), findsOneWidget);
    api.fail();
    await _pumpUntilFound(
      tester,
      find.byKey(const ValueKey('recovery-check-retry')),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('recovery-check-retry')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('recovery-check-status')), findsNothing);
    expect(find.byKey(const ValueKey('agent-profile-add')), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.tap(find.byKey(StudioDriverKeys.settingsBack));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.shell), findsOneWidget);
    await api.shutdownRuntime();
    await tester.pumpWidget(const SizedBox());
  });

  testWidgets('demo Studio smoke flow remains driver-addressable', (
    tester,
  ) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(DemoStudioApi()),
          studioUpdateEnabledProvider.overrideWithValue(false),
        ],
        child: const AnyworkApp(),
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

    expect(find.byKey(StudioDriverKeys.composerSubmit), findsNothing);
    // TurnStarted can arrive before the submission receipt. Only the receipt unlocks
    // the next draft; observing the stop button alone does not prove admission returned.
    await _pumpUntilFound(
      tester,
      find.byWidgetPredicate(
        (widget) =>
            widget is TextField &&
            widget.key == StudioDriverKeys.composerInput &&
            widget.enabled == true &&
            widget.controller?.text.isEmpty == true,
      ),
    );
    // Submission temporarily disables the field and closes its native input connection.
    // Refocus as a user would before entering another prompt on the same EditableText.
    await tester.tap(find.byKey(StudioDriverKeys.composerInput));
    await tester.pump();
    await tester.enterText(
      find.byKey(StudioDriverKeys.composerInput),
      'redirect integration smoke',
    );
    expect(
      ProviderScope.containerOf(
        tester.element(find.byKey(StudioDriverKeys.composerInput)),
      ).read(studioControllerProvider).value!.composer.draft,
      'redirect integration smoke',
    );
    await _pumpUntilFound(
      tester,
      find.byWidgetPredicate(
        (widget) =>
            widget is IconButton &&
            widget.key == StudioDriverKeys.composerSubmit &&
            widget.onPressed != null,
      ),
    );
    expect(find.byKey(StudioDriverKeys.composerStop), findsNothing);
    await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
    await _pumpUntilFound(tester, find.text('redirect integration smoke'));
    await _pumpUntilFound(tester, find.byKey(StudioDriverKeys.composerStop));
    await tester.tap(find.byKey(StudioDriverKeys.composerStop));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.composerStop), findsNothing);
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
          child: const AnyworkApp(),
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
        child: const AnyworkApp(),
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
    await tester.tap(find.byKey(StudioDriverKeys.sshTest('arm-dev')));
    await tester.pumpAndSettle();
    expect(api.testedServerAlias, 'arm-dev');
    await tester.tap(find.byKey(StudioDriverKeys.sshReconnect('arm-dev')));
    await tester.pumpAndSettle();
    expect(api.reconnectedServerAlias, 'arm-dev');
    await tester.tap(find.byKey(StudioDriverKeys.sshOpen('arm-dev')));
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsOneWidget);
    expect(api.browsedServerAlias, 'arm-dev');
    await tester.enterText(
      find.byKey(StudioDriverKeys.sshDirectoryPathInput),
      '/home/projects',
    );
    await tester.tap(find.byKey(StudioDriverKeys.sshDirectoryGo));
    await tester.pumpAndSettle();
    expect(
      find.byKey(StudioDriverKeys.sshDirectoryCurrent('/home/projects')),
      findsOneWidget,
    );
    await tester.tap(find.byKey(StudioDriverKeys.sshOpenCurrentDirectory));
    await tester.pumpAndSettle();
    expect(api.openedRemoteProject, ('arm-dev', '/home/projects'));
    expect(api.activatedProjectId, 'project-remote');
    // 打开成功且 canonical snapshot 采用远端项目后，目录对话框关闭。
    expect(find.byKey(StudioDriverKeys.sshDirectoryDialog), findsNothing);
    final adoptedState = await api.readStudioState();
    expect(adoptedState.selectedProjectId, 'project-remote');
    expect(
      adoptedState.projects.any((project) => project.id == 'project-remote'),
      isTrue,
    );

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

    // AnyworkApp 在同一 Driver 进程中复用路由器；显式返回工作区，
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
        child: const AnyworkApp(),
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
    final api = DriverDemoStudioApi()..preparePersistenceFailureScenario();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(api),
          studioUpdateEnabledProvider.overrideWithValue(false),
        ],
        child: const AnyworkApp(),
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
      isTrue,
    );
    expect(
      tester
          .widget<IconButton>(find.byKey(StudioDriverKeys.composerSubmit))
          .onPressed,
      isNull,
    );
    await tester.tap(find.byKey(const ValueKey('persistence-retry')));
    await tester.pumpAndSettle();

    expect(
      find.byKey(const ValueKey('persistence-state-banner')),
      findsNothing,
    );
  });

  testWidgets(
    'start page workspace mode selector drives a worktree sidebar marker',
    (tester) async {
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            studioApiProvider.overrideWithValue(DemoStudioApi()),
            studioUpdateEnabledProvider.overrideWithValue(false),
          ],
          child: const AnyworkApp(),
        ),
      );
      await tester.pumpAndSettle();

      await tester.tap(find.byKey(StudioDriverKeys.newSession));
      await tester.pumpAndSettle();

      final selector = find.byKey(StudioDriverKeys.sessionWorkspaceMode);
      expect(selector, findsOneWidget);
      final container = ProviderScope.containerOf(tester.element(selector));
      String? draftMode() => container
          .read(studioControllerProvider)
          .value
          ?.newThreadWorkspaceMode
          .id;
      expect(draftMode(), 'local');

      await tester.tap(selector);
      await tester.pumpAndSettle();
      final worktreeOption = find.byKey(
        StudioDriverKeys.sessionWorkspaceModeOption('worktree'),
      );
      expect(worktreeOption, findsOneWidget);
      await tester.tap(worktreeOption);
      await tester.pumpAndSettle();
      expect(draftMode(), 'worktree');

      await tester.enterText(
        find.byKey(StudioDriverKeys.composerInput),
        'integration worktree session',
      );
      await tester.pump();
      await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
      await _pumpUntilFound(tester, find.byKey(StudioDriverKeys.composerStop));

      final selectedThreadId = container
          .read(studioControllerProvider)
          .requireValue
          .selectedThreadId;
      expect(selectedThreadId, isNotNull);
      expect(
        find.byKey(StudioDriverKeys.threadWorkspaceMode(selectedThreadId!)),
        findsOneWidget,
      );
      expect(
        find.byKey(StudioDriverKeys.threadWorkspaceMode('thread-main')),
        findsNothing,
        reason: 'the local fixture session must not carry a worktree marker',
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'remote Project start page offers worktree and drives a worktree marker',
    (tester) async {
      // 远端项目经由 SSH 打开并激活后仍停留在起始页；这里直接预置该 canonical
      // 事实，避免重复驱动完整的目录浏览流程。
      final api = _RemoteDriverDemoStudioApi();
      await api.openRemoteProject('arm-dev', '/home/projects');
      await api.activateProject('project-remote');
      await tester.pumpWidget(
        ProviderScope(
          overrides: [
            studioApiProvider.overrideWithValue(api),
            studioUpdateEnabledProvider.overrideWithValue(false),
          ],
          child: const AnyworkApp(),
        ),
      );
      await tester.pumpAndSettle();

      final selector = find.byKey(StudioDriverKeys.sessionWorkspaceMode);
      expect(selector, findsOneWidget);
      final container = ProviderScope.containerOf(tester.element(selector));
      String? draftMode() => container
          .read(studioControllerProvider)
          .value
          ?.newThreadWorkspaceMode
          .id;
      expect(draftMode(), 'local');

      await tester.tap(selector);
      await tester.pumpAndSettle();
      final worktreeOption = find.byKey(
        StudioDriverKeys.sessionWorkspaceModeOption('worktree'),
      );
      expect(worktreeOption, findsOneWidget);
      expect(
        tester
            .widget<PopupMenuItem<ThreadWorkspaceMode>>(worktreeOption)
            .enabled,
        isTrue,
        reason: 'remote Projects must expose the worktree option',
      );
      await tester.tap(worktreeOption);
      await tester.pumpAndSettle();
      expect(draftMode(), 'worktree');

      await tester.enterText(
        find.byKey(StudioDriverKeys.composerInput),
        'remote worktree session',
      );
      await tester.pump();
      await tester.tap(find.byKey(StudioDriverKeys.composerSubmit));
      await _pumpUntilFound(tester, find.byKey(StudioDriverKeys.composerStop));

      final controller = container.read(studioControllerProvider).requireValue;
      expect(controller.selectedProjectId, 'project-remote');
      final selectedThreadId = controller.selectedThreadId;
      expect(selectedThreadId, isNotNull);
      expect(
        controller.threads
            .firstWhere((thread) => thread.id == selectedThreadId)
            .workspaceMode,
        ThreadWorkspaceMode.worktree,
      );
      expect(
        find.byKey(StudioDriverKeys.threadWorkspaceMode(selectedThreadId!)),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );
}

class _RemoteDriverDemoStudioApi extends DriverDemoStudioApi {
  String? testedServerAlias;
  String? reconnectedServerAlias;
  String? browsedServerAlias;
  (String, String)? openedRemoteProject;
  String? activatedProjectId;

  @override
  Future<SshConnectionView> testSshConnection(String alias) async {
    testedServerAlias = alias;
    return super.testSshConnection(alias);
  }

  @override
  Future<SshConnectionView> reconnectSshServer(String alias) async {
    reconnectedServerAlias = alias;
    return super.reconnectSshServer(alias);
  }

  @override
  Future<RemoteDirectoryListing> browseRemoteDirectories(
    String alias, {
    String? path,
  }) async {
    browsedServerAlias = alias;
    return super.browseRemoteDirectories(alias, path: path);
  }

  @override
  Future<StudioProject> openRemoteProject(String alias, String path) async {
    openedRemoteProject = (alias, path);
    return super.openRemoteProject(alias, path);
  }

  @override
  Future<void> activateProject(String projectId) async {
    await super.activateProject(projectId);
    activatedProjectId = projectId;
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

class _RecoveryLoadingDemo extends DemoStudioApi {
  Future<void>? _shutdown;
  @override
  Future<void> shutdownRuntime() => _shutdown ??= super.shutdownRuntime();
  final events = StreamController<Object>.broadcast();
  RecoveryStateSnapshot recovery = const RecoveryStateSnapshot.fromState(
    state: RefreshingObservedResource<List<StudioRecoveryIssue>>(
      revision: 100,
      operation: 'check',
      operationId: 'integration-recovery',
      startedAt: 0,
      lastCheckedAt: null,
      value: [],
    ),
  );
  @override
  Future<StudioState> readStudioState() async =>
      (await super.readStudioState()).copyWith(recoveryState: recovery);
  @override
  Stream<Object> subscribeProductEvents() => events.stream;
  void fail() {
    recovery = const RecoveryStateSnapshot.fromState(
      state: FailedObservedResource<List<StudioRecoveryIssue>>(
        revision: 101,
        failedAt: 1,
        operation: 'check',
        error: ObservedResourceError(
          code: 'unavailable',
          message: 'Workspace unavailable',
          retryable: true,
        ),
      ),
    );
    events.add(
      StudioBridgeEvent(payload: RecoveryStateChangedPayload(recovery)),
    );
  }

  @override
  Future<RecoveryStateSnapshot> retryRecovery() async {
    recovery = RecoveryStateSnapshot(revision: 102);
    return recovery;
  }
}
