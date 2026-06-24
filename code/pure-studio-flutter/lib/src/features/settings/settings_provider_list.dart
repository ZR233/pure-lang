part of 'settings_page.dart';

class _ProviderList extends StatelessWidget {
  const _ProviderList({
    required this.providers,
    required this.defaultProviderId,
    required this.filtering,
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
  final String? defaultProviderId;
  final bool filtering;
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
          title: context.l10n.settingsProvidersTitle,
          subtitle: context.l10n.settingsProvidersSubtitle,
          trailing: Wrap(
            spacing: 8,
            children: [
              OutlinedButton.icon(
                icon: const Icon(Icons.refresh),
                label: Text(context.l10n.settingsRefreshUsage),
                onPressed: () => onRefreshAll(),
              ),
              FilledButton.icon(
                icon: const Icon(Icons.add),
                label: Text(context.l10n.settingsAddProvider),
                onPressed: onAdd,
              ),
            ],
          ),
        ),
        const SizedBox(height: 14),
        _SettingsSearchField(
          hintText: context.l10n.settingsSearchProviders,
          onChanged: onQueryChanged,
        ),
        const SizedBox(height: 14),
        Expanded(
          child: providers.isEmpty
              ? Center(
                  child: StudioEmptyState(
                    icon: Icons.cloud_off_outlined,
                    title: filtering
                        ? context.l10n.settingsNoProvidersMatchTitle
                        : context.l10n.settingsNoProvidersTitle,
                    message: filtering
                        ? context.l10n.settingsNoProvidersMatchMessage
                        : context.l10n.settingsNoProvidersMessage,
                  ),
                )
              : LayoutBuilder(
                  builder: (context, constraints) {
                    final twoColumns = constraints.maxWidth >= 840;
                    final cardWidth = twoColumns
                        ? (constraints.maxWidth - 14) / 2
                        : constraints.maxWidth;
                    return SingleChildScrollView(
                      child: Wrap(
                        spacing: 14,
                        runSpacing: 14,
                        children: [
                          for (final provider in providers)
                            SizedBox(
                              width: cardWidth,
                              child: _ProviderCard(
                                provider: provider,
                                isDefault: provider.id == defaultProviderId,
                                usage: usageByProvider[provider.id],
                                usageLoading: loadingProviderIds.contains(
                                  provider.id,
                                ),
                                usageError: usageError,
                                onOpen: () => onSelect(provider),
                                onSetDefault: () => onSetDefault(provider),
                                onRefreshUsage: () =>
                                    onRefreshProvider(provider),
                                onEdit: () => onEdit(provider),
                                onDelete: onDelete == null
                                    ? null
                                    : () => onDelete!(provider),
                              ),
                            ),
                        ],
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }
}

class _ProviderCard extends StatelessWidget {
  const _ProviderCard({
    required this.provider,
    required this.isDefault,
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
  final bool isDefault;
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
    return StudioPanel(
      backgroundColor: colors.surfaceContainerLowest,
      borderColor: isDefault
          ? StudioColors.clay.withValues(alpha: 0.62)
          : context.studioLine,
      radius: StudioRadii.md,
      shadow: true,
      child: InkWell(
        onTap: onOpen,
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  _ProviderLogo(provider: provider, active: isDefault),
                  const SizedBox(width: 12),
                  Expanded(
                    child: _ProviderCardTitle(
                      provider: provider,
                      isDefault: isDefault,
                    ),
                  ),
                  const SizedBox(width: 8),
                  _ProviderCardActions(
                    isDefault: isDefault,
                    onOpen: onOpen,
                    onSetDefault: onSetDefault,
                    onRefreshUsage: onRefreshUsage,
                    onEdit: onEdit,
                    onDelete: onDelete,
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Wrap(
                spacing: 6,
                runSpacing: 6,
                children: [
                  _MiniMeta(icon: Icons.key_outlined, label: provider.id),
                  _ProviderStatusChip(provider: provider),
                  _MiniMeta(
                    icon: Icons.smart_toy_outlined,
                    label: provider.defaultModel,
                  ),
                  _MiniMeta(
                    icon: Icons.account_balance_wallet_outlined,
                    label: _providerUsageSummary(
                      context,
                      provider,
                      usage,
                      usageLoading,
                    ),
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
                    for (final model in models.take(4))
                      StudioPill(
                        label: model.slug,
                        backgroundColor: context.studioPaper2,
                        borderColor: context.studioLine,
                      ),
                    if (models.length > 4)
                      StudioPill(label: '+${models.length - 4}'),
                  ],
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

class _ProviderLogo extends StatelessWidget {
  const _ProviderLogo({required this.provider, required this.active});

  final ProviderSettingsView provider;
  final bool active;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SizedBox.square(
      dimension: 46,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: active ? StudioColors.clay : context.studioPaper2,
          borderRadius: BorderRadius.circular(11),
          border: Border.all(color: colors.outlineVariant),
        ),
        child: Center(
          child: Text(
            _initials(provider.name),
            style: Theme.of(context).textTheme.titleMedium?.copyWith(
              color: active ? Colors.white : context.studioInk,
              fontWeight: FontWeight.w800,
            ),
          ),
        ),
      ),
    );
  }
}

class _ProviderCardTitle extends StatelessWidget {
  const _ProviderCardTitle({required this.provider, required this.isDefault});

  final ProviderSettingsView provider;
  final bool isDefault;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                provider.name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.titleMedium?.copyWith(
                  color: context.studioInk,
                  fontWeight: FontWeight.w700,
                ),
              ),
            ),
            if (isDefault) ...[
              const SizedBox(width: 8),
              StudioPill(
                label: context.l10n.settingsDefaultBadge,
                backgroundColor: StudioColors.claySoft,
                foregroundColor: StudioColors.clayDeep,
                borderColor: StudioColors.claySoft,
              ),
            ],
          ],
        ),
        if (provider.subtitle.isNotEmpty) ...[
          const SizedBox(height: 2),
          Text(
            provider.subtitle,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: Theme.of(context).textTheme.bodySmall?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ],
    );
  }
}

class _ProviderCardActions extends StatelessWidget {
  const _ProviderCardActions({
    required this.isDefault,
    required this.onOpen,
    required this.onSetDefault,
    required this.onRefreshUsage,
    required this.onEdit,
    required this.onDelete,
  });

  final bool isDefault;
  final VoidCallback onOpen;
  final VoidCallback onSetDefault;
  final VoidCallback onRefreshUsage;
  final VoidCallback onEdit;
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) {
    return Wrap(
      spacing: 2,
      runSpacing: 2,
      crossAxisAlignment: WrapCrossAlignment.center,
      children: [
        Tooltip(
          message: isDefault
              ? context.l10n.settingsDefaultProvider
              : context.l10n.settingsSetAsDefaultProvider,
          child: IconButton(
            style: IconButton.styleFrom(
              minimumSize: const Size.square(30),
              tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            ),
            icon: Icon(
              isDefault
                  ? Icons.check_circle_outline
                  : Icons.radio_button_unchecked,
            ),
            onPressed: isDefault ? null : onSetDefault,
          ),
        ),
        IconButton(
          style: IconButton.styleFrom(
            minimumSize: const Size.square(30),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          tooltip: context.l10n.settingsOpenDetails,
          icon: const Icon(Icons.open_in_new),
          onPressed: onOpen,
        ),
        IconButton(
          style: IconButton.styleFrom(
            minimumSize: const Size.square(30),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          tooltip: context.l10n.settingsRefreshUsage,
          icon: const Icon(Icons.refresh),
          onPressed: onRefreshUsage,
        ),
        IconButton(
          style: IconButton.styleFrom(
            minimumSize: const Size.square(30),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          tooltip: context.l10n.settingsEditProvider,
          icon: const Icon(Icons.edit_outlined),
          onPressed: onEdit,
        ),
        IconButton(
          style: IconButton.styleFrom(
            minimumSize: const Size.square(30),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          tooltip: context.l10n.settingsDeleteProvider,
          icon: const Icon(Icons.delete_outline),
          onPressed: onDelete,
        ),
      ],
    );
  }
}
