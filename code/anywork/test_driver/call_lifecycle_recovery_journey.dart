import 'dart:convert';
import 'dart:io';

import 'realtime_journey.dart';
import 'websocket_recovery_journey.dart';

Future<void> main(List<String> args) => runRecoveryJourney(
  args,
  CallLifecycleRecoveryJourney.new,
  protocol: 'responsesHttp',
);

class CallLifecycleRecoveryJourney extends WebSocketRecoveryJourney {
  CallLifecycleRecoveryJourney(super.driver, super.args) : home = args[6];

  final String home;

  bool catalogIssue(Map<String, dynamic> snapshot) =>
      (snapshot['recoveryIssues'] as List? ?? const []).any(
        (issue) =>
            issue['category'] == 'toolCatalog' &&
            (issue['actions'] as List).contains('retry'),
      );

  @override
  Future<void> run() async {
    if (phase == 'restart') {
      await super.run();
      return;
    }
    await tap('sidebar-open-project');
    await tap('add-project-local');
    await tap('add-project-continue-ready');
    await tap('project-path-input');
    await driver.enterText(project);
    await tap('project-path-submit');
    await submit('Lifecycle seed');
    await waitFor(
      (s) => answerContains(s, 'Lifecycle seed answer') && settled(s),
      'seed',
    );
    await tap('session-mode-selector');
    await tap('session-mode-mode.task');
    await waitFor(
      (s) => workspaceOf(s)?['threadMode'] == 'mode.task',
      'task-mode',
    );

    // Only this harness-owned home is modified; the app still uses the regular
    // discovery command and Thread tool transfer, without injected diagnostics.
    final badSkill = File('$home/skills/broken/SKILL.md');
    await badSkill.parent.create(recursive: true);
    await badSkill.writeAsString('---\nname: [invalid YAML\n---\nbroken');
    await tap('settings-open');
    await tap('settings-tab-skills');
    await tap('skills-discover');
    await waitFor(catalogIssue, 'catalog-failed');
    await tap('settings-back');
    final selected =
        workspaceOf(await driver.readSnapshot())?['threadId'] as String;
    await tap('thread-row-$selected');
    await capture('catalog-failed');

    await submit('Lifecycle failed');
    final failed = await waitFor(
      (s) => settled(s) && turnStatus(s) == 'failed',
      'compaction-exhausted',
    );
    if (!jsonEncode(failed).contains('compaction unavailable') ||
        !jsonEncode(failed).contains('retryable')) {
      throw StateError('compaction lost its structured provider failure');
    }
    await capture('compaction-exhausted');
    await submit('Lifecycle wait');
    await waitFor(
      (s) =>
          isBusy(s) &&
          timelineRows(s).any(
            (row) => (row['tools'] as List? ?? const []).any(
              (tool) => tool['name'] == 'wait' && tool['status'] == 'running',
            ),
          ),
      'wait-running-with-diagnostic',
    );
    await capture('wait-running');
    await badSkill.delete();
    final revision = (await driver.readSnapshot())['settings']['revision'];
    await tap('thread-menu-$selected');
    await tap('recovery-retry-tool-refresh:$selected');
    await waitFor((s) => !catalogIssue(s), 'catalog-retried');
    if ((await driver.readSnapshot())['settings']['revision'] != revision) {
      throw StateError('explicit retry depended on a settings revision');
    }
    await capture('catalog-retried');
    await tap('composer-stop');
    await waitFor(
      (s) => settled(s) && turnStatus(s) == 'cancelled',
      'wait-stopped',
    );
    await capture('wait-stopped');
    await coordinator('cancel');
    await submit('Lifecycle next');
    final next = await waitFor(
      (s) => answerContains(s, 'Lifecycle next answer') && settled(s),
      'next',
    );
    for (final text in [
      'Lifecycle seed answer',
      'Lifecycle failed',
      'Lifecycle wait',
      'Lifecycle next answer',
    ]) {
      single(next, text);
    }
    await saveObserved();
    await capture('before-close');
  }
}
