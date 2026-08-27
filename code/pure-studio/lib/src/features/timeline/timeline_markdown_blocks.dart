part of 'timeline_view.dart';

enum _MarkdownSurface { assistant, user, panel, reasoning, error }

final List<MarkdownComponent> _studioMarkdownComponents = MarkdownComponent
    .globalComponents
    .map(
      (component) => component is BlockQuote ? _StudioBlockQuote() : component,
    )
    .toList(growable: false);

final List<MarkdownComponent> _studioMarkdownInlineComponents = [
  ...MarkdownComponent.inlineComponents,
  _StudioBareWebLink(),
];

class _AgentMarkdown extends ConsumerWidget {
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
  Widget build(BuildContext context, WidgetRef ref) {
    final repaired = repairAgentMarkdownForDisplay(text);
    return GptMarkdown(
      repaired,
      key: ValueKey('gpt-markdown-$id-$status'),
      style: _markdownBodyStyle(context, surface),
      components: _studioMarkdownComponents,
      inlineComponents: _studioMarkdownInlineComponents,
      onLinkTap: (url, _) {
        unawaited(_openTimelineWebLink(context, ref, url));
      },
      highlightBuilder: (context, text, style) {
        return _MarkdownInlineCode(text: text, style: style, surface: surface);
      },
      codeBuilder: (context, name, code, closed) {
        final scheme = Theme.of(context).colorScheme;
        final textTheme = Theme.of(context).textTheme;
        final codeBackground = surface == _MarkdownSurface.user
            ? scheme.surfaceContainerHigh
            : scheme.surfaceContainerLow;
        final bodyStyle = surface == _MarkdownSurface.reasoning
            ? textTheme.bodySmall
            : textTheme.bodyMedium;
        return StudioCodeBlock(
          text: code,
          language: name,
          margin: const EdgeInsets.symmetric(vertical: 6),
          backgroundColor: codeBackground,
          borderColor: scheme.outlineVariant,
          textStyle: bodyStyle?.copyWith(
            color: surface == _MarkdownSurface.reasoning
                ? context.studioInkSoft
                : surface == _MarkdownSurface.error
                ? scheme.error
                : scheme.onSurface,
            fontFamily: 'JetBrains Mono',
            fontFamilyFallback: const ['Consolas', 'monospace'],
            fontSize: (bodyStyle.fontSize ?? 14) * 0.92,
            height: 1.35,
          ),
        );
      },
    );
  }
}

Future<void> _openTimelineWebLink(
  BuildContext context,
  WidgetRef ref,
  String rawUrl,
) async {
  final url = safeExternalWebUrl(rawUrl);
  if (url == null) {
    return;
  }
  try {
    await ref.read(externalUrlLauncherProvider)(url);
  } catch (_) {
    if (!context.mounted) {
      return;
    }
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(context.l10n.timelineExternalLinkOpenFailed)),
    );
  }
}

class _StudioBareWebLink extends InlineMd {
  @override
  RegExp get exp => RegExp(r'(?<!\]\()[hH][tT][tT][pP][sS]?://[^\s<]+');

  @override
  InlineSpan span(BuildContext context, String text, GptMarkdownConfig config) {
    final urlEnd = _bareWebUrlEnd(text);
    final candidate = text.substring(0, urlEnd);
    final url = safeExternalWebUrl(candidate);
    if (url == null) {
      return TextSpan(text: text, style: config.style);
    }
    final theme = GptMarkdownTheme.of(context);
    return TextSpan(
      children: [
        WidgetSpan(
          alignment: PlaceholderAlignment.baseline,
          baseline: TextBaseline.alphabetic,
          child: _BareWebLinkText(
            text: candidate,
            style: config.style ?? const TextStyle(),
            color: theme.linkColor,
            hoverColor: theme.linkHoverColor,
            onTap: () => config.onLinkTap?.call(url, candidate),
          ),
        ),
        if (urlEnd < text.length)
          TextSpan(text: text.substring(urlEnd), style: config.style),
      ],
    );
  }
}

class _BareWebLinkText extends StatefulWidget {
  const _BareWebLinkText({
    required this.text,
    required this.style,
    required this.color,
    required this.hoverColor,
    required this.onTap,
  });

  final String text;
  final TextStyle style;
  final Color color;
  final Color hoverColor;
  final VoidCallback onTap;

  @override
  State<_BareWebLinkText> createState() => _BareWebLinkTextState();
}

class _BareWebLinkTextState extends State<_BareWebLinkText> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final color = _hovered ? widget.hoverColor : widget.color;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hovered = true),
      onExit: (_) => setState(() => _hovered = false),
      child: GestureDetector(
        onTap: widget.onTap,
        child: Text(
          widget.text,
          key: ValueKey('studio-markdown-web-link:${widget.text}'),
          style: widget.style.copyWith(
            color: color,
            decoration: TextDecoration.underline,
            decorationColor: color,
          ),
        ),
      ),
    );
  }
}

int _bareWebUrlEnd(String candidate) {
  final balances = <String, int>{')': 0, ']': 0, '}': 0, '>': 0};
  for (final char in candidate.characters) {
    switch (char) {
      case '(':
        balances[')'] = balances[')']! + 1;
      case ')':
        balances[')'] = balances[')']! - 1;
      case '[':
        balances[']'] = balances[']']! + 1;
      case ']':
        balances[']'] = balances[']']! - 1;
      case '{':
        balances['}'] = balances['}']! + 1;
      case '}':
        balances['}'] = balances['}']! - 1;
      case '<':
        balances['>'] = balances['>']! + 1;
      case '>':
        balances['>'] = balances['>']! - 1;
    }
  }

  var end = candidate.length;
  while (end > 0) {
    final char = candidate.substring(0, end).characters.last;
    final balance = balances[char];
    final shouldTrim = balance == null
        ? const {',', '.', ';', '!', "'", '"'}.contains(char)
        : balance < 0;
    if (!shouldTrim) {
      break;
    }
    if (balance != null) {
      balances[char] = balance + 1;
    }
    end -= char.length;
  }
  return end;
}

TextStyle? _markdownBodyStyle(BuildContext context, _MarkdownSurface surface) {
  final theme = Theme.of(context);
  if (surface == _MarkdownSurface.reasoning) {
    return theme.textTheme.bodySmall?.copyWith(
      color: context.studioInkSoft,
      height: 1.48,
    );
  }
  if (surface == _MarkdownSurface.error) {
    return theme.textTheme.bodyMedium?.copyWith(
      color: theme.colorScheme.error,
      height: 1.52,
    );
  }
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
