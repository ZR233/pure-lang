import {
  AlertCircle,
  Activity,
  CheckCircle2,
  Cpu,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Server,
  Trash2,
  Wallet,
} from "lucide-react";
import type { TFunction } from "i18next";
import { useMemo, useState, type Dispatch, type SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type {
  ModelRecord,
  ProviderKind,
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  ProviderTemplateRecord,
  ProviderUsageRecord,
  ZhipuQuotaLimit,
} from "../types";
import {
  applyProviderTemplate,
  cloneProvider,
  createProviderFromTemplate,
  normalizeProvider,
  replaceProvider,
  suggestProviderId,
} from "../lib/provider-settings";
import { allModels, initials, providerStatusClass, translateStatus } from "../lib/utils";
import { ProviderEditPage } from "./ProviderEditPage";

type ProviderSettingsProps = {
  providers: ProviderRecord[];
  templates: ProviderTemplateRecord[];
  providerUsages: ProviderUsageRecord[];
  providerUsagesLoading: boolean;
  providerUsageError: string | null;
  selectedProviderId: string | null;
  providerSearch: string;
  setProviderSearch: Dispatch<SetStateAction<string>>;
  onSaveProviderSettings: (snapshot: ProviderSettingsSaveSnapshot) => Promise<boolean>;
  onRefreshProviderUsages: () => void;
};

type ProviderDraft = {
  mode: "create" | "edit";
  originalId: string | null;
  provider: ProviderRecord;
  autoId: string;
  autoName: string;
};

export function ProviderSettings({
  providers,
  templates,
  providerUsages,
  providerUsagesLoading,
  providerUsageError,
  selectedProviderId,
  providerSearch,
  setProviderSearch,
  onSaveProviderSettings,
  onRefreshProviderUsages,
}: ProviderSettingsProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<ProviderDraft | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const providerUsageById = useMemo(
    () => new Map(providerUsages.map((usage) => [usage.providerId, usage])),
    [providerUsages],
  );
  const filteredProviders = providers.filter((provider) => {
    const query = providerSearch.trim().toLowerCase();
    if (!query) {
      return true;
    }
    const modelText = allModels(provider)
      .map((model) => `${model.slug} ${model.displayName}`)
      .join(" ")
      .toLowerCase();
    return (
      provider.name.toLowerCase().includes(query) ||
      provider.id.toLowerCase().includes(query) ||
      modelText.includes(query)
    );
  });
  const selectedProvider =
    providers.find((provider) => provider.id === selectedProviderId) ?? providers[0] ?? null;

  async function saveSnapshot(snapshot: ProviderSettingsSaveSnapshot) {
    if (isSaving) {
      return false;
    }
    setIsSaving(true);
    try {
      return await onSaveProviderSettings(snapshot);
    } finally {
      setIsSaving(false);
    }
  }

  function startAddProvider() {
    const template = templates[0];
    if (!template) {
      return;
    }
    const id = suggestProviderId(providers, template.id);
    setDraft({
      mode: "create",
      originalId: null,
      provider: createProviderFromTemplate(template, id),
      autoId: id,
      autoName: template.name,
    });
  }

  function startEditProvider(provider: ProviderRecord) {
    setDraft({
      mode: "edit",
      originalId: provider.id,
      provider: cloneProvider(provider),
      autoId: provider.id,
      autoName: provider.name,
    });
  }

  function updateDraftProvider(updater: (provider: ProviderRecord) => ProviderRecord) {
    setDraft((current) =>
      current
        ? {
            ...current,
            provider: normalizeProvider(updater(current.provider)),
          }
        : current,
    );
  }

  function changeDraftTemplate(kind: ProviderKind) {
    const template = templates.find((item) => item.id === kind);
    if (!template) {
      return;
    }
    setDraft((current) => {
      if (!current) {
        return current;
      }
      const shouldUseTemplateId =
        current.mode === "create" && current.provider.id === current.autoId;
      const shouldUseTemplateName =
        current.mode === "create" && current.provider.name === current.autoName;
      const nextId = shouldUseTemplateId
        ? suggestProviderId(providers, template.id)
        : current.provider.id;
      const nextName = shouldUseTemplateName ? template.name : current.provider.name;
      return {
        ...current,
        provider: applyProviderTemplate(current.provider, template, {
          id: nextId,
          name: nextName,
        }),
        autoId: current.mode === "create" ? nextId : current.autoId,
        autoName: current.mode === "create" ? template.name : current.autoName,
      };
    });
  }

  function addCustomModel() {
    updateDraftProvider((provider) => {
      const existing = new Set(allModels(provider).map((model) => model.slug));
      let slug = "custom-model";
      for (let index = 2; existing.has(slug); index += 1) {
        slug = `custom-model-${index}`;
      }
      return {
        ...provider,
        customModels: [
          ...provider.customModels,
          {
            slug,
            displayName: t("model.defaultCustomModelName"),
            reasoningEfforts: ["high"],
          },
        ],
        defaultModel: provider.defaultModel || slug,
      };
    });
  }

  function updateCustomModel(index: number, patch: Partial<ModelRecord>) {
    updateDraftProvider((provider) => ({
      ...provider,
      customModels: provider.customModels.map((model, modelIndex) =>
        modelIndex === index ? { ...model, ...patch } : model,
      ),
    }));
  }

  function removeCustomModel(index: number) {
    updateDraftProvider((provider) => ({
      ...provider,
      customModels: provider.customModels.filter((_, modelIndex) => modelIndex !== index),
    }));
  }

  async function saveDraft() {
    if (!draft) {
      return;
    }
    const nextProvider = normalizeProvider(draft.provider);
    const nextProviders =
      draft.mode === "create"
        ? [...providers, nextProvider]
        : replaceProvider(providers, draft.originalId ?? nextProvider.id, nextProvider);
    const nextSelectedProviderId =
      draft.mode === "create" || selectedProviderId === draft.originalId
        ? nextProvider.id
        : selectedProviderId;
    const saved = await saveSnapshot({
      providers: nextProviders,
      selectedProviderId: nextSelectedProviderId,
    });
    if (saved) {
      setDraft(null);
    }
  }

  async function selectProvider(providerId: string) {
    if (providerId === selectedProviderId) {
      return;
    }
    await saveSnapshot({ selectedProviderId: providerId });
  }

  async function removeProvider(providerId: string) {
    if (providers.length <= 1) {
      return;
    }
    const remainingProviders = providers.filter((provider) => provider.id !== providerId);
    const nextSelectedProviderId =
      selectedProviderId === providerId ? (remainingProviders[0]?.id ?? null) : selectedProviderId;
    const saved = await saveSnapshot({
      providers: remainingProviders,
      selectedProviderId: nextSelectedProviderId,
    });
    if (saved && draft?.originalId === providerId) {
      setDraft(null);
    }
  }

  return (
    <div className="provider-settings">
      {draft ? (
        <ProviderEditPage
          mode={draft.mode}
          provider={draft.provider}
          templates={templates}
          isSaving={isSaving}
          onCancel={() => setDraft(null)}
          onSave={() => void saveDraft()}
          onChangeTemplate={changeDraftTemplate}
          onUpdateProvider={updateDraftProvider}
          onAddCustomModel={addCustomModel}
          onUpdateCustomModel={updateCustomModel}
          onRemoveCustomModel={removeCustomModel}
        />
      ) : (
        <section className="provider-directory">
          <div className="provider-console-head">
            <div>
              <h2>{t("settings.providerRoute")}</h2>
              <p>{t("settings.providerRouteDesc")}</p>
            </div>
            <div className="provider-console-tools">
              <label className="search-box">
                <Search size={16} />
                <input
                  value={providerSearch}
                  onChange={(event) => setProviderSearch(event.target.value)}
                  placeholder={t("provider.searchPlaceholder")}
                />
              </label>
              <div className="provider-add-actions">
                <button
                  className="provider-refresh-button"
                  disabled={providerUsagesLoading}
                  onClick={onRefreshProviderUsages}
                  title={t("provider.refreshUsageTooltip")}
                >
                  <RefreshCw size={16} className={providerUsagesLoading ? "spin" : undefined} />
                  {t("provider.refreshUsage")}
                </button>
                <button disabled={isSaving || templates.length === 0} onClick={startAddProvider}>
                  <Plus size={16} />
                  {t("provider.addProvider")}
                </button>
              </div>
            </div>
          </div>

          <div className="provider-card-list">
            {providerUsageError ? (
              <div className="provider-usage-banner">
                <AlertCircle size={16} />
                <span>{providerUsageError}</span>
              </div>
            ) : null}
            {filteredProviders.length === 0 ? (
              <div className="provider-empty-state">
                <Server size={28} />
                <strong>{t("provider.noMatchingProviders")}</strong>
                <span>{t("provider.clearSearchHint")}</span>
              </div>
            ) : (
              filteredProviders.map((provider) => {
                const isActive = provider.id === selectedProvider?.id;
                const models = allModels(provider);
                const defaultModel =
                  models.find((model) => model.slug === provider.defaultModel)?.displayName ||
                  provider.defaultModel ||
                  t("provider.notSelected");
                const previewModels = models.slice(0, 4);
                const remainingModels = Math.max(0, models.length - previewModels.length);
                const usage = providerUsageById.get(provider.id);

                return (
                  <article
                    key={provider.id}
                    className={`provider-card ${isActive ? "active" : ""}`}
                  >
                    <div className="provider-card-shell">
                      <button
                        className="provider-card-select"
                        disabled={isSaving}
                        onClick={() => void selectProvider(provider.id)}
                      >
                        <span className={`provider-badge provider-${provider.templateKind}`}>
                          {initials(provider.name) || "P"}
                        </span>
                        <span className="provider-title-block">
                          <span className="provider-title-line">
                            <strong>{provider.name || provider.id}</strong>
                            <span className={`provider-state ${providerStatusClass(provider)}`}>
                              <CheckCircle2 size={14} />
                              {translateStatus(provider.status, t)}
                            </span>
                            {isActive ? (
                              <span className="default-route">{t("provider.defaultRoute")}</span>
                            ) : null}
                          </span>
                          <span className="provider-card-key">
                            <Server size={14} />
                            <span>{provider.id}</span>
                          </span>
                        </span>
                      </button>

                      <div className="provider-card-actions">
                        <button
                          className="icon-button provider-icon-action"
                          disabled={isSaving}
                          onClick={() => startEditProvider(provider)}
                          title={t("provider.editTooltip")}
                        >
                          <Pencil size={16} />
                        </button>
                        <button
                          className="icon-button provider-icon-action danger"
                          disabled={isSaving || providers.length <= 1}
                          onClick={() => void removeProvider(provider.id)}
                          title={t("provider.deleteTooltip")}
                        >
                          <Trash2 size={16} />
                        </button>
                      </div>
                    </div>

                    <div className="provider-card-meta">
                      <span>
                        <Cpu size={14} />
                        <small>{t("provider.defaultModel")}</small>
                        <strong>{defaultModel}</strong>
                      </span>
                      <span>
                        <Server size={14} />
                        <small>{t("model.title")}</small>
                        <strong>{t("provider.models", { count: provider.modelCount })}</strong>
                      </span>
                      <span className={`provider-usage-meta ${providerUsageStatusClass(usage, providerUsagesLoading)}`}>
                        <Wallet size={14} />
                        <small>{t("provider.usage")}</small>
                        <strong>{providerUsageSummary(provider, usage, providerUsagesLoading, t)}</strong>
                      </span>
                    </div>

                    <ProviderUsagePanel
                      usage={usage}
                      loading={providerUsagesLoading}
                      t={t}
                    />

                    <div className="provider-model-strip">
                      <span className="model-count-pill">
                        {t("provider.models", { count: provider.modelCount })}
                      </span>
                      {previewModels.map((model) => (
                        <span
                          key={model.slug}
                          className={
                            model.slug === provider.defaultModel ? "model-chip active" : "model-chip"
                          }
                        >
                          {model.displayName || model.slug}
                        </span>
                      ))}
                      {remainingModels > 0 ? (
                        <span className="model-chip muted-chip">+{remainingModels}</span>
                      ) : null}
                    </div>
                  </article>
                );
              })
            )}
          </div>
        </section>
      )}
    </div>
  );
}

type ProviderUsagePanelProps = {
  usage: ProviderUsageRecord | undefined;
  loading: boolean;
  t: TFunction;
};

function ProviderUsagePanel({ usage, loading, t }: ProviderUsagePanelProps) {
  if (loading && !usage) {
    return (
      <div className="provider-usage-panel checking">
        <Activity size={16} />
        <span>{t("provider.usageChecking")}</span>
      </div>
    );
  }

  if (!usage) {
    return (
      <div className="provider-usage-panel muted">
        <Activity size={16} />
        <span>{t("provider.usageNotLoaded")}</span>
      </div>
    );
  }

  switch (usage.status) {
    case "unsupported":
      return (
        <div className="provider-usage-panel muted">
          <Activity size={16} />
          <span>{t("provider.usageUnsupported")}</span>
        </div>
      );
    case "missingCredential":
      return (
        <div className="provider-usage-panel warning">
          <AlertCircle size={16} />
          <span>{t("provider.usageMissingCredential")}</span>
        </div>
      );
    case "failed":
      return (
        <div className="provider-usage-panel danger">
          <AlertCircle size={16} />
          <span>{usage.message || t("provider.usageFailed")}</span>
        </div>
      );
    case "ready":
      if (usage.usageKind === "deepseekBalance" && usage.balance) {
        return <DeepSeekUsagePanel usage={usage} loading={loading} t={t} />;
      }
      if (usage.usageKind === "zhipuCodingPlan" && usage.codingPlan) {
        return <ZhipuUsagePanel usage={usage} loading={loading} t={t} />;
      }
      return (
        <div className="provider-usage-panel muted">
          <Activity size={16} />
          <span>{t("provider.usageUnavailable")}</span>
        </div>
      );
  }
}

function DeepSeekUsagePanel({
  usage,
  loading,
  t,
}: {
  usage: ProviderUsageRecord;
  loading: boolean;
  t: TFunction;
}) {
  const balance = usage.balance;
  const primary = balance ? primaryBalance(balance.balances) : null;
  return (
    <div className={`provider-usage-panel deepseek-detail ${loading ? "refreshing" : ""}`}>
      <div className="provider-balance-main">
        <span className={balance?.isAvailable ? "usage-eyebrow ready" : "usage-eyebrow warning"}>
          <Wallet size={14} />
          {balance?.isAvailable ? t("provider.balanceAvailable") : t("provider.balanceUnavailable")}
        </span>
        <strong>{primary ? formatMoney(primary.totalBalance, primary.currency) : t("provider.usageUnavailable")}</strong>
      </div>
      <div className="provider-balance-grid">
        {(balance?.balances ?? []).map((item) => (
          <span key={item.currency}>
            <small>{item.currency}</small>
            <strong>{formatMoney(item.totalBalance, item.currency)}</strong>
            <em>
              {t("provider.balanceGranted")}: {item.grantedBalance || "0"} ·{" "}
              {t("provider.balanceToppedUp")}: {item.toppedUpBalance || "0"}
            </em>
          </span>
        ))}
      </div>
    </div>
  );
}

function ZhipuUsagePanel({
  usage,
  loading,
  t,
}: {
  usage: ProviderUsageRecord;
  loading: boolean;
  t: TFunction;
}) {
  const codingPlan = usage.codingPlan;
  const fiveHour = findLimit(codingPlan?.limits ?? [], "fiveHour");
  const weekly = findLimit(codingPlan?.limits ?? [], "weekly");
  const mcp = findLimit(codingPlan?.limits ?? [], "mcpMonthly");
  const quotaRows = [
    [t("provider.quotaFiveHour"), fiveHour] as const,
    [t("provider.quotaWeekly"), weekly] as const,
    [t("provider.quotaMcp"), mcp] as const,
  ].filter((item): item is readonly [string, ZhipuQuotaLimit] => Boolean(item[1]));

  return (
    <div className={`provider-usage-panel zhipu-detail ${loading ? "refreshing" : ""}`}>
      <div className="provider-quota-grid">
        {quotaRows.length > 0 ? (
          quotaRows.map(([label, limit]) => <QuotaRow key={limit.window} label={label} limit={limit} t={t} />)
        ) : (
          <div className="provider-usage-panel muted compact">
            <Activity size={16} />
            <span>{t("provider.usageUnavailable")}</span>
          </div>
        )}
      </div>
      {mcp?.usageDetails.length ? (
        <div className="provider-tool-usage">
          {mcp.usageDetails.map((detail) => (
            <span key={detail.name}>
              <small>{detail.name}</small>
              <strong>{formatToolUsage(detail)}</strong>
            </span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function QuotaRow({
  label,
  limit,
  t,
}: {
  label: string;
  limit: ZhipuQuotaLimit;
  t: TFunction;
}) {
  const remainingPercent = quotaRemainingPercent(limit);
  const resetText = formatResetAt(limit.nextResetAt);
  return (
    <div className="provider-quota-row">
      <div className="provider-quota-head">
        <strong>{label}</strong>
        <span>{t("provider.remainingPercent", { percent: formatPercent(remainingPercent) })}</span>
      </div>
      <div className="provider-quota-track">
        <span style={{ width: `${remainingPercent}%` }} />
      </div>
      <div className="provider-quota-foot">
        <span>{quotaAmountText(limit)}</span>
        {resetText ? <span>{t("provider.resetAt", { time: resetText })}</span> : null}
      </div>
    </div>
  );
}

function providerUsageSummary(
  provider: ProviderRecord,
  usage: ProviderUsageRecord | undefined,
  loading: boolean,
  t: TFunction,
) {
  if (loading && !usage) {
    return t("provider.usageChecking");
  }
  if (!usage) {
    return t("provider.usageNotLoaded");
  }
  switch (usage.status) {
    case "unsupported":
      return t("provider.usageUnsupported");
    case "missingCredential":
      return t("provider.usageMissingCredentialShort");
    case "failed":
      return t("provider.usageFailed");
    case "ready":
      if (usage.usageKind === "deepseekBalance" && usage.balance) {
        const primary = primaryBalance(usage.balance.balances);
        return primary ? formatMoney(primary.totalBalance, primary.currency) : t("provider.usageUnavailable");
      }
      if (provider.templateKind === "zhipu-coding-plan" && usage.codingPlan) {
        const fiveHour = findLimit(usage.codingPlan.limits, "fiveHour");
        const weekly = findLimit(usage.codingPlan.limits, "weekly");
        if (fiveHour && weekly) {
          return `${t("provider.quotaFiveHour")} ${formatPercent(quotaRemainingPercent(fiveHour))} · ${t(
            "provider.quotaWeekly",
          )} ${formatPercent(quotaRemainingPercent(weekly))}`;
        }
      }
      return t("provider.usageUnavailable");
  }
}

function providerUsageStatusClass(usage: ProviderUsageRecord | undefined, loading: boolean) {
  if (loading && !usage) {
    return "checking";
  }
  if (!usage) {
    return "muted";
  }
  switch (usage.status) {
    case "ready":
      return "ready";
    case "failed":
      return "danger";
    case "missingCredential":
      return "warning";
    case "unsupported":
      return "muted";
  }
}

function findLimit(limits: ZhipuQuotaLimit[], window: ZhipuQuotaLimit["window"]) {
  return limits.find((limit) => limit.window === window);
}

function primaryBalance(balances: { currency: string; totalBalance: string }[]) {
  return balances.find((item) => item.currency.toUpperCase() === "CNY") ?? balances[0] ?? null;
}

function quotaRemainingPercent(limit: ZhipuQuotaLimit) {
  if (typeof limit.remaining === "number" && typeof limit.total === "number" && limit.total > 0) {
    return clampPercent((limit.remaining / limit.total) * 100);
  }
  return clampPercent(100 - limit.percentage);
}

function quotaAmountText(limit: ZhipuQuotaLimit) {
  if (typeof limit.remaining === "number" && typeof limit.total === "number") {
    return `${formatNumber(limit.remaining)} / ${formatNumber(limit.total)}`;
  }
  if (typeof limit.remaining === "number") {
    return formatNumber(limit.remaining);
  }
  if (typeof limit.currentValue === "number" && typeof limit.total === "number") {
    return `${formatNumber(Math.max(0, limit.total - limit.currentValue))} / ${formatNumber(limit.total)}`;
  }
  return `${formatPercent(quotaRemainingPercent(limit))}`;
}

function formatToolUsage(detail: {
  currentValue?: number | null;
  total?: number | null;
  percentage?: number | null;
}) {
  if (typeof detail.currentValue === "number" && typeof detail.total === "number") {
    const remaining = Math.max(0, detail.total - detail.currentValue);
    return `${formatNumber(remaining)} / ${formatNumber(detail.total)}`;
  }
  if (typeof detail.percentage === "number") {
    return formatPercent(clampPercent(100 - detail.percentage));
  }
  return "--";
}

function formatMoney(amount: string, currency: string) {
  return `${currency} ${amount}`;
}

function formatNumber(value: number) {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value);
}

function formatPercent(value: number) {
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)}%`;
}

function formatResetAt(value: number | null | undefined) {
  if (!value) {
    return null;
  }
  return new Date(value * 1000).toLocaleString(undefined, {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function clampPercent(value: number) {
  if (!Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(100, value));
}
