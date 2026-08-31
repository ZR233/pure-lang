import 'package:flutter_test/flutter_test.dart';

import '../test_driver/subagents_acceptance_driver.dart';

void main() {
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
