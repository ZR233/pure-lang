import 'package:flutter_test/flutter_test.dart';

import '../test_driver/subagents_acceptance_driver.dart';

void main() {
  Map<String, dynamic> snapshot({
    bool busy = false,
    Object? interaction,
    String stage = 'completed',
    bool terminal = true,
    List<String> roles = const [
      'explorer',
      'executor',
      'worktree_executor',
      'reviewer',
    ],
    List<Map<String, dynamic>> timeline = const [
      {'type': 'finalAnswer', 'text': 'PURE_SUBAGENTS_LIVE_OK'},
    ],
  }) => {
    'workspace': {
      'isBusy': busy,
      'activeInteraction': interaction,
      'agents': [
        for (final role in roles) {'role': role},
      ],
      'timeline': timeline,
    },
    'workflow': {
      'currentRun': {'currentStageId': stage, 'terminal': terminal},
    },
  };

  test('user prompt sentinel while awaiting confirmation is not complete', () {
    expect(
      subagentsAcceptanceCompleted(
        snapshot(
          stage: 'awaiting_confirmation',
          terminal: false,
          timeline: const [
            {'type': 'userMessage', 'text': 'run PURE_SUBAGENTS_LIVE_OK'},
          ],
          roles: const ['explorer'],
        ),
      ),
      isFalse,
    );
  });

  test('final answer sentinel before completed workflow is not complete', () {
    expect(
      subagentsAcceptanceCompleted(snapshot(stage: 'awaiting_confirmation')),
      isFalse,
    );
  });

  test('completed final answer missing required role is not complete', () {
    expect(
      subagentsAcceptanceCompleted(
        snapshot(roles: const ['explorer', 'executor', 'reviewer']),
      ),
      isFalse,
    );
  });

  test('completed workflow with busy root or interaction is not complete', () {
    expect(subagentsAcceptanceCompleted(snapshot(busy: true)), isFalse);
    expect(
      subagentsAcceptanceCompleted(
        snapshot(interaction: {'kind': 'userInput'}),
      ),
      isFalse,
    );
  });

  test(
    'completed terminal workflow with all roles and final answer is complete',
    () {
      expect(subagentsAcceptanceCompleted(snapshot()), isTrue);
    },
  );

  test('commentary sentinel does not count as completion', () {
    expect(
      subagentsAcceptanceCompleted(
        snapshot(
          timeline: const [
            {'type': 'commentary', 'text': 'PURE_SUBAGENTS_LIVE_OK'},
          ],
        ),
      ),
      isFalse,
    );
  });

  test('same target with unchanged revision is canonical', () {
    expect(
      canonicalRouteReached(
        beforeProvider: 'openai',
        beforeModel: 'gpt-5',
        beforeRevision: 7,
        currentProvider: 'openai',
        currentModel: 'gpt-5',
        currentRevision: 7,
        targetProvider: 'openai',
        targetModel: 'gpt-5',
      ),
      isTrue,
    );
  });

  test('different target with unchanged revision is not canonical', () {
    expect(
      canonicalRouteReached(
        beforeProvider: 'openai',
        beforeModel: 'gpt-4',
        beforeRevision: 7,
        currentProvider: 'openai',
        currentModel: 'gpt-4',
        currentRevision: 7,
        targetProvider: 'openai',
        targetModel: 'gpt-5',
      ),
      isFalse,
    );
  });

  test('different target with advanced revision is canonical', () {
    expect(
      canonicalRouteReached(
        beforeProvider: 'openai',
        beforeModel: 'gpt-4',
        beforeRevision: 7,
        currentProvider: 'openai',
        currentModel: 'gpt-5',
        currentRevision: 8,
        targetProvider: 'openai',
        targetModel: 'gpt-5',
      ),
      isTrue,
    );
  });

  test('advanced revision without target route is not canonical', () {
    expect(
      canonicalRouteReached(
        beforeProvider: 'openai',
        beforeModel: 'gpt-4',
        beforeRevision: 7,
        currentProvider: 'anthropic',
        currentModel: 'claude',
        currentRevision: 8,
        targetProvider: 'openai',
        targetModel: 'gpt-5',
      ),
      isFalse,
    );
  });
}
