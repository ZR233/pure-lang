import 'package:flutter_test/flutter_test.dart';

import '../test_driver/subagents_acceptance_driver.dart';

void main() {
  Map<String, dynamic> snapshot({
    bool busy = false,
    Object? interaction,
    String state = 'completed',
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
      'currentRun': {'currentStateId': state, 'terminal': terminal},
    },
  };

  test(
    'completion requires the terminal workflow, roles, and final answer',
    () {
      final cases = [
        (
          name: 'user prompt sentinel',
          value: snapshot(
            state: 'planning',
            terminal: false,
            timeline: const [
              {'type': 'userMessage', 'text': 'run PURE_SUBAGENTS_LIVE_OK'},
            ],
            roles: const ['explorer'],
          ),
          expected: false,
        ),
        (
          name: 'final answer before completed workflow',
          value: snapshot(state: 'planning'),
          expected: false,
        ),
        (
          name: 'missing required role',
          value: snapshot(roles: const ['explorer', 'executor', 'reviewer']),
          expected: false,
        ),
        (name: 'busy root', value: snapshot(busy: true), expected: false),
        (
          name: 'pending interaction',
          value: snapshot(interaction: {'kind': 'userInput'}),
          expected: false,
        ),
        (
          name: 'commentary sentinel',
          value: snapshot(
            timeline: const [
              {'type': 'commentary', 'text': 'PURE_SUBAGENTS_LIVE_OK'},
            ],
          ),
          expected: false,
        ),
        (name: 'complete workflow', value: snapshot(), expected: true),
      ];

      for (final scenario in cases) {
        expect(
          subagentsAcceptanceCompleted(scenario.value),
          scenario.expected,
          reason: scenario.name,
        );
      }
    },
  );

  test(
    'canonical route requires the target and a valid revision transition',
    () {
      final cases = [
        (
          name: 'unchanged canonical target',
          beforeModel: 'gpt-5',
          currentProvider: 'openai',
          currentModel: 'gpt-5',
          currentRevision: 7,
          expected: true,
        ),
        (
          name: 'unchanged different target',
          beforeModel: 'gpt-4',
          currentProvider: 'openai',
          currentModel: 'gpt-4',
          currentRevision: 7,
          expected: false,
        ),
        (
          name: 'advanced canonical target',
          beforeModel: 'gpt-4',
          currentProvider: 'openai',
          currentModel: 'gpt-5',
          currentRevision: 8,
          expected: true,
        ),
        (
          name: 'advanced wrong target',
          beforeModel: 'gpt-4',
          currentProvider: 'anthropic',
          currentModel: 'claude',
          currentRevision: 8,
          expected: false,
        ),
      ];

      for (final scenario in cases) {
        expect(
          canonicalRouteReached(
            beforeProvider: 'openai',
            beforeModel: scenario.beforeModel,
            beforeRevision: 7,
            currentProvider: scenario.currentProvider,
            currentModel: scenario.currentModel,
            currentRevision: scenario.currentRevision,
            targetProvider: 'openai',
            targetModel: 'gpt-5',
          ),
          scenario.expected,
          reason: scenario.name,
        );
      }
    },
  );

  test('hidden Plan continuation cannot appear in the GUI timeline', () {
    final value = snapshot(
      timeline: [
        {
          'tools': [
            {
              'name': 'plan_submit',
              'status': 'succeeded',
              'arguments': '{"expectedRevision":0,"plan":"# Approved"}',
            },
          ],
        },
        {'type': 'userMessage', 'text': '# Approved'},
      ],
    );

    expect(
      () => validateSubagentSnapshotProjection(value),
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          contains('hidden Plan continuation'),
        ),
      ),
    );
  });
}
