import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pure_studio/src/l10n/app_localizations.dart';
import 'package:pure_studio/src/l10n/studio_l10n.dart';

/// Verifies that [BuildContext.roleLabel] maps protocol role keys to localized
/// display names, falls back for empty roles, and returns unknown roles
/// unchanged.
void main() {
  Future<BuildContext> pumpContext(WidgetTester tester, Locale locale) async {
    late BuildContext captured;
    await tester.pumpWidget(
      MaterialApp(
        locale: locale,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: Builder(
          builder: (context) {
            captured = context;
            return const SizedBox.shrink();
          },
        ),
      ),
    );
    await tester.pumpAndSettle();
    return captured;
  }

  testWidgets('localizes known roles and preserves fallback semantics', (
    tester,
  ) async {
    final context = await pumpContext(tester, const Locale('zh'));
    expect(context.roleLabel('explorer'), '探索者');
    expect(context.roleLabel('planner'), '计划者');
    expect(context.roleLabel('executor'), '执行者');
    expect(context.roleLabel('reviewer'), '审查者');
    expect(context.roleLabel(''), '代理');
    expect(context.roleLabel('custom-role'), 'custom-role');
    expect(context.roleLabel('explorer '), '探索者');

    final englishContext = await pumpContext(tester, const Locale('en'));
    expect(englishContext.roleLabel('explorer'), 'Explorer');
    expect(englishContext.roleLabel('planner'), 'Planner');
    expect(englishContext.roleLabel('executor'), 'Executor');
    expect(englishContext.roleLabel('reviewer'), 'Reviewer');
    expect(englishContext.roleLabel(''), 'Agent');
  });
}
