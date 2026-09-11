import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';

import '../test_driver/workflow_acceptance_evidence.dart';

void main() {
  test(
    'canonical failed turn stops acceptance even after active turn clears',
    () {
      final evidence = WorkflowAcceptanceEvidence();
      expect(
        () => evidence.observe({
          'workspace': {
            'isBusy': false,
            'turn': null,
            'lastTurn': {'status': 'failed', 'reason': 'model output rejected'},
            'timeline': [],
          },
        }),
        throwsStateError,
      );
    },
  );

  test(
    'minimal completed workflow requires submissions and actual verification',
    () {
      for (final missing in ['read_agent_submissions', 'exec']) {
        final snapshot = _minimalSnapshot();
        _tools(snapshot).removeWhere((tool) => tool['name'] == missing);
        final evidence = WorkflowAcceptanceEvidence()..observe(snapshot);
        expect(
          () => evidence.validateMinimalFlow(snapshot),
          throwsStateError,
          reason: 'completed must not hide missing $missing',
        );
      }
    },
  );

  test('minimal completed workflow preserves invalid completion failure', () {
    final snapshot = _minimalSnapshot();
    _tools(snapshot).add({
      'name': 'workflow_transition',
      'callId': 'invalid-completion',
      'status': 'failed',
      'arguments': '{}',
      'result': 'unknown field reason_note',
    });
    final evidence = WorkflowAcceptanceEvidence()..observe(snapshot);
    expect(() => evidence.validateMinimalFlow(snapshot), throwsStateError);
  });

  test('minimal approval must belong to the spawned reviewer', () {
    final snapshot = _minimalSnapshot();
    _tools(snapshot).firstWhere(
      (t) => t['name'] == 'read_agent_submissions',
    )['arguments'] = jsonEncode({
      'target': 'unrelated-agent',
    });
    final evidence = WorkflowAcceptanceEvidence()..observe(snapshot);
    expect(() => evidence.validateMinimalFlow(snapshot), throwsStateError);
  });

  test('minimal complete evidence is accepted', () {
    final snapshot = _minimalSnapshot();
    final evidence = WorkflowAcceptanceEvidence()..observe(snapshot);
    expect(() => evidence.validateMinimalFlow(snapshot), returnsNormally);
  });

  test('rebuilds interaction evidence from a durable resumed timeline', () {
    final evidence = WorkflowAcceptanceEvidence();

    evidence.observe(_completedSnapshot(includeRevision: true));

    expect(evidence.validateTaskFlow, returnsNormally);
  });

  test('rejects a durable timeline that skipped plan revision', () {
    final evidence = WorkflowAcceptanceEvidence();

    evidence.observe(_completedSnapshot(includeRevision: false));

    expect(
      evidence.validateTaskFlow,
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          contains('missing clarification/revision/approval evidence'),
        ),
      ),
    );
  });

  test(
    'rejects a hidden Plan continuation projected into the GUI timeline',
    () {
      final evidence = WorkflowAcceptanceEvidence();
      final snapshot = jsonDecode(
        jsonEncode(_completedSnapshot(includeRevision: true)),
      ) as Map<String, dynamic>;
      final workspace = snapshot['workspace'] as Map<String, dynamic>;
      final timeline = workspace['timeline'] as List<dynamic>;
      timeline.add(<String, dynamic>{
        'type': 'userMessage',
        'text': '# Revised',
        'tools': <Object>[],
      });

      expect(
        () => evidence.observe(snapshot),
        throwsA(
          isA<StateError>().having(
            (error) => error.message,
            'message',
            contains('hidden Plan continuation'),
          ),
        ),
      );
    },
  );

  test('accepts a revised and approved Plan-only timeline', () {
    final evidence = WorkflowAcceptanceEvidence()
      ..recordPlanRevisionRequest()
      ..recordRevisedPlanApproval();

    evidence.observe(_planOnlySnapshot());

    expect(evidence.canonicalPlanState, 'approved');
    expect(evidence.validatePlanOnlyFlow, returnsNormally);
  });

  test('Plan-only rejects request_user_input', () {
    final evidence = WorkflowAcceptanceEvidence()
      ..recordPlanRevisionRequest()
      ..recordRevisedPlanApproval();
    final snapshot = _planOnlySnapshot();
    _tools(snapshot).add({
      'callId': 'ask-1',
      'name': 'request_user_input',
      'status': 'succeeded',
      'arguments': '{}',
    });

    evidence.observe(snapshot);

    expect(
      evidence.validatePlanOnlyFlow,
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          contains('called request_user_input'),
        ),
      ),
    );
  });

  test('Plan-only rejects workflow_transition', () {
    final evidence = WorkflowAcceptanceEvidence()
      ..recordPlanRevisionRequest()
      ..recordRevisedPlanApproval();
    final snapshot = _planOnlySnapshot();
    _tools(snapshot).add({
      'callId': 'transition-1',
      'name': 'workflow_transition',
      'status': 'succeeded',
      'arguments': jsonEncode(_transition('planning', 'editing_documents')),
    });

    evidence.observe(snapshot);

    expect(
      evidence.validatePlanOnlyFlow,
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          contains('called workflow_transition'),
        ),
      ),
    );
  });

  test('Plan-only rejects a missing approved plan_current state', () {
    final evidence = WorkflowAcceptanceEvidence()
      ..recordPlanRevisionRequest()
      ..recordRevisedPlanApproval();
    final snapshot = _planOnlySnapshot(planState: 'awaitingConfirmation');

    evidence.observe(snapshot);

    expect(
      evidence.validatePlanOnlyFlow,
      throwsA(
        isA<StateError>().having(
          (error) => error.message,
          'message',
          contains('plan_current.state == approved'),
        ),
      ),
    );
  });
}

