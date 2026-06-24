part of 'settings_page.dart';

class _ProviderList extends StatelessWidget {
  const _ProviderList({
    required this.providers,
    required this.selectedProviderId,
    required this.usageByProvider,
    required this.loadingProviderIds,
    required this.usageError,
    required this.onQueryChanged,
    required this.onAdd,
    required this.onSelect,
    required this.onSetDefault,
    required this.onRefreshAll,
    required this.onRefreshProvider,
    required this.onEdit,
    required this.onDelete,
  });

  final List<ProviderSettingsView> providers;
  final String? selectedProviderId;
  final Map<String, ProviderUsageView> usageByProvider;
  final Set<String> loadingProviderIds;
  final String? usageError;
  final ValueChanged<String> onQueryChanged;
  final VoidCallback onAdd;
  final ValueChanged<ProviderSettingsView> onSelect;
  final ValueChanged<ProviderSettingsView> onSetDefault;
  final Future<void> Function({String? providerId}) onRefreshAll;
  final ValueChanged<ProviderSettingsView> onRefreshProvider;
  final ValueChanged<ProviderSettingsView> onEdit;
  final ValueChanged<ProviderSettingsView>? onDelete;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _SettingsHeader(
          title: 'Providers',
          subtitle: 'Model providers, credentials, models, and usage',
          trailing: Wrap(
            spacing: 8,
            children: [
              OutlinedButton.icon(
                icon: const Icon(Icons.refresh),
                label: const Text('Refresh usage'),
                onPressed: () => onRefreshAll(),
              ),
              FilledButton.icon(
                icon: const Icon(Icons.add),
                label: const Text('Add provider'),
                onPressed: onAdd,
              ),
            ],
          ),
        ),
        const SizedBox(height: 14),
        Row(
          children: [
            Expanded(
              child: SearchBar(
                leading: const Icon(Icons.search),
                hintText: 'Search providers',
                onChanged: onQueryChanged,
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        Expanded(
          child: Align(
            alignment: Alignment.topCenter,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 980),
              child: providers.isEmpty
                  ? const Center(
                      child: StudioEmptyState(
                        icon: Icons.cloud_off_outlined,
                        title: 'No providers found',
                        message:
                            'Add a provider to configure credentials and models.',
                      ),
                    )
                  : ListView.builder(
                      itemCount: providers.length,
                      itemBuilder: (context, index) {
                        final provider = providers[index];
                        return Padding(
                          padding: const EdgeInsets.only(bottom: 12),
                          child: _ProviderCard(
                            provider: provider,
                            selected: provider.id == selectedProviderId,
                            usage: usageByProvider[provider.id],
                            usageLoading: loadingProviderIds.contains(
                              provider.id,
                            ),
                            usageError: usageError,
                            onOpen: () => onSelect(provider),
                            onSetDefault: () => onSetDefault(provider),
                            onRefreshUsage: () => onRefreshProvider(provider),
                            onEdit: () => onEdit(provider),
                            onDelete: onDelete == null
                                ? null
                                : () => onDelete!(provider),
                          ),
                        );
                      },
                    ),
            ),
          ),
        ),
      ],
    );
  }
}

class _ProviderCard extends StatelessWidget {
  const _ProviderCard({
    required this.provider,
    required this.selected,
    required this.usage,
    required this.usageLoading,
    required this.usageError,
    required this.onOpen,
    required this.onSetDefault,
    required this.onRefreshUsage,
    required this.onEdit,
    required this.onDelete,
  });

