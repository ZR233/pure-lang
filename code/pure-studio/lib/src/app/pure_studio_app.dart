import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../data/frb/studio_api.dart';
import '../features/settings/settings.dart';
import '../features/shell/studio_shell.dart';
import '../features/update/studio_update_controller.dart';
import '../l10n/app_localizations.dart';
import '../l10n/studio_l10n.dart';
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
    return _StudioLifecycleCoordinator(
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
        ),
      ),
    );
  }
}

class _StudioLifecycleCoordinator extends StatefulWidget {
  const _StudioLifecycleCoordinator({required this.child});

  final Widget child;

  @override
  State<_StudioLifecycleCoordinator> createState() =>
      _StudioLifecycleCoordinatorState();
}

class _StudioLifecycleCoordinatorState
    extends State<_StudioLifecycleCoordinator>
    with WidgetsBindingObserver {
  bool _shutdownRequested = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.detached) {
      _shutdown();
    }
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _shutdown();
    super.dispose();
  }

  void _shutdown() {
    if (_shutdownRequested) {
      return;
    }
    _shutdownRequested = true;
    unawaited(FrbStudioApi.shutdownAndDispose());
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
