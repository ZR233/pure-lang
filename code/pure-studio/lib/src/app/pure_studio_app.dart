import 'dart:async';
import 'dart:ui' show AppExitResponse;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../data/frb/studio_api.dart';
import '../data/repositories/studio_api_provider.dart';
import '../features/settings/settings.dart';
import '../features/shell/studio_shell.dart';
import '../features/update/studio_update_controller.dart';
import '../l10n/app_localizations.dart';
import '../l10n/studio_l10n.dart';
import 'studio_shutdown.dart';
import 'theme/material3_theme.dart';

class PureStudioApp extends StatelessWidget {
  const PureStudioApp({super.key});

  static final GoRouter _router = GoRouter(
    routes: [
      GoRoute(path: '/', builder: (context, state) => const StudioShell()),
      GoRoute(
        path: '/settings',
        builder: (context, state) => const SettingsPage(),
      ),
    ],
  );

  @override
  Widget build(BuildContext context) {
    return StudioLifecycleCoordinator(
      child: _EagerInitialization(
        child: MaterialApp.router(
          onGenerateTitle: (context) => context.l10n.appTitle,
          debugShowCheckedModeBanner: false,
          theme: pureStudioTheme(Brightness.light),
          darkTheme: pureStudioTheme(Brightness.dark),
          themeMode: ThemeMode.system,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          routerConfig: _router,
          builder: (context, child) =>
              StudioShutdownOverlay(child: child ?? const SizedBox()),
        ),
      ),
    );
  }
}

class StudioLifecycleCoordinator extends ConsumerStatefulWidget {
  const StudioLifecycleCoordinator({
    required this.child,
    this.shutdown,
    super.key,
  });

  final Widget child;
  final Future<void> Function()? shutdown;

  @override
  ConsumerState<StudioLifecycleCoordinator> createState() =>
      _StudioLifecycleCoordinatorState();
}

class _StudioLifecycleCoordinatorState
    extends ConsumerState<StudioLifecycleCoordinator>
    with WidgetsBindingObserver {
  Future<void>? _shutdownFuture;
  late final StudioApi _api;
  late final StudioShutdownProgressState _shutdownProgress;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    // dispose 后 ConsumerState.ref 不可再用，关机依赖必须在挂载期间取得。
    _api = ref.read(studioApiProvider);
    _shutdownProgress = ref.read(studioShutdownProgressStateProvider.notifier);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.detached) {
      unawaited(_shutdown());
    }
  }

  @override
  Future<AppExitResponse> didRequestAppExit() async {
    await _shutdown();
    return AppExitResponse.exit;
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    // 卸载兜底：此时 overlay 与 provider container 均已销毁，只执行关机，
    // 不再向已销毁的 progress notifier 写状态。
    unawaited(_shutdownFuture ??= _disposeShutdown());
    super.dispose();
  }

  /// 幂等共享同一个 shutdown future；默认路径先呈现关机阶段 overlay，
  /// 等待 write-behind 落库排空（FlushingPersistence pending=0）后才放行退出。
  Future<void> _shutdown() {
    return _shutdownFuture ??= (widget.shutdown ?? _defaultShutdown)();
  }

  Future<void> _defaultShutdown() {
    return runStudioShutdown(_api, _shutdownProgress.update);
  }

  Future<void> _disposeShutdown() {
    final override = widget.shutdown;
    if (override != null) return override();
    return runStudioShutdown(_api, (_) {});
  }

  @override
  Widget build(BuildContext context) => widget.child;
}

class _EagerInitialization extends ConsumerWidget {
  const _EagerInitialization({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    ref.watch(studioUpdateControllerProvider);
    return child;
  }
}
