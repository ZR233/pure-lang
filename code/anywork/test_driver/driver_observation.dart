import 'dart:convert';

/// The provenance of runtime usage fields in a Driver observation.
///
/// `unknown` is intentional: a missing live/authoritative usage observation is
/// not represented by zero-valued counters.
enum DriverRuntimeUsageState { live, authoritative, unknown }

class DriverRuntimeObservation {
  const DriverRuntimeObservation({
    required this.usageState,
    this.turnId,
    this.attemptId,
    this.observationSequence,
    this.revision,
    this.contextTokens,
    this.contextWindow,
    this.totalTokens,
    this.inputTokens,
    this.outputTokens,
    this.reasoningTokens,
    this.liveOutputTokens,
    this.turnOutputTokens,
    this.decodeMillis,
    this.tokensPerSecond,
  });

  const DriverRuntimeObservation.unknown()
    : this(usageState: DriverRuntimeUsageState.unknown);

  final DriverRuntimeUsageState usageState;
  final String? turnId;
  final String? attemptId;
  final int? observationSequence;
  final int? revision;
  final int? contextTokens;
  final int? contextWindow;
  final int? totalTokens;
  final int? inputTokens;
  final int? outputTokens;
  final int? reasoningTokens;
  final int? liveOutputTokens;
  final int? turnOutputTokens;
  final int? decodeMillis;
  final double? tokensPerSecond;

  bool get isLive => usageState == DriverRuntimeUsageState.live;

  bool get isAuthoritative =>
      usageState == DriverRuntimeUsageState.authoritative;

  Map<String, Object?> toJson() => {
    'usageState': usageState.name,
    'turnId': turnId,
    'attemptId': attemptId,
    'observationSequence': observationSequence,
    'revision': revision,
    'contextTokens': contextTokens,
    'contextWindow': contextWindow,
    'totalTokens': totalTokens,
    'inputTokens': inputTokens,
    'outputTokens': outputTokens,
    'reasoningTokens': reasoningTokens,
    'liveOutputTokens': liveOutputTokens,
    'turnOutputTokens': turnOutputTokens,
    'decodeMillis': decodeMillis,
    'tokensPerSecond': tokensPerSecond,
  };
}

DriverRuntimeObservation driverRuntimeObservation(
  Map<String, dynamic> snapshot,
) {
  final diagnostics = snapshot['driverDiagnostics'];
  final runtime = diagnostics is Map ? diagnostics['runtime'] : null;
  if (runtime is! Map) return const DriverRuntimeObservation.unknown();
  final parsedUsageState = _runtimeUsageState(runtime['usageState']);
  final turnId = _string(runtime['turnId']);
  final attemptId = _string(runtime['attemptId']);
  final observationSequence = _int(runtime['observationSequence']);
  final hasIdentity =
      turnId != null && attemptId != null && observationSequence != null;
  final metricsAvailable =
      (parsedUsageState == DriverRuntimeUsageState.live ||
          parsedUsageState == DriverRuntimeUsageState.authoritative) &&
      hasIdentity;
  final usageState = metricsAvailable
      ? parsedUsageState
      : DriverRuntimeUsageState.unknown;
  return DriverRuntimeObservation(
    usageState: usageState,
    turnId: metricsAvailable ? turnId : null,
    attemptId: metricsAvailable ? attemptId : null,
    observationSequence: metricsAvailable ? observationSequence : null,
    revision: _int(runtime['revision']),
    contextTokens: metricsAvailable ? _int(runtime['contextTokens']) : null,
    contextWindow: metricsAvailable ? _int(runtime['contextWindow']) : null,
    totalTokens: metricsAvailable ? _int(runtime['totalTokens']) : null,
    inputTokens: metricsAvailable ? _int(runtime['inputTokens']) : null,
    outputTokens: metricsAvailable ? _int(runtime['outputTokens']) : null,
    reasoningTokens: metricsAvailable ? _int(runtime['reasoningTokens']) : null,
    liveOutputTokens: metricsAvailable
        ? _int(runtime['liveOutputTokens'])
        : null,
    turnOutputTokens: metricsAvailable
        ? _int(runtime['turnOutputTokens'])
        : null,
    decodeMillis: metricsAvailable ? _int(runtime['decodeMillis']) : null,
    tokensPerSecond: metricsAvailable
        ? _double(runtime['tokensPerSecond'])
        : null,
  );
}

DriverRuntimeUsageState _runtimeUsageState(Object? value) => switch (value) {
  'live' => DriverRuntimeUsageState.live,
  'authoritative' => DriverRuntimeUsageState.authoritative,
  _ => DriverRuntimeUsageState.unknown,
};

