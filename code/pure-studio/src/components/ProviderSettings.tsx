import {
  CheckCircle2,
  Cpu,
  KeyRound,
  Link2,
  Pencil,
  Plus,
  Search,
  Server,
  Trash2,
} from "lucide-react";
import { useState, type Dispatch, type SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type {
  ModelRecord,
  ProviderKind,
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  ProviderTemplateRecord,
} from "../types";
import {
  applyProviderTemplate,
  cloneProvider,
  createProviderFromTemplate,
  normalizeProvider,
  replaceProvider,
  suggestProviderId,
} from "../lib/provider-settings";
import { allModels, initials, providerStatusClass, translateStatus, translateUpdatedAt } from "../lib/utils";
import { ProviderEditPage } from "./ProviderEditPage";

type ProviderSettingsProps = {
  providers: ProviderRecord[];
  templates: ProviderTemplateRecord[];
  selectedProviderId: string | null;
  providerSearch: string;
  setProviderSearch: Dispatch<SetStateAction<string>>;
  onSaveProviderSettings: (snapshot: ProviderSettingsSaveSnapshot) => Promise<boolean>;
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
  selectedProviderId,
  providerSearch,
  setProviderSearch,
  onSaveProviderSettings,
}: ProviderSettingsProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<ProviderDraft | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const filteredProviders = providers.filter((provider) => {
    const query = providerSearch.trim().toLowerCase();
    if (!query) {
      return true;
    }
    return (
      provider.name.toLowerCase().includes(query) ||
      provider.id.toLowerCase().includes(query) ||
      provider.baseUrl.toLowerCase().includes(query)
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
                <button disabled={isSaving || templates.length === 0} onClick={startAddProvider}>
                  <Plus size={16} />
                  {t("provider.addProvider")}
                </button>
              </div>
            </div>
          </div>

          <div className="provider-card-list">
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
                const templateName =
                  templates.find((template) => template.id === provider.templateKind)?.name ??
                  provider.templateKind;
                const defaultModel =
                  models.find((model) => model.slug === provider.defaultModel)?.displayName ||
                  provider.defaultModel ||
                  t("provider.notSelected");
                const previewModels = models.slice(0, 4);
                const remainingModels = Math.max(0, models.length - previewModels.length);

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
                          <span className="provider-url">
                            <Link2 size={14} />
                            {provider.baseUrl || t("provider.defaultBaseUrl")}
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
                        <Server size={14} />
                        <small>{t("provider.key")}</small>
                        <strong>{provider.id}</strong>
                      </span>
                      <span>
                        <Cpu size={14} />
                        <small>{t("provider.defaultModel")}</small>
                        <strong>{defaultModel}</strong>
                      </span>
                      <span>
                        <KeyRound size={14} />
                        <small>{t("provider.apiKey")}</small>
                        <strong>
                          {provider.bearerToken || provider.hasBearerToken
                            ? t("provider.saved")
                            : t("provider.notConfigured")}
                        </strong>
                      </span>
                      <span>
                        <small>{t("provider.template")}</small>
                        <strong>{templateName}</strong>
                      </span>
                      <span>
                        <small>{t("provider.protocolType")}</small>
                        <strong>{provider.wireApi}</strong>
                      </span>
                      <span>
                        <small>{t("provider.updated")}</small>
                        <strong>{translateUpdatedAt(provider.updatedAt, t)}</strong>
                      </span>
                    </div>

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
