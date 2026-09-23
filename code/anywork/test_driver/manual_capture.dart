import 'dart:convert';
import 'dart:io';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  if (args.length != 2) {
    stderr.writeln(
      'usage: dart run test_driver/manual_capture.dart VM_URL OUTPUT_DIR',
    );
    exitCode = 64;
    return;
  }
  final output = Directory(args[1]);
  final driver = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  try {
    final snapshot = await driver.readSnapshot();
    await File('${output.path}/snapshot.json').writeAsString(
      '${const JsonEncoder.withIndent('  ').convert(_redact(snapshot))}\n',
    );
    await File('${output.path}/screenshot.png')
        .writeAsBytes(await driver.screenshot());
    final shutdown = jsonDecode(
      await driver.requestData(
        'shutdown',
        timeout: const Duration(seconds: 60),
      ),
    );
    if (shutdown is! Map || shutdown['shutdown'] != 'completed') {
      throw StateError('native GUI shutdown did not complete');
    }
    stdout.writeln('Evidence captured; human verdict pending.');
  } finally {
    await driver.close();
  }
}

Object? _redact(Object? value) => switch (value) {
  final Map map => {
    for (var index = 0; index < map.length; index++)
      'field_$index': _redact(map.values.elementAt(index)),
  },
  final List list => list.map(_redact).toList(),
  final num number => number,
  final bool boolean => boolean,
  null => null,
  _ => '[redacted]',
};
