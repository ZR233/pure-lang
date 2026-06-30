part of 'timeline_view.dart';

enum _MarkdownSurface { assistant, user, panel }

final List<MarkdownComponent> _studioMarkdownComponents = MarkdownComponent
    .globalComponents
    .map(
      (component) => component is BlockQuote ? _StudioBlockQuote() : component,
    )
    .toList(growable: false);

class _AgentMarkdown extends StatelessWidget {
  const _AgentMarkdown({
    required this.id,
    required this.status,
    required this.text,
    required this.surface,
  });

  final String id;
  final String status;
  final String text;
  final _MarkdownSurface surface;

  @override
  Widget build(BuildContext context) {
    final repaired = repairAgentMarkdownForDisplay(text);
    return GptMarkdown(
      repaired,
      key: ValueKey('gpt-markdown-$id-$status'),
      style: _markdownBodyStyle(context),
      components: _studioMarkdownComponents,
      highlightBuilder: (context, text, style) {
        return _MarkdownInlineCode(text: text, style: style, surface: surface);
      },
      codeBuilder: (context, name, code, closed) {
        final scheme = Theme.of(context).colorScheme;
        final textTheme = Theme.of(context).textTheme;
        final codeBackground = surface == _MarkdownSurface.user
            ? scheme.surfaceContainerHigh
            : scheme.surfaceContainerLow;
        return StudioCodeBlock(
          text: code,
          language: name,
          margin: const EdgeInsets.symmetric(vertical: 6),
          backgroundColor: codeBackground,
          borderColor: scheme.outlineVariant,
          textStyle: textTheme.bodyMedium?.copyWith(
            color: scheme.onSurface,
            fontFamily: 'JetBrains Mono',
            fontFamilyFallback: const ['Consolas', 'monospace'],
            fontSize: (textTheme.bodyMedium?.fontSize ?? 14) * 0.92,
            height: 1.35,
          ),
        );
      },
    );
  }
}

TextStyle? _markdownBodyStyle(BuildContext context) {
  final theme = Theme.of(context);
  return theme.textTheme.bodyMedium?.copyWith(
    color: theme.colorScheme.onSurface,
    height: 1.52,
  );
}

class _MarkdownInlineCode extends StatelessWidget {
  const _MarkdownInlineCode({
    required this.text,
    required this.style,
    required this.surface,
  });

  final String text;
  final TextStyle style;
  final _MarkdownSurface surface;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final fill = switch (surface) {
      _MarkdownSurface.user => scheme.surface.withValues(alpha: 0.72),
      _ => Color.alphaBlend(
        StudioColors.clay.withValues(alpha: context.isDark ? 0.16 : 0.09),
        context.studioPaper,
      ),
    };
    final border = switch (surface) {
      _MarkdownSurface.user => scheme.outlineVariant.withValues(alpha: 0.62),
      _ => StudioColors.clay.withValues(alpha: context.isDark ? 0.38 : 0.22),
    };
    final codeStyle = style.copyWith(
      color: scheme.onSurface,
      fontFamily: 'JetBrains Mono',
      fontFamilyFallback: const ['Consolas', 'monospace'],
      fontSize: (style.fontSize ?? 14) * 0.9,
      fontWeight: FontWeight.w600,
      height: 1.0,
      letterSpacing: 0,
      background: null,
    );

    return DecoratedBox(
      key: const ValueKey('studio-markdown-inline-code'),
      decoration: BoxDecoration(
        color: fill,
        borderRadius: BorderRadius.circular(StudioRadii.xs),
        border: Border.all(color: border),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1.5),
        child: Text(
          text,
          style: codeStyle,
          textHeightBehavior: const TextHeightBehavior(
            applyHeightToFirstAscent: false,
            applyHeightToLastDescent: false,
          ),
        ),
      ),
    );
  }
}

class _StudioBlockQuote extends MarkdownComponent {
  @override
  bool get inline => false;

  @override
  RegExp get exp => RegExp(
    r"(?:(?:^)\ *>[^\n]+)(?:(?:\n)\ *>[^\n]+)*",
    dotAll: true,
    multiLine: true,
  );

  @override
  InlineSpan span(BuildContext context, String text, GptMarkdownConfig config) {
    final data = _plainQuoteText(text);
    final scheme = Theme.of(context).colorScheme;
    final quoteStyle = (config.style ?? DefaultTextStyle.of(context).style)
        .copyWith(color: context.studioInkSoft, height: 1.5);
    final quoteConfig = config.copyWith(style: quoteStyle);
    final child = TextSpan(
      children: MarkdownComponent.generate(context, data, quoteConfig, true),
    );

    return WidgetSpan(
      child: Directionality(
        textDirection: config.textDirection,
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 6),
          child: DecoratedBox(
            key: const ValueKey('studio-markdown-quote'),
            decoration: BoxDecoration(
              color: Color.alphaBlend(
                scheme.primary.withValues(alpha: context.isDark ? 0.12 : 0.05),
                context.studioPaper,
              ),
              border: BorderDirectional(
                start: BorderSide(color: StudioColors.clay, width: 3),
              ),
              borderRadius: BorderRadius.circular(StudioRadii.sm),
            ),
            child: Padding(
              padding: const EdgeInsetsDirectional.fromSTEB(10, 7, 10, 7),
              child: config.getRich(child),
            ),
          ),
        ),
      ),
    );
  }
}

String _plainQuoteText(String text) {
  return text
      .split('\n')
      .map((line) {
        final trimmed = line.trimLeft();
        if (!trimmed.startsWith('>')) {
          return line;
        }
        final body = trimmed.substring(1);
        return body.startsWith(' ') ? body.substring(1) : body;
      })
      .join('\n')
      .trim();
}