String? _string(Object? value) =>
    value is String && value.isNotEmpty ? value : null;

int? _int(Object? value) => value is num ? value.toInt() : null;

double? _double(Object? value) => value is num ? value.toDouble() : null;

class DriverAttachmentObservation {
  const DriverAttachmentObservation({
    required this.id,
    required this.name,
    required this.modality,
    required this.previewReady,
    this.byteSize,
  });

  final String id;
  final String name;
  final String? modality;
  final bool? previewReady;
  final int? byteSize;

  Map<String, Object?> toJson() => {
    'id': id,
    'name': name,
    'modality': modality,
    'previewReady': previewReady,
    'byteSize': byteSize,
  };
}

class DriverComposerAttachmentObservation {
  const DriverComposerAttachmentObservation({
    required this.present,
    required this.previewReady,
    required this.attachments,
    this.attachmentGeneration,
    this.submissionPending,
    this.failed,
  });

  final bool? present;
  final bool? previewReady;
  final List<DriverAttachmentObservation> attachments;
  final int? attachmentGeneration;
  final bool? submissionPending;
  final bool? failed;

  Map<String, Object?> toJson() => {
    'present': present,
    'previewReady': previewReady,
    'attachmentGeneration': attachmentGeneration,
    'submissionPending': submissionPending,
    'failed': failed,
    'attachments': [for (final attachment in attachments) attachment.toJson()],
  };
}

DriverComposerAttachmentObservation driverComposerAttachments(
  Map<String, dynamic> snapshot, {
  String target = 'thread',
}) {
  final diagnostics = snapshot['driverDiagnostics'];
  final composer = diagnostics is Map ? diagnostics['composer'] : null;
  final targetValue = composer is Map ? composer[target] : null;
  if (targetValue is! Map) {
    return const DriverComposerAttachmentObservation(
      present: null,
      previewReady: null,
      attachments: [],
    );
  }
  final attachments = <DriverAttachmentObservation>[];
  final rawAttachments = targetValue['attachments'];
  if (rawAttachments is List) {
    for (final raw in rawAttachments) {
      if (raw is! Map || raw['id'] is! String) continue;
      attachments.add(
        DriverAttachmentObservation(
          id: raw['id'] as String,
          name: raw['name'] is String ? raw['name'] as String : '',
          modality: raw['modality'] as String?,
          previewReady: raw['previewReady'] as bool?,
          byteSize: _int(raw['byteSize']),
        ),
      );
    }
  }
  final previewReady = targetValue['previewReady'];
  return DriverComposerAttachmentObservation(
    present: targetValue['present'] as bool? ?? attachments.isNotEmpty,
    previewReady: previewReady is bool
        ? previewReady
        : attachments.isNotEmpty &&
              attachments.every(
                (attachment) => attachment.previewReady == true,
              ),
    attachments: attachments,
    attachmentGeneration: _int(targetValue['attachmentGeneration']),
    submissionPending: targetValue['submissionPending'] as bool?,
    failed: targetValue['failed'] as bool?,
  );
}

List<Map<String, dynamic>> driverProviderListObservation(
  Map<String, dynamic> snapshot,
) {
  final providers = _providerObservation(snapshot)?['list'];
  if (providers is! List) return const [];
  return [
    for (final provider in providers)
      if (provider is Map) provider.cast<String, dynamic>(),
  ];
}

List<Map<String, dynamic>> driverProviderDetailsObservation(
  Map<String, dynamic> snapshot,
) {
  final providers = _providerObservation(snapshot)?['details'];
  if (providers is! List) return const [];
  return [
    for (final provider in providers)
      if (provider is Map) provider.cast<String, dynamic>(),
  ];
}

Map<String, dynamic>? _providerObservation(Map<String, dynamic> snapshot) {
  final diagnostics = snapshot['driverDiagnostics'];
  final providers = diagnostics is Map ? diagnostics['providers'] : null;
  return providers is Map ? providers.cast<String, dynamic>() : null;
}

