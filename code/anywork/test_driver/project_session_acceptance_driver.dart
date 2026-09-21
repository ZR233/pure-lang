// Native (non-demo) acceptance for the W6-3 Project/Thread chain.
//
// Launched only by `tool/project_session_native_harness.py`, which isolates
// `ANYWORK_HOME`, a temporary workspace and a scripted loopback provider, then
// runs `cargo xtask run-gui --driver` twice on the same Studio home:
//
//   --phase create   create a real local Project and Thread through GUI
//                    interaction, send scripted prompts, walk the bounded
//                    history window and record the live full body
//   --phase reopen   restart on the same Studio home, prove the restored
//                    selection is not opened automatically, open the saved
//                    Thread, exercise the preview -> full-body retrieval path
//                    and confirm recovery settles
//
// Both phases keep the fast 2-Turn path by default. With `--turns N` (N >= 130,
// passed by `--long-session`) the create phase additionally submits N-2 short
// scripted Turns before the large-body Turn, so the durable session holds more
// than one 500-item bounded window, and both phases then walk the real GUI to
// the oldest and back to the newest page, asserting that previously nonresident
// canonical items become reachable and that the exact first/latest item
// identities are observed at the window boundaries.
//
// The create-phase walk is a hard requirement, not an observation: live window
// eviction must raise the history window's older boundary (see
// `_liveEvictionHistory` in the reducer), so a session that created its window
// from the live stream must still page back to the oldest durable SQL page.
// Both walks must also add no conversation request to the provider wire index,
// which proves deep paging never re-ran a prior model.
//
// The Studio home and its SQLite/TOML artifacts stay on disk for the operator
// to inspect; this driver never asserts on a mock or a fabricated database.
//
// Honest evidence limits: the driver snapshot exposes the bounded window's item
// identities, `hasOlder`/`hasNewer` and the load state, but no raw page cursor
// tokens (that would require editing the product driver-state projection, which
// is out of scope here). Page transitions are therefore reported with the
// window's boundary item identities as the cursor proxy, and the canonical
// cursors/watermarks stay for the operator to read from the SQLite artifacts.
import 'dart:convert';
import 'dart:io';

import 'package:flutter_driver/flutter_driver.dart';

import 'raw_tap.dart';

const _largeSentinel = 'NATIVE_ACCEPT_LARGE_END';
const _historyWindowBudget = 500;
const _cacheBudget = 900;
const _liveTailBudget = 400;

/// Long-session mode needs more Turns than one window can hold: at four items
/// per Turn, 130 Turns already exceed the 500-item bounded window.
const _minLongTurns = 130;

/// One bounded drag step of the deep-history walk; each round watches a single
/// page transition instead of skipping several pages at once.
const _longScrollDelta = 2600.0;

/// Hard cap on deep-walk rounds so a stuck window fails fast instead of looping.
const _longRoundCap = 80;

/// Extra drags that may be needed to pin the window to an edge once paging
/// reports no more pages. Over-dragging at a clamped edge is a no-op, so this
/// is an upper bound on a bounded, deterministic settle loop.
const _longEdgeSettleCap = 32;

Future<void> main(List<String> arguments) async {
  final options = _DriverOptions.parse(arguments);
  final output = Directory(options.output);
  await output.create(recursive: true);
  final driver = await FlutterDriver.connect(
    dartVmServiceUrl: options.vmServiceUrl,
    printCommunication: false,
    logCommunicationToFile: false,
  );
  await driver.sendCommand(const SetFrameSync(false));
  final context = _Context(driver: driver, options: options, output: output);
  Object? failure;
  StackTrace? failureStack;
  try {
    if (options.phase == 'create') {
      await context.createPhase();
    } else if (options.phase == 'reopen') {
      await context.reopenPhase();
    } else {
      throw ArgumentError('Unknown --phase ${options.phase}');
    }
  } catch (error, stack) {
    failure = error;
    failureStack = stack;
    try {
      await context.capture('failure-${options.phase}');
      await File('${output.path}/failure-${options.phase}.json')
          .writeAsString(jsonEncode(await context.readSnapshot()));
    } on Object catch (captureError) {
      stderr.writeln('Failure evidence unavailable: $captureError');
    }
  } finally {
    try {
      final shutdown = await driver.requestData(
        'shutdown-await',
        timeout: const Duration(minutes: 3),
      );
      final decoded = jsonDecode(shutdown) as Map;
      await File('${output.path}/shutdown-${options.phase}.json')
          .writeAsString(jsonEncode(decoded));
      if (decoded['shutdown'] != 'completed' && failure == null) {
        failure = StateError('Studio shutdown did not complete: $shutdown');
      }
    } on Object catch (error) {
      if (failure == null) {
        failure = error;
        failureStack = StackTrace.current;
      } else {
        stderr.writeln('Shutdown after failure also failed: $error');
      }
    } finally {
      await driver.close();
    }
  }
  if (failure != null) {
    Error.throwWithStackTrace(failure, failureStack ?? StackTrace.current);
  }
}

class _Context {
  _Context({required this.driver, required this.options, required this.output});

  final FlutterDriver driver;
  final _DriverOptions options;
  final Directory output;

  late final File _snapshots = File(
    '${output.path}/snapshots-${options.phase}.jsonl',
  );

  Future<Map<String, dynamic>> readSnapshot() async {
    final raw = await driver.requestData(
      'snapshot',
      timeout: const Duration(seconds: 60),
    );
    return jsonDecode(raw) as Map<String, dynamic>;
  }

  /// Snapshots kept as durable evidence; the tight polling loops below do not
  /// log every poll so a >256 KiB body cannot flood the evidence file.
  Future<Map<String, dynamic>> record(String label) async {
    final state = await readSnapshot();
    await File('${output.path}/phase-${options.phase}-$label.json')
        .writeAsString(jsonEncode(state));
    await _snapshots.writeAsString(
      '${jsonEncode({'label': label, 'snapshot': state})}\n',
      mode: FileMode.append,
    );
    return state;
  }

  Future<Map<String, dynamic>> waitFor(
    String description,
    bool Function(Map<String, dynamic> state) ready, {
    Duration timeout = const Duration(minutes: 3),
  }) async {
    final deadline = DateTime.now().add(timeout);
    Map<String, dynamic> state = const {};
    while (DateTime.now().isBefore(deadline)) {
      state = await readSnapshot();
      if (ready(state)) return state;
      await Future<void>.delayed(const Duration(milliseconds: 200));
    }
    throw StateError(
      'Timed out waiting for $description; observed ${jsonEncode(_brief(state))}',
    );
  }

