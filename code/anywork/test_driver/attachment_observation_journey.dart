import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'driver_observation.dart';
import 'flutter_driver_session.dart';

/// Deterministic Driver-only attachment observation.
///
/// Usage:
///   dart run test_driver/attachment_observation_journey.dart VM_URL PROJECT_DIR OUTPUT_DIR
///
/// The current app has no Driver-only ClipboardImageReader override. The
/// journey therefore calls the explicit Driver fixture admission hook, which
/// starts admission of the checked-in PNG through the existing controller path
/// without touching the OS clipboard. A second typed Driver-only hook awaits
/// the preview continuation; the journey observes metadata-only admission
/// before it asks for that preview stage.
Future<void> main(List<String> args) async {
  if (args.length != 3) {
    stderr.writeln(
      'usage: attachment_observation_journey.dart VM_URL PROJECT_DIR OUTPUT_DIR',
    );
    exitCode = 64;
    return;
  }

  final output = Directory(args[2]);
  await output.create(recursive: true);
  FlutterDriverSession? driver;
  Object? failure;
  StackTrace? failureStack;
  Object? shutdownFailure;
  try {
    final connected = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
    driver = connected;
    try {
      await _openProject(connected, args[1]);
      final admissionHook = jsonDecode(
        await connected.requestData(
          'driver-fixture-clipboard-image',
          timeout: const Duration(seconds: 60),
        ),
      );
      if (admissionHook is! Map ||
          admissionHook['ok'] != true ||
          admissionHook['stage'] != 'admission-started') {
        throw StateError(
          'Driver fixture admission hook failed: $admissionHook',
        );
      }
      if (admissionHook['pasteSeam'] != 'unavailable' ||
          admissionHook['systemClipboardTouched'] != false ||
          admissionHook['previewPending'] != true) {
        throw StateError(
          'fixture admission hook falsely claimed its stage/seam: '
          '$admissionHook',
        );
      }

      // This is the required metadata-only intermediate observation. A final
      // ready snapshot is not allowed to stand in for it.
      final admitted = await _waitForAttachment(connected, previewReady: false);
      if (admitted.present != true ||
          admitted.attachments.isEmpty ||
          admitted.previewReady != false ||
          admitted.attachments.any(
            (attachment) => attachment.previewReady != false,
          )) {
        throw StateError(
          'attachment admission did not expose metadata-only state: '
          '${admitted.toJson()}',
        );
      }
      final attachment = admitted.attachments.single;
      if (attachment.id.isEmpty || attachment.name.isEmpty) {
        throw StateError('attachment metadata did not expose a stable id/name');
      }
      await _writeJson(output, 'attachment-admission.json', <String, Object?>{
        'hook': {
          'name': admissionHook['hook'],
          'stage': admissionHook['stage'],
          'pasteSeam': admissionHook['pasteSeam'],
          'systemClipboardTouched': admissionHook['systemClipboardTouched'],
        },
        'draft': admitted.toJson(),
      });

      final previewHook = jsonDecode(
        await connected.requestData(
          'driver-fixture-clipboard-image-preview',
          timeout: const Duration(seconds: 60),
        ),
      );
      if (previewHook is! Map ||
          previewHook['ok'] != true ||
          previewHook['stage'] != 'preview-complete') {
        throw StateError('Driver fixture preview hook failed: $previewHook');
      }
      final ready = await _waitForAttachment(connected, previewReady: true);
      if (ready.attachments.length != 1 ||
          ready.attachments.single.id != attachment.id ||
          ready.attachments.single.previewReady != true) {
        throw StateError(
          'attachment preview did not become ready for the admitted id: '
          '${ready.toJson()}',
        );
      }
      await _writeJson(output, 'attachment-draft.json', <String, Object?>{
        'hook': {
          'name': previewHook['hook'],
          'stage': previewHook['stage'],
          'pasteSeam': previewHook['pasteSeam'],
          'systemClipboardTouched': previewHook['systemClipboardTouched'],
        },
        'draft': ready.toJson(),
      });

      await connected.tap(
        find.byValueKey('attachment-remove-${attachment.id}'),
      );
      final removed = await _waitForAttachment(connected, requireAbsent: true);
      if (removed.present != false || removed.attachments.isNotEmpty) {
        throw StateError('attachment draft was not removed canonically');
      }
      await _writeJson(output, 'attachment-removed.json', <String, Object?>{
        'present': removed.present,
        'previewReady': removed.previewReady,
        'attachments': const <Object?>[],
      });

      stdout.writeln(
        'Attachment observation completed; real paste seam remains pending.',
      );
    } catch (error, stackTrace) {
      failure = error;
      failureStack = stackTrace;
    }

    // Always attempt the typed native shutdown before closing the Driver
    // connection, including assertion failures. Closing the connection alone
    // is not a substitute for native GUI/process reclamation.
    try {
      await _requestNativeShutdown(connected);
    } catch (error) {
      shutdownFailure = error;
    }
  } catch (error, stackTrace) {
    // A connection failure still propagates as the primary failure. There is
    // no Driver session to issue a typed native shutdown against on this path.
    failure ??= error;
    failureStack ??= stackTrace;
  } finally {
    final connected = driver;
    try {
      if (connected != null) {
        await connected.close().timeout(const Duration(seconds: 5));
      }
    } catch (_) {
      // Preserve the journey or typed shutdown failure; close is only the
      // observation-connection cleanup after native shutdown was attempted.
    }
  }

  if (failure != null) {
    if (shutdownFailure != null) {
      stderr.writeln('native GUI shutdown also failed: $shutdownFailure');
    }
    Error.throwWithStackTrace(failure, failureStack!);
  }
  if (shutdownFailure != null) {
    throw StateError('native GUI shutdown failed: $shutdownFailure');
  }
}