/// Verifies the list/detail split without treating a detail model slug as a
/// list label. The list projection is deliberately metadata-only; model slugs
/// remain observable only in the detail projection.
void verifyProviderListRedacted(
  Map<String, dynamic> snapshot, {
  required String fixtureModelSlug,
  String? providerId,
}) {
  final list = driverProviderListObservation(snapshot);
  if (list.isEmpty) {
    throw StateError('provider list observation is unavailable');
  }
  final encodedList = jsonEncode(list);
  if (encodedList.contains(fixtureModelSlug)) {
    throw StateError(
      'provider list observation leaked fixture model slug $fixtureModelSlug',
    );
  }
  if (RegExp(r'\+\d+').hasMatch(encodedList)) {
    throw StateError('provider list observation leaked a +N model label');
  }

  final details = driverProviderDetailsObservation(snapshot);
  final matchingProvider = providerId == null
      ? details
      : details.where((provider) => provider['id'] == providerId).toList();
  final hasModelDetail = matchingProvider.any((provider) {
    final models = provider['models'];
    return models is List &&
        models.any(
          (model) => model is Map && model['slug'] == fixtureModelSlug,
        );
  });
  if (!hasModelDetail) {
    throw StateError(
      'provider detail observation omitted fixture model $fixtureModelSlug',
    );
  }
}

class DriverPacedRuntimeEvidence {
  factory DriverPacedRuntimeEvidence({
    required Iterable<DriverRuntimeObservation> samples,
    String? targetTurnId,
    String? targetAttemptId,
  }) {
    final allSamples = List<DriverRuntimeObservation>.unmodifiable(samples);
    final selection = _selectRuntimeGroup(
      allSamples,
      targetTurnId: targetTurnId,
      targetAttemptId: targetAttemptId,
    );
    return DriverPacedRuntimeEvidence._(
      samples: selection.samples,
      targetTurnId: selection.turnId,
      targetAttemptId: selection.attemptId,
      targetSelection: selection.kind,
    );
  }

  const DriverPacedRuntimeEvidence._({
    required this.samples,
    required this.targetTurnId,
    required this.targetAttemptId,
    required this.targetSelection,
  });

  /// Samples are filtered to exactly one identity, never a largest-live group
  /// combined with a terminal sample from another turn/attempt.
  final List<DriverRuntimeObservation> samples;
  final String? targetTurnId;
  final String? targetAttemptId;
  final String targetSelection;

  List<DriverRuntimeObservation> get liveSamples => [
    for (final sample in samples)
      if (sample.isLive) sample,
  ];

  List<DriverRuntimeObservation> get authoritativeSamples => [
    for (final sample in samples)
      if (sample.isAuthoritative) sample,
  ];

  List<DriverRuntimeObservation> get unknownSamples => [
    for (final sample in samples)
      if (sample.usageState == DriverRuntimeUsageState.unknown) sample,
  ];

  List<DriverRuntimeObservation> get terminalSamples => [
    for (final sample in samples)
      if (sample.isAuthoritative ||
          sample.usageState == DriverRuntimeUsageState.unknown)
        sample,
  ];

  /// Every live sample in the selected identity must advance the sequence.
  /// One later value is not sufficient: a duplicate or out-of-order sample
  /// invalidates the paced evidence.
  bool get liveSequenceStrictlyIncreasing {
    final live = liveSamples;
    if (live.length < 2 ||
        live.any((sample) => sample.observationSequence == null)) {
      return false;
    }
    for (var index = 1; index < live.length; index += 1) {
      if (live[index].observationSequence! <=
          live[index - 1].observationSequence!) {
        return false;
      }
    }
    return true;
  }

  /// Backward-compatible name retained for existing journey JSON consumers;
  /// it now means the full strict ordering proof above, not merely one `>`.
  bool get liveSequenceAdvanced => liveSequenceStrictlyIncreasing;

  bool _terminalAfterLive({bool authoritativeOnly = false}) {
    final live = liveSamples;
    if (live.isEmpty) return false;
    final terminalIndexes = <int>[];
    for (var index = 0; index < samples.length; index += 1) {
      final sample = samples[index];
      final isTerminal = authoritativeOnly
          ? sample.isAuthoritative
          : sample.isAuthoritative ||
                sample.usageState == DriverRuntimeUsageState.unknown;
      if (isTerminal) terminalIndexes.add(index);
    }
    if (terminalIndexes.isEmpty) return false;
    final lastLiveIndex = samples.lastIndexWhere((sample) => sample.isLive);
    if (lastLiveIndex < 0 ||
        terminalIndexes.any((index) => index <= lastLiveIndex)) {
      return false;
    }
    final lastLiveSequence = live.last.observationSequence;
    if (lastLiveSequence == null) return false;
    return terminalIndexes.every((index) {
      final sequence = samples[index].observationSequence;
      return sequence != null && sequence > lastLiveSequence;
    });
  }