  final ProviderSettingsView provider;
  final bool selected;
  final ProviderUsageView? usage;
  final bool usageLoading;
  final String? usageError;
  final VoidCallback onOpen;
  final VoidCallback onSetDefault;
  final VoidCallback onRefreshUsage;
  final VoidCallback onEdit;
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final models = provider.allModels;
    return Material(
      color: selected ? StudioColors.claySoft : colors.surfaceContainerLowest,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(StudioRadii.md),
        side: BorderSide(
          color: selected
              ? StudioColors.clay.withValues(alpha: 0.34)
              : colors.outlineVariant,
        ),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(StudioRadii.md),
        onTap: onOpen,
        child: Padding(
          padding: const EdgeInsets.all(14),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  CircleAvatar(
                    radius: 18,
                    backgroundColor: selected
                        ? StudioColors.clay
                        : colors.surfaceContainerHigh,
                    child: Text(
                      _initials(provider.name),
                      style: Theme.of(context).textTheme.labelSmall?.copyWith(
                        color: selected ? Colors.white : context.studioInk,
                        fontWeight: FontWeight.w700,
                      ),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Expanded(
                              child: Text(
                                provider.name,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: Theme.of(context).textTheme.titleMedium
                                    ?.copyWith(
                                      color: selected
                                          ? StudioColors.clayDeep
                                          : context.studioInk,
                                      fontWeight: FontWeight.w700,
                                    ),
                              ),
                            ),
                            if (selected)
                              const _InfoPill(
                                icon: Icons.star_outline,
                                label: 'default',
                              ),
                          ],
                        ),
                        const SizedBox(height: 3),
                        Text(
                          provider.id,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: colors.onSurfaceVariant),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 10),
                  _ProviderStatusChip(provider: provider),
                ],
              ),
              const SizedBox(height: 10),
              Wrap(
                spacing: 8,
                runSpacing: 6,
                children: [
                  _MiniMeta(
                    icon: Icons.smart_toy_outlined,
                    label: provider.defaultModel,
                  ),
                  _MiniMeta(
                    icon: Icons.memory_outlined,
                    label: '${models.length} models',
                  ),
                  _MiniMeta(
                    icon: Icons.hub_outlined,
                    label: provider.providerKind,
                  ),
                  _MiniMeta(
                    icon: Icons.account_balance_wallet_outlined,
                    label: _providerUsageSummary(provider, usage, usageLoading),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              _ProviderUsagePanel(
                provider: provider,
                usage: usage,
                loading: usageLoading,
                error: usageError,
                onRefresh: onRefreshUsage,
              ),
              if (models.isNotEmpty) ...[
                const SizedBox(height: 12),
                Wrap(
                  spacing: 6,
                  runSpacing: 6,
                  children: [
                    for (final model in models.take(5))
                      StudioPill(label: model.slug),
                    if (models.length > 5)
                      StudioPill(label: '+${models.length - 5}'),
                  ],
                ),
              ],
              const SizedBox(height: 10),
              Align(
                alignment: Alignment.centerRight,
                child: Wrap(
                  spacing: 4,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    TextButton.icon(
                      icon: const Icon(Icons.open_in_new),
                      label: const Text('Details'),
                      onPressed: onOpen,
                    ),
                    TextButton.icon(
                      icon: const Icon(Icons.star_outline),
                      label: const Text('Set default'),
                      onPressed: selected ? null : onSetDefault,
                    ),
                    IconButton(
                      tooltip: 'Edit provider',
                      icon: const Icon(Icons.edit_outlined),
                      onPressed: onEdit,
                    ),
                    IconButton(
                      tooltip: 'Delete provider',
                      icon: const Icon(Icons.delete_outline),
                      onPressed: onDelete,
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ProviderUsagePanel extends StatelessWidget {
  const _ProviderUsagePanel({
    required this.provider,
    required this.usage,
    required this.loading,
    required this.error,
    required this.onRefresh,
  });

  final ProviderSettingsView provider;
  final ProviderUsageView? usage;
  final bool loading;
  final String? error;
  final VoidCallback onRefresh;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final usage = this.usage;
    return StudioPanel(
      backgroundColor: colors.surfaceContainerLowest.withValues(alpha: 0.78),
      borderColor: colors.outlineVariant.withValues(alpha: 0.76),
      radius: StudioRadii.sm,
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                Icons.account_balance_wallet_outlined,
                size: 17,
                color: colors.onSurfaceVariant,
              ),
              const SizedBox(width: 7),
              Expanded(
                child: Text(
                  'Usage',
                  style: Theme.of(context).textTheme.titleSmall,
                ),
              ),
              Text(
                _usageUpdatedLabel(usage?.updatedAt),
                style: Theme.of(context).textTheme.labelSmall?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
              const SizedBox(width: 4),
              IconButton(
                tooltip: _providerSupportsUsage(provider)
                    ? 'Refresh usage'
                    : 'Usage is not supported',
                icon: loading
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.refresh, size: 18),
                onPressed: loading || !_providerSupportsUsage(provider)
                    ? null
                    : onRefresh,
              ),
            ],
          ),
          if (error != null && error!.isNotEmpty) ...[
            const SizedBox(height: 8),
            _ProviderUsageMessage(
              icon: Icons.error_outline,
              message: error!,
              tone: _UsageTone.failed,
            ),
          ],
          const SizedBox(height: 8),
          if (usage == null)
            _ProviderUsageMessage(
              icon: loading
                  ? Icons.hourglass_empty
                  : Icons.account_balance_wallet_outlined,
              message: loading
                  ? 'Checking usage...'
                  : _providerSupportsUsage(provider)
                  ? 'Usage not loaded'
                  : 'Usage not supported',
              tone: loading ? _UsageTone.neutral : _UsageTone.muted,
            )
          else if (usage.status == 'ready' &&
              usage.usageKind == 'deepseekBalance' &&
              usage.balance != null)
            _DeepSeekUsage(usage: usage.balance!)
          else if (usage.status == 'ready' &&
              usage.usageKind == 'zhipuCodingPlan' &&
              usage.codingPlan != null)
            _ZhipuCodingPlanUsage(usage: usage.codingPlan!)
          else
            _ProviderUsageMessage(
              icon:
                  usage.status == 'failed' ||
                      usage.status == 'missingCredential'
                  ? Icons.error_outline
                  : Icons.info_outline,
              message: _providerUsageMessage(provider, usage),
              tone: usage.status == 'failed'
                  ? _UsageTone.failed
                  : usage.status == 'missingCredential'
                  ? _UsageTone.warning
                  : _UsageTone.muted,
            ),
        ],
      ),
    );
  }
}

