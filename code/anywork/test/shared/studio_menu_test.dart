import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:anywork/src/shared/studio_menu.dart';

void main() {
  testWidgets('opening a menu does not scroll its outer page', (tester) async {
    final outerController = ScrollController();
    addTearDown(outerController.dispose);

    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SizedBox(
            height: 320,
            child: SingleChildScrollView(
              controller: outerController,
              child: Column(
                children: [
                  const SizedBox(height: 700),
                  StudioMenu<String>(
                    key: const ValueKey('effort-selector'),
                    tooltip: 'Reasoning effort',
                    menuConstraints: const BoxConstraints(
                      minWidth: 160,
                      maxWidth: 160,
                      maxHeight: 120,
                    ),
                    itemBuilder: (context) => [
                      for (final effort in const ['low', 'medium', 'high'])
                        StudioMenuItem<String>(
                          value: effort,
                          selected: effort == 'high',
                          itemKey: ValueKey('effort-$effort'),
                          child: Text(effort),
                        ),
                    ],
                    child: const Text('high'),
                  ),
                  const SizedBox(height: 500),
                ],
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    outerController.jumpTo(520);
    await tester.pump();
    final offsetBeforeOpen = outerController.offset;

    await tester.tap(find.byKey(const ValueKey('effort-selector')));
    await tester.pump();
    await tester.pump();

    expect(find.text('low'), findsOneWidget);
    expect(outerController.offset, offsetBeforeOpen);
  });
}
