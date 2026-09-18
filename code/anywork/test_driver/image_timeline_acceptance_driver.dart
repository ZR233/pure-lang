// Real Rust/FRB + local or SSH tool-image acceptance. Run via timeline_native_harness.py --image.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

Future<void> main(List<String> args) async {
  if (args.length != 5) {
    throw ArgumentError(
      'Expected VM URL, output, provider URL, workspace, user@host or empty',
    );
  }
  final session = await FlutterDriverSession.connect(vmServiceUrl: args[0]);
  final output = Directory(args[1]);
  Future<Map<String, dynamic>> snapshot() async {
    final state = await session.readSnapshot();
    await File('${output.path}/image-snapshots.jsonl')
        .writeAsString('${jsonEncode(state)}\n', mode: FileMode.append);
    return state;
  }

  Future<Map<String, dynamic>> waitForState(
    bool Function(Map<String, dynamic>) predicate,
  ) async {
    final deadline = DateTime.now().add(const Duration(minutes: 2));
    while (DateTime.now().isBefore(deadline)) {
      final state = await snapshot();
      if (predicate(state)) return state;
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    throw StateError('image acceptance state timed out; see snapshots');
  }

  Future<void> enter(String key, String text) async {
    await session.waitFor(find.byValueKey(key));
    await session.tap(find.byValueKey(key));
    await session.enterText(text);
    await session.waitForNoPendingFrame();
  }

  Future<void> screenshot(String name) async {
    await File('${output.path}/$name.png')
        .writeAsBytes(await session.screenshot());
    await File('${output.path}/$name.tree.txt')
        .writeAsString(await session.renderTree());
  }

  try {
    await session.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    );
    if (args[4].isEmpty) {
      await session.tap(find.byValueKey('sidebar-open-project'));
      await session.tap(find.byValueKey('add-project-local'));
      await session.waitForNoPendingFrame();
      await session.tap(find.byValueKey('add-project-continue'));
      await enter('project-path-input', args[3]);
      await session.sendTextInputAction(TextInputAction.done);
    } else {
      final split = args[4].split('@');
      if (split.length != 2) {
        throw ArgumentError('SSH target must be user@host');
      }
      await session.tap(find.byValueKey('settings-open'));
      await session.tap(find.byValueKey('settings-tab-ssh'));
      await session.tap(find.byValueKey('ssh-add-server'));
      await enter('ssh-server-alias-input', 'image-acceptance');
      await enter('ssh-server-host-input', split[1]);
      await enter('ssh-server-username-input', split[0]);
      await session.tap(find.byValueKey('ssh-server-save'));
      await session.waitForAbsent(find.byValueKey('ssh-server-dialog'));
      String? serverId;
      final saveDeadline = DateTime.now().add(const Duration(seconds: 30));
      while (DateTime.now().isBefore(saveDeadline)) {
        final reply = jsonDecode(
          await session.requestData('ssh-server-alias:image-acceptance'),
        ) as Map;
        if (reply['alias'] case final String id) {
          serverId = id;
          break;
        }
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      if (serverId == null) throw StateError('SSH server was not saved');
      await session.tap(find.byValueKey('ssh-test-$serverId'));
      await session.waitFor(
        find.byValueKey('ssh-ready-$serverId'),
        timeout: const Duration(minutes: 3),
      );
      await session.tap(find.byValueKey('ssh-open-$serverId'));
      await session.waitFor(find.byValueKey('ssh-directory-dialog'));
      final browseDeadline = DateTime.now().add(const Duration(minutes: 2));
      var browseReady = false;
      while (DateTime.now().isBefore(browseDeadline)) {
        final tree = await session.renderTree();
        if (tree.contains('ssh-directory-error')) {
          throw StateError('initial SSH directory browse failed: $tree');
        }
        if (tree.contains('ssh-directory-list') ||
            tree.contains('ssh-directory-empty')) {
          browseReady = true;
          break;
        }
        await Future<void>.delayed(const Duration(milliseconds: 100));
      }
      if (!browseReady) throw StateError('initial SSH browse timed out');
      await enter('ssh-directory-path-input', args[3]);
      await session.tap(find.byValueKey('ssh-directory-go'));
      await session.waitFor(
        find.byValueKey('ssh-directory-current-${args[3]}'),
      );
      await session.tap(find.byValueKey('ssh-open-current-directory'));
      await session.waitForAbsent(find.byValueKey('ssh-directory-dialog'));
      await session.tap(find.byValueKey('settings-back'));
    }
    await waitForState(
      (state) => (state['project'] as Map?)?['path'] == args[3],
    );
    await session.tap(find.byValueKey('sidebar-new-session'));
    await session.tap(find.byValueKey('session-mode-selector'));
    await session.tap(find.byValueKey('session-mode-mode.simple'));
    await enter('composer-input', 'image-timeline-fixture: read fixture.png');
    await session.tap(find.byValueKey('composer-submit'));
    final state = await waitForState((state) {
      final workspace = state['workspace'] as Map?;
      final rows = workspace?['timeline'] as List? ?? [];
      if ((workspace?['lastTurn'] as Map?)?['status'] != 'completed' ||
          workspace?['isBusy'] != false) {
        return false;
      }
      return rows.any(
        (row) =>
            (row as Map)['text']?.toString().contains('IMAGE_READ_COMPLETE') ==
            true,
      );
    });
    final threadId = (state['navigation'] as Map)['selectedThreadId'] as String;
    final tools = ((state['workspace'] as Map)['timeline'] as List)
        .cast<Map>()
        .expand((row) => (row['tools'] as List).cast<Map>())
        .toList();
    final imageTool = tools.singleWhere((tool) => tool['name'] == 'view_image');
    final attachment = (imageTool['attachments'] as List).cast<Map>().single;
    final id = attachment['id'] as String;
    if (attachment['modality'] != 'image' ||
        (attachment['byteSize'] as num) <= 0) {
      throw StateError('real tool did not project valid image metadata');
    }
    final callId = imageTool['callId'] as String;
    final entryId = '$callId:$id';
    final toggle = find.byValueKey('view-image-toggle-$entryId');
    final inline = find.byValueKey('view-image-thumbnail-$entryId');
    await session.waitFor(toggle);
    await session.waitForAbsent(inline);
    await screenshot('image-collapsed');
    await session.tap(toggle);
    await session.waitFor(inline);
    await screenshot('image-inline');
    await session.tap(inline);
    await session.waitFor(find.byValueKey('view-image-dialog-$entryId'));
    await screenshot('image-enlarged');
    await session.tap(find.byValueKey('timeline-image-close'));
    await session.waitForAbsent(find.byValueKey('view-image-dialog-$entryId'));
    await session.waitForNoPendingFrame();
    await session.tap(toggle);
    await session.waitForAbsent(inline);

    final client = HttpClient();
    try {
      final response = await (await client.getUrl(
        Uri.parse('${args[2]}/delete-image'),
      )).close();
      if (response.statusCode != 200) {
        throw StateError('source image deletion failed');
      }
      await response.drain<void>();
    } finally {
      client.close(force: true);
    }
    // Dispose the timeline/cache and reload canonical history through the normal sidebar.
    await session.tap(find.byValueKey('sidebar-new-session'));
    await session.waitForAbsent(toggle);
    await session.tap(find.byValueKey('thread-row-$threadId'));
    await session.waitFor(toggle);
    await session.waitForAbsent(inline);
    await session.tap(toggle);
    await session.waitFor(inline);
    await screenshot('image-history-after-source-deletion');
    await File('${output.path}/image-result.json').writeAsString(
      jsonEncode({
        'platform': 'linux',
        'transport': args[4].isEmpty ? 'local' : 'ssh',
        'threadId': threadId,
        'attachmentId': id,
        'inline': true,
        'historicalReadAfterSourceDeletion': true,
      }),
    );
    final shutdown = jsonDecode(
      await session.requestData(
        'shutdown-await',
        timeout: const Duration(seconds: 30),
      ),
    ) as Map;
    await File('${output.path}/image-shutdown.json')
        .writeAsString(jsonEncode(shutdown));
    if (shutdown['shutdown'] != 'completed') {
      throw StateError('native runtime shutdown failed: $shutdown');
    }
  } catch (error, stack) {
    stderr.writeln('Image acceptance failed: $error\n$stack');
    try {
      await screenshot('image-failure');
      await snapshot();
    } catch (diagnosticError) {
      stderr.writeln('Failure capture unavailable: $diagnosticError');
    }
    rethrow;
  } finally {
    await session.close();
  }
}
