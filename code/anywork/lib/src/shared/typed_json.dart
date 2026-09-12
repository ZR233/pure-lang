import 'dart:convert';

/// Decodes a tool argument payload as a tolerant string-keyed JSON object.
///
/// Tool arguments cross a provider boundary as text and may be malformed while
/// streaming. Display projections use one policy everywhere: non-objects,
/// empty input, invalid JSON, and empty string values are treated as absent.
Map<String, Object?> decodeJsonObject(String value) {
  if (value.trim().isEmpty) {
    return const {};
  }
  try {
    return jsonObject(jsonDecode(value));
  } catch (_) {
    return const {};
  }
}

Map<String, Object?> jsonObject(Object? value) {
  if (value is Map<String, Object?>) {
    return value;
  }
  if (value is Map) {
    return value.map((key, value) => MapEntry(key.toString(), value));
  }
  return const {};
}

String? jsonStringValue(Object? value) {
  if (value == null) {
    return null;
  }
  final text = value.toString().trim();
  return text.isEmpty ? null : text;
}