  Future<void> tapKey(String key) async {
    final finder = find.byValueKey(key);
    await driver.waitFor(finder, timeout: const Duration(seconds: 40));
    await driver.sendCommand(
      RawTap(finder, timeout: const Duration(seconds: 30)),
    );
  }

  Future<void> enterText(String text) async {
    await driver.enterText(text);
    await driver.waitForCondition(
      const NoPendingFrame(),
      timeout: const Duration(seconds: 20),
    );
  }

  Future<bool> finderAppears(
    String key, {
    Duration timeout = const Duration(seconds: 30),
  }) async {
    try {
      await driver.waitFor(find.byValueKey(key), timeout: timeout);
      return true;
    } on Object {
      return false;
    }
  }

  Future<bool> finderDisappears(
    String key, {
    Duration timeout = const Duration(seconds: 30),
  }) async {
    try {
      await driver.waitForAbsent(find.byValueKey(key), timeout: timeout);
      return true;
    } on Object {
      return false;
    }
  }

  Future<void> capture(String name) async {
    await driver.waitForCondition(
      const NoPendingFrame(),
      timeout: const Duration(seconds: 20),
    );
    await File('${output.path}/$name-${options.phase}.png')
        .writeAsBytes(await driver.screenshot());
    final tree = (await driver.getRenderTree()).tree ?? '';
    await File('${output.path}/$name-${options.phase}.txt').writeAsString(tree);
  }

  Future<void> writeJson(String name, Map<String, Object?> value) async {
    await File('${output.path}/$name')
        .writeAsString(jsonEncode(value), flush: true);
  }

  /// Provider request counters read from the harness wire index.
  ///
  /// `conversation` counts only requests that carried conversation messages, so
  /// a cold reopen that re-ran a prior model would show up here; catalog or
  /// usage probes are counted in `total` but never mistaken for a Turn.
  Map<String, Object?> _wireRequestStats() {
    final file = File('${output.path}/wire-index.jsonl');
    if (!file.existsSync()) {
      return {'available': false};
    }
    var total = 0;
    var conversation = 0;
    for (final line in file.readAsLinesSync()) {
      final trimmed = line.trim();
      if (trimmed.isEmpty) continue;
      total += 1;
      try {
        final entry = jsonDecode(trimmed) as Map;
        if (((entry['userMessages'] as num?) ?? 0) > 0) conversation += 1;
      } on Object {
        // A partially flushed final line from the live provider is not evidence.
        continue;
      }
    }
    return {'available': true, 'total': total, 'conversation': conversation};
  }

  /// Proves an acceptance step did not re-run a prior model conversation.
  ///
  /// `conversation` counts only provider requests that carried conversation
  /// messages, so a cold reopen or a deep-paging walk that silently resumed a
  /// model would show up as a positive delta; catalog/usage traffic is still
  /// recorded in `total` but never mistaken for a Turn.
  Map<String, Object?> _requireNoConversationRequests(
    Map<String, Object?> before,
    String phase,
  ) {
    final after = _wireRequestStats();
    final available = before['available'] == true && after['available'] == true;
    final delta = available
        ? ((after['conversation'] as num?)?.toInt() ?? 0) -
              ((before['conversation'] as num?)?.toInt() ?? 0)
        : null;
    final guard = <String, Object?>{
      'available': available,
      'before': before,
      'after': after,
      'conversationDelta': delta,
    };
    _require(
      available,
      '$phase requires the harness wire index (wire-index.jsonl)',
    );
    _require(
      delta == 0,
      '$phase re-executed a model conversation request: ${jsonEncode(guard)}',
    );
    return guard;
  }

  // ---------------------------------------------------------------- phase 1

  Future<void> createPhase() async {
    await driver.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    );
    // The sidebar only renders once the controller published its canonical
    // state, so this makes the lazy-startup assertion below meaningful.
    await driver.waitFor(
      find.byValueKey('sidebar-open-project'),
      timeout: const Duration(minutes: 2),
    );
    final startup = await record('startup');
    // §6.1: the first GUI screen carries zero loaded session state, DB or
    // history; the restored selection is not an open session.
    _require(
      (startup['navigation'] as Map)['selectedThreadId'] == null,
      'a fresh Studio home must not select a Thread',
    );
    _require(
      _sessionNotOpened(startup),
      'the first screen must not open a session',
    );
    await capture('first-screen');

    final workspace = options.workspace;
    await tapKey('sidebar-open-project');
    await tapKey('add-project-local');
    await tapKey('add-project-continue');
    await driver.waitFor(
      find.byValueKey('project-path-dialog'),
      timeout: const Duration(seconds: 40),
    );
    await tapKey('project-path-input');
    await enterText(workspace);
    await tapKey('project-path-submit');
    await driver.waitForAbsent(
      find.byValueKey('project-path-dialog'),
      timeout: const Duration(minutes: 2),
    );
    final opened = await waitFor('temporary Project selection', (state) {
      final path = _projectPath(state);
      return path != null && _normalized(path) == _normalized(workspace);
    }, timeout: const Duration(minutes: 2));
    final projectId = (opened['project'] as Map)['id'] as String;
    await record('project-open');

    await tapKey('sidebar-new-session');
    await driver.waitFor(
      find.byValueKey('studio-start-page'),
      timeout: const Duration(seconds: 60),
    );
    final firstTurn = await _submitPrompt(
      'NATIVE_ACCEPT_TURN first acceptance prompt',
      previousTurnId: null,
    );
    final threadId = _threadId(firstTurn)!;
    await record('first-turn');
    // The first Turn is the only state in which the bounded window holds the
    // whole session, so this is the canonical oldest item identity; it must be
    // provably outside later windows and reachable again through real paging.
    final firstItemId = _itemIds(firstTurn).firstOrNull;
    _require(
      firstItemId != null,
      'the first Turn exposed no canonical first item identity',
    );
    final progression = <Map<String, Object?>>[_turnRecord(1, firstTurn)];
    var previousTurnId = _lastTurnId(firstTurn);
    if (options.longSession) {
      // Opt-in long-session mode: enough short scripted Turns to overflow one
      // bounded 500-item window (four items per Turn), so GUI paging past the
      // first window becomes observable instead of assumed.
      for (var index = 2; index < options.turns; index++) {
        final state = await _submitPrompt(
          'NATIVE_ACCEPT_TURN long-session turn $index',
          previousTurnId: previousTurnId,
        );
        previousTurnId = _lastTurnId(state);
        progression.add(_turnRecord(index, state));
      }
    }

