import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart' show visibleForTesting;

import '../../domain/models/studio_models.dart';
import '../../rust/api/studio.dart' as frb;
import '../../rust/frb_generated.dart';
import '../../shared/studio_driver_state.dart';

part 'studio_bridge_event.dart';
part 'studio_thread_stream.dart';
part 'studio_api_contract.dart';
part 'studio_frb_converters.dart';
part 'studio_state_converters.dart';
part 'studio_settings_converters.dart';
part 'studio_provider_catalog_converters.dart';
part 'studio_demo_api.dart';
part 'studio_demo_settings.dart';

PermissionMode _permissionMode(Object? value) {
  return switch (_string(value)) {
    'request-approval' => PermissionMode.requestApproval,
    'auto-review' => PermissionMode.autoReview,
    'full-access' => PermissionMode.fullAccess,
    final label => throw FormatException('Unknown permission mode: $label'),
  };
}

String _permissionModeLabel(PermissionMode mode) {
  return switch (mode) {
    PermissionMode.requestApproval => 'request-approval',
    PermissionMode.autoReview => 'auto-review',
    PermissionMode.fullAccess => 'full-access',
  };
}

String _compactAmount(String value) {
  final parsed = double.tryParse(value);
  if (parsed == null) {
    return value;
  }
  final fixed = parsed.toStringAsFixed(4);
  return fixed.replaceFirst(RegExp(r'\.?0+$'), '');
}

String _string(Object? value, {String fallback = ''}) {
  if (value == null) {
    return fallback;
  }
  if (value is String) {
    return value.isEmpty ? fallback : value;
  }
  return value.toString();
}

int _frbInt(Object value) {
  if (value is BigInt) {
    return value.toInt();
  }
  if (value is int) {
    return value;
  }
  if (value is num) {
    return value.toInt();
  }
  return int.parse(value.toString());
}

int? _frbNullableInt(Object? value) {
  if (value == null) {
    return null;
  }
  return _frbInt(value);
}

DateTime _dateFromUnix(Object seconds) {
  return DateTime.fromMillisecondsSinceEpoch(_frbInt(seconds) * 1000);
}

ThreadRuntimeView _emptyRuntimeView() {
  return const ThreadRuntimeView(
    model: '',
    contextTokens: 0,
    contextWindow: 0,
    totalTokens: 0,
    costLabel: '',
    activeSkills: [],
    activeMcpServers: [],
    activeLspServers: [],
    agentCount: 0,
  );
}

String _jsonText(Object? value) {
  if (value == null) {
    return '';
  }
  if (value is String) {
    return value;
  }
  return const JsonEncoder.withIndent('  ').convert(value);
}
