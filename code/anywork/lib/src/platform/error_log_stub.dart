import 'dart:developer' as developer;

int _correlationCounter = 0;

/// 非空本地 correlation id（Web 无文件日志，仍用于关联诊断字段）。
String newStudioCorrelationId() {
  final micros = DateTime.now().microsecondsSinceEpoch.toRadixString(16);
  return 'corr-$micros-${(_correlationCounter++).toRadixString(16)}';
}

/// Web/非 IO 环境没有本地日志目录；native 诊断配置据此跳过。
String? studioLogDirectory() => null;

void recordDartError(
  Object error,
  StackTrace? stack, {
  String? stage,
  String? correlationId,
  int? elapsedMs,
}) {
  developer.log(
    'Unhandled anywork Web error stage=${stage ?? '-'} '
    'correlation=${correlationId ?? '-'} elapsed=${elapsedMs ?? '-'}',
    name: 'anywork',
    error: error,
    stackTrace: stack ?? StackTrace.current,
  );
}