    final largeTurn = await _submitPrompt(
      'NATIVE_ACCEPT_LARGE persisted preview body',
      previousTurnId: previousTurnId,
    );
    final latestItemId = _itemIds(largeTurn).lastOrNull;
    _require(
      latestItemId != null,
      'the last Turn exposed no canonical latest item identity',
    );
    // The reading window is bounded: a body above the single-item budget may be
    // projected as a preview, so the full ~272 KiB text is not required to stay
    // resident. Identify the canonical item from the window rows and read the
    // persisted body through the real per-item route instead.
    var identified = largeTurn;
    if (_largeBodyItemId(identified) == null) {
      identified = await waitFor(
        'canonical large body item in the bounded window',
        (state) => _largeBodyItemId(state) != null,
        timeout: const Duration(minutes: 3),
      );
    }
    final largeItemId = _largeBodyItemId(identified);
    if (largeItemId == null) {
      throw StateError(
        'the bounded window exposed neither the body nor a preview of '
        '$_largeSentinel',
      );
    }
    var bodyResident = _timelineText(identified).contains(_largeSentinel);
    if (bodyResident) {
      // The bounded window is expected to replace the streamed payload with the
      // persisted preview shortly after the Turn commits; only when it stays
      // resident do we accept the body without a per-item read.
      if (await finderAppears(
        'timeline-item-body-notice-$largeItemId',
        timeout: const Duration(seconds: 30),
      )) {
        bodyResident = false;
      }
    }
    final largeBody = bodyResident
        ? <String, Object?>{
            'itemId': largeItemId,
            'affordance': 'resident',
            'fullBodyRetrieved': true,
            'noticeVisible': false,
            'previewTruncated': false,
          }
        : await _retrieveItemBody(largeItemId);
    _require(
      largeBody['fullBodyRetrieved'] == true,
      'the persisted large assistant body was not retrievable by canonical item id',
    );

