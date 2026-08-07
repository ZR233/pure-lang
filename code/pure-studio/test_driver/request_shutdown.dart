import 'dart:async';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

Future<void> main(List<String> arguments) async {
  if (arguments.length != 1) {
    stderr.writeln(
      'usage: dart run test_driver/request_shutdown.dart <vm-url>',
    );
    exitCode = 64;
    return;
  }

  final driver = await FlutterDriver.connect(
    dartVmServiceUrl: arguments.single,
    printCommunication: false,
    logCommunicationToFile: false,
  ).timeout(const Duration(seconds: 15));
  try {
    final response = await driver
        .requestData('shutdown')
        .timeout(const Duration(seconds: 30));
    stdout.writeln(response);
  } finally {
    await driver.close().timeout(const Duration(seconds: 10));
  }
}
