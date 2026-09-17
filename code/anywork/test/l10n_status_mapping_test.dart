import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:anywork/src/domain/models/thread_directory_models.dart';
import 'package:anywork/src/l10n/app_localizations.dart';
import 'package:anywork/src/l10n/studio_l10n.dart';

/// Verifies the display-layer canonical status mappings: known canonical values
/// render localized labels, and unknown values are passed through unchanged so
/// runtime or future states are never hidden behind a wrong translation.
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

  testWidgets('maps known canonical statuses and preserves unknown values', (
    tester,
  ) async {
    final context = await pumpContext(tester, const Locale('zh'));

    expect(context.providerStatusLabel('ready'), '就绪');
    expect(context.providerStatusLabel('missingCredential'), '缺少凭据');
    expect(context.providerStatusLabel(' ready '), '就绪');
    expect(context.providerStatusLabel('custom-status'), 'custom-status');

    expect(context.sshConnectionStateLabel('ready'), '已连接');
    expect(context.sshConnectionStateLabel('failed'), '连接失败');
    expect(context.sshConnectionStateLabel('custom-state'), 'custom-state');

    expect(context.worktreeStateLabel('preserved'), '已保留');
    expect(context.worktreeStateLabel('custom'), 'custom');

    expect(context.toolStatusLabel('succeeded'), '已完成');
    expect(context.toolStatusLabel('awaitingApproval'), '等待授权');
    expect(context.toolStatusLabel('custom'), 'custom');

    expect(context.todoStatusLabel('inProgress'), '进行中');
    expect(context.todoStatusLabel('custom'), 'custom');

    expect(context.threadStatusLabel(ThreadStatusView.idle), '空闲');
    expect(context.threadStatusLabel(ThreadStatusView.faulted), '出错');
  });
}
