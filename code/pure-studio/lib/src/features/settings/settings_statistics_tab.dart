import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class StatisticsTab extends StatefulWidget {
  const StatisticsTab({required this.snapshot, super.key});

  final ModelPerformanceSnapshotView snapshot;

  @override
  State<StatisticsTab> createState() => _StatisticsTabState();
}

class _StatisticsTabState extends State<StatisticsTab> {
  String? _filter;

  @override
  void didUpdateWidget(covariant StatisticsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    final filter = _filter;
    if (filter != null &&
        !widget.snapshot.summaries.any((item) => item.filterKey == filter)) {
      _filter = null;
    }
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 760;
        final history = [
          for (final item in widget.snapshot.history)
            if (_filter == null || item.filterKey == _filter) item,
        ];
        return Align(
          alignment: Alignment.topCenter,
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 1120),
            child: CustomScrollView(
              key: StudioDriverKeys.statisticsHistory,
              slivers: [
                SliverPadding(
                  padding: const EdgeInsets.fromLTRB(28, 22, 28, 16),
                  sliver: SliverToBoxAdapter(
                    child: SettingsHeader(
                      title: context.l10n.settingsStatisticsTitle,
                      subtitle: context.l10n.settingsStatisticsSubtitle,
                    ),
                  ),
                ),
                SliverPadding(
                  padding: const EdgeInsets.symmetric(horizontal: 28),
                  sliver: SliverToBoxAdapter(
                    child: _SummarySection(
                      compact: compact,
                      summaries: widget.snapshot.summaries,
                    ),
                  ),
                ),
                SliverPadding(
                  padding: const EdgeInsets.fromLTRB(28, 20, 28, 10),
                  sliver: SliverToBoxAdapter(
                    child: _HistoryHeader(
                      summaries: widget.snapshot.summaries,
                      value: _filter,
                      onChanged: (value) => setState(() => _filter = value),
                    ),
                  ),
                ),
                if (history.isEmpty)
                  SliverPadding(
                    padding: const EdgeInsets.fromLTRB(28, 0, 28, 30),
                    sliver: SliverToBoxAdapter(
                      child: _EmptyState(
                        label: context.l10n.settingsStatisticsEmpty,
                      ),
                    ),
                  )
                else ...[
                  if (!compact)
                    SliverPadding(
                      padding: const EdgeInsets.symmetric(horizontal: 28),
                      sliver: SliverToBoxAdapter(child: _WideHistoryHeader()),
                    ),
                  SliverPadding(
                    padding: const EdgeInsets.fromLTRB(28, 0, 28, 30),
                    sliver: SliverList.builder(
                      itemCount: history.length,
                      itemBuilder: (context, index) {
                        final sample = history[index];
                        final key = StudioDriverKeys.statisticsHistoryRow(
                          sample.providerInstanceId,
                          sample.model,
                          sample.completedAt.millisecondsSinceEpoch,
                        );
                        return compact
                            ? _CompactHistoryCard(key: key, sample: sample)
                            : _WideHistoryRow(key: key, sample: sample);
                      },
                    ),
                  ),
                ],
              ],
            ),
          ),
        );
      },
    );
  }
}

class _SummarySection extends StatelessWidget {
  const _SummarySection({required this.compact, required this.summaries});

  final bool compact;
  final List<ModelPerformanceSummaryView> summaries;

  @override
  Widget build(BuildContext context) {
    return SettingsSectionPanel(
      key: StudioDriverKeys.statisticsSummary,
      title: context.l10n.settingsStatisticsSummaryTitle,
      children: [
        if (summaries.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 16),
            child: Text(
              context.l10n.settingsStatisticsEmpty,
              style: Theme.of(context).textTheme.bodyMedium
                  ?.copyWith(color: context.studioInkSoft),
            ),
          )
        else if (compact)
          for (final summary in summaries) _CompactSummaryCard(summary: summary)
        else
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: DataTable(
              columns: [
                DataColumn(label: Text(context.l10n.statisticsModel)),
                DataColumn(label: Text(context.l10n.statisticsSpeed)),
                DataColumn(label: Text(context.l10n.statisticsSamples)),
                DataColumn(label: Text(context.l10n.statisticsOutputTokens)),
                DataColumn(label: Text(context.l10n.statisticsAverageTtft)),
                DataColumn(label: Text(context.l10n.statisticsAverageResponse)),
              ],
              rows: [
                for (final summary in summaries)
                  DataRow(
                    cells: [
                      DataCell(_ModelLabel(summary: summary)),
                      DataCell(
                        Text(formatTokenThroughput(summary.tokensPerSecond)),
                      ),
                      DataCell(Text('${summary.sampleCount}')),
                      DataCell(Text('${summary.completionTokens}')),
                      DataCell(Text(_formatMillis(summary.averageTtftMillis))),
                      DataCell(
                        Text(_formatMillis(summary.averageResponseMillis)),
                      ),
                    ],
                  ),
              ],
            ),
          ),
      ],
    );
  }
}

class _CompactSummaryCard extends StatelessWidget {
  const _CompactSummaryCard({required this.summary});

