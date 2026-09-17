import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'flutter_driver_session.dart';

/// Samples the real selected workspace through Driver; never injects child state.
class SubagentsVisibilityEvidence {
  SubagentsVisibilityEvidence(this.artifactPrefix);

  final String artifactPrefix;
  final Map<String, _ChildEvidence> _children = {};
  bool _listCaptured = false;
  bool _rootProgress = false;
  DateTime _nextSample = DateTime.fromMillisecondsSinceEpoch(0);

  Future<void> observe(
    FlutterDriverSession session,
    Map<String, dynamic> root,
  ) async {
    final workspace = root['workspace'] as Map?;
    if (workspace == null) return;
    final rootId = workspace['rootThreadId'] as String;
    if (workspace['threadId'] != rootId) {
      throw StateError('visibility sampling must start in the root workspace');
    }
    final rows = _rows(root);
    if (workspace['isBusy'] == true &&
        rows.any(
          (r) => r['type'] == 'commentary' && _text(r).trim().isNotEmpty,
        )) {
      _rootProgress = true;
    }
    final tools = rows
        .expand((r) => (r['tools'] as List? ?? []).whereType<Map>())
        .toList();
    for (final tool in tools.where(
      (t) => t['name'] == 'spawn_agent' && t['status'] == 'succeeded',
    )) {
      final arguments = _object(tool['arguments']);
      if (arguments['profileId'] != 'explorer') continue;
      final receipt = _object(tool['result']);
      if (receipt['messageAccepted'] != true) continue;
      final id = receipt['agentId'] as String;
      if (_children.length >= 2 && !_children.containsKey(id)) continue;
      _children.putIfAbsent(
        id,
        () => _ChildEvidence(
          id,
          arguments['taskSummary'] as String,
          arguments['message'] as String,
        ),
      );
    }
    for (final tool in tools.where(
      (t) => t['name'] == 'send_message' && t['status'] == 'succeeded',
    )) {
      final arguments = _object(tool['arguments']);
      final child = _children[arguments['target']];
      final message = arguments['message'];
      if (child != null &&
          message is String &&
          message.contains('PARENT_FOLLOWUP_VISIBILITY')) {
        child.followup = message;
      }
    }
    if (_children.length < 2 ||
        workspace['activeInteraction'] != null ||
        (DateTime.now().isBefore(_nextSample) && workspace['isBusy'] == true)) {
      return;
    }
    _nextSample = DateTime.now().add(const Duration(seconds: 3));
    if (!_listCaptured) {
      await session.tap(find.byValueKey('agent-switcher'));
      for (final child in _children.values) {
        final key = find.byValueKey('agent-task-summary-${child.id}');
        await session.waitFor(key);
        final displayed = await session.getText(key);
        if (displayed != child.summary) {
          throw StateError('child summary mismatch: ${child.id}');
        }
      }
      if (_children.values.map((c) => c.summary).toSet().length != 2) {
        throw StateError('two children must have distinct task summaries');
      }
      await _capture(session, 'child-list', root);
      await session.tap(find.byValueKey('agent-thread-$rootId'));
      _listCaptured = true;
    }
    for (final child in _children.values) {
      if (child.initialVisible &&
          child.progressWhileRunning &&
          child.finalVisible &&
          (child.followup == null || child.followupFinalVisible)) {
        continue;
      }
      try {
        final expectedMessages = [child.initial, ?child.followup];
        final selected = await _select(
          session,
          child.id,
          parentMessages: expectedMessages,
        );
        final childRows = _rows(selected);
        final parentRows = childRows
            .where((r) => r['type'] == 'parentAgentMessage')
            .toList();
        final initial = parentRows
            .where((r) => _text(r) == child.initial)
            .toList();
        if (initial.length != 1) {
          throw StateError(
            'initial parent message missing or duplicated: ${child.id}',
          );
        }
        if (!child.initialVisible) {
          await _showRow(session, initial.single, backwards: true);
          await _capture(session, '${child.id}-initial', selected);
          child.initialVisible = true;
        }
        final progress = childRows
            .where(
              (r) => r['type'] == 'commentary' && _text(r).trim().isNotEmpty,
            )
            .toList();
        if (!child.progressWhileRunning &&
            progress.isNotEmpty &&
            (selected['workspace'] as Map)['isBusy'] == true) {
          await _showRow(session, progress.last);
          await _capture(session, '${child.id}-progress', selected);
          child.progressWhileRunning = true;
        }
        final finals = childRows
            .where(
              (r) => r['type'] == 'finalAnswer' && _text(r).trim().isNotEmpty,
            )
            .toList();
        if (finals.isNotEmpty && !child.finalVisible) {
          await _showRow(session, finals.last);
          await _capture(session, '${child.id}-final', selected);
          child.finalVisible = true;
        }
        if (child.followup != null &&
            !child.followupFinalVisible &&
            finals.isNotEmpty) {
          final followupIndex = childRows.indexWhere(
            (r) =>
                r['type'] == 'parentAgentMessage' && _text(r) == child.followup,
          );
          if (followupIndex >= 0 &&
              childRows.indexOf(finals.last) > followupIndex &&
              (selected['workspace'] as Map)['isBusy'] == false) {
            await _showRow(session, finals.last);
            await _capture(session, '${child.id}-followup-final', selected);
            child.followupFinalVisible = true;
          }
        }
        if (child.followup != null && !child.followupVisible) {
          final followups = parentRows
              .where((r) => _text(r) == child.followup)
              .toList();
          if (followups.length != 1 ||
              parentRows.indexOf(followups.single) <=
                  parentRows.indexOf(initial.single)) {
            throw StateError(
              'follow-up parent message missing, duplicated or reordered: ${child.id}',
            );
          }
          await _showRow(session, followups.single, backwards: true);
          await _capture(session, '${child.id}-followup', selected);
          child.followupVisible = true;
          await _select(session, rootId);
          final reopened = await _select(
            session,
            child.id,
            parentMessages: expectedMessages,
          );
          final reopenedMessages = _rows(reopened)
              .where((r) => r['type'] == 'parentAgentMessage')
              .map(_text)
              .toList();
          if (!reopenedMessages.contains(child.initial) ||
              !reopenedMessages.contains(child.followup)) {
            throw StateError('parent dialogue lost after switching back');
          }
          await _capture(session, '${child.id}-reopened', reopened);
        }
      } catch (error, stackTrace) {
        try {
          await _capture(
            session,
            '${child.id}-failure',
            await session.readSnapshot(),
          );
          await _select(session, rootId);
        } on Object catch (diagnosticError) {
          stderr.writeln('Visibility failure diagnostics: $diagnosticError');
        }
        Error.throwWithStackTrace(error, stackTrace);
      }
      await _select(session, rootId);
    }
    await File('$artifactPrefix.visibility.json').writeAsString(
      jsonEncode({
        'rootProgressWhileRunning': _rootProgress,
        'childListCaptured': _listCaptured,
        'children': [for (final child in _children.values) child.toJson()],
      }),
      flush: true,
    );
  }

