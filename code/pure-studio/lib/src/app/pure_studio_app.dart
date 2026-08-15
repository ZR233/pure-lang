import 'dart:async';
import 'dart:ui' show AppExitResponse;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

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

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
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
    unawaited(_shutdown());
    super.dispose();
  }

  /// 幂等共享同一个 shutdown future；默认路径先呈现关机阶段 overlay，
  /// 等待 write-behind 落库排空（FlushingPersistence pending=0）后才放行退出。
  Future<void> _shutdown() {
    return _shutdownFuture ??= (widget.shutdown ?? _defaultShutdown)();
  }

  Future<void> _defaultShutdown() {
    final api = ref.read(studioApiProvider);
    final progress = ref.read(studioShutdownProgressStateProvider.notifier);
    return runStudioShutdown(api, progress.update);
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
