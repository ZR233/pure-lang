part of 'timeline_view.dart';

Map<String, String> _markdownImageAlts(String source) {
  final alts = <String, String>{};

  void visit(MdNode node) {
    if (node is MdImage) {
      alts.putIfAbsent(node.url, () => node.alt.trim());
      return;
    }
    final children = switch (node) {
      MdHeading(:final children) ||
      MdParagraph(:final children) ||
      MdBlockQuote(:final children) ||
      MdCheckbox(:final children) ||
      MdRadio(:final children) ||
      MdBold(:final children) ||
      MdItalic(:final children) ||
      MdStrike(:final children) ||
      MdUnderline(:final children) ||
      MdLink(:final children) => children,
      MdUnorderedList(:final items) || MdOrderedList(:final items) => [
        for (final item in items) ...item.children,
      ],
      MdTable(:final header, :final rows) => [
        for (final cell in header.cells) ...cell.content,
        for (final row in rows)
          for (final cell in row.cells) ...cell.content,
      ],
      _ => const <MdNode>[],
    };
    for (final child in children) {
      visit(child);
    }
  }

  for (final node in Plusparse.parse(source).children) {
    visit(node);
  }
  return alts;
}

Widget _studioMarkdownImage(
  BuildContext context,
  String destination,
  String alt,
  _MarkdownSurface surface,
) {
  final url = _safeHttpsImageUrl(destination);
  final enabled =
      surface == _MarkdownSurface.assistant ||
      surface == _MarkdownSurface.panel;
  if (!enabled || url == null) {
    return Text(alt.isNotEmpty ? alt : destination);
  }
  return _RemoteMarkdownImageCard(url: url, alt: alt);
}

String? _safeHttpsImageUrl(String value) {
  if (value.isEmpty || value.length > 8 * 1024) return null;
  if (value.codeUnits.any((unit) => unit < 0x20 || unit == 0x7f)) {
    return null;
  }
  final uri = Uri.tryParse(value);
  if (uri == null || uri.scheme.toLowerCase() != 'https' || uri.host.isEmpty) {
    return null;
  }
  return uri.toString();
}

class _RemoteMarkdownImageCard extends ConsumerStatefulWidget {
  const _RemoteMarkdownImageCard({required this.url, required this.alt});

  final String url;
  final String alt;

  @override
  ConsumerState<_RemoteMarkdownImageCard> createState() =>
      _RemoteMarkdownImageCardState();
}

class _RemoteMarkdownImageCardState
    extends ConsumerState<_RemoteMarkdownImageCard> {
  ImageProvider? _image;
  bool _failed = false;

  @override
  void didUpdateWidget(covariant _RemoteMarkdownImageCard oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.url != oldWidget.url) {
      _image = null;
      _failed = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final image = _image;
    return ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 360),
      child: Material(
        key: StudioDriverKeys.markdownImageSource(widget.url),
        color: context.colors.surface,
        shape: RoundedRectangleBorder(
          side: BorderSide(color: context.colors.outlineVariant),
          borderRadius: BorderRadius.circular(10),
        ),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: image == null ? _loadAndOpen : _open,
          child: image == null
              ? Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 12,
                    vertical: 10,
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(
                        _failed
                            ? Icons.broken_image_outlined
                            : Icons.image_outlined,
                        color: _failed
                            ? Theme.of(context).colorScheme.error
                            : context.colors.onSurfaceVariant,
                      ),
                      const SizedBox(width: 10),
                      Flexible(
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              widget.alt.isNotEmpty
                                  ? widget.alt
                                  : context.l10n.timelineRemoteImageSource(
                                      Uri.parse(widget.url).host,
                                    ),
                              maxLines: 2,
                              overflow: TextOverflow.ellipsis,
                            ),
                            Text(
                              _failed
                                  ? context.l10n.timelineImageRetry
                                  : context.l10n.timelineRemoteImageOpen,
                              style: context.text.labelSmall?.copyWith(
                                color: _failed
                                    ? Theme.of(context).colorScheme.error
                                    : context.colors.onSurfaceVariant,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                )
              : SizedBox(
                  key: StudioDriverKeys.markdownImageThumbnail(widget.url),
                  width: 240,
                  height: 180,
                  child: Image(
                    image: image,
                    fit: BoxFit.scaleDown,
                    errorBuilder: (context, error, stackTrace) {
                      WidgetsBinding.instance.addPostFrameCallback((_) {
                        if (mounted && _image != null) {
                          setState(() {
                            _image = null;
                            _failed = true;
                          });
                        }
                      });
                      return Icon(
                        Icons.broken_image_outlined,
                        color: Theme.of(context).colorScheme.error,
                      );
                    },
                  ),
                ),
        ),
      ),
    );
  }

  Future<void> _loadAndOpen() async {
    final provider = ref.read(timelineRemoteImageProviderFactoryProvider)(
      widget.url,
    );
    Object? loadError;
    await precacheImage(
      provider,
      context,
      onError: (error, stackTrace) {
        loadError = error;
      },
    );
    if (!mounted) return;
    if (loadError != null) {
      setState(() {
        _image = null;
        _failed = true;
      });
      return;
    }
    setState(() {
      _image = provider;
      _failed = false;
    });
    await _open();
  }

  Future<void> _open() async {
    final image = _image;
    if (image == null || !mounted) return;
    await _showStudioImageDialog(
      context,
      image,
      dialogKey: StudioDriverKeys.markdownImageDialog(widget.url),
      label: widget.alt.isEmpty ? Uri.parse(widget.url).host : widget.alt,
    );
  }
}
