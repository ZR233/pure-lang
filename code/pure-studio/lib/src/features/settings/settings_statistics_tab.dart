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
        return SettingsPageLayout(
          maxWidth: 1120,
          header: SettingsHeader(
            title: context.l10n.settingsStatisticsTitle,
            subtitle: context.l10n.settingsStatisticsSubtitle,
          ),
          child: CustomScrollView(
            key: StudioDriverKeys.statisticsHistory,
            slivers: [
              SliverToBoxAdapter(
                child: _SummarySection(
                  compact: compact,
                  summaries: widget.snapshot.summaries,
                ),
              ),
              SliverPadding(
                padding: const EdgeInsets.symmetric(vertical: 20),
                sliver: SliverToBoxAdapter(
                  child: _HistoryHeader(
                    summaries: widget.snapshot.summaries,
                    value: _filter,
                    onChanged: (value) => setState(() => _filter = value),
                  ),
                ),
              ),
              if (history.isEmpty)
                SliverToBoxAdapter(
                  child: _EmptyState(
                    label: context.l10n.settingsStatisticsEmpty,
                  ),
                )
              else ...[
                if (!compact) SliverToBoxAdapter(child: _WideHistoryHeader()),
                SliverList.builder(
                  itemCount: history.length,
                  itemBuilder: (context, index) {
                    final sample = history[index];
                    final key = StudioDriverKeys.statisticsHistoryRow(
                      sample.providerInstanceId,
                      sample.model,
                      sample.reasoningEffort,
                      index,
                    );
                    return compact
                        ? _CompactHistoryCard(key: key, sample: sample)
                        : _WideHistoryRow(key: key, sample: sample);
                  },
                ),
              ],
              const SliverToBoxAdapter(child: SizedBox(height: 24)),
            ],
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
                DataColumn(label: Text(context.l10n.statisticsReasoningEffort)),
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
                      DataCell(
                        _ModelLabel(
                          key: StudioDriverKeys.statisticsSummaryRow(
                            summary.providerInstanceId,
                            summary.model,
                            summary.reasoningEffort,
                          ),
                          summary: summary,
                        ),
                      ),
                      DataCell(
                        Text(
                          _formatReasoningEffort(
                            context,
                            summary.reasoningEffort,
                          ),
                        ),
                      ),
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
  Widget build(BuildContext context) => SettingsResourceRow(
    key: StudioDriverKeys.statisticsSummaryRow(
      summary.providerInstanceId,
      summary.model,
      summary.reasoningEffort,
    ),
    title: summary.model,
    subtitle: '${summary.providerDisplayName} · ${summary.providerInstanceId}',
    children: [
      Wrap(
        spacing: 16,
        runSpacing: 8,
        children: [
          SettingsMetric(
            context.l10n.statisticsReasoningEffort,
            _formatReasoningEffort(context, summary.reasoningEffort),
          ),
          SettingsMetric(
            context.l10n.statisticsSpeed,
            formatTokenThroughput(summary.tokensPerSecond),
          ),
          SettingsMetric(
            context.l10n.statisticsSamples,
            '${summary.sampleCount}',
          ),
          SettingsMetric(
            context.l10n.statisticsOutputTokens,
            '${summary.completionTokens}',
          ),
          SettingsMetric(
            context.l10n.statisticsAverageTtft,
            _formatMillis(summary.averageTtftMillis),
          ),
          SettingsMetric(
            context.l10n.statisticsAverageResponse,
            _formatMillis(summary.averageResponseMillis),
          ),
        ],
      ),
      const Divider(height: 24),
    ],
  );
}

class _ModelLabel extends StatelessWidget {
  const _ModelLabel({required this.summary, super.key});

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
    return LayoutBuilder(
      builder: (context, constraints) {
        final filterWidth = constraints.maxWidth < 240
            ? constraints.maxWidth
            : 240.0;
        final title = Text(
          context.l10n.settingsStatisticsHistoryTitle,
          style: Theme.of(context).textTheme.titleMedium
              ?.copyWith(fontWeight: FontWeight.w600),
        );
        final filter = SizedBox(
          width: filterWidth,
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
                    _formatPerformanceIdentity(context, summary),
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
            ],
            onChanged: onChanged,
          ),
        );
        if (constraints.maxWidth < 376) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [title, const SizedBox(height: 12), filter],
          );
        }
        return Row(
          children: [
            Expanded(child: title),
            filter,
          ],
        );
      },
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
        context.l10n.statisticsReasoningEffort,
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
        '${sample.providerDisplayName} · ${sample.providerInstanceId} '
            '· ${sample.model}',
        _formatReasoningEffort(context, sample.reasoningEffort),
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
  Widget build(BuildContext context) => SettingsResourceRow(
    title:
        '${sample.providerDisplayName} · ${sample.providerInstanceId} '
        '· ${sample.model}',
    subtitle: _formatCompletedAt(context, sample.completedAt),
    children: [
      Wrap(
        spacing: 16,
        runSpacing: 8,
        children: [
          SettingsMetric(
            context.l10n.statisticsReasoningEffort,
            _formatReasoningEffort(context, sample.reasoningEffort),
          ),
          SettingsMetric(
            context.l10n.statisticsSpeed,
            formatTokenThroughput(sample.tokensPerSecond),
          ),
          SettingsMetric(
            context.l10n.statisticsOutputTokens,
            '${sample.completionTokens}',
          ),
          SettingsMetric('TTFT', _formatMillis(sample.ttftMillis.toDouble())),
          SettingsMetric(
            context.l10n.statisticsDecode,
            _formatMillis(sample.decodeMillis.toDouble()),
          ),
          SettingsMetric(
            context.l10n.statisticsTotalResponse,
            _formatMillis(sample.totalResponseMillis.toDouble()),
          ),
        ],
      ),
      const Divider(height: 24),
    ],
  );
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

String _formatReasoningEffort(BuildContext context, String? effort) {
  return effort ?? context.l10n.statisticsReasoningEffortUnspecified;
}

String _formatPerformanceIdentity(
  BuildContext context,
  ModelPerformanceSummaryView summary,
) {
  return [
    summary.providerDisplayName,
    summary.providerInstanceId,
    summary.model,
    _formatReasoningEffort(context, summary.reasoningEffort),
  ].join(' · ');
}

String _formatCompletedAt(BuildContext context, DateTime value) {
  final local = MaterialLocalizations.of(context);
  return '${local.formatShortDate(value)} ${TimeOfDay.fromDateTime(value).format(context)}';
}