class _DeepSeekUsage extends StatelessWidget {
  const _DeepSeekUsage({required this.usage});

  final DeepSeekBalanceUsageView usage;

  @override
  Widget build(BuildContext context) {
    final primary =
        usage.balances
            .where((item) => item.currency.toUpperCase() == 'CNY')
            .firstOrNull ??
        usage.balances.firstOrNull;
    if (primary == null) {
      return const _ProviderUsageMessage(
        icon: Icons.info_outline,
        message: 'Usage unavailable',
        tone: _UsageTone.muted,
      );
    }
    final colors = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                usage.isAvailable ? 'Available balance' : 'Balance unavailable',
                style: Theme.of(context).textTheme.labelMedium?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
            ),
            Text(
              '${primary.currency} ${primary.totalBalance}',
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ],
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 6,
          children: [
            _InfoPill(
              icon: Icons.card_giftcard_outlined,
              label: 'Granted ${primary.grantedBalance}',
            ),
            _InfoPill(
              icon: Icons.payments_outlined,
              label: 'Topped up ${primary.toppedUpBalance}',
            ),
            for (final item in usage.balances.where(
              (item) => item.currency != primary.currency,
            ))
              _InfoPill(
                icon: Icons.account_balance_wallet_outlined,
                label: '${item.currency} ${item.totalBalance}',
              ),
          ],
        ),
      ],
    );
  }
}

class _ZhipuCodingPlanUsage extends StatelessWidget {
  const _ZhipuCodingPlanUsage({required this.usage});

  final ZhipuCodingPlanUsageView usage;

  @override
  Widget build(BuildContext context) {
    final ordered = [
      _findQuotaLimit(usage.limits, 'fiveHour'),
      _findQuotaLimit(usage.limits, 'weekly'),
      _findQuotaLimit(usage.limits, 'mcpMonthly'),
      ...usage.limits.where((limit) => limit.window == 'other'),
    ].whereType<ZhipuQuotaLimitView>().toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (usage.level != null && usage.level!.isNotEmpty) ...[
          _InfoPill(
            icon: Icons.workspace_premium_outlined,
            label: usage.level!,
          ),
          const SizedBox(height: 8),
        ],
        if (ordered.isEmpty)
          const _ProviderUsageMessage(
            icon: Icons.info_outline,
            message: 'Usage unavailable',
            tone: _UsageTone.muted,
          )
        else
          LayoutBuilder(
            builder: (context, constraints) {
              final twoColumns = constraints.maxWidth >= 650;
              return Wrap(
                spacing: 10,
                runSpacing: 10,
                children: [
                  for (final limit in ordered)
                    SizedBox(
                      width: twoColumns
                          ? (constraints.maxWidth - 10) / 2
                          : constraints.maxWidth,
                      child: _QuotaCard(limit: limit),
                    ),
                ],
              );
            },
          ),
      ],
    );
  }
}

class _QuotaCard extends StatelessWidget {
  const _QuotaCard({required this.limit});

  final ZhipuQuotaLimitView limit;

  @override
  Widget build(BuildContext context) {
    final percent = _quotaRemainingPercent(limit);
    final colors = Theme.of(context).colorScheme;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: colors.surface,
        border: Border.all(color: colors.outlineVariant),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    _quotaTitle(limit),
                    style: Theme.of(context).textTheme.labelLarge,
                  ),
                ),
                Text(
                  _resetLabel(limit.nextResetAt),
                  style: Theme.of(context).textTheme.labelSmall?.copyWith(
                    color: colors.onSurfaceVariant,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 7),
            Text(
              _formatPercent(percent),
              style: Theme.of(context).textTheme.titleMedium,
            ),
            Text(
              _quotaDetail(limit),
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: colors.onSurfaceVariant),
            ),
            const SizedBox(height: 8),
            ClipRRect(
              borderRadius: BorderRadius.circular(999),
              child: LinearProgressIndicator(
                value: percent / 100,
                minHeight: 6,
                backgroundColor: colors.surfaceContainerHighest,
              ),
            ),
            if (limit.usageDetails.isNotEmpty) ...[
              const SizedBox(height: 8),
              Wrap(
                spacing: 6,
                runSpacing: 4,
                children: [
                  for (final detail in limit.usageDetails)
                    Tooltip(
                      message: detail.name,
                      child: Chip(
                        visualDensity: VisualDensity.compact,
                        label: Text(
                          '${detail.name} ${_formatToolUsage(detail)}',
                          overflow: TextOverflow.ellipsis,
                        ),
                      ),
                    ),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class _ProviderUsageMessage extends StatelessWidget {
  const _ProviderUsageMessage({
    required this.icon,
    required this.message,
    required this.tone,
  });

  final IconData icon;
  final String message;
  final _UsageTone tone;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final color = switch (tone) {
      _UsageTone.failed => colors.error,
      _UsageTone.warning => colors.tertiary,
      _UsageTone.neutral || _UsageTone.muted => colors.onSurfaceVariant,
    };
    return StudioInlineMessage(icon: icon, message: message, color: color);
  }
}

enum _UsageTone { failed, warning, neutral, muted }
