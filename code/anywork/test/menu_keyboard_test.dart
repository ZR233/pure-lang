import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:anywork/src/shared/upward_popup_menu.dart';

void main() {
  testWidgets('text menu opens with keyboard and commits selected value', (
    tester,
  ) async {
    String? selected;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Align(
            alignment: Alignment.bottomLeft,
            child: UpwardPopupMenu<String>(
              tooltip: 'Choose mode',
              onSelected: (value) => selected = value,
              itemBuilder: (_) => const [
                PopupMenuItem(value: 'task', child: Text('Task')),
              ],
              child: const StudioMenuLabel(label: 'Simple'),
            ),
          ),
        ),
      ),
    );
    await tester.sendKeyEvent(LogicalKeyboardKey.tab);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(find.text('Task'), findsOneWidget);
    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(selected, 'task');
    expect(find.text('Task'), findsNothing);
  });
}
