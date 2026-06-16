import {
  Activity,
  AlertCircle,
  ArrowLeft,
  BookOpen,
  Bot,
  CheckCircle2,
  Cpu,
  Globe2,
  Link2,
  Pencil,
  Plus,
  Power,
  RefreshCw,
  Save,
  Search,
  Server,
  Terminal,
  Trash2,
  Unlock,
  UserCheck,
  Wallet,
  X,
} from "lucide-solid";
import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import type { JSX } from "solid-js";
import type {
  DiscoveredSkillsPayload,
  InstructionsInput,
  McpSettingsInput,
  McpServerInput,
  McpServerRecord,
  ModelRecord,
  PermissionMode,
  ProviderKind,
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  ProviderTemplateRecord,
  ProviderUsageRecord,
  RoleRecord,
  SkillRecord,
  DeepSeekBalanceInfo,
  ZhipuQuotaLimit,
} from "../../types";
import {
  applyProviderTemplate,
  cloneProvider,
  createProviderFromTemplate,
  normalizeProvider,
  replaceProvider,
  suggestProviderId,
} from "../../lib/provider-settings";
import { defaultTextCapabilities } from "../../lib/provider-mapper";
import { allModels, errorText, initials, providerStatusClass, translateStatus } from "../../lib/utils";
import { listDiscoveredSkills } from "../../lib/tauri";
import i18n from "../../i18n";
import {
  effortForModel,
  emptyMcpDraftServer,
  endpointSummary,
  instructionsDraft,
  instructionsInput,
  isLockedMcpServer,
  mcpDraftServer,
  modelForProvider,
  normalizeMcpServerInput,
  normalizeRolesForProviders,
  providerDefaultModel,
  searchableMcpServerText,
  settingsTabs,
  uniqueMcpServerId,
  type McpDraftServer,
  type SettingsTab,
} from "./settings-data";

export { normalizeMcpServerInput, normalizeRolesForProviders, instructionsDraft, instructionsInput };

type SettingsPanelProps = {
  activeTab: SettingsTab;
  providers: ProviderRecord[];
  templates: ProviderTemplateRecord[];
  providerUsages: ProviderUsageRecord[];
  providerUsagesLoading: boolean;
  providerUsageError: string | null;
  providerUsageErrors: Record<string, string | undefined>;
  providerUsageRefreshing: Record<string, boolean | undefined>;
  providerUsagesLoadedAt: number | null;
  providerSearch: string;
  selectedProviderId: string | null;
  roles: RoleRecord[];
  instructions: InstructionsInput;
  mcpServers: McpServerRecord[];
  selectedProjectId: string | null;
  configExists: boolean;
  configToml: string;
  permissionMode: PermissionMode;
  onClose: () => void;
  onSetTab: (tab: SettingsTab) => void;
  onSetProviderSearch: (value: string) => void;
  onSetSelectedProviderId: (providerId: string | null) => void;
  onRefreshProviderUsages: (providerId?: string) => void;
  onSaveProviderSettings: (snapshot?: ProviderSettingsSaveSnapshot) => Promise<boolean>;
  onSaveInstructionsSettings: (input: InstructionsInput) => Promise<boolean>;
  onSaveMcpSettings: (input: McpSettingsInput) => Promise<boolean>;
  onSavePermissionMode: (mode: PermissionMode) => Promise<void>;
};

export function SettingsPanel(props: SettingsPanelProps) {
  const tabLabel = (tab: SettingsTab) => i18n.t(`settings.tabs.${tab}`);
  return (
    <div class="settings-overlay">
      <div class="settings-page">
        <header class="settings-topbar">
          <button type="button" class="icon-button" onClick={props.onClose} aria-label={i18n.t("actions.close")}>
            <ArrowLeft size={17} />
          </button>
          <div>
            <h2>{i18n.t("settings.title")}</h2>
            <p>{props.configExists ? "~/.pure/config.toml" : i18n.t("settings.defaultConfigDraft")}</p>
          </div>
        </header>
        <nav class="settings-tabs" aria-label={i18n.t("settings.title")}>
          <For each={settingsTabs}>
            {(tab) => (
              <button
                type="button"
                data-active={props.activeTab === tab || undefined}
                onClick={() => props.onSetTab(tab)}
              >
                {tabLabel(tab)}
              </button>
            )}
          </For>
        </nav>
        <div class="settings-content">
          <Show when={props.activeTab === "providers"}>
            <ProviderSettings
              providers={props.providers}
              templates={props.templates}
              usages={props.providerUsages}
              usagesLoading={props.providerUsagesLoading}
              usageError={props.providerUsageError}
              usageErrors={props.providerUsageErrors}
              usageRefreshing={props.providerUsageRefreshing}
              usagesLoadedAt={props.providerUsagesLoadedAt}
              selectedProviderId={props.selectedProviderId}
              search={props.providerSearch}
              onSearch={props.onSetProviderSearch}
              onSelectProvider={props.onSetSelectedProviderId}
              onRefreshUsages={props.onRefreshProviderUsages}
              onSave={props.onSaveProviderSettings}
            />
          </Show>
          <Show when={props.activeTab === "instructions"}>
            <InstructionsSettings instructions={props.instructions} onSave={props.onSaveInstructionsSettings} />
          </Show>
          <Show when={props.activeTab === "skills"}>
            <SkillsSettings selectedProjectId={props.selectedProjectId} />
          </Show>
          <Show when={props.activeTab === "roles"}>
            <RoleSettings providers={props.providers} roles={props.roles} onSaveRoles={(roles) => props.onSaveProviderSettings({ roles })} />
          </Show>
          <Show when={props.activeTab === "mcp"}>
            <McpSettings servers={props.mcpServers} onSave={(servers) => props.onSaveMcpSettings({ servers })} />
          </Show>
          <Show when={props.activeTab === "security"}>
            <SecuritySettings permissionMode={props.permissionMode} onSavePermissionMode={props.onSavePermissionMode} />
          </Show>
          <Show when={props.activeTab === "general"}>
            <GeneralSettings configExists={props.configExists} configToml={props.configToml} />
          </Show>
        </div>
      </div>
    </div>
  );
}

type ProviderDraft = {
  mode: "create" | "edit";
  originalId: string | null;
  provider: ProviderRecord;
  autoId: string;
  autoName: string;
};

