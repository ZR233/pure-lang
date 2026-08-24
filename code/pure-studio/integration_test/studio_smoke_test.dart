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

  testWidgets('provider settings and typed interactions expose stable keys', (
    tester,
  ) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          studioApiProvider.overrideWithValue(DriverDemoStudioApi()),
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

    expect(find.byKey(StudioDriverKeys.planAdjustmentInput), findsOneWidget);
    await tester.enterText(
      find.byKey(StudioDriverKeys.planAdjustmentInput),
      'Keep the typed origin-turn assertion.',
    );
    await tester.pumpAndSettle();
    expect(find.byKey(StudioDriverKeys.planRevise), findsOneWidget);
    await tester.tap(find.byKey(StudioDriverKeys.planRevise));
    await tester.pumpAndSettle();

    expect(find.byKey(StudioDriverKeys.planConfirm), findsOneWidget);
    await tester.tap(find.byKey(StudioDriverKeys.planConfirm));
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
