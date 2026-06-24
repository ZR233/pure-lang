part of 'settings_page.dart';

enum _ProviderDraftMode { create, edit }

class _ProviderDraft {
  const _ProviderDraft({
    required this.mode,
    required this.originalId,
    required this.provider,
  });

  factory _ProviderDraft.create(ProviderSettingsView provider) {
    return _ProviderDraft(
      mode: _ProviderDraftMode.create,
      originalId: provider.id,
      provider: provider,
    );
  }

  factory _ProviderDraft.edit(ProviderSettingsView provider) {
    return _ProviderDraft(
      mode: _ProviderDraftMode.edit,
      originalId: provider.id,
      provider: provider,
    );
  }

  final _ProviderDraftMode mode;
  final String originalId;
  final ProviderSettingsView provider;

  _ProviderDraft copyWith({ProviderSettingsView? provider}) {
    return _ProviderDraft(
      mode: mode,
      originalId: originalId,
      provider: provider ?? this.provider,
    );
  }
}

class _ProviderTemplate {
  const _ProviderTemplate({
    required this.id,
    required this.name,
    required this.baseUrl,
    required this.defaultModel,
    required this.providerKind,
    required this.defaultModels,
  });

  final String id;
  final String name;
  final String baseUrl;
  final String defaultModel;
  final String providerKind;
  final List<ProviderModelView> defaultModels;

  ProviderSettingsView createProvider(String providerId) {
    return ProviderSettingsView(
      id: providerId,
      templateKind: id,
      name: name,
      subtitle: '$name Platform',
      baseUrl: baseUrl,
      bearerToken: '',
      hasBearerToken: false,
      defaultModel: defaultModel,
      models: defaultModels,
      defaultModels: defaultModels,
      customModels: const [],
      status: 'missingCredential',
      usageLabel: '${defaultModels.length} models',
      modelCount: '${defaultModels.length}',
      updatedAt: 'Draft',
      providerKind: providerKind,
    );
  }
}