function ProviderSettings(props: {
  providers: ProviderRecord[];
  templates: ProviderTemplateRecord[];
  usages: ProviderUsageRecord[];
  usagesLoading: boolean;
  usageError: string | null;
  usageErrors: Record<string, string | undefined>;
  usageRefreshing: Record<string, boolean | undefined>;
  usagesLoadedAt: number | null;
  selectedProviderId: string | null;
  search: string;
  onSearch: (value: string) => void;
  onSelectProvider: (providerId: string | null) => void;
  onRefreshUsages: (providerId?: string) => void;
  onSave: (snapshot?: ProviderSettingsSaveSnapshot) => Promise<boolean>;
}) {
  const [draft, setDraft] = createSignal<ProviderDraft | null>(null);
  const [saving, setSaving] = createSignal(false);
  const usageById = createMemo(() => new Map(props.usages.map((usage) => [usage.providerId, usage])));
  const filtered = createMemo(() => {
    const query = props.search.trim().toLowerCase();
    if (!query) return props.providers;
    return props.providers.filter((provider) =>
      [
        provider.name,
        provider.id,
        ...allModels(provider).flatMap((model) => [model.slug, model.displayName]),
      ].join(" ").toLowerCase().includes(query),
    );
  });

  async function saveSnapshot(snapshot?: ProviderSettingsSaveSnapshot) {
    if (saving()) return false;
    setSaving(true);
    try {
      return await props.onSave(snapshot);
    } finally {
      setSaving(false);
    }
  }

  function startAdd() {
    const template = props.templates[0];
    if (!template) return;
    const id = suggestProviderId(props.providers, template.id);
    setDraft({
      mode: "create",
      originalId: null,
      provider: createProviderFromTemplate(template, id),
      autoId: id,
      autoName: template.name,
    });
  }

  function startEdit(provider: ProviderRecord) {
    setDraft({
      mode: "edit",
      originalId: provider.id,
      provider: cloneProvider(provider),
      autoId: provider.id,
      autoName: provider.name,
    });
  }

  function updateDraft(updater: (provider: ProviderRecord) => ProviderRecord) {
    setDraft((current) => current ? { ...current, provider: normalizeProvider(updater(current.provider)) } : current);
  }

  function changeTemplate(kind: ProviderKind) {
    const template = props.templates.find((item) => item.id === kind);
    if (!template) return;
    setDraft((current) => {
      if (!current) return current;
      const useTemplateId = current.mode === "create" && current.provider.id === current.autoId;
      const useTemplateName = current.mode === "create" && current.provider.name === current.autoName;
      const nextId = useTemplateId ? suggestProviderId(props.providers, template.id) : current.provider.id;
      const nextName = useTemplateName ? template.name : current.provider.name;
      return {
        ...current,
        provider: applyProviderTemplate(current.provider, template, { id: nextId, name: nextName }),
        autoId: current.mode === "create" ? nextId : current.autoId,
        autoName: current.mode === "create" ? template.name : current.autoName,
      };
    });
  }

  function addCustomModel() {
    updateDraft((provider) => {
      const existing = new Set(allModels(provider).map((model) => model.slug));
      let slug = "custom-model";
      for (let index = 2; existing.has(slug); index += 1) slug = `custom-model-${index}`;
      return {
        ...provider,
        customModels: [
          ...provider.customModels,
          {
            slug,
            displayName: i18n.t("model.defaultCustomModelName"),
            reasoningEfforts: ["high"],
            capabilities: defaultTextCapabilities(),
            baseInstructions: "",
          },
        ],
        defaultModel: provider.defaultModel || slug,
      };
    });
  }

  function updateCustomModel(index: number, patch: Partial<ModelRecord>) {
    updateDraft((provider) => ({
      ...provider,
      customModels: provider.customModels.map((model, modelIndex) => modelIndex === index ? { ...model, ...patch } : model),
    }));
  }

  function removeCustomModel(index: number) {
    updateDraft((provider) => ({
      ...provider,
      customModels: provider.customModels.filter((_, modelIndex) => modelIndex !== index),
    }));
  }

  async function saveDraft() {
    const current = draft();
    if (!current) return;
    const nextProvider = normalizeProvider(current.provider);
    const nextProviders = current.mode === "create"
      ? [...props.providers, nextProvider]
      : replaceProvider(props.providers, current.originalId ?? nextProvider.id, nextProvider);
    const selectedProviderId = current.mode === "create" || props.selectedProviderId === current.originalId
      ? nextProvider.id
      : props.selectedProviderId;
    if (await saveSnapshot({ providers: nextProviders, selectedProviderId })) setDraft(null);
  }

  async function removeProvider(providerId: string) {
    if (props.providers.length <= 1) return;
    const providers = props.providers.filter((provider) => provider.id !== providerId);
    const selectedProviderId = props.selectedProviderId === providerId ? providers[0]?.id ?? null : props.selectedProviderId;
    if (await saveSnapshot({ providers, selectedProviderId })) {
      if (draft()?.originalId === providerId) setDraft(null);
    }
  }

  return (
    <Show
      when={draft()}
      fallback={
        <section class="settings-section">
          <SettingsSectionHeader title={i18n.t("settings.providerRoute")} description={i18n.t("settings.providerRouteDesc")}>
            <div class="settings-toolbar">
              <SearchInput value={props.search} placeholder={i18n.t("provider.searchPlaceholder")} onInput={props.onSearch} />
              <button type="button" class="icon-button" onClick={() => props.onRefreshUsages()} disabled={props.usagesLoading} title={i18n.t("provider.refreshUsageTooltip")}>
                <RefreshCw size={15} class={props.usagesLoading ? "spin" : ""} />
              </button>
              <button type="button" class="settings-button primary" onClick={startAdd}>
                <Plus size={15} />
                {i18n.t("provider.addProvider")}
              </button>
            </div>
          </SettingsSectionHeader>
          <Show when={props.usageError}>
            {(message) => <div class="settings-error"><AlertCircle size={15} />{message()}</div>}
          </Show>
          <div class="settings-card-list">
            <For each={filtered()}>
              {(provider) => {
                const usage = () => usageById().get(provider.id);
                const usageLoading = () => props.usagesLoading || Boolean(props.usageRefreshing[provider.id]);
                const usageError = () => props.usageErrors[provider.id] ?? null;
                const defaultModel = () => allModels(provider).find((model) => model.slug === provider.defaultModel);
                return (
                  <article class="settings-card provider-card" data-active={provider.id === props.selectedProviderId || undefined}>
                    <button type="button" class="provider-card-main" onClick={() => {
                      props.onSelectProvider(provider.id);
                      void saveSnapshot({ selectedProviderId: provider.id });
                    }}>
                      <span class="provider-avatar">{initials(provider.name || provider.id)}</span>
                      <span class="provider-info">
                        <strong>{provider.name || provider.id}</strong>
                        <small>{provider.id}</small>
                      </span>
                      <span class={`provider-status ${providerStatusClass(provider)}`}>{translateStatus(provider.status, i18n.t)}</span>
                    </button>
                    <div class="settings-meta-row">
                      <span><Cpu size={13} />{defaultModel()?.displayName ?? provider.defaultModel}</span>
                      <span><Server size={13} />{i18n.t("provider.models", { count: provider.modelCount })}</span>
                      <span><Wallet size={13} />{providerUsageSummary(provider, usage(), usageLoading())}</span>
                    </div>
                    <ProviderUsagePanel
                      provider={provider}
                      usage={usage()}
                      loading={usageLoading()}
                      error={usageError()}
                      loadedAt={props.usagesLoadedAt}
                      onRefresh={() => props.onRefreshUsages(provider.id)}
                    />
                    <div class="settings-chip-row">
                      <For each={allModels(provider).slice(0, 4)}>
                        {(model) => <span class="settings-chip" data-active={model.slug === provider.defaultModel || undefined}>{model.displayName || model.slug}</span>}
                      </For>
                    </div>
                    <div class="settings-card-actions">
                      <button type="button" class="icon-button" onClick={() => startEdit(provider)} title={i18n.t("provider.editTooltip")}><Pencil size={15} /></button>
                      <button type="button" class="icon-button" disabled={props.providers.length <= 1} onClick={() => void removeProvider(provider.id)} title={i18n.t("provider.deleteTooltip")}><Trash2 size={15} /></button>
                    </div>
                  </article>
                );
              }}
            </For>
          </div>
        </section>
      }
    >
      {(current) => (
        <ProviderEdit
          draft={current()}
          templates={props.templates}
          saving={saving()}
          onCancel={() => setDraft(null)}
          onSave={() => void saveDraft()}
          onChangeTemplate={changeTemplate}
          onUpdateProvider={updateDraft}
          onAddCustomModel={addCustomModel}
          onUpdateCustomModel={updateCustomModel}
          onRemoveCustomModel={removeCustomModel}
        />
      )}
    </Show>
  );
}