Future<DriverComposerAttachmentObservation> _waitForAttachment(
  FlutterDriverSession driver, {
  bool? previewReady,
  bool requireAbsent = false,
}) async {
  final deadline = DateTime.now().add(const Duration(seconds: 45));
  DriverComposerAttachmentObservation? last;
  while (DateTime.now().isBefore(deadline)) {
    final snapshot = await driver.readSnapshot();
    final current = driverComposerAttachments(snapshot, target: 'newThread');
    last = current;
    if (!requireAbsent &&
        current.present == true &&
        current.attachments.isNotEmpty &&
        (previewReady == null || current.previewReady == previewReady)) {
      return current;
    }
    if (requireAbsent &&
        current.present == false &&
        current.attachments.isEmpty) {
      return current;
    }
    await Future<void>.delayed(const Duration(milliseconds: 100));
  }
  throw StateError(
    'timed out waiting for attachment state: ${last?.toJson() ?? 'unknown'}',
  );
}

Future<void> _requestNativeShutdown(FlutterDriverSession driver) async {
  final shutdown = jsonDecode(
    await driver
        .requestData('shutdown', timeout: const Duration(seconds: 60))
        .timeout(const Duration(seconds: 65)),
  );
  if (shutdown is! Map || shutdown['shutdown'] != 'completed') {
    throw StateError('native GUI shutdown did not complete: $shutdown');
  }
}

Future<void> _openProject(FlutterDriverSession driver, String project) async {
  await driver.waitFor(
    find.byValueKey('sidebar-open-project'),
    timeout: const Duration(seconds: 60),
  );
  await driver.tap(find.byValueKey('sidebar-open-project'));
  await driver.tap(find.byValueKey('add-project-local'));
  await driver.waitFor(find.byValueKey('add-project-continue-ready'));
  await driver.tap(find.byValueKey('add-project-continue-ready'));
  await driver.waitFor(find.byValueKey('project-path-input'));
  await driver.tap(find.byValueKey('project-path-input'));
  await driver.enterText(project);
  await driver.waitFor(find.byValueKey('project-path-submit'));
  await driver.tap(find.byValueKey('project-path-submit'));
  await driver.waitFor(
    find.byValueKey('composer-input'),
    timeout: const Duration(seconds: 60),
  );
}

Future<void> _writeJson(
  Directory output,
  String name,
  Map<String, Object?> value,
) => File('${output.path}/$name')
    .writeAsString('${const JsonEncoder.withIndent('  ').convert(value)}\n');
