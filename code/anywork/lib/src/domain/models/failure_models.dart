enum StudioFailureCode {
  notInitialized,
  runtimeStopped,
  invalidArgument,
  notFound,
  busy,
  conflict,
  staleRevision,
  permissionDenied,
  cancelled,
  unavailable,
  protocol,
  storage,
  update,
  internal,
}

class StudioFailure implements Exception {
  const StudioFailure({
    required this.code,
    required this.message,
    required this.retryable,
    required this.correlationId,
    this.detailsJson,
  });

  final StudioFailureCode code;
  final String message;
  final bool retryable;
  final String correlationId;
  final String? detailsJson;

  @override
  String toString() => '$message (correlation id: $correlationId)';
}