function ProviderEdit(props: {
  draft: ProviderDraft;
  templates: ProviderTemplateRecord[];
  saving: boolean;
  onCancel: () => void;
  onSave: () => void;
  onChangeTemplate: (kind: ProviderKind) => void;
  onUpdateProvider: (updater: (provider: ProviderRecord) => ProviderRecord) => void;
  onAddCustomModel: () => void;
  onUpdateCustomModel: (index: number, patch: Partial<ModelRecord>) => void;
  onRemoveCustomModel: (index: number) => void;
}) {
  const provider = () => props.draft.provider;
  const models = () => allModels(provider());
  return (
    <section class="settings-section">
      <SettingsSectionHeader title={props.draft.mode === "create" ? i18n.t("provider.newProvider") : provider().name || provider().id} description={provider().baseUrl || i18n.t("provider.defaultBaseUrl")}>
        <div class="settings-toolbar">
          <button type="button" class="settings-button" onClick={props.onCancel} disabled={props.saving}><X size={15} />{i18n.t("actions.cancel")}</button>
          <button type="button" class="settings-button primary" onClick={props.onSave} disabled={props.saving}><Save size={15} />{i18n.t("actions.save")}</button>
        </div>
      </SettingsSectionHeader>
      <div class="settings-form-grid two">
        <Field label={i18n.t("provider.providerKey")}>
          <input value={provider().id} disabled={props.saving} onInput={(event) => props.onUpdateProvider((current) => ({ ...current, id: event.currentTarget.value }))} />
        </Field>
        <Field label={i18n.t("provider.providerType")}>
          <select value={provider().templateKind} disabled={props.saving} onChange={(event) => props.onChangeTemplate(event.currentTarget.value as ProviderKind)}>
            <For each={props.templates}>{(template) => <option value={template.id}>{template.name}</option>}</For>
          </select>
        </Field>
        <Field label={i18n.t("provider.displayName")}>
          <input value={provider().name} disabled={props.saving} onInput={(event) => props.onUpdateProvider((current) => ({ ...current, name: event.currentTarget.value }))} />
        </Field>
        <Field label={i18n.t("provider.protocolType")}>
          <div class="readonly-field">{provider().providerKind}</div>
        </Field>
        <Field label={i18n.t("provider.baseUrl")} span>
          <input value={provider().baseUrl} disabled={props.saving} onInput={(event) => props.onUpdateProvider((current) => ({ ...current, baseUrl: event.currentTarget.value }))} />
        </Field>
        <Field label={i18n.t("provider.apiKey")} span>
          <input type="password" value={provider().bearerToken} disabled={props.saving} onInput={(event) => props.onUpdateProvider((current) => ({ ...current, bearerToken: event.currentTarget.value }))} />
        </Field>
        <Field label={i18n.t("provider.defaultModel")} span>
          <select value={provider().defaultModel} disabled={props.saving} onChange={(event) => props.onUpdateProvider((current) => ({ ...current, defaultModel: event.currentTarget.value }))}>
            <For each={models()}>{(model) => <option value={model.slug}>{model.displayName} ({model.slug})</option>}</For>
          </select>
        </Field>
      </div>
      <ProviderModelEditor
        provider={provider()}
        disabled={props.saving}
        onAddCustomModel={props.onAddCustomModel}
        onUpdateCustomModel={props.onUpdateCustomModel}
        onRemoveCustomModel={props.onRemoveCustomModel}
      />
    </section>
  );
}

