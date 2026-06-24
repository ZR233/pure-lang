part of 'settings_page.dart';

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
      backgroundColor: context.studioPaper2,
      borderColor: context.studioLine,
      radius: StudioRadii.sm,
      padding: const EdgeInsets.all(13),
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
                  context.l10n.settingsUsageTitle,
                  style: Theme.of(context).textTheme.titleSmall?.copyWith(
                    color: context.studioInk,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              Text(
                _usageUpdatedLabel(context, usage?.updatedAt),
                style: Theme.of(context).textTheme.labelSmall?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
              const SizedBox(width: 4),
              IconButton(
                tooltip: _providerSupportsUsage(provider)
                    ? context.l10n.settingsRefreshUsage
                    : context.l10n.settingsUsageNotSupported,
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
                  ? context.l10n.settingsUsageChecking
                  : _providerSupportsUsage(provider)
                  ? context.l10n.settingsUsageNotLoaded
                  : context.l10n.settingsUsageNotSupported,
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
              message: _providerUsageMessage(context, provider, usage),
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
      return _ProviderUsageMessage(
        icon: Icons.info_outline,
        message: context.l10n.settingsUsageUnavailable,
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
                usage.isAvailable
                    ? context.l10n.settingsUsageAvailableBalance
                    : context.l10n.settingsUsageBalanceUnavailable,
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
              label: context.l10n.settingsUsageGranted(primary.grantedBalance),
            ),
            _InfoPill(
              icon: Icons.payments_outlined,
              label: context.l10n.settingsUsageToppedUp(
                primary.toppedUpBalance,
              ),
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
          _ProviderUsageMessage(
            icon: Icons.info_outline,
            message: context.l10n.settingsUsageUnavailable,
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
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.colors.surfaceContainerLowest,
        border: Border.all(color: context.studioLine),
        borderRadius: BorderRadius.circular(StudioRadii.sm),
      ),
      child: Padding(
        padding: const EdgeInsets.all(11),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    _quotaTitle(context, limit),
                    style: Theme.of(
                      context,
                    ).textTheme.labelLarge?.copyWith(color: context.studioInk),
                  ),
                ),
                Text(
                  _resetLabel(context, limit.nextResetAt),
                  style: Theme.of(context).textTheme.labelSmall?.copyWith(
                    color: context.studioInkSoft,
                  ),
                ),
              ],
            ),
            const SizedBox(height: 7),
            Text(
              _formatPercent(percent),
              style: Theme.of(context).textTheme.titleMedium?.copyWith(
                color: context.studioInk,
                fontWeight: FontWeight.w600,
              ),
            ),
            Text(
              _quotaDetail(context, limit),
              style: Theme.of(
                context,
              ).textTheme.bodySmall?.copyWith(color: context.studioInkSoft),
            ),
            const SizedBox(height: 8),
            ClipRRect(
              borderRadius: BorderRadius.circular(999),
              child: SizedBox(
                height: 6,
                child: LinearProgressIndicator(
                  value: percent / 100,
                  backgroundColor: context.studioPaper3,
                  color: percent >= 65 ? StudioColors.sage : StudioColors.clay,
                ),
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
                      child: StudioPill(
                        label: '${detail.name} ${_formatToolUsage(detail)}',
                        backgroundColor: context.studioPaper2,
                        borderColor: context.studioLine,
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
