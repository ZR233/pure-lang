import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:gpt_markdown/gpt_markdown.dart';
import 'package:anywork/src/app/theme/material3_theme.dart';
import 'package:anywork/src/app/theme/studio_tokens.dart';
import 'package:anywork/src/features/settings/settings_fields.dart';
import 'package:anywork/src/features/todo/todo_panel.dart';
import 'package:anywork/src/domain/models/studio_models.dart';
import 'package:anywork/src/l10n/app_localizations.dart';
import 'package:anywork/src/shared/studio_badges.dart';

void main() {
  test('readable theme roles retain contrast on their actual surfaces', () {
    final theme = pureStudioTheme();
    final scheme = theme.colorScheme;
    final status = theme.extension<StudioSemanticColors>()!;
    final surfaces = [
      scheme.surface,
      scheme.surfaceContainerLowest,
      scheme.surfaceContainerLow,
      scheme.surfaceContainer,
      scheme.surfaceContainerHigh,
    ];
    for (final background in surfaces) {
      for (final foreground in [scheme.onSurface, scheme.onSurfaceVariant]) {
        expect(_contrast(foreground, background), greaterThanOrEqualTo(4.5));
      }
      expect(
        _contrast(status.activeIndicator, background),
        greaterThanOrEqualTo(3),
      );
    }
    for (final pair in [
      (scheme.onPrimary, scheme.primary),
      (scheme.onPrimaryContainer, scheme.primaryContainer),
      (scheme.onSecondary, scheme.secondary),
      (scheme.onTertiary, scheme.tertiary),
      (scheme.onError, scheme.error),
      (scheme.onErrorContainer, scheme.errorContainer),
      (scheme.onInverseSurface, scheme.inverseSurface),
      (status.success, status.successContainer),
      (status.warning, status.warningContainer),
    ]) {
      expect(_contrast(pair.$1, pair.$2), greaterThanOrEqualTo(4.5));
    }
    expect(
      _contrast(scheme.outline, scheme.surfaceContainerLowest),
      greaterThanOrEqualTo(3),
    );
    expect(
      _contrast(scheme.primary, scheme.surfaceContainerLowest),
      greaterThanOrEqualTo(3),
    );
  });

  testWidgets(
    'shared content and fields inherit injected semantic theme roles',
    (tester) async {
      final base = pureStudioTheme();
      final scheme = base.colorScheme.copyWith(
        primary: const Color(0xff452070),
        onSurface: const Color(0xff182030),
        onSurfaceVariant: const Color(0xff304050),
        surfaceContainerLow: const Color(0xffe0e3ea),
      );
      final status = base.extension<StudioSemanticColors>()!.copyWith(
        activeIndicator: const Color(0xff206040),
      );
      final theme = base.copyWith(
        colorScheme: scheme,
        extensions: [status, base.extension<GptMarkdownThemeData>()!],
      );
      await tester.pumpWidget(
        MaterialApp(
          theme: theme,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          supportedLocales: AppLocalizations.supportedLocales,
          home: Scaffold(
            body: Column(
              children: [
                const StudioPill(
                  label: 'Running',
                  icon: Icons.play_arrow,
                  tone: StudioTone.active,
                ),
                const StudioIconBadge(icon: Icons.send, filled: true),
                SettingsTextEdit(
                  label: 'Name',
                  value: 'Provider',
                  onChanged: (_) {},
                ),
                const Expanded(
                  child: TodoPanel(
                    todo: TimelineTodoListUpdate(
                      callId: 'theme-todo',
                      explanation: 'Tasks',
                      items: [
                        TimelineTodoItem(
                          step: 'Inspect palette',
                          status: 'inProgress',
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      );
      final running = tester.widget<Text>(find.text('Running'));
      expect(running.style!.color, scheme.onSurface);
      expect(
        tester.widget<Icon>(find.byIcon(Icons.play_arrow)).color,
        status.activeIndicator,
      );
      expect(
        tester.widget<Icon>(find.byIcon(Icons.radio_button_checked)).color,
        status.activeIndicator,
      );
      expect(
        tester.widget<Text>(find.text('Inspect palette')).style!.color,
        scheme.onSurface,
      );
      final decorated = tester.widget<DecoratedBox>(
        find
            .descendant(
              of: find.byType(StudioPill),
              matching: find.byType(DecoratedBox),
            )
            .first,
      );
      expect(
        (decorated.decoration as BoxDecoration).color,
        scheme.surfaceContainerLow,
      );
      final badge = tester.widget<DecoratedBox>(
        find
            .descendant(
              of: find.byType(StudioIconBadge),
              matching: find.byType(DecoratedBox),
            )
            .first,
      );
      expect((badge.decoration as BoxDecoration).color, scheme.primary);
      final field = tester.widget<TextField>(find.byType(TextField));
      expect(field.decoration!.fillColor, theme.inputDecorationTheme.fillColor);
      expect(
        field.decoration!.focusedBorder,
        theme.inputDecorationTheme.focusedBorder,
      );
    },
  );

  testWidgets('Markdown links use the configured brown role for hover too', (
    tester,
  ) async {
    final theme = pureStudioTheme();
    await tester.pumpWidget(
      MaterialApp(
        theme: theme,
        home: const Scaffold(
          body: GptMarkdown('[Documentation](https://example.com)'),
        ),
      ),
    );
    final context = tester.element(find.byType(GptMarkdown));
    final markdown = GptMarkdownTheme.of(context);
    expect(markdown.linkColor, theme.colorScheme.primary);
    expect(markdown.linkHoverColor, theme.colorScheme.onPrimaryContainer);
    expect(
      _contrast(markdown.linkColor, theme.colorScheme.surface),
      greaterThanOrEqualTo(4.5),
    );
  });
}

double _contrast(Color foreground, Color background) {
  final a = Color.alphaBlend(foreground, background).computeLuminance();
  final b = background.computeLuminance();
  return ((a > b ? a : b) + 0.05) / ((a > b ? b : a) + 0.05);
}