  void validate() {
    if (!_rootProgress ||
        !_listCaptured ||
        _children.length != 2 ||
        _children.values.any((c) => !c.initialVisible || !c.finalVisible) ||
        !_children.values.any((c) => c.progressWhileRunning) ||
        !_children.values.any((c) => c.followupFinalVisible)) {
      throw StateError(
        'incomplete real GUI subagent visibility evidence: '
        '${_children.values.map((c) => c.toJson()).toList()}, rootProgress=$_rootProgress, list=$_listCaptured',
      );
    }
  }

  Future<void> _capture(
    FlutterDriverSession session,
    String name,
    Map<String, dynamic> snapshot,
  ) async {
    await File('$artifactPrefix.$name.png')
        .writeAsBytes(await session.screenshot(), flush: true);
    await File('$artifactPrefix.$name.render-tree.txt')
        .writeAsString(await session.renderTree(), flush: true);
    await File('$artifactPrefix.$name.snapshot.json')
        .writeAsString(jsonEncode(snapshot), flush: true);
  }
}

Future<Map<String, dynamic>> _select(
  FlutterDriverSession session,
  String id, {
  List<String> parentMessages = const [],
}) async {
  await session.tap(find.byValueKey('agent-switcher'));
  await session.tap(find.byValueKey('agent-thread-$id'));
  final deadline = DateTime.now().add(const Duration(seconds: 30));
  do {
    final snapshot = await session.readSnapshot();
    if ((snapshot['workspace'] as Map?)?['threadId'] == id &&
        (snapshot['timelineWindow'] as Map?)?['hasNewer'] == true) {
      // Inspect the live tail through the same user action as historical readers.
      // Switching back intentionally preserves the previous reading window.
      await session.tap(find.byValueKey('timeline-jump-latest'));
      continue;
    }
    final messages = _rows(snapshot)
        .where((r) => r['type'] == 'parentAgentMessage')
        .map(_text)
        .toList();
    if ((snapshot['workspace'] as Map?)?['threadId'] == id &&
        (snapshot['workspace'] as Map)['syncState'] == 'ready' &&
        parentMessages.every(messages.contains)) {
      return snapshot;
    }
    await Future<void>.delayed(const Duration(milliseconds: 100));
  } while (DateTime.now().isBefore(deadline));
  throw StateError('selected child workspace did not arrive: $id');
}

Future<void> _showRow(
  FlutterDriverSession session,
  Map row, {
  bool backwards = false,
}) => session.scrollUntilVisible(
  find.byValueKey('timeline-scrollable'),
  find.byValueKey(switch (row['type']) {
    'parentAgentMessage' => 'timeline-parent-agent-label-${row['id']}',
    'finalAnswer' => 'timeline-tail',
    _ => 'timeline-row-${row['id']}',
  }),
  dyScroll: backwards ? 300 : -300,
  timeout: const Duration(seconds: 30),
);

List<Map> _rows(Map snapshot) =>
    (((snapshot['workspace'] as Map?)?['timeline'] as List?) ?? [])
        .whereType<Map>()
        .toList();
String _text(Map row) => row['text'] as String? ?? '';
Map _object(Object? value) {
  final decoded = value is String ? jsonDecode(value) : value;
  if (decoded is! Map) {
    throw StateError('expected canonical tool object, got $decoded');
  }
  return decoded;
}

class _ChildEvidence {
  _ChildEvidence(this.id, String summary, this.initial)
    : summary = summary.trim().split(RegExp(r'\s+')).join(' ');
  final String id;
  final String summary;
  final String initial;
  String? followup;
  bool initialVisible = false;
  bool progressWhileRunning = false;
  bool finalVisible = false;
  bool followupVisible = false;
  bool followupFinalVisible = false;
  Map<String, Object?> toJson() => {
    'agentId': id,
    'taskSummary': summary,
    'initialVisible': initialVisible,
    'progressWhileRunning': progressWhileRunning,
    'finalVisible': finalVisible,
    'followupVisible': followupVisible,
    'followupFinalVisible': followupFinalVisible,
  };
}
