//! Durable evidence collected by the real workflow GUI acceptance Driver.

import 'dart:convert';

Map<String, dynamic>? workflowFromSnapshot(Map<String, dynamic> snapshot) {
  final workflow = snapshot['workflow'];
  return workflow is Map<String, dynamic> ? workflow : null;
}

class WorkflowAcceptanceEvidence {
  final Set<String> _visitedStates = {};
  final Set<String> _workflowTools = {};
  final Set<String> _planTools = {};
  final Set<String> _submittedPlans = {};
  var _successfulPlanSubmissions = 0;
  bool _sawClarification = false;
  bool _sawPlanRevisionRequest = false;
  bool _sawRevisedPlanApproval = false;

  bool get hasRevisedPlanApproval => _sawRevisedPlanApproval;

  void recordClarificationAnswer() {
    _sawClarification = true;
  }

  void recordPlanRevisionRequest() {
    _sawPlanRevisionRequest = true;
  }

  void recordRevisedPlanApproval() {
    _sawRevisedPlanApproval = true;
  }

  void observe(Map<String, dynamic> snapshot) {
    final run = workflowFromSnapshot(snapshot)?['currentRun'];
    if (run is Map) {
      final state = run['currentStateId'];
      if (state is String) _visitedStates.add(state);
      for (final forbidden in ['stages', 'history', 'definition', 'prompt']) {
        if (run.containsKey(forbidden)) {
          throw StateError('GUI workflow snapshot leaked `$forbidden`: $run');
        }
      }
    }
    final timeline =
        ((snapshot['workspace'] as Map?)?['timeline'] as List? ?? const [])
            .whereType<Map>();
    for (final row in timeline) {
      final tools = row['tools'];
      if (tools is! List) continue;
      for (final tool in tools.whereType<Map>()) {
        final name = tool['name'];
        if (name == 'workflow_state') {
          throw StateError('legacy workflow_state appeared in the real GUI');
        }
        if (name == 'submit_plan') {
          throw StateError('legacy submit_plan appeared in the real GUI');
        }
        if (name == 'request_user_input' && tool['status'] == 'succeeded') {
          _sawClarification = true;
        }
        if (name is String && name.startsWith('plan_')) {
          _planTools.add(name);
          if (name == 'plan_submit' && tool['status'] == 'succeeded') {
            _successfulPlanSubmissions += 1;
            final arguments = tool['arguments'];
            final decoded = arguments is String
                ? jsonDecode(arguments)
                : arguments;
            if (decoded is Map && decoded['plan'] is String) {
              _submittedPlans.add((decoded['plan'] as String).trim());
            }
            if (_successfulPlanSubmissions >= 2) {
              _sawPlanRevisionRequest = true;
            }
          }
          final arguments = tool['arguments'];
          final encoded = arguments is String
              ? arguments
              : jsonEncode(arguments);
          for (final forbidden in ['definition', 'compile', 'supersede']) {
            if (encoded.contains('"$forbidden"')) {
              throw StateError(
                '$name received forbidden graph-authoring input: $encoded',
              );
            }
          }
        }
        if (name is! String || !name.startsWith('workflow_')) continue;
        _workflowTools.add(name);
        final arguments = tool['arguments'];
        final encoded = arguments is String ? arguments : jsonEncode(arguments);
        for (final forbidden in ['definition', 'compile', 'supersede']) {
          if (encoded.contains('"$forbidden"')) {
            throw StateError(
              '$name received forbidden graph-authoring input: $encoded',
            );
          }
        }
        if (name == 'workflow_transition' && tool['status'] == 'succeeded') {
          final decoded = arguments is String
              ? jsonDecode(arguments)
              : arguments;
          if (decoded is Map && decoded['targetStateId'] is String) {
            final source = decoded['expectedStateId'];
            final target = decoded['targetStateId'] as String;
            if (source is String) _visitedStates.add(source);
            _visitedStates.add(target);
            if (source == 'planning' &&
                target == 'editing_documents' &&
                _successfulPlanSubmissions >= 2) {
              _sawRevisedPlanApproval = true;
            }
          }
        }
      }
    }
    for (final row in timeline) {
      final text = row['text'];
      if (text is String && _submittedPlans.contains(text.trim())) {
        throw StateError(
          'hidden Plan continuation was projected as a GUI timeline message',
        );
      }
    }
  }

  void validateTaskFlow() {
    if (!_sawClarification ||
        !_sawPlanRevisionRequest ||
        !_sawRevisedPlanApproval) {
      throw StateError(
        'missing clarification/revision/approval evidence: '
        '$_sawClarification/$_sawPlanRevisionRequest/'
        '$_sawRevisedPlanApproval',
      );
    }
    for (final state in [
      'planning',
      'editing_documents',
      'working',
      'integrating',
      'reviewing',
      'completed',
    ]) {
      if (!_visitedStates.contains(state)) {
        throw StateError(
          'workflow transition trace is missing $state: $_visitedStates',
        );
      }
    }
    if (!_workflowTools.contains('workflow_transition')) {
      throw StateError('workflow_transition was never called: $_workflowTools');
    }
    if (!_workflowTools.any(
      const {
        'workflow_current',
        'workflow_next',
        'workflow_graph',
        'workflow_history',
      }.contains,
    )) {
      throw StateError(
        'no split workflow query tool was called: $_workflowTools',
      );
    }
    for (final required in [
      'plan_current',
      'plan_next',
      'plan_history',
      'plan_submit',
    ]) {
      if (!_planTools.contains(required)) {
        throw StateError(
          'required Plan tool $required is missing: $_planTools',
        );
      }
    }
    if (_successfulPlanSubmissions < 2) {
      throw StateError(
        'expected two successful plan_submit calls, got '
        '$_successfulPlanSubmissions',
      );
    }
  }
}
