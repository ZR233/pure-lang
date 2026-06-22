import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../features/settings/settings_page.dart';
import '../features/shell/studio_shell.dart';
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
    return MaterialApp.router(
      title: 'Pure Studio',
      debugShowCheckedModeBanner: false,
      theme: pureStudioTheme(Brightness.light),
      darkTheme: pureStudioTheme(Brightness.dark),
      themeMode: ThemeMode.system,
      routerConfig: _router,
    );
  }
}