    Map<String, Object?>? persistenceReadiness;
    Map<String, Object?>? modelRequestGuard;
    Map<String, Object?> navigation;
    if (options.longSession) {
      // Long-session paging must read durable history, so wait for the
      // persistence state to settle before walking the pages. The recorded
      // revision is the client's applied persistence revision; the canonical
      // SQLite watermarks stay for the operator/agent to inspect.
      final durable = await waitFor('terminal persistence readiness', (state) {
        final persistence = state['persistence'] as Map;
        return persistence['kind'] == 'ready' &&
            ((persistence['pendingCommits'] as num?) ?? 0) == 0 &&
            persistence['needsAttention'] != true;
      }, timeout: const Duration(minutes: 3));
      final persistence = durable['persistence'] as Map;
      persistenceReadiness = {
        'kind': persistence['kind'],
        'revision': persistence['revision'],
        'pendingCommits': persistence['pendingCommits'],
        'oldestPendingRevision': persistence['oldestPendingRevision'],
        'needsAttention': persistence['needsAttention'],
      };
      _require(
        persistence['kind'] == 'ready' &&
            ((persistence['pendingCommits'] as num?) ?? 0) == 0,
        'terminal Turns were not durably persisted before deep paging: '
        '${jsonEncode(persistenceReadiness)}',
      );
      final liveWindow = await record('live-window');
      final liveHistory = liveWindow['timelineWindow'] as Map;
      final liveIds = _itemIds(liveWindow);
      _checkBudgets(liveWindow, liveHistory);
      // Live window eviction must have advanced the older boundary, otherwise
      // the trimmed window can never page back into durable SQL history in the
      // session that created it. This is a hard failure, not a recorded
      // observation: the create phase must really reach the oldest page.
      _require(
        liveHistory['hasOlder'] == true,
        'the live-trimmed window exposed no older durable history after '
        '${progression.length} Turns '
        '(hasOlder=${liveHistory['hasOlder']}, '
        'resident=${liveIds.length}, '
        'firstResident=${liveIds.firstOrNull})',
      );
      // Deep paging asserts the canonical oldest/latest identities at the
      // window boundaries; a missing identity must fail here instead of being
      // replaced by a placeholder ID that could page to the wrong edge.
      final canonicalFirstItemId = firstItemId;
      final canonicalLatestItemId = latestItemId;
      if (canonicalFirstItemId == null || canonicalLatestItemId == null) {
        throw StateError(
          'the session exposed no canonical oldest/latest item identity '
          '(oldest=$canonicalFirstItemId, latest=$canonicalLatestItemId)',
        );
      }
      // Deep paging must not re-run a prior model conversation.
      final requestsBeforeWalk = _wireRequestStats();
      navigation = await _navigateLongSession(
        threadId: threadId,
        firstItemId: canonicalFirstItemId,
        latestItemId: canonicalLatestItemId,
      );
      modelRequestGuard = _requireNoConversationRequests(
        requestsBeforeWalk,
        'native create-phase deep paging',
      );
    } else {
      navigation = await _navigateBoundedWindow();
    }
    final finalState = await record('final');
    await writeJson('session.json', {
      'phase': 'create',
      'projectId': projectId,
      'threadId': threadId,
      'workspace': workspace,
      'workspacePathSelected': _projectPath(opened),
      'providerUrl': options.providerUrl,
      'mode': options.longSession ? 'long-session' : 'two-turn',
      'requestedTurns': options.longSession ? options.turns : 2,
      'completedTurns': progression.length,
      'lastTurnId': _lastTurnId(finalState),
      'firstItemId': firstItemId,
      'latestItemId': latestItemId,
      'largeItemId': largeItemId,
      'largeSentinel': _largeSentinel,
      'largeBodyResidentInWindow': bodyResident,
      'largeBodyRetrieval': largeBody,
      'firstTurnId': _lastTurnId(firstTurn),
      'turnProgression': progression,
      'providerRequests': _wireRequestStats(),
      'persistenceReadiness': persistenceReadiness,
      'modelRequestGuard': modelRequestGuard,
      'navigation': navigation,
      'persistence': finalState['persistence'],
    });
    await capture('create-complete');
  }

  Future<Map<String, dynamic>> _submitPrompt(
    String prompt, {
    required String? previousTurnId,
  }) async {
    await tapKey('composer-input');
    await enterText(prompt);
    await tapKey('composer-submit');
    return waitFor('completed Turn for "$prompt"', (state) {
      final turnId = _lastTurnId(state);
      return turnId != null &&
          turnId != previousTurnId &&
          _lastTurnStatus(state) == 'completed';
    }, timeout: const Duration(minutes: 6));
  }

  /// Explicit per-item read of the persisted body behind a bounded preview.
  ///
  /// A body above the single-item budget is only ever exposed as a preview plus
  /// a visible load/retry affordance (`loadItemBody` -> `readTimelineItem`), so
  /// this drives that real route and reports the observed identity, reading
  /// window and reading-position facts instead of asserting on a resident body.
  Future<Map<String, Object?>> _retrieveItemBody(String itemId) async {
    final noticeKey = 'timeline-item-body-notice-$itemId';
    final loadKey = 'timeline-item-body-load-$itemId';
    final retryKey = 'timeline-item-body-retry-$itemId';
    final noticeVisible = await finderAppears(
      noticeKey,
      timeout: const Duration(seconds: 60),
    );
    var affordance = 'none';
    var previewTruncated = false;
    var fullBodyRetrieved = false;
    var windowStable = false;
    var anchorItemStable = false;
    var followingBottomStable = false;
    var targetRowPresent = false;
    var readingPositionStable = false;
    var noticeCleared = false;
    var loadedByIdentity = false;
    var previewFlagCleared = false;
    var idsBefore = const <String>[];
    var pinnedBefore = false;
    var pinnedAfter = false;
    var detachedAnchorItemStable = false;
    String? anchorItemBefore;
    String? anchorItemAfter;
    double? anchorOffsetBefore;
    double? anchorOffsetAfter;
    if (noticeVisible) {
      final preview = await record('preview');
      previewTruncated = !_timelineText(preview).contains(_largeSentinel);
      await capture('preview');
      // "load" is the plain preview route and "retry" the route after a failed
      // retrieval; both reach the same canonical per-item read.
      final loadVisible = await finderAppears(
        loadKey,
        timeout: const Duration(seconds: 10),
      );
      final retryVisible =
          !loadVisible &&
          await finderAppears(retryKey, timeout: const Duration(seconds: 10));
      final affordanceKey = loadVisible
          ? loadKey
          : retryVisible
          ? retryKey
          : null;
      if (affordanceKey != null) {
        affordance = loadVisible ? 'load' : 'retry';
        // The affordance sits at the end of the previewed row; make sure it is
        // inside the viewport before dispatching the raw tap. The reading-position
        // baseline is sampled *after* this driver-initiated scroll, so the verdict
        // attributes only the body expansion to the product and never the driver's
        // own scrolling.
        try {
          await driver.scrollUntilVisible(
            find.byValueKey('timeline-scrollable'),
            find.byValueKey(affordanceKey),
            dyScroll: -240,
            timeout: const Duration(seconds: 30),
          );
        } on Object {
          // Already visible or not scrollable: fall through to the raw tap.
        }
        final baseline = await record('preview-position');
        idsBefore = _itemIds(baseline);
        final before = _readingPosition(baseline);
        pinnedBefore = before.pinned;
        anchorItemBefore = before.itemId;
        anchorOffsetBefore = before.offset;
        await tapKey(affordanceKey);
        final loaded = await waitFor(
          'full body retrieved through the canonical item id',
          (state) => _timelineText(state).contains(_largeSentinel),
          timeout: const Duration(minutes: 2),
        );
        fullBodyRetrieved = true;
        loadedByIdentity = _loadedItemIds(loaded).contains(itemId);
        previewFlagCleared = !_previewedItemIds(loaded).contains(itemId);
        final idsAfter = _itemIds(loaded);
        windowStable =
            idsBefore.toSet().containsAll(idsAfter) &&
            idsAfter.toSet().containsAll(idsBefore);
        final after = _readingPosition(loaded);
        pinnedAfter = after.pinned;
        anchorItemAfter = after.itemId;
        anchorOffsetAfter = after.offset;
        anchorItemStable = anchorItemBefore == anchorItemAfter;
        followingBottomStable = pinnedBefore == pinnedAfter;
        // 脱离阅读时锚点身份必须保持：展开不得把读者挪到另一行。
        detachedAnchorItemStable =
            anchorItemBefore != null && anchorItemBefore == anchorItemAfter;
        targetRowPresent = idsAfter.contains(itemId);
        // 展开一条被截断的超大正文只允许改变像素高度：目标行必须仍在窗口里、窗口内容
        // 不得变化，且读者必须停在原来的阅读位置：钉在末尾的读者仍钉在末尾，脱离阅读的
        // 读者仍停在同一个可见锚点上。未发布锚点等价于"默认跟随末尾"
        // （`timeline_view.dart` 无锚点默认 `_followingBottom = true`），不能把 null 与
        // `followingBottom: true` 当成阅读位置变化；反之，钉在末尾的读者被展开甩到脱离
        // 位置、脱离中的读者被甩回末尾或挪到另一行，都是真实跳动。
        readingPositionStable =
            targetRowPresent &&
            windowStable &&
            followingBottomStable &&
            (pinnedBefore || detachedAnchorItemStable);
        noticeCleared = await finderDisappears(
          noticeKey,
          timeout: const Duration(seconds: 30),
        );
        await capture('fullbody');
      }
    }
    return {
      'itemId': itemId,
      'noticeVisible': noticeVisible,
      'affordance': affordance,
      'previewTruncated': previewTruncated,
      'fullBodyRetrieved': fullBodyRetrieved,
      'windowStable': windowStable,
      'anchorItemStable': anchorItemStable,
      'followingBottomStable': followingBottomStable,
      'targetRowPresent': targetRowPresent,
      'readingPositionStable': readingPositionStable,
      'pinnedBefore': pinnedBefore,
      'pinnedAfter': pinnedAfter,
      'detachedAnchorItemStable': detachedAnchorItemStable,
      'anchorItemBefore': anchorItemBefore,
      'anchorItemAfter': anchorItemAfter,
      'anchorOffsetBefore': anchorOffsetBefore,
      'anchorOffsetAfter': anchorOffsetAfter,
      'noticeCleared': noticeCleared,
      'loadedByIdentity': loadedByIdentity,
      'previewFlagCleared': previewFlagCleared,
      'trailingSentinel': _largeSentinel,
    };
  }

  Future<Map<String, Object?>> _navigateBoundedWindow() async {
    final rounds = <Map<String, Object?>>[];
    // Older direction first, then back to the tail; every intermediate state
    // must keep the history window and the live tail independently bounded.
    for (final direction in ['older', 'newer']) {
      final delta = direction == 'older' ? 6500.0 : -6500.0;
      for (var round = 0; round < 12; round++) {
        await driver.scroll(
          find.byValueKey('timeline-scrollable'),
          0,
          delta,
          const Duration(milliseconds: 150),
        );
        await Future<void>.delayed(const Duration(milliseconds: 150));
        final state = await waitFor(
          '$direction page settled',
          (state) => (state['timelineWindow'] as Map)['loading'] != true,
          timeout: const Duration(seconds: 60),
        );
        final window = state['timelineWindow'] as Map;
        _checkBudgets(state, window);
        rounds.add({
          'direction': direction,
          'round': round,
          'itemCount': _itemIds(state).length,
          'cacheCount': window['cacheCount'],
          'tailCount': window['tailCount'],
          'hasOlder': window['hasOlder'],
          'hasNewer': window['hasNewer'],
          'anchorFollowingBottom':
              (window['anchor'] as Map?)?['followingBottom'],
        });
        final done =
            (window['hasOlder'] == false && direction == 'older') ||
            (window['hasNewer'] == false && direction == 'newer');
        if (done) break;
      }
    }
    final finalState = await record('window-final');
    final window = finalState['timelineWindow'] as Map;
    return {
      'historyWindowBudget': _historyWindowBudget,
      'cacheBudget': _cacheBudget,
      'liveTailBudget': _liveTailBudget,
      'finalItemCount': _itemIds(finalState).length,
      'finalCacheCount': window['cacheCount'],
      'finalTailCount': window['tailCount'],
      'finalHasOlder': window['hasOlder'],
      'finalHasNewer': window['hasNewer'],
      'rounds': rounds,
    };
  }

  /// Opt-in deep-history walk over the real GUI.
  ///
  /// One bounded window cannot hold the long session, so this drives the actual
  /// scroll/pagination path to the oldest page and back to the newest page,
  /// asserting that previously nonresident canonical items become reachable and
  /// that the exact first/latest item identities appear at the window
  /// boundaries. Every observed window stays inside the strict item budgets.
  Future<Map<String, Object?>> _navigateLongSession({
    required String threadId,
    required String firstItemId,
    required String latestItemId,
  }) async {
    final initial = await record('long-initial');
    final initialIds = _itemIds(initial);
    final initialWindow = initial['timelineWindow'] as Map;
    _checkBudgets(initial, initialWindow);
    _require(
      initialWindow['hasOlder'] == true,
      'the long session must start with older items outside the bounded window',
    );
    _require(
      !initialIds.contains(firstItemId),
      'the canonical first item must be nonresident before paging discovers it',
    );
    final discovered = <String>{...initialIds};
    final olderRounds = <Map<String, Object?>>[];
    final newerRounds = <Map<String, Object?>>[];
    var olderPagesLoaded = 0;
    // 本轮"离开上一个窗口"的条目数：>0 说明真的取回了一页更旧历史，而不只是重排或
    // 重设阅读锚点。
    int observe(List<String> ids) {
      final fresh = ids.where((id) => !discovered.contains(id)).length;
      discovered.addAll(ids);
      return fresh;
    }

    var state = initial;
    for (var round = 0; round < _longRoundCap; round++) {
      await _dragTimeline('older');
      state = await _awaitWindow('older page settled');
      _requireThread(state, threadId);
      final window = state['timelineWindow'] as Map;
      final ids = _itemIds(state);
      _checkBudgets(state, window);
      if (observe(ids) > 0) olderPagesLoaded += 1;
      olderRounds.add(_walkRecord(round, ids, window));
      if (window['hasOlder'] != true) break;
    }
    // Paging may stop with the window still anchored away from the oldest page;
    // keep dragging the real timeline until its first identity is the canonical
    // first item. Over-dragging at the edge is a clamped no-op, so this is a
    // bounded number of drags, not a fixed count of successful pages.
    for (var settle = 0; settle < _longEdgeSettleCap; settle++) {
      if (_itemIds(state).firstOrNull == firstItemId) break;
      await _dragTimeline('older');
      state = await _awaitWindow('oldest edge settled');
      _checkBudgets(state, state['timelineWindow'] as Map);
      if (observe(_itemIds(state)) > 0) olderPagesLoaded += 1;
    }
    final oldestWindow = state['timelineWindow'] as Map;
    final oldestIds = _itemIds(state);
    final firstIdentityReached = oldestIds.firstOrNull == firstItemId;
    _require(
      oldestWindow['hasOlder'] != true,
      'older navigation never reached the beginning of the session',
    );
    _require(
      firstIdentityReached,
      'older navigation did not reach the canonical first item $firstItemId '
      '(oldest window starts at ${oldestIds.firstOrNull})',
    );
    _require(
      discovered.length > _historyWindowBudget,
      'deep navigation discovered only ${discovered.length} distinct items, '
      'which does not exceed one $_historyWindowBudget-item window',
    );
    _require(
      olderPagesLoaded >= 1,
      'older navigation never loaded a page holding items outside the previous '
      'window, so no durable SQL history was actually paged in',
    );
    _require(
      oldestIds.length <= _historyWindowBudget,
      'oldest history window exceeds $_historyWindowBudget items',
    );

    for (var round = 0; round < _longRoundCap; round++) {
      await _dragTimeline('newer');
      state = await _awaitWindow('newer page settled');
      _requireThread(state, threadId);
      final window = state['timelineWindow'] as Map;
      final ids = _itemIds(state);
      _checkBudgets(state, window);
      discovered.addAll(ids);
      newerRounds.add(_walkRecord(round, ids, window));
      if (window['hasNewer'] != true) break;
    }
    for (var settle = 0; settle < _longEdgeSettleCap; settle++) {
      if (_itemIds(state).lastOrNull == latestItemId) break;
      await _dragTimeline('newer');
      state = await _awaitWindow('newest edge settled');
      _checkBudgets(state, state['timelineWindow'] as Map);
      discovered.addAll(_itemIds(state));
    }
    final newestWindow = state['timelineWindow'] as Map;
    final newestIds = _itemIds(state);
    final latestIdentityReached = newestIds.lastOrNull == latestItemId;
    _require(
      newestWindow['hasNewer'] != true,
      'newer navigation never returned to the latest page',
    );
    _require(
      latestIdentityReached,
      'newer navigation did not return to the canonical latest item '
      '$latestItemId (newest window ends at ${newestIds.lastOrNull})',
    );

    final finalState = await record('long-final');
    _checkBudgets(finalState, finalState['timelineWindow'] as Map);
    final directory = finalState['sidebarDirectory'] as Map;
    final navigation = finalState['navigation'] as Map;
    final isolation = {
      'threadId': threadId,
      'selectedThreadId': navigation['selectedThreadId'],
      'workspaceThreadId': _threadId(finalState),
      'directoryCount': directory['count'],
      'directoryIds': directory['ids'],
      'singleThreadDirectory':
          directory['count'] == 1 &&
          (directory['ids'] as List?)?.length == 1 &&
          (directory['ids'] as List).first == threadId,
    };
    _require(
      isolation['singleThreadDirectory'] == true &&
          navigation['selectedThreadId'] == threadId &&
          _threadId(finalState) == threadId,
      'deep navigation activated an unrelated Thread: $isolation',
    );
    return {
      'historyWindowBudget': _historyWindowBudget,
      'cacheBudget': _cacheBudget,
      'liveTailBudget': _liveTailBudget,
      'initialItemCount': initialIds.length,
      'initialHasOlder': initialWindow['hasOlder'],
      'initialHasNewer': initialWindow['hasNewer'],
      'initialFirstItemId': initialIds.firstOrNull,
      'initialLastItemId': initialIds.lastOrNull,
      'firstItemId': firstItemId,
      'firstIdentityInitiallyResident': initialIds.contains(firstItemId),
      'firstIdentityReached': firstIdentityReached,
      'olderPagesLoaded': olderPagesLoaded,
      'oldestItemCount': oldestIds.length,
      'oldestHasOlder': oldestWindow['hasOlder'],
      'latestItemId': latestItemId,
      'latestIdentityReached': latestIdentityReached,
      'newestItemCount': newestIds.length,
      'newestHasNewer': newestWindow['hasNewer'],
      'cumulativeDistinctItems': discovered.length,
      // The driver snapshot has no raw page cursor tokens, so the observed
      // window boundary identities are the cursor proxy for both transitions.
      'cursorProxy': {
        'oldestWindowFirstItemId': oldestIds.firstOrNull,
        'oldestWindowLastItemId': oldestIds.lastOrNull,
        'newestWindowFirstItemId': newestIds.firstOrNull,
        'newestWindowLastItemId': newestIds.lastOrNull,
      },
      'olderRounds': olderRounds,
      'newerRounds': newerRounds,
      'threadIsolation': isolation,
      'finalPersistence': finalState['persistence'],
    };
  }

  /// 有界窗口/缓存/实时尾部的硬预算，深分页每一步都必须满足。
  void _checkBudgets(Map<String, dynamic> state, Map window) {
    _require(
      _itemIds(state).length <= _historyWindowBudget,
      'history window exceeds $_historyWindowBudget items',
    );
    _require(
      (window['cacheCount'] as num) <= _cacheBudget,
      'business-layer cache exceeds $_cacheBudget items',
    );
    _require(
      (window['tailCount'] as num) <= _liveTailBudget,
      'live tail exceeds $_liveTailBudget items',
    );
  }

  void _requireThread(Map<String, dynamic> state, String threadId) {
    _require(
      _threadId(state) == threadId &&
          (state['navigation'] as Map)['selectedThreadId'] == threadId,
      'deep navigation left the accepted Thread: ${jsonEncode(_brief(state))}',
    );
  }

  Future<void> _dragTimeline(String direction) async {
    await driver.scroll(
      find.byValueKey('timeline-scrollable'),
      0,
      direction == 'older' ? _longScrollDelta : -_longScrollDelta,
      const Duration(milliseconds: 150),
    );
    await Future<void>.delayed(const Duration(milliseconds: 200));
  }

  Future<Map<String, dynamic>> _awaitWindow(String description) {
    return waitFor(
      description,
      (state) => (state['timelineWindow'] as Map)['loading'] != true,
      timeout: const Duration(seconds: 90),
    );
  }

  // ---------------------------------------------------------------- phase 2

  Future<void> reopenPhase() async {
    final sessionFile = File('${output.path}/session.json');
    if (!sessionFile.existsSync()) {
      throw StateError(
        'reopen phase requires session.json from the create phase',
      );
    }
    final session = jsonDecode(await sessionFile.readAsString()) as Map;
    final threadId = session['threadId'] as String;
    final largeItemId = session['largeItemId'] as String;
    final previousTurnId = session['lastTurnId'] as String?;

    await driver.waitFor(
      find.byValueKey('studio-shell'),
      timeout: const Duration(minutes: 2),
    );
    await driver.waitFor(
      find.byValueKey('sidebar-open-project'),
      timeout: const Duration(minutes: 2),
    );
    final startup = await record('startup');
    // Restoring a saved selection is not opening a session: no workspace, no
    // history window and no cached items on the first screen after restart.
    _require(
      _sessionNotOpened(startup),
      'restoring a saved selection must not open the session',
    );
    final selectionRestored =
        (startup['navigation'] as Map)['selectedThreadId'] == threadId;
    final unopenedBanner = await finderAppears(
      'studio-unopened-thread',
      timeout: const Duration(seconds: 40),
    );
    await capture('reopen-first-screen');
    // Provider traffic sampled right before opening the saved Thread: a cold
    // reopen that silently re-ran a prior model would add a conversation
    // request (catalog/usage probes are counted separately, not as Turns).
    final requestsBefore = options.longSession ? _wireRequestStats() : null;

    if (unopenedBanner) {
      await tapKey('studio-open-thread-$threadId');
    } else {
      await tapKey('thread-row-$threadId');
    }
    final opened = await waitFor(
      'saved Thread opened from SQL',
      (state) =>
          _threadId(state) == threadId &&
          _itemIds(state).isNotEmpty &&
          (previousTurnId == null || _lastTurnId(state) != null),
      timeout: const Duration(minutes: 3),
    );
    // Opening only restores state and subscription; it never resumes the model
    // or tools, so the last completed Turn must be unchanged.
    _require(
      _lastTurnId(opened) == previousTurnId,
      'opening the saved Thread must not execute a new Turn',
    );
    _require(
      (opened['workspace'] as Map)['isBusy'] == false,
      'opened Thread must not be busy',
    );
    await record('opened');
    final openedWindow = opened['timelineWindow'] as Map;
    final firstItemId = session['firstItemId'] as String?;
    final latestItemId = session['latestItemId'] as String?;
    if (options.longSession) {
      _require(
        firstItemId != null && latestItemId != null,
        'long-session reopen requires firstItemId/latestItemId from the create phase',
      );
      _checkBudgets(opened, openedWindow);
      // The restored window must be a bounded page: the long session holds more
      // items than one window, so older pages must still be reachable from SQL.
      _require(
        openedWindow['hasOlder'] == true,
        'the restored history window is not a bounded page of the long session',
      );
    }

    // After a restart the newest page is read from SQL, so the >256 KiB terminal
    // body must arrive as a truncated preview and only the explicit per-item read
    // may return the entire persisted body, trailing sentinel included.
    final largeBody = await _retrieveItemBody(largeItemId);
    _require(
      largeBody['noticeVisible'] == true,
      'the restored window did not expose the persisted large-body preview',
    );
    _require(
      largeBody['previewTruncated'] == true,
      'the restored window was not a bounded preview of the persisted body',
    );
    _require(
      largeBody['fullBodyRetrieved'] == true,
      'the entire persisted body was not loaded by canonical item id',
    );
    _require(
      largeBody['loadedByIdentity'] == true &&
          largeBody['previewFlagCleared'] == true,
      'the restored preview flag did not become an explicitly loaded body',
    );
    // 展开完整正文不得把读者抛到别处：窗口内容与目标行必须保持，且阅读位置语义不变
    // （原本钉在末尾的仍钉在末尾，原本脱离阅读的仍停在同一个可见锚点上）。判据只用
    // 阅读位置语义、锚点 item 身份与窗口身份；最上方可见行的像素偏移会随超大行高度
    // 变化，只作为观测记录，不作为判据。
    _require(
      largeBody['readingPositionStable'] == true,
      'loading the full body moved the reading position or dropped the target '
      'row (targetRowPresent=${largeBody['targetRowPresent']}, '
      'windowStable=${largeBody['windowStable']}, '
      'pinnedBefore=${largeBody['pinnedBefore']}, '
      'pinnedAfter=${largeBody['pinnedAfter']}, '
      'anchorItemBefore=${largeBody['anchorItemBefore']}, '
      'anchorItemAfter=${largeBody['anchorItemAfter']})',
    );

    final navigation = options.longSession
        ? await _navigateLongSession(
            threadId: threadId,
            firstItemId: firstItemId!,
            latestItemId: latestItemId!,
          )
        : await _navigateBoundedWindow();
    Map<String, Object?>? modelRequestGuard;
    if (options.longSession) {
      modelRequestGuard = _requireNoConversationRequests(
        requestsBefore!,
        'native cold reopen + deep paging',
      );
    }
    // Recovery check must settle (nothing left spinning); a retryable failure
    // keeps the banner and is recorded for the operator instead of passing.
    final recoverySettled = await finderDisappears(
      'recovery-check-status',
      timeout: const Duration(minutes: 3),
    );
    final finalState = await record('final');
    await writeJson('reopen.json', {
      'phase': 'reopen',
      'threadId': threadId,
      'largeItemId': largeItemId,
      'providerUrl': options.providerUrl,
      'selectionRestored': selectionRestored,
      'unopenedBannerVisible': unopenedBanner,
      'openedWithoutExecution': _lastTurnId(finalState) == previousTurnId,
      'largeBodyRetrieval': largeBody,
      'previewNoticeVisible': largeBody['noticeVisible'],
      'affordance': largeBody['affordance'],
      'previewTruncated': largeBody['previewTruncated'],
      'fullBodyRetrieved': largeBody['fullBodyRetrieved'],
      'windowStable': largeBody['windowStable'],
      'anchorItemStable': largeBody['anchorItemStable'],
      'followingBottomStable': largeBody['followingBottomStable'],
      'pinnedBefore': largeBody['pinnedBefore'],
      'pinnedAfter': largeBody['pinnedAfter'],
      'detachedAnchorItemStable': largeBody['detachedAnchorItemStable'],
      'anchorItemBefore': largeBody['anchorItemBefore'],
      'anchorItemAfter': largeBody['anchorItemAfter'],
      'anchorOffsetBefore': largeBody['anchorOffsetBefore'],
      'anchorOffsetAfter': largeBody['anchorOffsetAfter'],
      'targetRowPresent': largeBody['targetRowPresent'],
      'readingPositionStable': largeBody['readingPositionStable'],
      'noticeCleared': largeBody['noticeCleared'],
      'firstItemId': firstItemId,
      'latestItemId': latestItemId,
      'coldReopen': options.longSession
          ? {
              'initialWindowItemCount':
                  (openedWindow['itemIds'] as List).length,
              'initialWindowHasOlder': openedWindow['hasOlder'],
              'initialWindowHasNewer': openedWindow['hasNewer'],
              'boundedInitialWindow':
                  (openedWindow['itemIds'] as List).length <=
                  _historyWindowBudget,
              'firstIdentityReached': navigation['firstIdentityReached'],
              'olderPagesLoaded': navigation['olderPagesLoaded'],
              'latestIdentityReached': navigation['latestIdentityReached'],
              'cumulativeDistinctItems': navigation['cumulativeDistinctItems'],
              'olderRoundCount': (navigation['olderRounds'] as List?)?.length,
              'newerRoundCount': (navigation['newerRounds'] as List?)?.length,
            }
          : null,
      'modelRequestGuard': modelRequestGuard,
      'recoveryCheckSettled': recoverySettled,
      'navigation': navigation,
      'persistence': finalState['persistence'],
      'window': finalState['timelineWindow'],
    });
    await capture('reopen-complete');
  }
}

