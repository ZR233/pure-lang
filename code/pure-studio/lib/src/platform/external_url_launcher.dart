import 'dart:convert';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'studio_platform.dart';

typedef ExternalUrlLauncher = Future<void> Function(String url);

const _maxExternalUrlBytes = 8 * 1024;
final _urlControlCharacters = RegExp(r'[\u0000-\u001F\u007F-\u009F]');

final externalUrlLauncherProvider = Provider<ExternalUrlLauncher>(
  (ref) => openExternalUrl,
);

/// Returns a browser-safe web destination or `null` for unsupported links.
String? safeExternalWebUrl(String value) {
  if (utf8.encode(value).length > _maxExternalUrlBytes) {
    return null;
  }
  final sanitized = value.replaceAll(_urlControlCharacters, '');
  final uri = Uri.tryParse(sanitized);
  if (uri == null || uri.host.isEmpty) {
    return null;
  }
  final scheme = uri.scheme.toLowerCase();
  return scheme == 'http' || scheme == 'https' ? sanitized : null;
}
