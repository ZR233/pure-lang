import 'dart:async';
import 'dart:convert';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import '../../domain/models/studio_models.dart';
import '../../rust/api/studio.dart' as frb;
import '../../rust/frb_generated.dart';

part 'studio_bridge_event.dart';
part 'studio_api_contract.dart';
part 'studio_frb_converters.dart';
part 'studio_legacy_json_converters.dart';
part 'studio_state_converters.dart';
part 'studio_config_converters.dart';
part 'studio_demo_api.dart';
part 'studio_demo_settings.dart';

bool _isIgnoredTimelinePartType(Object? value) {
  return isInternalTimelinePartType(_partType(value));
}

CompileMode _compileMode(Object? value) {
  return _string(value) == 'task' ? CompileMode.task : CompileMode.simple;
}

String _compileModeLabel(CompileMode mode) {
  return switch (mode) {
    CompileMode.simple => 'simple',
    CompileMode.task => 'task',
  };
}

PermissionMode _permissionMode(Object? value) {
  return switch (_string(value)) {
    'autoReview' || 'auto-review' => PermissionMode.autoReview,
    'fullAccess' || 'full-access' => PermissionMode.fullAccess,
    _ => PermissionMode.requestApproval,
  };
}

TurnPhase _turnPhaseFromStatus(String status) {
  return switch (status) {
    'queued' => TurnPhase.queued,
    'contextLoading' => TurnPhase.contextLoading,
    'waitingForModel' => TurnPhase.waitingForModel,
    'streaming' => TurnPhase.streaming,
    'waitingForInteraction' => TurnPhase.waitingForInteraction,
    'runningTool' => TurnPhase.runningTool,
    'completed' => TurnPhase.completed,
    'failed' => TurnPhase.failed,
    'cancelled' => TurnPhase.cancelled,
    _ => TurnPhase.idle,
  };
}

String _permissionModeLabel(PermissionMode mode) {
  return switch (mode) {
    PermissionMode.requestApproval => 'request-approval',
    PermissionMode.autoReview => 'auto-review',
    PermissionMode.fullAccess => 'full-access',
  };
}

InteractionKind _interactionKind(String value) {
  return switch (value) {
    'userInput' => InteractionKind.userInput,
    'planConfirmation' => InteractionKind.planConfirmation,
    _ => InteractionKind.toolApproval,
  };
}

String _costLabel(Object? value, bool hasUnpricedUsage) {
  final costs = _list(value);
  if (costs.isEmpty) {
    return hasUnpricedUsage ? 'unpriced usage' : '';
  }
  return costs
      .map((cost) {
        final map = _map(cost);
        final amount = _compactAmount(
          _string(map['amount'], fallback: _string(map['value'])),
        );
        final currency = _string(map['currency']);
        return [currency, amount].where((part) => part.isNotEmpty).join(' ');
      })
      .where((label) => label.isNotEmpty)
      .join(', ');
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

Object? _tryDecodeJsonValue(String json) {
  if (json.trim().isEmpty) {
    return null;
  }
  try {
    return jsonDecode(json);
  } catch (_) {
    return null;
  }
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

Object? _firstValue(Map<String, Object?> json, List<String> keys) {
  for (final key in keys) {
    if (json.containsKey(key)) {
      return json[key];
    }
  }
  return null;
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

String _normalizedWireLabel(Object? value) {
  final label = _string(value).trim();
  if (label.isEmpty) {
    return '';
  }
  return label
      .replaceAllMapped(
        RegExp(r'([a-z0-9])([A-Z])'),
        (match) => '${match.group(1)}_${match.group(2)}',
      )
      .replaceAll('-', '_')
      .toLowerCase();
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

double _double(Object? value) => _nullableDouble(value) ?? 0;

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

PlatformInt64 _frbPlatformInt64(int value) => PlatformInt64Util.from(value);

PlatformInt64? _frbNullablePlatformInt64(int? value) {
  if (value == null) {
    return null;
  }
  return _frbPlatformInt64(value);
}

DateTime _dateFromUnix(Object seconds) {
  return DateTime.fromMillisecondsSinceEpoch(_frbInt(seconds) * 1000);
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