// ------------------------------------------------------------------- helpers

List<String> _itemIds(Map<String, dynamic> state) =>
    ((state['timelineWindow'] as Map)['itemIds'] as List).cast<String>();

String _timelineText(Map<String, dynamic> state) =>
    ((state['workspace'] as Map?)?['timeline'] as List? ?? [])
        .map((row) => (row as Map)['text'] ?? '')
        .join('\n');

String? _threadId(Map<String, dynamic> state) =>
    (state['workspace'] as Map?)?['threadId'] as String?;

String? _lastTurnId(Map<String, dynamic> state) =>
    ((state['workspace'] as Map?)?['lastTurn'] as Map?)?['id'] as String?;

String? _lastTurnStatus(Map<String, dynamic> state) =>
    ((state['workspace'] as Map?)?['lastTurn'] as Map?)?['status'] as String?;

String? _anchorItemId(Map<String, dynamic> state) =>
    ((state['timelineWindow'] as Map)['anchor'] as Map?)?['itemId'] as String?;

bool? _anchorFollowingBottom(Map<String, dynamic> state) =>
    ((state['timelineWindow'] as Map)['anchor'] as Map?)?['followingBottom']
        as bool?;

/// 阅读位置的语义采样：未发布锚点等价于"默认跟随末尾"，不是"位置未知"。
///
/// `timeline_view.dart` 没有锚点时默认 `_followingBottom = true`，因此
/// `anchor == null` 与 `anchor.followingBottom == true` 描述的是同一个阅读位置；把两者
/// 当成不同会把一次正常的正文展开误判成阅读位置漂移。锚点身份与偏移单独记录，便于
/// 脱离阅读时判断读者是否被挪到了另一行。
({bool pinned, String? itemId, double? offset}) _readingPosition(
  Map<String, dynamic> state,
) {
  final anchor = (state['timelineWindow'] as Map)['anchor'] as Map?;
  return (
    pinned: anchor == null || _anchorFollowingBottom(state) == true,
    itemId: _anchorItemId(state),
    offset: (anchor?['offset'] as num?)?.toDouble(),
  );
}