const _providerTemplates = [
  _ProviderTemplate(
    id: 'deepseek',
    name: 'DeepSeek',
    baseUrl: 'https://api.deepseek.com',
    defaultModel: 'deepseek-v4-flash',
    providerKind: 'deep_seek',
    defaultModels: [
      ProviderModelView(
        slug: 'deepseek-v4-flash',
        displayName: 'DeepSeek V4 Flash',
        reasoningEfforts: ['high', 'max'],
        contextWindow: 1000000,
        maxOutputTokens: 384000,
        currency: 'CNY',
        inputPricePerMTok: 1,
        outputPricePerMTok: 2,
      ),
      ProviderModelView(
        slug: 'deepseek-v4-pro',
        displayName: 'DeepSeek V4 Pro',
        reasoningEfforts: ['high', 'max'],
        contextWindow: 1000000,
        maxOutputTokens: 384000,
        currency: 'CNY',
        inputPricePerMTok: 3,
        outputPricePerMTok: 6,
      ),
    ],
  ),
  _ProviderTemplate(
    id: 'openai',
    name: 'OpenAI',
    baseUrl: 'https://api.openai.com/v1',
    defaultModel: 'gpt-5.5',
    providerKind: 'open_ai',
    defaultModels: [
      ProviderModelView(
        slug: 'gpt-5.5',
        displayName: 'GPT-5.5',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
      ProviderModelView(
        slug: 'gpt-5.4',
        displayName: 'GPT-5.4',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
      ProviderModelView(
        slug: 'gpt-5.4-mini',
        displayName: 'GPT-5.4-Mini',
        reasoningEfforts: ['medium', 'low', 'high', 'xhigh'],
      ),
    ],
  ),
  _ProviderTemplate(
    id: 'zhipu',
    name: 'Zhipu',
    baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
    defaultModel: 'glm-5.2',
    providerKind: 'zhipu',
    defaultModels: [
      ProviderModelView(
        slug: 'glm-5.2',
        displayName: 'GLM-5.2',
        reasoningEfforts: ['enabled', 'none'],
      ),
      ProviderModelView(
        slug: 'glm-5',
        displayName: 'GLM-5',
        reasoningEfforts: ['enabled', 'none'],
      ),
    ],
  ),
  _ProviderTemplate(
    id: 'zhipu-coding-plan',
    name: 'Zhipu Coding Plan',
    baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
    defaultModel: 'glm-5.2',
    providerKind: 'zhipu',
    defaultModels: [
      ProviderModelView(
        slug: 'glm-5.2',
        displayName: 'GLM-5.2',
        reasoningEfforts: ['enabled', 'none'],
      ),
      ProviderModelView(
        slug: 'glm-5',
        displayName: 'GLM-5',
        reasoningEfforts: ['enabled', 'none'],
      ),
    ],
  ),
];

String _initials(String value) {
  final words = value.trim().split(RegExp(r'\s+'));
  if (words.isEmpty || words.first.isEmpty) {
    return '?';
  }
  if (words.length == 1) {
    final word = words.first;
    return word.substring(0, word.length < 2 ? word.length : 2).toUpperCase();
  }
  return words
      .take(2)
      .map((word) => word.isEmpty ? '' : word.substring(0, 1))
      .join()
      .toUpperCase();
}

String _modelPriceLabel(ProviderModelView model) {
  if (model.currency.isEmpty ||
      model.inputPricePerMTok == null ||
      model.outputPricePerMTok == null) {
    return '';
  }
  return '${model.currency} ${_trimNumber(model.inputPricePerMTok!)}/${_trimNumber(model.outputPricePerMTok!)}';
}

String _trimNumber(double value) {
  return value
      .toStringAsFixed(value.truncateToDouble() == value ? 0 : 3)
      .replaceFirst(RegExp(r'\.?0+$'), '');
}

bool _providerSupportsUsage(ProviderSettingsView provider) {
  return provider.templateKind == 'deepseek' ||
      provider.templateKind == 'zhipu-coding-plan';
}

String _providerUsageSummary(
  BuildContext context,
  ProviderSettingsView provider,
  ProviderUsageView? usage,
  bool loading,
) {
  if (loading && usage == null) {
    return context.l10n.settingsUsageCheckingShort;
  }
  if (usage == null) {
    return _providerSupportsUsage(provider)
        ? context.l10n.settingsUsageNotLoaded
        : context.l10n.settingsUsageUnsupported;
  }
  return switch (usage.status) {
    'unsupported' => context.l10n.settingsUsageUnsupported,
    'missingCredential' => context.l10n.settingsUsageMissingKey,
    'failed' => context.l10n.settingsUsageFailed,
    'ready' => _readyProviderUsageSummary(provider, usage),
    _ => context.l10n.settingsUsageUnavailable,
  };
}

String _readyProviderUsageSummary(
  ProviderSettingsView provider,
  ProviderUsageView usage,
) {
  if (usage.usageKind == 'deepseekBalance' && usage.balance != null) {
    final primary =
        usage.balance!.balances
            .where((item) => item.currency.toUpperCase() == 'CNY')
            .firstOrNull ??
        usage.balance!.balances.firstOrNull;
    return primary == null
        ? 'Usage unavailable'
        : '${primary.currency} ${primary.totalBalance}';
  }
  if (provider.templateKind == 'zhipu-coding-plan' &&
      usage.codingPlan != null) {
    final fiveHour = _findQuotaLimit(usage.codingPlan!.limits, 'fiveHour');
    final weekly = _findQuotaLimit(usage.codingPlan!.limits, 'weekly');
    if (fiveHour != null && weekly != null) {
      return '5h ${_formatPercent(_quotaRemainingPercent(fiveHour))} · 7d ${_formatPercent(_quotaRemainingPercent(weekly))}';
    }
  }
  return 'Usage unavailable';
}

String _providerUsageMessage(
  BuildContext context,
  ProviderSettingsView provider,
  ProviderUsageView usage,
) {
  return switch (usage.status) {
    'missingCredential' =>
      usage.message ?? context.l10n.settingsUsageApiKeyMissing,
    'failed' => usage.message ?? context.l10n.settingsUsageQueryFailed,
    'unsupported' => context.l10n.settingsUsageUnsupportedForProvider(
      provider.name,
    ),
    _ => context.l10n.settingsUsageUnavailable,
  };
}

String _usageUpdatedLabel(BuildContext context, int? seconds) {
  if (seconds == null || seconds <= 0) {
    return context.l10n.settingsUsageNotChecked;
  }
  return context.l10n.settingsUsageUpdated(_formatUnixShort(seconds));
}

ZhipuQuotaLimitView? _findQuotaLimit(
  List<ZhipuQuotaLimitView> limits,
  String window,
) {
  return limits.where((limit) => limit.window == window).firstOrNull;
}

double _quotaRemainingPercent(ZhipuQuotaLimitView limit) {
  final remaining = limit.remaining;
  final total = limit.total;
  if (remaining != null && total != null && total > 0) {
    return _clampPercent((remaining / total) * 100);
  }
  return _clampPercent(100 - limit.percentage);
}

String _quotaTitle(BuildContext context, ZhipuQuotaLimitView limit) {
  return switch (limit.window) {
    'fiveHour' => context.l10n.settingsUsageFiveHourQuota,
    'weekly' => context.l10n.settingsUsageWeeklyQuota,
    'mcpMonthly' => context.l10n.settingsUsageMcpQuota,
    _ => limit.label.isEmpty ? context.l10n.settingsUsageQuota : limit.label,
  };
}

String _quotaDetail(BuildContext context, ZhipuQuotaLimitView limit) {
  final remaining = limit.remaining;
  final currentValue = limit.currentValue;
  final total = limit.total;
  if (remaining != null && total != null) {
    return context.l10n.settingsUsageQuotaRemaining(
      _formatCompactNumber(remaining),
      _formatCompactNumber(total),
    );
  }
  if (currentValue != null && total != null) {
    return context.l10n.settingsUsageQuotaUsed(
      _formatCompactNumber(currentValue),
      _formatCompactNumber(total),
    );
  }
  return context.l10n.settingsUsagePercentRemaining(
    _formatPercent(_quotaRemainingPercent(limit)),
  );
}

String _resetLabel(BuildContext context, int? seconds) {
  if (seconds == null || seconds <= 0) {
    return '';
  }
  return context.l10n.settingsUsageReset(_formatUnixShort(seconds));
}

String _formatToolUsage(ZhipuToolUsageDetailView detail) {
  final currentValue = detail.currentValue;
  final total = detail.total;
  if (currentValue != null && total != null) {
    return '${_formatCompactNumber((total - currentValue).clamp(0, total))}/${_formatCompactNumber(total)}';
  }
  if (currentValue != null) {
    return _formatCompactNumber(currentValue);
  }
  if (detail.percentage != null) {
    return _formatPercent(_clampPercent(100 - detail.percentage!));
  }
  return '';
}

String _formatUnixShort(int seconds) {
  final date = DateTime.fromMillisecondsSinceEpoch(seconds * 1000);
  final hour = date.hour.toString().padLeft(2, '0');
  final minute = date.minute.toString().padLeft(2, '0');
  return '${date.month}/${date.day} $hour:$minute';
}

String _formatCompactNumber(num value) {
  final number = value.toDouble();
  final abs = number.abs();
  if (abs >= 1000000) {
    return '${_trimNumber(number / 1000000)}M';
  }
  if (abs >= 1000) {
    return '${_trimNumber(number / 1000)}K';
  }
  return _trimNumber(number);
}

String _formatPercent(double value) => '${_trimNumber(value)}%';

double _clampPercent(double value) {
  if (!value.isFinite) {
    return 0;
  }
  if (value < 0) {
    return 0;
  }
  if (value > 100) {
    return 100;
  }
  return value;
}
