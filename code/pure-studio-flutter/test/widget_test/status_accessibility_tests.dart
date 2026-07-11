part of '../widget_test.dart';

void registerStatusAccessibilityTests() {
  group('status detail accessibility', () {
    testWidgets('context readout is a keyboard-operable semantic button', (
      tester,
    ) async {
      await tester.pumpWidget(
        _localizedApp(
          home: const Scaffold(
            body: Align(
              alignment: Alignment.bottomLeft,
              child: ContextUsageReadout(runtime: _contextRuntime),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      final contextButton = find.bySemanticsLabel('Context');
      expect(contextButton, findsOneWidget);
      var semantics = tester.getSemantics(contextButton);
      var flags = semantics.getSemanticsData().flagsCollection;
      expect(flags.isButton, isTrue);
      expect(flags.isFocused, isNot(Tristate.none));
      expect(semantics.getSemanticsData().value, '42%');
      expect(
        semantics.getSemanticsData().hasAction(SemanticsAction.tap),
        isTrue,
      );

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      semantics = tester.getSemantics(contextButton);
      flags = semantics.getSemanticsData().flagsCollection;
      expect(flags.isFocused, Tristate.isTrue);

      await tester.sendKeyEvent(LogicalKeyboardKey.enter);
      await tester.pumpAndSettle();
      expect(find.text('42 / 100'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.escape);
      await tester.pumpAndSettle();
      expect(find.text('42 / 100'), findsNothing);

      await tester.sendKeyEvent(LogicalKeyboardKey.space);
      await tester.pumpAndSettle();
      expect(find.text('42 / 100'), findsOneWidget);
    });

    testWidgets('context detail keeps hover behavior and shared radius', (
      tester,
    ) async {
      await tester.pumpWidget(
        _localizedApp(
          home: const Scaffold(
            body: Align(
              alignment: Alignment.bottomLeft,
              child: ContextUsageReadout(runtime: _contextRuntime),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      final contextButton = find.bySemanticsLabel('Context');
      final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
      addTearDown(gesture.removePointer);
      await gesture.addPointer();
      await gesture.moveTo(tester.getCenter(contextButton));
      await tester.pumpAndSettle();

      final detailValue = find.text('42 / 100');
      expect(detailValue, findsOneWidget);
      final detailCard = find.ancestor(
        of: detailValue,
        matching: find.byWidgetPredicate(_isLiftedDetailCard),
      );
      expect(detailCard, findsOneWidget);
      final decoration =
          tester.widget<DecoratedBox>(detailCard).decoration as BoxDecoration;
      expect(decoration.borderRadius, BorderRadius.circular(StudioRadii.md));

      await gesture.moveTo(Offset.zero);
      await tester.pump(const Duration(milliseconds: 150));
      expect(detailValue, findsNothing);
    });

    testWidgets('context detail survives quick focus return', (tester) async {
      final contextButton = await _pumpContextWithNextFocus(tester);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.space);
      await tester.pump();
      expect(find.text('42 / 100'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump(const Duration(milliseconds: 60));
      await tester.sendKeyDownEvent(LogicalKeyboardKey.shiftLeft);
      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.sendKeyUpEvent(LogicalKeyboardKey.shiftLeft);
      await tester.pump();
      expect(
        tester
            .getSemantics(contextButton)
            .getSemanticsData()
            .flagsCollection
            .isFocused,
        Tristate.isTrue,
      );

      await tester.pump(const Duration(milliseconds: 80));
      expect(find.text('42 / 100'), findsOneWidget);
    });

    testWidgets('context detail stays open when focused pointer exits', (
      tester,
    ) async {
      final contextButton = await _pumpContextWithNextFocus(tester);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.space);
      await tester.pump();
      expect(find.text('42 / 100'), findsOneWidget);

      final gesture = await tester.createGesture(kind: PointerDeviceKind.mouse);
      addTearDown(gesture.removePointer);
      await gesture.addPointer();
      await gesture.moveTo(tester.getCenter(contextButton));
      await tester.pump();
      await gesture.moveTo(Offset.zero);
      await tester.pump(const Duration(milliseconds: 150));

      expect(
        tester
            .getSemantics(contextButton)
            .getSemanticsData()
            .flagsCollection
            .isFocused,
        Tristate.isTrue,
      );
      expect(find.text('42 / 100'), findsOneWidget);
    });

    testWidgets('context detail closes after focus leaves', (tester) async {
      await _pumpContextWithNextFocus(tester);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump();
      await tester.sendKeyEvent(LogicalKeyboardKey.space);
      await tester.pump();
      expect(find.text('42 / 100'), findsOneWidget);

      await tester.sendKeyEvent(LogicalKeyboardKey.tab);
      await tester.pump(const Duration(milliseconds: 150));
      expect(find.text('42 / 100'), findsNothing);
    });
  });
}

Future<Finder> _pumpContextWithNextFocus(WidgetTester tester) async {
  await tester.pumpWidget(
    _localizedApp(
      home: Scaffold(
        body: Align(
          alignment: Alignment.bottomLeft,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const ContextUsageReadout(
                key: _contextFocusTargetKey,
                runtime: _contextRuntime,
              ),
              TextButton(onPressed: () {}, child: const Text('Next focus')),
            ],
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
  final contextButton = find.descendant(
    of: find.byKey(_contextFocusTargetKey),
    matching: find.bySemanticsLabel('Context'),
  );
  expect(contextButton, findsOneWidget);
  return contextButton;
}

const _contextFocusTargetKey = ValueKey('context-focus-target');

const _contextRuntime = SessionRuntimeView(
  model: 'planner/local',
  contextTokens: 42,
  contextWindow: 100,
  totalTokens: 128,
  costLabel: '',
  activeSkills: [],
  activeMcpServers: [],
  activeLspServers: [],
  agentCount: 0,
);