String? _projectPath(Map<String, dynamic> state) =>
    (state['project'] as Map?)?['path'] as String?;

/// Canonical id of the large assistant body in the bounded reading window.
///
/// The id comes from the window's own bounded-preview identity list (in reading
/// order), so it never depends on how many bytes the preview happens to render;
/// the item id is never synthesized from text or from a synthetic row identity.
String? _largeBodyItemId(Map<String, dynamic> state) {
  final previewed = _previewedItemIds(state);
  return previewed.isEmpty ? null : previewed.last;
}

List<String> _previewedItemIds(Map<String, dynamic> state) =>
    (((state['timelineWindow'] as Map)['previewedItemIds'] as List?) ??
            const [])
        .whereType<String>()
        .toList();

List<String> _loadedItemIds(Map<String, dynamic> state) =>
    (((state['timelineWindow'] as Map)['loadedItemIds'] as List?) ?? const [])
        .whereType<String>()
        .toList();

/// Per-Turn progression evidence: identities plus bounded-window facts only, so
/// 130+ Turns stay a compact artifact instead of per-Turn full snapshots.
Map<String, Object?> _turnRecord(int index, Map<String, dynamic> state) {
  final window = state['timelineWindow'] as Map;
  final ids = _itemIds(state);
  return {
    'turn': index,
    'turnId': _lastTurnId(state),
    'turnStatus': _lastTurnStatus(state),
    'itemCount': ids.length,
    'firstItemId': ids.firstOrNull,
    'lastItemId': ids.lastOrNull,
    'hasOlder': window['hasOlder'],
    'hasNewer': window['hasNewer'],
    'cacheCount': window['cacheCount'],
    'tailCount': window['tailCount'],
  };
}

