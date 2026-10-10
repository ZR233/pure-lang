import 'dart:async';
import 'dart:ui' show AppExitResponse;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../data/frb/studio_api.dart';
import '../data/repositories/studio_repository.dart';
import '../features/settings/settings.dart';
import '../features/shell/studio_shell.dart';
import '../features/update/studio_update_controller.dart';
import '../l10n/app_localizations.dart';
import '../l10n/studio_l10n.dart';
import '../platform/error_log.dart';
import 'studio_host_lifecycle.dart';
import 'studio_navigation_coordinator.dart';
import 'studio_shutdown.dart';
import 'theme/material3_theme.dart';

class AnyworkApp extends ConsumerStatefulWidget {
  const AnyworkApp({super.key});

  @override
  ConsumerState<AnyworkApp> createState() => _AnyworkAppState();
}

class _AnyworkAppState extends ConsumerState<AnyworkApp> {
  late final GoRouter _router;

  @override
  void initState() {
    super.initState();
    _router = GoRouter(
      restorationScopeId: 'anywork-router',
      observers: [
        StudioNavigationCoordinator(
          ref.read(studioControllerProvider.notifier),
        ),
      ],
      routes: [
        GoRoute(
          path: '/',
          name: 'studio',
          builder: (context, state) => const StudioShell(),
          routes: [
            GoRoute(
              path: 'settings',
              name: 'settings',
              pageBuilder: (context, state) => const MaterialPage<void>(
                restorationId: 'settings-page',
                child: SettingsPage(),
              ),
            ),
          ],
        ),
      ],
    );
  }

  @override
  void dispose() {
    _router.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return StudioLifecycleCoordinator(
      child: _EagerInitialization(
        child: MaterialApp.router(
          onGenerateTitle: (context) => context.l10n.appTitle,
          debugShowCheckedModeBanner: false,
          theme: pureStudioTheme(),
          themeMode: ThemeMode.light,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          routerConfig: _router,
          restorationScopeId: 'anywork-app',
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
  late final StudioBridgeDataSource _api;
  late final StudioShutdownProgressState _shutdownProgress;
  late final StudioExitCoordinator _coordinator;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    // dispose 后 ConsumerState.ref 不可再用，关机依赖必须在挂载期间取得。
    _api = ref.read(studioBridgeDataSourceProvider);
    _shutdownProgress = ref.read(studioShutdownProgressStateProvider.notifier);
    _coordinator = StudioExitCoordinator(
      _api,
      // dispose 之后 overlay 与 provider container 均已销毁：不再向已销毁的
      // progress notifier 写状态。
      (progress) {
        if (mounted) _shutdownProgress.update(progress);
      },
      onFailure: (error) {
        if (mounted) _shutdownProgress.fail(error);
      },
      shutdownOverride: widget.shutdown,
    );
    // 安装 native -> Dart 退出回调，并提前把 canonical 日志目录交给 native。
    StudioExitCoordinator.install(_coordinator);
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.detached) {
      unawaited(StudioExitCoordinator.requestExit().catchError(_logExitError));
    }
  }

  @override
  Future<AppExitResponse> didRequestAppExit() async {
    // 与窗口关闭同一协调器：arm native deadline，执行有界清理并结束本实例。
    await StudioExitCoordinator.requestExit().catchError(_logExitError);
    return AppExitResponse.exit;
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    // 卸载兜底：此时 overlay 与 provider container 均已销毁，只执行关机；协调器
    // 通过共享 future 复用同一次收束，且不会再向已销毁的 progress notifier 写状态。
    unawaited(StudioExitCoordinator.requestExit().catchError(_logExitError));
    super.dispose();
  }

  void _logExitError(Object error, StackTrace stackTrace) {
    // 退出路径的异常不静默丢弃：cause/stack 经 canonical 诊断日志（含 system temp /
    // stderr 兜底）记录，并带 stage 与 correlation；只记录允许的诊断字段，绝不把
    // 任意正文当作 wire 安全消息。native 硬期限仍会兜底结束本进程。
    debugPrint('studio_exit_unhandled=$error\n$stackTrace');
    recordDartError(
      error,
      stackTrace,
      stage: 'exit-unhandled',
      correlationId: newStudioCorrelationId(),
    );
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
