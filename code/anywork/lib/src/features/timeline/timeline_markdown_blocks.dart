part of 'timeline_view.dart';

enum _MarkdownSurface { assistant, user, panel, reasoning, error }

final RegExp _streamingMarkdownSyntax = RegExp(r'[\r\n\\`*_\[\]()#>~!|]');
const _longPlainTextThreshold = 1024;
const _plainTextPageSize = 1024;

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
    final plainText =
        !_streamingMarkdownSyntax.hasMatch(text) &&
        !text.contains('://') &&
        !text.contains('www.');
    if (text.length > _longPlainTextThreshold && plainText) {
      return _PagedPlainText(
        key: ValueKey('plain-paged-$id'),
        id: id,
        text: text,
        surface: surface,
      );
    }
    if (plainText) {
      final content = Text(
        text,
        key: ValueKey('plain-$id'),
        style: _markdownBodyStyle(context, surface),
      );
      return status == 'streaming'
          ? SelectionContainer.disabled(child: content)
          : content;
    }
    final repaired = repairAgentMarkdownForDisplay(text);
    final imageAlts = repaired.contains('![')
        ? _markdownImageAlts(repaired)
        : const <String, String>{};
    final scheme = Theme.of(context).colorScheme;
    return GptMarkdown(
      repaired,
      key: ValueKey('gpt-markdown-$id-$status'),
      style: _markdownBodyStyle(context, surface),
      styleSheet: GptMarkdownStyleSheet(
        blockQuote: BlockQuoteStyle(
          textStyle: TextStyle(
            color: context.colors.onSurfaceVariant,
            height: 1.5,
          ),
        ),
      ),
      onLinkTap: (url, _) {
        unawaited(_openTimelineWebLink(context, ref, url));
      },
      inlineLinkBuilder: (link) => safeExternalWebUrl(link.url) == null
          ? TextSpan(children: link.labelSpans, style: link.style)
          : link.defaultSpan(),
      inlineCodeStyle: InlineCodeStyle(
        fontFamily: 'JetBrains Mono',
        fontFamilyFallback: const ['Consolas', 'monospace'],
        fontSizeFactor: 0.9,
        fontWeight: FontWeight.w600,
        color: scheme.onSurface,
        backgroundColor: surface == _MarkdownSurface.user
            ? scheme.surfaceContainerLowest
            : scheme.surfaceContainerLow,
        borderColor: scheme.outlineVariant,
        borderRadius: Radius.circular(StudioRadii.xs),
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1.5),
      ),
      blockQuoteBuilder: (context, content, style) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 6),
        child: DecoratedBox(
          key: const ValueKey('studio-markdown-quote'),
          decoration: BoxDecoration(
            color: scheme.surfaceContainerLow,
            border: BorderDirectional(
              start: BorderSide(color: context.colors.primary, width: 3),
            ),
            borderRadius: BorderRadius.circular(StudioRadii.sm),
          ),
          child: Padding(
            padding: const EdgeInsetsDirectional.fromSTEB(10, 7, 10, 7),
            child: content,
          ),
        ),
      ),
      imageBuilder: (context, url, _, _) =>
          _studioMarkdownImage(context, url, imageAlts[url] ?? '', surface),
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
                ? context.colors.onSurfaceVariant
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

class _PagedPlainText extends StatefulWidget {
  const _PagedPlainText({
    required this.id,
    required this.text,
    required this.surface,
    super.key,
  });

  final String id;
  final String text;
  final _MarkdownSurface surface;

  @override
  State<_PagedPlainText> createState() => _PagedPlainTextState();
}

class _PagedPlainTextState extends State<_PagedPlainText> {
  int _page = 0;
  bool _followTail = true;

  @override
  Widget build(BuildContext context) {
    final text = widget.text;
    final boundaries = <int>[0];
    while (boundaries.last < text.length) {
      var end = math.min(boundaries.last + _plainTextPageSize, text.length);
      if (end < text.length) {
        final previous = text.codeUnitAt(end - 1);
        final next = text.codeUnitAt(end);
        if (previous >= 0xD800 &&
            previous <= 0xDBFF &&
            next >= 0xDC00 &&
            next <= 0xDFFF) {
          end -= 1;
        }
      }
      boundaries.add(end);
    }
    final last = boundaries.length - 2;
    final page = _followTail ? last : _page.clamp(0, last);
    final start = boundaries[page];
    final end = boundaries[page + 1];
    final material = MaterialLocalizations.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SelectableText(
          text.substring(start, end),
          key: ValueKey('plain-page-${widget.id}-$page'),
          style: _markdownBodyStyle(context, widget.surface),
        ),
        Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            IconButton(
              tooltip: material.previousPageTooltip,
              onPressed: page == 0
                  ? null
                  : () => setState(() {
                      _followTail = false;
                      _page = page - 1;
                    }),
              icon: const Icon(Icons.chevron_left),
            ),
            Text('${start + 1}–$end / ${text.length}'),
            IconButton(
              tooltip: material.nextPageTooltip,
              onPressed: page == last
                  ? null
                  : () => setState(() {
                      _page = page + 1;
                      _followTail = _page == last;
                    }),
              icon: const Icon(Icons.chevron_right),
            ),
          ],
        ),
      ],
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

TextStyle? _markdownBodyStyle(BuildContext context, _MarkdownSurface surface) {
  final theme = Theme.of(context);
  if (surface == _MarkdownSurface.reasoning) {
    return theme.textTheme.bodySmall?.copyWith(
      color: context.colors.onSurfaceVariant,
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