/// One observed window of the deep-history walk.
Map<String, Object?> _walkRecord(int round, List<String> ids, Map window) => {
  'round': round,
  'itemCount': ids.length,
  'firstItemId': ids.firstOrNull,
  'lastItemId': ids.lastOrNull,
  'hasOlder': window['hasOlder'],
  'hasNewer': window['hasNewer'],
  'cacheCount': window['cacheCount'],
  'tailCount': window['tailCount'],
};

Map<String, Object?> _brief(Map<String, dynamic> state) => {
  'selectedThreadId': (state['navigation'] as Map?)?['selectedThreadId'],
  'workspaceThreadId': _threadId(state),
  'lastTurnId': _lastTurnId(state),
  'lastTurnStatus': _lastTurnStatus(state),
  'itemCount': _itemIds(state).length,
  'persistence': state['persistence'],
  'window': state['timelineWindow'],
};

/// True when no session state, DB-backed history or live tail is loaded yet.
bool _sessionNotOpened(Map<String, dynamic> state) {
  final window = state['timelineWindow'] as Map;
  return _itemIds(state).isEmpty &&
      window['cacheCount'] == 0 &&
      window['tailCount'] == 0 &&
      _timelineText(state).isEmpty &&
      _lastTurnId(state) == null;
}

void _require(bool condition, String message) {
  if (!condition) throw StateError(message);
}

