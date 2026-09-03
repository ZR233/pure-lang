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
  final Set<String> _observedToolCalls = {};
  final Set<String> _observedSuccessfulToolCalls = {};
  final Set<String> _failedPlanTools = {};
  var _successfulPlanSubmissions = 0;
  var _requestUserInputCalls = 0;
  var _workflowTransitionCalls = 0;
  var _completeCalls = 0;
  bool _sawClarification = false;
  bool _sawPlanRevisionRequest = false;
  bool _sawRevisedPlanApproval = false;
  String? _canonicalPlanState;

  bool get hasRevisedPlanApproval => _sawRevisedPlanApproval;
  String? get canonicalPlanState => _canonicalPlanState;

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
        final identity = _toolCallIdentity(row, tool);
        final firstObservation = _observedToolCalls.add(identity);
        final firstSuccessfulObservation =
            tool['status'] == 'succeeded' &&
            _observedSuccessfulToolCalls.add(identity);
        if (name == 'workflow_state') {
          throw StateError('legacy workflow_state appeared in the real GUI');
        }
        if (name == 'submit_plan') {
          throw StateError('legacy submit_plan appeared in the real GUI');
        }
        if (name == 'request_user_input' && firstObservation) {
          _requestUserInputCalls += 1;
          if (tool['status'] == 'succeeded') {
            _sawClarification = true;
          }
        }
        if (name == 'complete' && firstObservation) {
          _completeCalls += 1;
        }
        if (name is String && name.startsWith('plan_')) {
          _planTools.add(name);
          if (tool['status'] == 'failed') {
            _failedPlanTools.add(name);
          }
          if (name == 'plan_submit' && firstSuccessfulObservation) {
            _successfulPlanSubmissions += 1;
            final decoded = _decodeMap(tool['arguments']);
            if (decoded != null && decoded['plan'] is String) {
              _submittedPlans.add((decoded['plan'] as String).trim());
            }
            if (_successfulPlanSubmissions >= 2) {
              _sawPlanRevisionRequest = true;
            }
          }
          if (name == 'plan_current' && tool['status'] == 'succeeded') {
            final result = _decodeMap(tool['result']);
            final state = result?['state'];
            if (state is String) _canonicalPlanState = state;
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
        if (name == 'workflow_transition' && firstObservation) {
          _workflowTransitionCalls += 1;
        }
        if (name == 'workflow_transition' && tool['status'] == 'succeeded') {
          final decoded = _decodeMap(arguments);
          if (decoded != null && decoded['targetStateId'] is String) {
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

  void validatePlanOnlyFlow() {
    if (_requestUserInputCalls != 0) {
      throw StateError(
        'Plan-only flow called request_user_input '
        '$_requestUserInputCalls time(s)',
      );
    }
    if (!_sawPlanRevisionRequest || !_sawRevisedPlanApproval) {
      throw StateError(
        'missing Plan-only revision/approval evidence: '
        '$_sawPlanRevisionRequest/$_sawRevisedPlanApproval',
      );
    }
    if (_successfulPlanSubmissions < 2 || _submittedPlans.length < 2) {
      throw StateError(
        'expected two distinct successful plan_submit calls, got '
        '$_successfulPlanSubmissions/${_submittedPlans.length}',
      );
    }
    if (_submittedPlans.any((plan) => !plan.trimLeft().startsWith('# '))) {
      throw StateError(
        'plan_submit omitted a complete top-level Markdown Plan',
      );
    }
    if (!_planTools.contains('plan_current') ||
        !_planTools.contains('plan_submit')) {
      throw StateError(
        'Plan-only flow is missing canonical Plan tools: $_planTools',
      );
    }
    if (_canonicalPlanState != 'approved') {
      throw StateError(
        'Plan-only flow never observed plan_current.state == approved: '
        '$_canonicalPlanState',
      );
    }
    if (_workflowTransitionCalls != 0) {
      throw StateError(
        'Plan-only flow called workflow_transition '
        '$_workflowTransitionCalls time(s)',
      );
    }
    if (_completeCalls != 0) {
      throw StateError(
        'Plan-only flow called complete $_completeCalls time(s)',
      );
    }
    if (_visitedStates.length != 1 || !_visitedStates.contains('planning')) {
      throw StateError('Plan-only workflow left planning: $_visitedStates');
    }
    if (_failedPlanTools.isNotEmpty) {
      throw StateError(
        'Plan-only flow has failed Plan tools: $_failedPlanTools',
      );
    }
  }
}

String _toolCallIdentity(Map row, Map tool) {
  final callId = tool['callId'];
  if (callId is String && callId.isNotEmpty) return callId;
  return jsonEncode([row['id'], tool['name'], tool['arguments']]);
}

Map<String, dynamic>? _decodeMap(Object? value) {
  if (value is Map<String, dynamic>) return value;
  if (value is Map) return value.cast<String, dynamic>();
  if (value is! String) return null;
  try {
    final decoded = jsonDecode(value);
    if (decoded is Map<String, dynamic>) return decoded;
    if (decoded is Map) return decoded.cast<String, dynamic>();
  } on FormatException {
    return null;
  }
  return null;
}
