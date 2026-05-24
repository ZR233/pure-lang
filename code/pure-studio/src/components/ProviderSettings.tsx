import { Plus, Save, Search, Trash2 } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import type { ModelRecord, ProviderKind, ProviderRecord, ProviderTemplateRecord } from "../types";

type ProviderSettingsProps = {
  providers: ProviderRecord[];
  templates: ProviderTemplateRecord[];
  selectedProviderId: string | null;
  providerSearch: string;
  configToml: string;
  setProviders: Dispatch<SetStateAction<ProviderRecord[]>>;
  setSelectedProviderId: Dispatch<SetStateAction<string | null>>;
  setProviderSearch: Dispatch<SetStateAction<string>>;
  setConfigToml: Dispatch<SetStateAction<string>>;
  onSaveToml: () => void;
};

function initials(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

function allModels(provider: ProviderRecord) {
  return [...provider.defaultModels, ...provider.customModels];
}

function normalizeProvider(provider: ProviderRecord): ProviderRecord {
  const models = allModels(provider);
  const defaultModel = models.some((model) => model.slug === provider.defaultModel)
    ? provider.defaultModel
    : (models[0]?.slug ?? "");
  return {
    ...provider,
    subtitle: `${provider.name || provider.id} Platform`,
    status: provider.bearerToken.trim() || provider.envKey.trim() ? "Healthy" : "Needs setup",
    defaultModel,
    models,
    modelCount: models.length.toString(),
  };
}

function templateProvider(template: ProviderTemplateRecord, id: string): ProviderRecord {
  const provider: ProviderRecord = {
    id,
    templateKind: template.id,
    name: template.name,
    subtitle: `${template.name} Platform`,
    status: "Needs setup",
    baseUrl: template.baseUrl,
    envKey: template.envKey,
    bearerToken: "",
    defaultModel: template.defaultModel,
    modelCount: template.defaultModels.length.toString(),
    updatedAt: "Draft",
    wireApi: template.wireApi,
    models: template.defaultModels,
    defaultModels: template.defaultModels,
    customModels: [],
  };
  return normalizeProvider(provider);
}

function suggestProviderId(providers: ProviderRecord[], kind: ProviderKind) {
  if (!providers.some((provider) => provider.id === kind)) {
    return kind;
  }
  for (let index = 2; ; index += 1) {
    const candidate = `${kind}-${index}`;
    if (!providers.some((provider) => provider.id === candidate)) {
      return candidate;
    }
  }
}

function modelEffortsText(model: ModelRecord) {
  return model.reasoningEfforts.join(", ");
}

function effortsFromText(value: string) {
  return value
    .split(",")
    .map((effort) => effort.trim())
    .filter(Boolean);
}

function cloneModel(model: ModelRecord): ModelRecord {
  return {
    slug: model.slug,
    displayName: model.displayName,
    reasoningEfforts: [...model.reasoningEfforts],
  };
}

export function ProviderSettings({
  providers,
  templates,
  selectedProviderId,
  providerSearch,
  configToml,
  setProviders,
  setSelectedProviderId,
  setProviderSearch,
  setConfigToml,
  onSaveToml,
}: ProviderSettingsProps) {
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

  function updateSelectedProvider(updater: (provider: ProviderRecord) => ProviderRecord) {
    if (!selectedProvider) {
      return;
    }
    const nextProvider = normalizeProvider(updater(selectedProvider));
    setProviders((current) =>
      current.map((provider) => (provider.id === selectedProvider.id ? nextProvider : provider)),
    );
    setSelectedProviderId(nextProvider.id);
  }

  function addProvider(kind: ProviderKind) {
    const template = templates.find((item) => item.id === kind);
    if (!template) {
      return;
    }
    const nextProvider = templateProvider(template, suggestProviderId(providers, kind));
    setProviders((current) => [...current, nextProvider]);
    setSelectedProviderId(nextProvider.id);
  }

  function changeTemplate(kind: ProviderKind) {
    const template = templates.find((item) => item.id === kind);
    if (!template) {
      return;
    }
    updateSelectedProvider((provider) => ({
      ...provider,
      templateKind: kind,
      name: provider.name || template.name,
      baseUrl: template.baseUrl,
      envKey: template.envKey,
      wireApi: template.wireApi,
      defaultModel: template.defaultModel,
      defaultModels: template.defaultModels.map(cloneModel),
    }));
  }

  function addCustomModel() {
    updateSelectedProvider((provider) => {
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
            displayName: "Custom Model",
            reasoningEfforts: ["high"],
          },
        ],
        defaultModel: provider.defaultModel || slug,
      };
    });
  }

  function updateCustomModel(index: number, patch: Partial<ModelRecord>) {
    updateSelectedProvider((provider) => ({
      ...provider,
      customModels: provider.customModels.map((model, modelIndex) =>
        modelIndex === index ? { ...model, ...patch } : model,
      ),
    }));
  }

  function removeCustomModel(index: number) {
    updateSelectedProvider((provider) => ({
      ...provider,
      customModels: provider.customModels.filter((_, modelIndex) => modelIndex !== index),
    }));
  }

  return (
    <div className="provider-settings">
      <section className="provider-list-card">
        <div className="provider-toolbar">
          <div className="search-box">
            <Search size={16} />
            <input
              value={providerSearch}
              onChange={(event) => setProviderSearch(event.target.value)}
              placeholder="Search providers..."
            />
          </div>
          <div className="provider-add-actions">
            <button onClick={() => addProvider("deepseek")}>
              <Plus size={16} />
              DeepSeek
            </button>
            <button onClick={() => addProvider("openai")}>
              <Plus size={16} />
              OpenAI
            </button>
          </div>
        </div>
        <div className="provider-table">
          <div className="provider-row header">
            <span className="provider-name-header">Provider</span>
            <span>Status</span>
            <span>Base URL</span>
            <span>Models</span>
            <span>Updated</span>
          </div>
          {filteredProviders.map((provider) => (
            <button
              key={provider.id}
              className={`provider-row ${provider.id === selectedProvider?.id ? "active" : ""}`}
              onClick={() => setSelectedProviderId(provider.id)}
            >
              <span className="provider-name-cell">
                <span className="provider-badge">{initials(provider.name) || "P"}</span>
                <span>
                  <strong>{provider.name || provider.id}</strong>
                  <small>{provider.id}</small>
                </span>
              </span>
              <span className="health">{provider.status}</span>
              <span>{provider.baseUrl || "(default)"}</span>
              <span>{provider.modelCount}</span>
              <span>{provider.updatedAt}</span>
            </button>
          ))}
        </div>
      </section>

      <section className="provider-detail-card editable">
        {selectedProvider ? (
          <>
            <div className="provider-detail-head">
              <span className="provider-badge large">{initials(selectedProvider.name) || "P"}</span>
              <div>
                <h2>{selectedProvider.name || selectedProvider.id}</h2>
                <p>{selectedProvider.id}</p>
              </div>
              <span className="health">{selectedProvider.status}</span>
            </div>

            <div className="provider-form-grid">
              <label>
                <span>Provider Key</span>
                <input
                  value={selectedProvider.id}
                  onChange={(event) =>
                    updateSelectedProvider((provider) => ({
                      ...provider,
                      id: event.target.value,
                    }))
                  }
                />
              </label>
              <label>
                <span>Template</span>
                <select
                  value={selectedProvider.templateKind}
                  onChange={(event) => changeTemplate(event.target.value as ProviderKind)}
                >
                  {templates.map((template) => (
                    <option key={template.id} value={template.id}>
                      {template.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                <span>Name</span>
                <input
                  value={selectedProvider.name}
                  onChange={(event) =>
                    updateSelectedProvider((provider) => ({
                      ...provider,
                      name: event.target.value,
                    }))
                  }
                />
              </label>
              <label>
                <span>Wire API</span>
                <select
                  value={selectedProvider.wireApi}
                  onChange={(event) =>
                    updateSelectedProvider((provider) => ({
                      ...provider,
                      wireApi: event.target.value,
                    }))
                  }
                >
                  <option value="chat">OpenAI Chat</option>
                  <option value="responses">Responses</option>
                </select>
              </label>
              <label className="wide">
                <span>Base URL</span>
                <input
                  value={selectedProvider.baseUrl}
                  onChange={(event) =>
                    updateSelectedProvider((provider) => ({
                      ...provider,
                      baseUrl: event.target.value,
                    }))
                  }
                />
              </label>
              <label>
                <span>Env Key</span>
                <input
                  value={selectedProvider.envKey}
                  onChange={(event) =>
                    updateSelectedProvider((provider) => ({
                      ...provider,
                      envKey: event.target.value,
                    }))
                  }
                />
              </label>
              <label>
                <span>API Key</span>
                <input
                  type="password"
                  value={selectedProvider.bearerToken}
                  onChange={(event) =>
                    updateSelectedProvider((provider) => ({
                      ...provider,
                      bearerToken: event.target.value,
                    }))
                  }
                />
              </label>
              <label className="wide">
                <span>Default Model</span>
                <select
                  value={selectedProvider.defaultModel}
                  onChange={(event) =>
                    updateSelectedProvider((provider) => ({
                      ...provider,
                      defaultModel: event.target.value,
                    }))
                  }
                >
                  {allModels(selectedProvider).map((model) => (
                    <option key={model.slug} value={model.slug}>
                      {model.displayName} ({model.slug})
                    </option>
                  ))}
                </select>
              </label>
            </div>
          </>
        ) : (
          <p className="muted">No provider configured.</p>
        )}
      </section>

      <section className="model-editor-card">
        {selectedProvider ? (
          <>
            <div className="model-editor-heading">
              <div>
                <h2>Models</h2>
                <p>Default models are saved first. Custom models are appended after them.</p>
              </div>
              <button onClick={addCustomModel}>
                <Plus size={16} />
                Custom Model
              </button>
            </div>

            <div className="model-section-title">Default Models</div>
            <div className="model-list">
              {selectedProvider.defaultModels.map((model) => (
                <div className="model-row readonly" key={model.slug}>
                  <strong>{model.displayName}</strong>
                  <span>{model.slug}</span>
                  <small>{modelEffortsText(model)}</small>
                </div>
              ))}
            </div>

            <div className="model-section-title">Custom Models</div>
            <div className="custom-model-list">
              {selectedProvider.customModels.length === 0 ? (
                <p className="muted">No custom models.</p>
              ) : (
                selectedProvider.customModels.map((model, index) => (
                  <div className="custom-model-row" key={`${model.slug}-${index}`}>
                    <input
                      value={model.slug}
                      onChange={(event) => updateCustomModel(index, { slug: event.target.value })}
                      placeholder="model-slug"
                    />
                    <input
                      value={model.displayName}
                      onChange={(event) =>
                        updateCustomModel(index, { displayName: event.target.value })
                      }
                      placeholder="Display name"
                    />
                    <input
                      value={modelEffortsText(model)}
                      onChange={(event) =>
                        updateCustomModel(index, {
                          reasoningEfforts: effortsFromText(event.target.value),
                        })
                      }
                      placeholder="high, xhigh"
                    />
                    <button className="icon-button" onClick={() => removeCustomModel(index)}>
                      <Trash2 size={16} />
                    </button>
                  </div>
                ))
              )}
            </div>
          </>
        ) : null}
      </section>

      <section className="config-editor-card">
        <div className="config-editor-heading">
          <div>
            <h2>Config TOML</h2>
            <p>Advanced editor. Structured provider save regenerates this document.</p>
          </div>
          <button onClick={onSaveToml}>
            <Save size={16} />
            Save TOML
          </button>
        </div>
        <textarea value={configToml} onChange={(event) => setConfigToml(event.target.value)} />
      </section>
    </div>
  );
}