String _normalized(String path) {
  var normalized = File(path).absolute.path.replaceAll('\\', '/');
  while (normalized.length > 1 && normalized.endsWith('/')) {
    normalized = normalized.substring(0, normalized.length - 1);
  }
  return Platform.isWindows ? normalized.toLowerCase() : normalized;
}

class _DriverOptions {
  const _DriverOptions({
    required this.phase,
    required this.vmServiceUrl,
    required this.output,
    required this.workspace,
    required this.providerUrl,
    required this.turns,
  });

  final String phase;
  final String vmServiceUrl;
  final String output;
  final String workspace;
  final String providerUrl;

  /// 0 keeps the fast two-Turn acceptance path; >= [_minLongTurns] runs the
  /// opt-in long-session walk that exceeds one bounded history window.
  final int turns;

  bool get longSession => turns > 0;

  static _DriverOptions parse(List<String> arguments) {
    final values = <String, String>{};
    for (var index = 0; index < arguments.length; index += 2) {
      if (index + 1 >= arguments.length || !arguments[index].startsWith('--')) {
        throw const FormatException('Expected --name value arguments');
      }
      values[arguments[index].substring(2)] = arguments[index + 1];
    }
    String required(String name) {
      final value = values[name];
      if (value == null || value.isEmpty) {
        throw FormatException('Missing --$name');
      }
      return value;
    }

    final turns = int.tryParse(values['turns'] ?? '0') ?? 0;
    if (turns != 0 && turns < _minLongTurns) {
      throw FormatException(
        '--turns must be 0 (two-Turn path) or >= $_minLongTurns',
      );
    }
    return _DriverOptions(
      phase: required('phase'),
      vmServiceUrl: required('vm-service-url'),
      output: required('output'),
      workspace: required('workspace'),
      providerUrl: values['provider-url'] ?? '',
      turns: turns,
    );
  }
}
