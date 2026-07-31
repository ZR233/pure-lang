import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart' show visibleForTesting;

import '../../domain/models/studio_models.dart';
import '../../rust/api/studio.dart' as frb;
import '../../rust/frb_generated.dart';

part 'studio_bridge_event.dart';
part 'studio_session_stream.dart';
part 'studio_api_contract.dart';
part 'studio_frb_converters.dart';
part 'studio_state_converters.dart';
part 'studio_config_converters.dart';
part 'studio_provider_catalog_converters.dart';
part 'studio_demo_api.dart';
part 'studio_demo_settings.dart';

bool _isIgnoredTimelinePartType(TimelinePartType type) {
  return isInternalTimelinePartType(type);
}

StudioMode _compileMode(Object? value) {
  return switch (_string(value)) {
    'simple' => StudioMode.simple,
    'task' => StudioMode.task,
    final label => throw FormatException('Unknown Studio mode: $label'),
  };
}

String _compileModeLabel(StudioMode mode) {
  return switch (mode) {
    StudioMode.simple => 'simple',
    StudioMode.task => 'task',
  };
}

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

Map<String, Object?> _decodeJson(String json) {
  final value = jsonDecode(json);
  return _map(value);
}

Map<String, Object?> _map(Object? value) {
  if (value is Map<String, Object?>) {
    return value;
  }
  if (value is Map) {
    return value.map((key, value) => MapEntry(key.toString(), value));
  }
  return const {};
}

List<Object?> _list(Object? value) {
  if (value is List) {
    return value.cast<Object?>();
  }
  return const [];
}

List<String> _stringList(Object? value) {
  return _list(value).map(_string).where((item) => item.isNotEmpty).toList();
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

String? _nullableString(Object? value) {
  final string = _string(value);
  return string.isEmpty ? null : string;
}

int _int(Object? value, {int fallback = 0}) {
  if (value is int) {
    return value;
  }
  if (value is BigInt) {
    return value.toInt();
  }
  return int.tryParse(_string(value)) ?? fallback;
}

int? _nullableInt(Object? value) {
  final string = _string(value);
  if (string.isEmpty) {
    return null;
  }
  return _int(value);
}

double? _nullableDouble(Object? value) {
  if (value is num) {
    return value.toDouble();
  }
  final string = _string(value);
  return string.isEmpty ? null : double.tryParse(string);
}

bool _bool(Object? value) {
  if (value is bool) {
    return value;
  }
  return _string(value) == 'true';
}

bool _boolWithDefault(Object? value, bool fallback) {
  if (value == null) {
    return fallback;
  }
  return _bool(value);
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

SessionRuntimeView _emptyRuntimeView() {
  return const SessionRuntimeView(
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
