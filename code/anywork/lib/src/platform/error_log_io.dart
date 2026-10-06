import 'dart:io';
import 'dart:math';

import 'package:flutter/foundation.dart' show debugPrint;

final Random _correlationRandom = Random();
int _correlationCounter = 0;

/// 生成非空的本地 correlation id，把 Dart 侧诊断与同步日志 / 桥报告关联起来。
///
/// 只承载诊断身份，不含凭据或正文；不引入额外依赖。
String newStudioCorrelationId() {
  final micros = DateTime.now().microsecondsSinceEpoch.toRadixString(16);
  final salt = _correlationRandom.nextInt(0x7fffffff).toRadixString(16);
  final seq = (_correlationCounter++).toRadixString(16);
  return 'corr-$micros-$salt-$seq';
}

/// 规范诊断日志目录（与 Dart 错误日志同一路径，不造第二事实源）。
///
/// 返回 `null` 表示当前环境无法确定目录；调用方据此跳过 native 诊断配置。
String? studioLogDirectory() {
  try {
    return _logsDirectory().path;
  } on Object {
    return null;
  }
}

Directory _logsDirectory() {
  final root = _studioRoot();
  return Directory('${root.path}${Platform.pathSeparator}logs')
    ..createSync(recursive: true);
}

/// 规范诊断根目录，必须与 Rust `diagnostics_root()` 完全一致（同一事实源）：
/// `ANYWORK_HOME` -> `<home>/studio`；否则 `LOCALAPPDATA` -> `<localappdata>/anywork`；
/// 否则 `USERPROFILE|HOME` -> `<home>/.anywork/studio`；否则 `./anywork-diagnostics`。
///
/// 隔离验收通过 `ANYWORK_HOME` 指定目录，绝不再回落到用户真实主目录。
Directory _studioRoot() {
  final anyworkHome = _env('ANYWORK_HOME');
  if (anyworkHome != null) {
    return Directory('$anyworkHome${Platform.pathSeparator}studio');
  }
  final localAppData = _env('LOCALAPPDATA');
  if (localAppData != null) {
    return Directory('$localAppData${Platform.pathSeparator}anywork');
  }
  final home = _env('USERPROFILE') ?? _env('HOME');
  if (home != null) {
    return Directory(
      '$home${Platform.pathSeparator}.anywork${Platform.pathSeparator}studio',
    );
  }
  return Directory('.${Platform.pathSeparator}anywork-diagnostics');
}

String? _env(String name) {
  final value = Platform.environment[name];
  return value == null || value.isEmpty ? null : value;
}

/// 记录一次 Dart 错误：主日志目录 -> 系统 temp 文件 -> stderr/debugPrint 三级兜底。
///
/// 绝不静默丢弃错误与堆栈；任一写盘失败都继续下一级，绝不抛出。[stage] 与
/// [correlationId] 是与桥/native 诊断对齐的允许字段，正文/凭据不会写进这里。
void recordDartError(
  Object error,
  StackTrace? stack, {
  String? stage,
  String? correlationId,
  int? elapsedMs,
}) {
  final now = DateTime.now();
  final date =
      '${now.year.toString().padLeft(4, '0')}-'
      '${now.month.toString().padLeft(2, '0')}-'
      '${now.day.toString().padLeft(2, '0')}';
  final header = <String>[
    now.toUtc().toIso8601String(),
    if (stage != null) 'stage=$stage',
    if (correlationId != null) 'correlation=$correlationId',
    if (elapsedMs != null) 'elapsed_ms=$elapsedMs',
  ].join(' ');
  final entry =
      '$header $error\n'
      '${stack ?? StackTrace.current}\n\n';
  // 1) canonical 日志目录。
  try {
    final logs = _logsDirectory();
    File('${logs.path}${Platform.pathSeparator}dart-error-$date.log')
        .writeAsStringSync(entry, mode: FileMode.append, flush: true);
    return;
  } on Object {
    // 继续下级兜底。
  }
  // 2) 系统 temp 文件。
  try {
    final temp = File(
      '${Directory.systemTemp.path}${Platform.pathSeparator}'
      'anywork-dart-error-$date.log',
    );
    temp.writeAsStringSync(entry, mode: FileMode.append, flush: true);
    return;
  } on Object {
    // 继续 stderr。
  }
  // 3) stderr（Windows 无控制台时另经 debugPrint 输出）。
  try {
    stderr.writeln('anywork dart error: $error');
  } on Object {
    // 兜底输出也不允许抛出。
  }
  debugPrint(
    'anywork_dart_error stage=${stage ?? '-'} correlation=${correlationId ?? '-'}'
    ' elapsed=${elapsedMs ?? '-'} $error\n${stack ?? StackTrace.current}',
  );
}
