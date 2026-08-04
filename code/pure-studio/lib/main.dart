import 'dart:async';
import 'dart:io';
import 'dart:ui';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'src/app/pure_studio_app.dart';

void main() {
  FlutterError.onError = (details) {
    FlutterError.presentError(details);
    _recordDartError(details.exception, details.stack);
  };
  PlatformDispatcher.instance.onError = (error, stack) {
    _recordDartError(error, stack);
    return true;
  };
  runZonedGuarded(
    () => runApp(const ProviderScope(child: PureStudioApp())),
    _recordDartError,
  );
}

void _recordDartError(Object error, StackTrace? stack) {
  try {
    final localAppData = Platform.environment['LOCALAPPDATA'];
    final root = localAppData == null || localAppData.isEmpty
        ? Directory.current
        : Directory('$localAppData${Platform.pathSeparator}Pure Studio');
    final logs = Directory('${root.path}${Platform.pathSeparator}logs')
      ..createSync(recursive: true);
    final now = DateTime.now();
    final date =
        '${now.year.toString().padLeft(4, '0')}-'
        '${now.month.toString().padLeft(2, '0')}-'
        '${now.day.toString().padLeft(2, '0')}';
    final file = File(
      '${logs.path}${Platform.pathSeparator}dart-error-$date.log',
    );
    file.writeAsStringSync(
      '${DateTime.now().toUtc().toIso8601String()} $error\n'
      '${stack ?? StackTrace.current}\n\n',
      mode: FileMode.append,
      flush: true,
    );
  } on Object {
    // Error reporting must never take down the UI isolate.
  }
}