  final ModelPerformanceSummaryView summary;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: context.studioPaper2,
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          border: Border.all(color: context.studioLine),
        ),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _ModelLabel(summary: summary),
              const SizedBox(height: 10),
              Wrap(
                spacing: 16,
                runSpacing: 8,
                children: [
                  _Metric(
                    context.l10n.statisticsSpeed,
                    formatTokenThroughput(summary.tokensPerSecond),
                  ),
                  _Metric(
                    context.l10n.statisticsSamples,
                    '${summary.sampleCount}',
                  ),
                  _Metric(
                    context.l10n.statisticsOutputTokens,
                    '${summary.completionTokens}',
                  ),
                  _Metric(
                    context.l10n.statisticsAverageTtft,
                    _formatMillis(summary.averageTtftMillis),
                  ),
                  _Metric(
                    context.l10n.statisticsAverageResponse,
                    _formatMillis(summary.averageResponseMillis),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ModelLabel extends StatelessWidget {
  const _ModelLabel({required this.summary});

  final ModelPerformanceSummaryView summary;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(summary.model, maxLines: 1, overflow: TextOverflow.ellipsis),
        Text(
          '${summary.providerDisplayName} · ${summary.providerInstanceId}',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.labelSmall
              ?.copyWith(color: context.studioInkSoft),
        ),
      ],
    );
  }
}

class _HistoryHeader extends StatelessWidget {
  const _HistoryHeader({
    required this.summaries,
    required this.value,
    required this.onChanged,
  });

  final List<ModelPerformanceSummaryView> summaries;
  final String? value;
  final ValueChanged<String?> onChanged;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Expanded(
          child: Text(
            context.l10n.settingsStatisticsHistoryTitle,
            style: Theme.of(context).textTheme.titleMedium
                ?.copyWith(fontWeight: FontWeight.w600),
          ),
        ),
        SizedBox(
          width: 240,
          child: DropdownButtonFormField<String?>(
            key: StudioDriverKeys.statisticsFilter,
            initialValue: value,
            isExpanded: true,
            decoration: const InputDecoration(isDense: true),
            items: [
              DropdownMenuItem<String?>(
                child: Text(context.l10n.settingsStatisticsAllModels),
              ),
              for (final summary in summaries)
                DropdownMenuItem<String?>(
                  value: summary.filterKey,
                  child: Text(
                    '${summary.providerDisplayName} · ${summary.model}',
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
            ],
            onChanged: onChanged,
          ),
        ),
      ],
    );
  }
}

class _WideHistoryHeader extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return _WideCells(
      emphasized: true,
      values: [
        context.l10n.statisticsCompletedAt,
        context.l10n.statisticsModel,
        context.l10n.statisticsOutputTokens,
        'TTFT',
        context.l10n.statisticsDecode,
        context.l10n.statisticsTotalResponse,
        context.l10n.statisticsSpeed,
      ],
    );
  }
}

class _WideHistoryRow extends StatelessWidget {
  const _WideHistoryRow({required this.sample, super.key});

  final ModelPerformanceSampleView sample;

  @override
  Widget build(BuildContext context) {
    return _WideCells(
      values: [
        _formatCompletedAt(context, sample.completedAt),
        '${sample.providerDisplayName} · ${sample.model}',
        '${sample.completionTokens}',
        _formatMillis(sample.ttftMillis.toDouble()),
        _formatMillis(sample.decodeMillis.toDouble()),
        _formatMillis(sample.totalResponseMillis.toDouble()),
        formatTokenThroughput(sample.tokensPerSecond),
      ],
    );
  }
}

class _WideCells extends StatelessWidget {
  const _WideCells({required this.values, this.emphasized = false});

  final List<String> values;
  final bool emphasized;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: emphasized ? context.studioPaper2 : Colors.transparent,
        border: Border(bottom: BorderSide(color: context.studioLine)),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
        child: Row(
          children: [
            for (var index = 0; index < values.length; index++)
              Expanded(
                flex: index == 1 ? 2 : 1,
                child: Text(
                  values[index],
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: emphasized
                      ? Theme.of(context).textTheme.labelMedium
                            ?.copyWith(fontWeight: FontWeight.w600)
                      : Theme.of(context).textTheme.bodySmall,
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _CompactHistoryCard extends StatelessWidget {
  const _CompactHistoryCard({required this.sample, super.key});

  final ModelPerformanceSampleView sample;

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: const EdgeInsets.only(bottom: 10),
      child: Padding(
        padding: const EdgeInsets.all(12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('${sample.providerDisplayName} · ${sample.model}'),
            const SizedBox(height: 3),
            Text(
              _formatCompletedAt(context, sample.completedAt),
              style: Theme.of(context).textTheme.labelSmall
                  ?.copyWith(color: context.studioInkSoft),
            ),
            const SizedBox(height: 10),
            Wrap(
              spacing: 16,
              runSpacing: 8,
              children: [
                _Metric(
                  context.l10n.statisticsSpeed,
                  formatTokenThroughput(sample.tokensPerSecond),
                ),
                _Metric(
                  context.l10n.statisticsOutputTokens,
                  '${sample.completionTokens}',
                ),
                _Metric('TTFT', _formatMillis(sample.ttftMillis.toDouble())),
                _Metric(
                  context.l10n.statisticsDecode,
                  _formatMillis(sample.decodeMillis.toDouble()),
                ),
                _Metric(
                  context.l10n.statisticsTotalResponse,
                  _formatMillis(sample.totalResponseMillis.toDouble()),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}

class _Metric extends StatelessWidget {
  const _Metric(this.label, this.value);

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(label, style: Theme.of(context).textTheme.labelSmall),
        Text(value, style: Theme.of(context).textTheme.bodyMedium),
      ],
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 48),
      child: Center(
        child: Text(label, style: TextStyle(color: context.studioInkSoft)),
      ),
    );
  }
}

String _formatMillis(double millis) {
  if (millis < 1_000) return '${millis.round()} ms';
  return '${(millis / 1_000).toStringAsFixed(1)} s';
}

String _formatCompletedAt(BuildContext context, DateTime value) {
  final local = MaterialLocalizations.of(context);
  return '${local.formatShortDate(value)} ${TimeOfDay.fromDateTime(value).format(context)}';
}