Map<String, dynamic> _completedSnapshot({required bool includeRevision}) {
  final transitions = <Map<String, String>>[
    _transition('planning', 'editing_documents'),
    _transition('editing_documents', 'working'),
    _transition('working', 'integrating'),
    _transition('integrating', 'reviewing'),
    _transition('reviewing', 'completed'),
  ];
  return {
    'workflow': {
      'currentRun': {'currentStateId': 'completed', 'terminal': true},
    },
    'workspace': {
      'timeline': [
        {
          'tools': [
            {
              'name': 'request_user_input',
              'status': 'succeeded',
              'arguments': '{}',
            },
            {
              'name': 'workflow_current',
              'status': 'succeeded',
              'arguments': '{}',
            },
            {'name': 'plan_current', 'status': 'succeeded', 'arguments': '{}'},
            {'name': 'plan_next', 'status': 'succeeded', 'arguments': '{}'},
            {'name': 'plan_history', 'status': 'succeeded', 'arguments': '{}'},
            {
              'name': 'plan_submit',
              'status': 'succeeded',
              'arguments': '{"expectedRevision":0,"plan":"# Plan"}',
            },
            if (includeRevision)
              {
                'name': 'plan_submit',
                'status': 'succeeded',
                'arguments': '{"expectedRevision":2,"plan":"# Revised"}',
              },
            for (final transition in transitions)
              {
                'name': 'workflow_transition',
                'status': 'succeeded',
                'arguments': jsonEncode(transition),
              },
          ],
        },
      ],
    },
  };
}

Map<String, String> _transition(String source, String target) => {
  'expectedRunId': 'run-test',
  'expectedRevision': '1',
  'expectedStateId': source,
  'targetStateId': target,
};

Map<String, dynamic> _planOnlySnapshot({String planState = 'approved'}) => {
  'workflow': {
    'currentRun': {'currentStateId': 'planning', 'terminal': false},
  },
  'workspace': {
    'timeline': [
      {
        'id': 'tools-1',
        'tools': [
          {
            'callId': 'submit-1',
            'name': 'plan_submit',
            'status': 'succeeded',
            'arguments': '{"expectedRevision":0,"plan":"# Initial Plan"}',
          },
          {
            'callId': 'submit-2',
            'name': 'plan_submit',
            'status': 'succeeded',
            'arguments': '{"expectedRevision":2,"plan":"# Revised Plan"}',
          },
          {
            'callId': 'current-1',
            'name': 'plan_current',
            'status': 'succeeded',
            'arguments': '{}',
            'result': jsonEncode({'state': planState}),
          },
        ],
      },
    ],
  },
};

List<dynamic> _tools(Map<String, dynamic> snapshot) {
  final workspace = snapshot['workspace'] as Map<String, dynamic>;
  final timeline = workspace['timeline'] as List<dynamic>;
  final row = timeline.single as Map<String, dynamic>;
  return row['tools'] as List<dynamic>;
}

Map<String, dynamic> _minimalSnapshot() {
  final snapshot = jsonDecode(
    jsonEncode(_completedSnapshot(includeRevision: false)),
  ) as Map<String, dynamic>;
  final workspace = snapshot['workspace'] as Map<String, dynamic>;
  workspace['agents'] = [];
  _tools(snapshot).addAll([
    for (final name in [
      'spawn_agent',
      'read_agent_submissions',
      'close_agent',
      'exec',
      'complete',
    ])
      {
        'name': name,
        'callId': name,
        'status': 'succeeded',
        'arguments': jsonEncode(
          name == 'spawn_agent'
              ? {'profileId': 'reviewer'}
              : name == 'complete'
              ? {'summary': '结果：hello.txt\n\n验证：通过\n\n剩余问题：无'}
              : {'target': 'reviewer-1'},
        ),
        'result': name == 'exec'
            ? 'PURE_MINIMAL_VERIFY_OK'
            : jsonEncode(
                name == 'spawn_agent'
                    ? {'agentId': 'reviewer-1'}
                    : {
                        'items': [
                          {'summary': 'REVIEWER_READ_ONLY_APPROVED'},
                        ],
                      },
              ),
      },
  ]);
  return snapshot;
}