function ProviderModelEditor(props: {
  provider: ProviderRecord;
  disabled: boolean;
  onAddCustomModel: () => void;
  onUpdateCustomModel: (index: number, patch: Partial<ModelRecord>) => void;
  onRemoveCustomModel: (index: number) => void;
}) {
  return (
    <div class="settings-subsection">
      <SettingsSectionHeader title={i18n.t("model.title")} description={i18n.t("model.defaultModelDesc")}>
        <button type="button" class="settings-button" disabled={props.disabled} onClick={props.onAddCustomModel}>
          <Plus size={15} />
          {i18n.t("model.customModelButton")}
        </button>
      </SettingsSectionHeader>
      <h4>{i18n.t("model.defaultModels")}</h4>
      <div class="settings-card-list compact">
        <For each={props.provider.defaultModels}>
          {(model) => <ModelCard model={model} />}
        </For>
      </div>
      <h4>{i18n.t("model.customModels")}</h4>
      <Show when={props.provider.customModels.length > 0} fallback={<p class="settings-muted">{i18n.t("model.noCustomModels")}</p>}>
        <div class="settings-card-list compact">
          <For each={props.provider.customModels}>
            {(model, index) => (
              <article class="settings-card model-edit-card">
                <input value={model.slug} disabled={props.disabled} placeholder={i18n.t("model.slugPlaceholder")} onInput={(event) => props.onUpdateCustomModel(index(), { slug: event.currentTarget.value })} />
                <input value={model.displayName} disabled={props.disabled} placeholder={i18n.t("model.displayNamePlaceholder")} onInput={(event) => props.onUpdateCustomModel(index(), { displayName: event.currentTarget.value })} />
                <input value={model.reasoningEfforts.join(", ")} disabled={props.disabled} placeholder={i18n.t("model.effortsPlaceholder")} onInput={(event) => props.onUpdateCustomModel(index(), { reasoningEfforts: event.currentTarget.value.split(",").map((item) => item.trim()).filter(Boolean) })} />
                <button type="button" class="icon-button" disabled={props.disabled} onClick={() => props.onRemoveCustomModel(index())}><Trash2 size={15} /></button>
                <ModelParameterGrid model={model} />
              </article>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

function RoleSettings(props: {
  providers: ProviderRecord[];
  roles: RoleRecord[];
  onSaveRoles: (roles: RoleRecord[]) => Promise<boolean>;
}) {
  const roles = createMemo(() => normalizeRolesForProviders(props.roles, props.providers));
  async function replaceRole(nextRole: RoleRecord) {
    await props.onSaveRoles(roles().map((role) => role.key === nextRole.key ? nextRole : role));
  }
  return (
    <section class="settings-section">
      <SettingsSectionHeader title={i18n.t("settings.roleRoute")} description={i18n.t("settings.roleRouteDesc")} />
      <div class="settings-role-grid">
        <For each={roles()}>
          {(role) => {
            const provider = () => props.providers.find((item) => item.id === role.provider) ?? props.providers[0];
            const models = () => provider() ? allModels(provider()!) : [];
            const selectedModel = () => models().find((model) => model.slug === role.model);
            return (
              <article class="settings-card role-card">
                <header>
                  <strong>{i18n.t(`roles.${role.key}`)}</strong>
                  <small>{i18n.t(`roles.${role.key}Hint`)}</small>
                </header>
                <Field label={i18n.t("roleRoute.provider")}>
                  <select value={role.provider} onChange={(event) => {
                    const nextProvider = props.providers.find((item) => item.id === event.currentTarget.value);
                    if (!nextProvider) return;
                    const model = providerDefaultModel(nextProvider);
                    void replaceRole({ ...role, provider: nextProvider.id, model, effort: effortForModel(nextProvider, model) });
                  }}>
                    <For each={props.providers}>{(item) => <option value={item.id}>{item.name || item.id}</option>}</For>
                  </select>
                </Field>
                <Field label={i18n.t("roleRoute.model")}>
                  <select value={role.model} onChange={(event) => {
                    const currentProvider = provider();
                    if (!currentProvider) return;
                    void replaceRole({ ...role, model: event.currentTarget.value, effort: effortForModel(currentProvider, event.currentTarget.value) });
                  }}>
                    <For each={models()}>{(model) => <option value={model.slug}>{model.displayName || model.slug}</option>}</For>
                  </select>
                </Field>
                <Field label={i18n.t("roleRoute.effort")}>
                  <select value={role.effort} onChange={(event) => void replaceRole({ ...role, effort: event.currentTarget.value })}>
                    <For each={selectedModel()?.reasoningEfforts ?? []}>{(effort) => <option value={effort}>{effort}</option>}</For>
                  </select>
                </Field>
              </article>
            );
          }}
        </For>
      </div>
    </section>
  );
}

function InstructionsSettings(props: {
  instructions: InstructionsInput;
  onSave: (input: InstructionsInput) => Promise<boolean>;
}) {
  const [draft, setDraft] = createSignal(instructionsDraft(props.instructions));
  const [saving, setSaving] = createSignal(false);
  createEffect(() => setDraft(instructionsDraft(props.instructions)));
  async function save() {
    setSaving(true);
    try {
      await props.onSave(instructionsInput(draft()));
    } finally {
      setSaving(false);
    }
  }
  function update(patch: Partial<ReturnType<typeof instructionsDraft>>) {
    setDraft((current) => ({ ...current, ...patch }));
  }
  return (
    <section class="settings-section">
      <SettingsSectionHeader title={i18n.t("settings.instructions.title")} description={i18n.t("settings.instructions.description")}>
        <div class="settings-toolbar">
          <button type="button" class="settings-button" disabled={saving()} onClick={() => setDraft(instructionsDraft(props.instructions))}><RefreshCw size={15} />{i18n.t("actions.reset")}</button>
          <button type="button" class="settings-button primary" disabled={saving()} onClick={() => void save()}><Save size={15} />{i18n.t("actions.save")}</button>
        </div>
      </SettingsSectionHeader>
      <div class="settings-form-grid">
        <Field label={i18n.t("settings.instructions.baseOverride")} description={i18n.t("settings.instructions.baseOverrideDesc")}><textarea value={draft().baseOverride} onInput={(event) => update({ baseOverride: event.currentTarget.value })} /></Field>
        <Field label={i18n.t("settings.instructions.developer")} description={i18n.t("settings.instructions.developerDesc")}><textarea value={draft().developer} onInput={(event) => update({ developer: event.currentTarget.value })} /></Field>
        <Field label={i18n.t("settings.instructions.user")} description={i18n.t("settings.instructions.userDesc")}><textarea value={draft().user} onInput={(event) => update({ user: event.currentTarget.value })} /></Field>
        <div class="settings-form-grid two">
          <Field label={i18n.t("settings.instructions.projectDocMaxBytes")}><input type="number" min="0" step="1024" value={draft().projectDocMaxBytes} onInput={(event) => update({ projectDocMaxBytes: event.currentTarget.value })} /></Field>
          <Field label={i18n.t("settings.instructions.fallbackFilenames")} description={i18n.t("settings.instructions.fallbackFilenamesDesc")}><input value={draft().fallbackFilenames} placeholder="PURE.md, PROJECT.md" onInput={(event) => update({ fallbackFilenames: event.currentTarget.value })} /></Field>
        </div>
      </div>
    </section>
  );
}

function McpSettings(props: {
  servers: McpServerRecord[];
  onSave: (servers: McpServerInput[]) => Promise<boolean>;
}) {
  const [drafts, setDrafts] = createSignal<McpDraftServer[]>(props.servers.map(mcpDraftServer));
  const [editingId, setEditingId] = createSignal(props.servers[0]?.id ?? null);
  const [search, setSearch] = createSignal("");
  const [saving, setSaving] = createSignal(false);
  createEffect(() => {
    setDrafts(props.servers.map(mcpDraftServer));
    setEditingId((current) => current && props.servers.some((server) => server.id === current) ? current : props.servers[0]?.id ?? null);
  });
  const filtered = createMemo(() => {
    const query = search().trim().toLowerCase();
    if (!query) return drafts();
    return drafts().filter((server) => searchableMcpServerText(server).includes(query));
  });
  const editing = createMemo(() => drafts().find((server) => server.id === editingId()) ?? null);
  async function save(nextDrafts = drafts()) {
    setSaving(true);
    try {
      if (await props.onSave(nextDrafts.map(normalizeMcpServerInput))) setDrafts(nextDrafts);
    } finally {
      setSaving(false);
    }
  }
  function updateEditing(updater: (server: McpDraftServer) => McpDraftServer) {
    const id = editingId();
    if (!id) return;
    setDrafts((current) => current.map((server) => server.id === id ? updater(server) : server));
  }
  function addServer() {
    const server = emptyMcpDraftServer(uniqueMcpServerId(drafts()));
    setDrafts((current) => [...current, server]);
    setEditingId(server.id);
  }
  return (
    <section class="settings-section">
      <SettingsSectionHeader title={i18n.t("settings.mcp.title")} description={i18n.t("settings.mcp.description")}>
        <div class="settings-toolbar">
          <SearchInput value={search()} placeholder={i18n.t("settings.mcp.searchPlaceholder")} onInput={setSearch} />
          <button type="button" class="settings-button primary" onClick={addServer}><Plus size={15} />{i18n.t("settings.mcp.addServer")}</button>
        </div>
      </SettingsSectionHeader>
      <div class="settings-split">
        <div class="settings-card-list">
          <For each={filtered()}>
            {(server) => (
              <article class="settings-card mcp-card" data-active={server.id === editingId() || undefined}>
                <button type="button" class="mcp-card-main" onClick={() => setEditingId(server.id)}>
                  <Show when={server.transport === "stdio"} fallback={<Globe2 size={17} />}><Terminal size={17} /></Show>
                  <span><strong>{server.id}</strong><small>{endpointSummary(server) || i18n.t("settings.mcp.noEndpoint")}</small></span>
                </button>
                <div class="settings-chip-row">
                  <span class="settings-chip">{i18n.t(`settings.mcp.status.${server.statusKind ?? (server.enabled ? "enabled" : "disabled")}`)}</span>
                  <span class="settings-chip">{i18n.t(`settings.mcp.availability.${server.availabilityKind ?? (server.enabled ? "checking" : "disabled")}`)}</span>
                  <Show when={server.sourceKind === "builtIn"}><span class="settings-chip">{i18n.t("settings.mcp.builtInSource")}</span></Show>
                </div>
                <div class="settings-card-actions">
                  <button type="button" class="icon-button" disabled={saving()} onClick={() => void save(drafts().map((item) => item.id === server.id ? { ...item, enabled: !item.enabled } : item))}><Power size={15} /></button>
                  <button type="button" class="icon-button" disabled={saving() || isLockedMcpServer(server)} onClick={() => void save(drafts().filter((item) => item.id !== server.id))}><Trash2 size={15} /></button>
                </div>
              </article>
            )}
          </For>
        </div>
        <Show when={editing()}>
          {(server) => (
            <form class="settings-editor" onSubmit={(event) => { event.preventDefault(); void save(); }}>
              <SettingsSectionHeader title={server().id || i18n.t("settings.mcp.newServer")} description={server().availabilityMessage ?? i18n.t("settings.mcp.editorSubtitle")}>
                <button type="submit" class="settings-button primary" disabled={saving() || isLockedMcpServer(server())}><Save size={15} />{i18n.t("actions.save")}</button>
              </SettingsSectionHeader>
              <Field label={i18n.t("settings.mcp.serverId")}><input value={server().id} disabled={isLockedMcpServer(server())} onInput={(event) => {
                const previous = server().id;
                const next = event.currentTarget.value;
                setDrafts((current) => current.map((item) => item.id === previous ? { ...item, id: next } : item));
                setEditingId(next);
              }} /></Field>
              <div class="segmented">
                <For each={["stdio", "streamableHttp"] as const}>
                  {(transport) => <button type="button" data-active={server().transport === transport || undefined} disabled={isLockedMcpServer(server())} onClick={() => updateEditing((current) => ({ ...current, transport }))}>{i18n.t(`settings.mcp.transportOptions.${transport}`)}</button>}
                </For>
              </div>
              <Show when={server().transport === "stdio"} fallback={<HttpFields server={server()} locked={isLockedMcpServer(server())} update={updateEditing} />}>
                <StdioFields server={server()} locked={isLockedMcpServer(server())} update={updateEditing} />
              </Show>
            </form>
          )}
        </Show>
      </div>
    </section>
  );
}

function SecuritySettings(props: {
  permissionMode: PermissionMode;
  onSavePermissionMode: (mode: PermissionMode) => Promise<void>;
}) {
  const options: Array<{ mode: PermissionMode; icon: typeof UserCheck }> = [
    { mode: "request-approval", icon: UserCheck },
    { mode: "auto-review", icon: Bot },
    { mode: "full-access", icon: Unlock },
  ];
  return (
    <section class="settings-section">
      <SettingsSectionHeader title={i18n.t("settings.security.title")} description={i18n.t("settings.security.description")} />
      <div class="settings-role-grid">
        <For each={options}>
          {({ mode, icon: Icon }) => (
            <button type="button" class="settings-card security-card" data-active={props.permissionMode === mode || undefined} onClick={() => {
              if (props.permissionMode !== mode) void props.onSavePermissionMode(mode);
            }}>
              <Icon size={18} />
              <span><strong>{i18n.t(`permissionMode.${mode}`)}</strong><small>{i18n.t(`settings.security.modeDesc.${mode}`)}</small></span>
            </button>
          )}
        </For>
      </div>
    </section>
  );
}

function SkillsSettings(props: { selectedProjectId: string | null }) {
  const [payload, setPayload] = createSignal<DiscoveredSkillsPayload | null>(null);
  const [search, setSearch] = createSignal("");
  const [loading, setLoading] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);
  const [reloadKey, setReloadKey] = createSignal(0);
  createEffect(() => {
    const projectId = props.selectedProjectId;
    reloadKey();
    if (!projectId) {
      setPayload(null);
      setError(null);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listDiscoveredSkills(projectId)
      .then((next) => { if (!cancelled) setPayload(next); })
      .catch((loadError) => { if (!cancelled) setError(errorText(loadError)); })
      .finally(() => { if (!cancelled) setLoading(false); });
    onCleanup(() => { cancelled = true; });
  });
  const filtered = createMemo(() => {
    const query = search().trim().toLowerCase();
    const skills = payload()?.skills ?? [];
    if (!query) return skills;
    return skills.filter((skill) => searchableSkill(skill).includes(query));
  });
  return (
    <section class="settings-section">
      <SettingsSectionHeader title={i18n.t("skills.title")} description={i18n.t("skills.subtitle")}>
        <div class="settings-toolbar">
          <SearchInput value={search()} placeholder={i18n.t("skills.searchPlaceholder")} onInput={setSearch} />
          <button type="button" class="settings-button" disabled={!props.selectedProjectId || loading()} onClick={() => setReloadKey((value) => value + 1)}><RefreshCw size={15} />{i18n.t("actions.reload")}</button>
        </div>
      </SettingsSectionHeader>
      <div class="settings-meta-row">
        <span>{i18n.t("skills.count", { count: payload()?.skills.length ?? 0 })}</span>
        <Show when={payload()?.projectDir}>{(dir) => <code>{dir()}</code>}</Show>
      </div>
      <Show when={(payload()?.warnings.length ?? 0) > 0}>
        <div class="settings-warning"><AlertCircle size={15} />{payload()?.warnings.slice(0, 3).join(" · ")}</div>
      </Show>
      <Show when={!props.selectedProjectId || loading() || error() || filtered().length === 0} fallback={<SkillList skills={filtered()} />}>
        <div class="settings-empty">
          <BookOpen size={28} />
          <strong>{!props.selectedProjectId ? i18n.t("skills.noProject") : loading() ? i18n.t("skills.loading") : error() ? i18n.t("skills.loadFailed") : search().trim() ? i18n.t("skills.noMatches") : i18n.t("skills.empty")}</strong>
          <Show when={error()}>{(message) => <span>{message()}</span>}</Show>
        </div>
      </Show>
    </section>
  );
}

function GeneralSettings(props: { configExists: boolean; configToml: string }) {
  return (
    <section class="settings-section">
      <SettingsSectionHeader title={i18n.t("settings.tabs.general")} description={props.configExists ? "~/.pure/config.toml" : i18n.t("settings.defaultConfigDraft")} />
      <pre class="settings-toml-preview">{props.configToml || i18n.t("settings.comingSoonDesc")}</pre>
    </section>
  );
}

function SettingsSectionHeader(props: { title: string; description?: string; children?: JSX.Element }) {
  return (
    <div class="settings-section-header">
      <div>
        <h3>{props.title}</h3>
        <Show when={props.description}><p>{props.description}</p></Show>
      </div>
      {props.children}
    </div>
  );
}

function Field(props: { label: string; description?: string; span?: boolean; children: JSX.Element }) {
  return (
    <label class="settings-field" data-span={props.span || undefined}>
      <span>{props.label}</span>
      {props.children}
      <Show when={props.description}><small>{props.description}</small></Show>
    </label>
  );
}

function SearchInput(props: { value: string; placeholder: string; onInput: (value: string) => void }) {
  return (
    <label class="settings-search">
      <Search size={15} />
      <input value={props.value} placeholder={props.placeholder} onInput={(event) => props.onInput(event.currentTarget.value)} />
    </label>
  );
}

function StdioFields(props: { server: McpDraftServer; locked: boolean; update: (updater: (server: McpDraftServer) => McpDraftServer) => void }) {
  return (
    <>
      <Field label={i18n.t("settings.mcp.command")}><input value={props.server.command ?? ""} disabled={props.locked} onInput={(event) => props.update((server) => ({ ...server, command: event.currentTarget.value }))} /></Field>
      <StringListEditor label={i18n.t("settings.mcp.args")} values={props.server.args} locked={props.locked} onChange={(args) => props.update((server) => ({ ...server, args }))} />
      <Field label={i18n.t("settings.mcp.cwd")}><input value={props.server.cwd ?? ""} disabled={props.locked} onInput={(event) => props.update((server) => ({ ...server, cwd: event.currentTarget.value }))} /></Field>
      <KeyValueEditor label={i18n.t("settings.mcp.env")} values={props.server.env} locked={props.locked} onChange={(env) => props.update((server) => ({ ...server, env }))} />
    </>
  );
}

function HttpFields(props: { server: McpDraftServer; locked: boolean; update: (updater: (server: McpDraftServer) => McpDraftServer) => void }) {
  return (
    <>
      <Field label={i18n.t("settings.mcp.url")}><input value={props.server.url ?? ""} disabled={props.locked} onInput={(event) => props.update((server) => ({ ...server, url: event.currentTarget.value }))} /></Field>
      <Field label={i18n.t("settings.mcp.bearerTokenEnvVar")}><input value={props.server.bearerTokenEnvVar ?? ""} disabled={props.locked} onInput={(event) => props.update((server) => ({ ...server, bearerTokenEnvVar: event.currentTarget.value }))} /></Field>
      <KeyValueEditor label={i18n.t("settings.mcp.headers")} values={props.server.headers} locked={props.locked} onChange={(headers) => props.update((server) => ({ ...server, headers }))} />
    </>
  );
}

function StringListEditor(props: { label: string; values: string[]; locked: boolean; onChange: (values: string[]) => void }) {
  const rows = () => props.values.length ? props.values : [""];
  return (
    <Field label={props.label}>
      <div class="settings-list-editor">
        <For each={rows()}>
          {(value, index) => (
            <div class="settings-inline-row">
              <input value={value} disabled={props.locked} onInput={(event) => {
                const next = [...rows()];
                next[index()] = event.currentTarget.value;
                props.onChange(next);
              }} />
              <button type="button" class="icon-button" disabled={props.locked} onClick={() => props.onChange(rows().filter((_, rowIndex) => rowIndex !== index()))}><X size={14} /></button>
            </div>
          )}
        </For>
        <button type="button" class="settings-button" disabled={props.locked} onClick={() => props.onChange([...props.values, ""])}><Plus size={14} />{i18n.t("settings.mcp.addRow")}</button>
      </div>
    </Field>
  );
}

function KeyValueEditor(props: { label: string; values: Array<{ key: string; value: string }>; locked: boolean; onChange: (values: Array<{ key: string; value: string }>) => void }) {
  const rows = () => props.values.length ? props.values : [{ key: "", value: "" }];
  return (
    <Field label={props.label}>
      <div class="settings-list-editor">
        <For each={rows()}>
          {(entry, index) => (
            <div class="settings-inline-row two">
              <input value={entry.key} placeholder={i18n.t("settings.mcp.key")} disabled={props.locked} onInput={(event) => {
                const next = rows().map((row) => ({ ...row }));
                next[index()]!.key = event.currentTarget.value;
                props.onChange(next);
              }} />
              <input value={entry.value} placeholder={i18n.t("settings.mcp.value")} disabled={props.locked} onInput={(event) => {
                const next = rows().map((row) => ({ ...row }));
                next[index()]!.value = event.currentTarget.value;
                props.onChange(next);
              }} />
              <button type="button" class="icon-button" disabled={props.locked} onClick={() => props.onChange(rows().filter((_, rowIndex) => rowIndex !== index()))}><X size={14} /></button>
            </div>
          )}
        </For>
        <button type="button" class="settings-button" disabled={props.locked} onClick={() => props.onChange([...props.values, { key: "", value: "" }])}><Plus size={14} />{i18n.t("settings.mcp.addRow")}</button>
      </div>
    </Field>
  );
}

function ModelCard(props: { model: ModelRecord }) {
  return (
    <article class="settings-card model-card">
      <strong>{props.model.displayName}</strong>
      <small>{props.model.slug}</small>
      <ModelParameterGrid model={props.model} />
    </article>
  );
}

function ModelParameterGrid(props: { model: ModelRecord }) {
  return (
    <div class="model-parameter-grid">
      <span><small>{i18n.t("model.context")}</small><strong>{formatTokens(props.model.contextWindow ?? props.model.maxContextWindow)}</strong></span>
      <span><small>{i18n.t("model.maxOutput")}</small><strong>{formatTokens(props.model.maxOutputTokens)}</strong></span>
      <span><small>{i18n.t("model.efforts")}</small><strong>{props.model.reasoningEfforts.join(", ") || i18n.t("provider.notConfigured")}</strong></span>
      <span><small>{i18n.t("model.pricing")}</small><strong>{formatPrice(props.model)}</strong></span>
    </div>
  );
}

function SkillList(props: { skills: SkillRecord[] }) {
  return (
    <div class="settings-card-list compact">
      <For each={props.skills}>
        {(skill) => (
          <article class="settings-card skill-card">
            <div><strong>{skill.name}</strong><p>{skill.description}</p></div>
            <span class="settings-chip">{i18n.t(`skills.scope.${skill.scope}`)}</span>
            <code>{skill.path}</code>
          </article>
        )}
      </For>
    </div>
  );
}

function searchableSkill(skill: SkillRecord) {
  return [skill.name, skill.description, skill.category ?? "", skill.scope, skill.path, ...skill.platforms].join(" ").toLowerCase();
}

function ProviderUsagePanel(props: {
  provider: ProviderRecord;
  usage: ProviderUsageRecord | undefined;
  loading: boolean;
  error: string | null;
  loadedAt: number | null;
  onRefresh: () => void;
}) {
  const usage = () => props.usage;
  const supportsUsage = () => providerSupportsUsage(props.provider);
  return (
    <div class="provider-usage-panel">
      <div class="provider-usage-head">
        <span class="usage-card-title">{i18n.t("provider.usage")}</span>
        <span>{usageUpdatedLabel(usage()?.updatedAt ?? props.loadedAt)}</span>
        <button
          type="button"
          class="icon-button provider-usage-refresh"
          disabled={props.loading || !supportsUsage()}
          title={supportsUsage() ? i18n.t("provider.refreshProviderUsageTooltip") : i18n.t("provider.usageUnsupported")}
          onClick={(event) => {
            event.stopPropagation();
            props.onRefresh();
          }}
        >
          <RefreshCw size={13} class={props.loading ? "spin" : ""} />
        </button>
      </div>
      <Show when={props.error}>
        {(message) => (
          <div class="provider-usage-message" data-state="failed">
            <AlertCircle size={14} />
            <span>{message()}</span>
          </div>
        )}
      </Show>
      <Show when={usage()} fallback={<ProviderUsageMessage loading={props.loading} provider={props.provider} />}>
        {(record) => (
          <SwitchUsage
            provider={props.provider}
            usage={record()}
            loading={props.loading}
          />
        )}
      </Show>
    </div>
  );
}

function SwitchUsage(props: {
  provider: ProviderRecord;
  usage: ProviderUsageRecord;
  loading: boolean;
}) {
  switch (props.usage.status) {
    case "ready":
      if (props.usage.usageKind === "deepseekBalance" && props.usage.balance) {
        return <DeepSeekUsage usage={props.usage.balance.balances} available={props.usage.balance.isAvailable} />;
      }
      if (props.usage.usageKind === "zhipuCodingPlan" && props.usage.codingPlan) {
        return <ZhipuCodingPlanUsage limits={props.usage.codingPlan.limits} level={props.usage.codingPlan.level} />;
      }
      return <ProviderUsageMessage provider={props.provider} usage={props.usage} loading={props.loading} />;
    case "unsupported":
    case "missingCredential":
    case "failed":
      return <ProviderUsageMessage provider={props.provider} usage={props.usage} loading={props.loading} />;
  }
}

function ProviderUsageMessage(props: {
  provider?: ProviderRecord;
  supportsUsage?: boolean;
  usage?: ProviderUsageRecord;
  loading: boolean;
}) {
  const state = () => props.usage?.status ?? (props.loading ? "loading" : "idle");
  const message = () => {
    if (props.loading && !props.usage) return i18n.t("provider.usageChecking");
    if (!props.usage) return (props.provider ? providerSupportsUsage(props.provider) : props.supportsUsage) ? i18n.t("provider.usageNotLoaded") : i18n.t("provider.usageUnsupported");
    if (props.usage.status === "missingCredential") return props.usage.message || i18n.t("provider.usageMissingCredential");
    if (props.usage.status === "failed") return props.usage.message || i18n.t("provider.usageFailed");
    if (props.usage.status === "unsupported") return i18n.t("provider.usageUnsupported");
    return i18n.t("provider.usageUnavailable");
  };
  return (
    <div class="provider-usage-message" data-state={state()}>
      {state() === "failed" || state() === "missingCredential" ? <AlertCircle size={14} /> : <Wallet size={14} />}
      <span>{message()}</span>
    </div>
  );
}

function DeepSeekUsage(props: { usage: DeepSeekBalanceInfo[]; available: boolean }) {
  const primary = () => props.usage.find((item) => item.currency.toUpperCase() === "CNY") ?? props.usage[0];
  return (
    <Show when={primary()} fallback={<ProviderUsageMessage supportsUsage loading={false} />}>
      {(balance) => (
        <div class="provider-balance-card">
          <div>
            <span class="usage-card-title">{props.available ? i18n.t("provider.balanceAvailable") : i18n.t("provider.balanceUnavailable")}</span>
            <strong>{balance().currency} {balance().totalBalance}</strong>
          </div>
          <div class="provider-balance-breakdown">
            <span>{i18n.t("provider.balanceGranted")} <strong>{balance().grantedBalance}</strong></span>
            <span>{i18n.t("provider.balanceToppedUp")} <strong>{balance().toppedUpBalance}</strong></span>
          </div>
          <Show when={props.usage.length > 1}>
            <div class="provider-balance-list">
              <For each={props.usage.filter((item) => item.currency !== balance().currency)}>
                {(item) => (
                  <span>
                    {item.currency}
                    <strong>{item.totalBalance}</strong>
                  </span>
                )}
              </For>
            </div>
          </Show>
        </div>
      )}
    </Show>
  );
}

function ZhipuCodingPlanUsage(props: { limits: ZhipuQuotaLimit[]; level?: string | null }) {
  const ordered = () => [
    findLimit(props.limits, "fiveHour"),
    findLimit(props.limits, "weekly"),
    findLimit(props.limits, "mcpMonthly"),
    ...props.limits.filter((limit) => limit.window === "other"),
  ].filter(Boolean) as ZhipuQuotaLimit[];
  return (
    <div class="provider-quota-stack">
      <div class="provider-quota-grid">
        <For each={ordered()} fallback={<ProviderUsageMessage supportsUsage loading={false} />}>
          {(limit) => <QuotaCard limit={limit} />}
        </For>
      </div>
      <Show when={props.level}>
        {(level) => (
          <div class="provider-usage-footer">
            <span class="settings-chip usage-level">{i18n.t("provider.planLevel", { level: level() })}</span>
          </div>
        )}
      </Show>
    </div>
  );
}

function QuotaCard(props: { limit: ZhipuQuotaLimit }) {
  const percent = () => quotaRemainingPercent(props.limit);
  const detail = () => quotaDetail(props.limit);
  return (
    <div class="provider-quota-card" data-window={props.limit.window}>
      <div class="quota-card-head">
        <span class="usage-card-title">{quotaTitle(props.limit)}</span>
        <small>{resetLabel(props.limit.nextResetAt)}</small>
      </div>
      <div class="quota-card-value">
        <strong>{formatPercent(percent())}</strong>
        <span>{detail()}</span>
      </div>
      <div class="quota-progress"><span style={{ width: `${percent()}%` }} /></div>
      <Show when={props.limit.usageDetails.length > 0}>
        <div class="quota-tool-details">
          <div class="quota-tool-details-head">{i18n.t("provider.toolUsageDetails")}</div>
          <For each={props.limit.usageDetails}>
            {(item) => (
              <span title={item.name}>
                {item.name}
                <small>{formatToolUsage(item.currentValue, item.total, item.percentage)}</small>
              </span>
            )}
          </For>
        </div>
      </Show>
    </div>
  );
}

function providerUsageSummary(provider: ProviderRecord, usage: ProviderUsageRecord | undefined, loading: boolean) {
  if (loading && !usage) return i18n.t("provider.usageChecking");
  if (!usage) return providerSupportsUsage(provider) ? i18n.t("provider.usageNotLoaded") : i18n.t("provider.usageUnsupported");
  switch (usage.status) {
    case "unsupported":
      return i18n.t("provider.usageUnsupported");
    case "missingCredential":
      return i18n.t("provider.usageMissingCredentialShort");
    case "failed":
      return i18n.t("provider.usageFailed");
    case "ready":
      if (usage.usageKind === "deepseekBalance" && usage.balance) {
        const primary = usage.balance.balances.find((item) => item.currency.toUpperCase() === "CNY") ?? usage.balance.balances[0];
        return primary ? `${primary.currency} ${primary.totalBalance}` : i18n.t("provider.usageUnavailable");
      }
      if (provider.templateKind === "zhipu-coding-plan" && usage.codingPlan) {
        const fiveHour = findLimit(usage.codingPlan.limits, "fiveHour");
        const weekly = findLimit(usage.codingPlan.limits, "weekly");
        if (fiveHour && weekly) return `${i18n.t("provider.quotaFiveHour")} ${formatPercent(quotaRemainingPercent(fiveHour))} · ${i18n.t("provider.quotaWeekly")} ${formatPercent(quotaRemainingPercent(weekly))}`;
      }
      return i18n.t("provider.usageUnavailable");
  }
}

function providerSupportsUsage(provider: ProviderRecord) {
  return provider.templateKind === "deepseek" || provider.templateKind === "zhipu-coding-plan";
}

function findLimit(limits: ZhipuQuotaLimit[], window: ZhipuQuotaLimit["window"]) {
  return limits.find((limit) => limit.window === window);
}

function quotaRemainingPercent(limit: ZhipuQuotaLimit) {
  if (typeof limit.remaining === "number" && typeof limit.total === "number" && limit.total > 0) {
    return clampPercent((limit.remaining / limit.total) * 100);
  }
  return clampPercent(100 - limit.percentage);
}

function quotaTitle(limit: ZhipuQuotaLimit) {
  switch (limit.window) {
    case "fiveHour":
      return i18n.t("provider.quotaFiveHour");
    case "weekly":
      return i18n.t("provider.quotaWeekly");
    case "mcpMonthly":
      return i18n.t("provider.quotaMcp");
    case "other":
      return limit.label || i18n.t("provider.usage");
  }
}

function quotaDetail(limit: ZhipuQuotaLimit) {
  if (typeof limit.remaining === "number" && typeof limit.total === "number") {
    return i18n.t("provider.remainingOfTotal", {
      remaining: formatCompactNumber(limit.remaining),
      total: formatCompactNumber(limit.total),
    });
  }
  if (typeof limit.currentValue === "number" && typeof limit.total === "number") {
    return i18n.t("provider.usedOfTotal", {
      current: formatCompactNumber(limit.currentValue),
      total: formatCompactNumber(limit.total),
    });
  }
  return i18n.t("provider.remainingPercent", { percent: formatPercent(quotaRemainingPercent(limit)) });
}

function usageUpdatedLabel(value: number | null | undefined) {
  if (!value) return i18n.t("provider.usageNotLoaded");
  return i18n.t("provider.usageUpdatedAt", { time: formatShortDateTime(value) });
}

function resetLabel(value: number | null | undefined) {
  if (!value) return "";
  return i18n.t("provider.resetAt", { time: formatShortDateTime(value) });
}

function formatToolUsage(current: number | null | undefined, total: number | null | undefined, percentage: number | null | undefined) {
  if (typeof current === "number" && typeof total === "number") {
    return i18n.t("provider.remainingOfTotal", {
      remaining: formatCompactNumber(Math.max(0, total - current)),
      total: formatCompactNumber(total),
    });
  }
  if (typeof current === "number") return i18n.t("provider.usedValue", { value: formatCompactNumber(current) });
  if (typeof percentage === "number") return i18n.t("provider.remainingPercent", { percent: formatPercent(clampPercent(100 - percentage)) });
  return i18n.t("provider.usageUnavailable");
}

function formatTokens(value: number | undefined | null) {
  if (value === undefined || value === null) return i18n.t("provider.notConfigured");
  if (value >= 1_000_000) return `${trimNumber(value / 1_000_000)}M`;
  if (value >= 1_000) return `${trimNumber(value / 1_000)}K`;
  return value.toString();
}

function formatCompactNumber(value: number) {
  return new Intl.NumberFormat(undefined, {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

function formatShortDateTime(value: number) {
  return new Intl.DateTimeFormat(undefined, {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value * 1000));
}

function formatPrice(model: ModelRecord) {
  if (!model.currency || model.inputPricePerMTok == null || model.outputPricePerMTok == null || model.cacheReadPricePerMTok == null) return i18n.t("provider.notConfigured");
  return `${model.currency} ${model.cacheReadPricePerMTok}/${model.inputPricePerMTok}/${model.outputPricePerMTok}`;
}

function trimNumber(value: number) {
  return Number.isInteger(value) ? value.toString() : value.toFixed(1);
}

function formatPercent(value: number) {
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)}%`;
}

function clampPercent(value: number) {
  if (!Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(100, value));
}