  /// All selected authoritative/unknown samples must follow all selected live
  /// samples and advance the same identity's observation sequence.
  bool get terminalAfterLive => _terminalAfterLive();

  bool get authoritativeAfterLive =>
      _terminalAfterLive(authoritativeOnly: true);

  Map<String, Object?> toJson() => {
    'targetTurnId': targetTurnId,
    'targetAttemptId': targetAttemptId,
    'targetSelection': targetSelection,
    'sampleCount': samples.length,
    'liveSampleCount': liveSamples.length,
    'authoritativeSampleCount': authoritativeSamples.length,
    'unknownSampleCount': unknownSamples.length,
    'terminalSampleCount': terminalSamples.length,
    'liveSequenceStrictlyIncreasing': liveSequenceStrictlyIncreasing,
    'liveSequenceAdvanced': liveSequenceAdvanced,
    'terminalAfterLive': terminalAfterLive,
    'authoritativeAfterLive': authoritativeAfterLive,
    // This endpoint is not a runtime event stream. Snapshot polling can
    // describe ordering, but cannot prove event delivery freshness.
    'pacedEventVerdict': 'pending',
    'eventEndpoint': 'unsupported',
    'pollingSamplesAreReadinessOnly': true,
    'samples': [for (final sample in samples) sample.toJson()],
  };
}

class _RuntimeGroupSelection {
  const _RuntimeGroupSelection({
    required this.samples,
    required this.turnId,
    required this.attemptId,
    required this.kind,
  });

  final List<DriverRuntimeObservation> samples;
  final String? turnId;
  final String? attemptId;
  final String kind;
}

_RuntimeGroupSelection _selectRuntimeGroup(
  List<DriverRuntimeObservation> samples, {
  String? targetTurnId,
  String? targetAttemptId,
}) {
  List<DriverRuntimeObservation> forIdentity(String turnId, String attemptId) =>
      samples
          .where(
            (sample) =>
                sample.turnId == turnId && sample.attemptId == attemptId,
          )
          .toList(growable: false);

  if (targetTurnId != null && targetAttemptId != null) {
    final selected = forIdentity(targetTurnId, targetAttemptId);
    return _RuntimeGroupSelection(
      samples: List.unmodifiable(selected),
      turnId: targetTurnId,
      attemptId: targetAttemptId,
      kind: selected.isEmpty ? 'requested-empty' : 'requested',
    );
  }

  final groups = <String, List<DriverRuntimeObservation>>{};
  for (final sample in samples) {
    final turnId = sample.turnId;
    final attemptId = sample.attemptId;
    if (turnId == null || attemptId == null) continue;
    groups.putIfAbsent('$turnId\u0000$attemptId', () => []).add(sample);
  }

  List<DriverRuntimeObservation>? selected;
  String? selectedKey;
  // Prefer the first complete identity (live plus authoritative/unknown),
  // otherwise use the first identity with live samples. Map insertion order is
  // the Driver observation order, so this is deterministic and never picks a
  // synthetic "largest" group.
  for (final entry in groups.entries) {
    final group = entry.value;
    final hasLive = group.any((sample) => sample.isLive);
    final hasTerminal = group.any(
      (sample) =>
          sample.isAuthoritative ||
          sample.usageState == DriverRuntimeUsageState.unknown,
    );
    if (hasLive && hasTerminal) {
      selected = group;
      selectedKey = entry.key;
      break;
    }
  }
  if (selected == null) {
    for (final entry in groups.entries) {
      if (entry.value.any((sample) => sample.isLive)) {
        selected = entry.value;
        selectedKey = entry.key;
        break;
      }
    }
  }
  selected ??= groups.values.firstOrNull;
  selectedKey ??= groups.keys.firstOrNull;
  if (selected == null || selectedKey == null) {
    return const _RuntimeGroupSelection(
      samples: [],
      turnId: null,
      attemptId: null,
      kind: 'none',
    );
  }
  final separator = selectedKey.indexOf('\u0000');
  return _RuntimeGroupSelection(
    samples: List.unmodifiable(selected),
    turnId: selectedKey.substring(0, separator),
    attemptId: selectedKey.substring(separator + 1),
    kind: 'first-complete-or-live-group',
  );
}

DriverPacedRuntimeEvidence evaluatePacedRuntime(
  Iterable<DriverRuntimeObservation> samples, {
  String? targetTurnId,
  String? targetAttemptId,
}) => DriverPacedRuntimeEvidence(
  samples: samples,
  targetTurnId: targetTurnId,
  targetAttemptId: targetAttemptId,
);
