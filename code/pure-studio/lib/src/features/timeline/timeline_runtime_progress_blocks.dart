part of 'timeline_view.dart';

class _TimelineProgressGroupBlock extends StatelessWidget {
  const _TimelineProgressGroupBlock({required this.block, super.key});

  final _TimelineDisplayBlock block;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 18),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 700),
        child: _RuntimeProgressGroup(rows: block.rows),
      ),
    );
  }
}

class _RuntimeProgressGroup extends StatefulWidget {
  const _RuntimeProgressGroup({required this.rows});

  final List<TimelineRow> rows;

  @override
  State<_RuntimeProgressGroup> createState() => _RuntimeProgressGroupState();
}

class _RuntimeProgressGroupState extends State<_RuntimeProgressGroup> {
  bool expanded = false;

  @override
  Widget build(BuildContext context) {
    final parts = [
      for (final row in widget.rows)
        if (row.part != null) row.part!,
    ];
    final latest = parts.last;
    final scheme = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.studioPaper2.withValues(alpha: 0.58),
        border: Border.all(color: scheme.outlineVariant.withValues(alpha: 0.6)),
        borderRadius: BorderRadius.circular(StudioRadii.md),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Material(
            color: Colors.transparent,
            child: InkWell(
              borderRadius: BorderRadius.circular(StudioRadii.md),
              onTap: () => setState(() => expanded = !expanded),
              child: Padding(
                padding: const EdgeInsets.fromLTRB(12, 9, 10, 9),
                child: Row(
                  children: [
                    Icon(
                      expanded
                          ? Icons.keyboard_arrow_up_rounded
                          : Icons.keyboard_arrow_down_rounded,
                      size: 18,
                      color: context.studioInkSoft,
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        latest.text,
                        maxLines: expanded ? 1 : 2,
                        overflow: TextOverflow.ellipsis,
                        style: context.text.bodySmall?.copyWith(
                          color: context.studioInk,
                          height: 1.38,
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                    StudioPill(label: parts.length.toString()),
                  ],
                ),
              ),
            ),
          ),
          if (expanded)
            Padding(
              padding: const EdgeInsets.fromLTRB(15, 0, 15, 12),
              child: SelectionArea(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Divider(
                      height: 12,
                      color: scheme.outlineVariant.withValues(alpha: 0.45),
                    ),
                    for (final part in parts)
                      _RuntimeProgressStep(
                        text: part.text,
                        isLatest: part.id == latest.id,
                      ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _RuntimeProgressStep extends StatelessWidget {
  const _RuntimeProgressStep({required this.text, required this.isLatest});

  final String text;
  final bool isLatest;

  @override
  Widget build(BuildContext context) {
    final color = isLatest ? context.studioInk : context.studioInkSoft;
    return Padding(
      padding: const EdgeInsets.only(top: 7),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 7),
            child: DecoratedBox(
              decoration: BoxDecoration(
                color: color.withValues(alpha: isLatest ? 0.76 : 0.34),
                shape: BoxShape.circle,
              ),
              child: const SizedBox.square(dimension: 5),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              text,
              style: context.text.bodySmall?.copyWith(
                color: color,
                height: 1.42,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
