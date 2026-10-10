import 'dart:async';

import 'package:flutter/material.dart';

import '../data/repositories/studio_controller.dart';

/// Navigation is the owner of conversation visibility and bounded route
/// synchronization.  Widgets do not infer business visibility from
/// initState/dispose, and a settings page may be destroyed while its app-level
/// mutation lane is still draining.
final class StudioNavigationCoordinator extends NavigatorObserver {
  StudioNavigationCoordinator(this._controller);

  final StudioController _controller;
  bool? _conversationVisible;

  bool _isSettings(Route<dynamic>? route) {
    final name = route?.settings.name;
    return name == 'settings' || name?.endsWith('/settings') == true;
  }

  void _sync(Route<dynamic>? route) {
    final visible = !_isSettings(route);
    if (_conversationVisible == visible) return;
    _conversationVisible = visible;
    unawaited(
      _controller.setConversationVisible(visible).catchError((error, stack) {
        debugPrint(
          'route_visibility_failed visible=$visible error=$error\n$stack',
        );
      }),
    );
  }

  /// Gives a caller a bounded synchronization point before leaving Settings.
  /// A timeout returns control to navigation; the repository continues the
  /// pending operation in the background.
  Future<bool> flushPending({Duration timeout = const Duration(seconds: 5)}) =>
      _controller.flushPending(timeout: timeout);

  @override
  void didPush(Route<dynamic> route, Route<dynamic>? previousRoute) {
    _sync(route);
  }

  @override
  void didPop(Route<dynamic> route, Route<dynamic>? previousRoute) {
    _sync(previousRoute);
  }

  @override
  void didRemove(Route<dynamic> route, Route<dynamic>? previousRoute) {
    _sync(previousRoute);
  }

  @override
  void didReplace({Route<dynamic>? newRoute, Route<dynamic>? oldRoute}) {
    _sync(newRoute);
  }
}
