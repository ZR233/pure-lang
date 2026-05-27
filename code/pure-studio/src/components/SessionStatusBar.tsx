import { Boxes, ChevronDown, Cpu, Database, FolderGit2 } from "lucide-react";
import type { Dispatch, ReactNode, SetStateAction } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ModelRecord, ProjectRecord, ProviderRecord, RoleRecord, SessionRecord, SessionRuntime } from "../types";
import { allModels } from "../lib/utils";

type SessionStatusBarProps = {
  runtime: SessionRuntime | null;
  selectedSession: SessionRecord | null;
  selectedProject: ProjectRecord | null;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveProviderSettings: (explicitRoles?: RoleRecord[]) => void;
};

function formatTokenCount(value?: number | null): string {
  const tokens = value ?? 0;
  if (tokens >= 1_000_000) {
    return `${formatNumber(tokens / 1_000_000)}m`;
  }
  if (tokens >= 1_000) {
    return `${formatNumber(tokens / 1_000)}k`;
  }
  return tokens.toString();
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1);
}

function formatPercent(value?: number | null): string {
  if (value == null) {
    return "0%";
  }
  return `${Math.round(value * 100)}%`;
}

function formatCost(runtime: SessionRuntime | null, fallback: string): string {
  if (!runtime?.currency || runtime.estimatedCost == null) {
    return fallback;
  }
  return `${runtime.currency} ${runtime.estimatedCost.toFixed(2)}`;
}

function contextPercent(runtime: SessionRuntime | null): number {
  if (!runtime?.contextWindow) {
    return 0;
  }
  return Math.min(100, (runtime.latestContextTokens / runtime.contextWindow) * 100);
}

function StatusPopover({
  className,
  trigger,
  children,
}: {
  className?: string;
  trigger: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className={`status-popover-wrap ${className ?? ""}`} tabIndex={0}>
      {trigger}
      <div className="status-popover">{children}</div>
    </div>
  );
}

function ListPopover({ title, items }: { title: string; items: string[] }) {
  const { t } = useTranslation();

  return (
    <div className="status-list-popover">
      <strong>{title}</strong>
      {items.length === 0 ? (
        <span className="status-empty">{t("statusBar.notConfigured")}</span>
      ) : (
        items.map((item) => (
          <span key={item} className="status-list-row">
            <span className="status-list-icon">{item.slice(0, 1).toUpperCase()}</span>
            {item}
          </span>
        ))
      )}
    </div>
  );
}

function findPlannerRole(roles: RoleRecord[]): RoleRecord | null {
  return roles.find((r) => r.key === "planner") ?? null;
}

function findModelInProviders(
  providers: ProviderRecord[],
  modelSlug: string,
): { provider: ProviderRecord; model: ModelRecord } | null {
  for (const provider of providers) {
    const model = allModels(provider).find((m) => m.slug === modelSlug);
    if (model) return { provider, model };
  }
  return null;
}

function ModelSelector({
  runtime,
  providers,
  roles,
  setRoles,
  onSaveProviderSettings,
}: {
  runtime: SessionRuntime | null;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveProviderSettings: (explicitRoles?: RoleRecord[]) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const plannerRole = findPlannerRole(roles);
  const currentModelSlug = plannerRole?.model ?? runtime?.model ?? "";
  const currentModelInfo = findModelInProviders(providers, currentModelSlug);
  const currentEffort = plannerRole?.effort ?? "";
  const currentEfforts = currentModelInfo?.model.reasoningEfforts ?? [];

  useEffect(() => {
    if (!open) return;
    function handleClickOutside(event: MouseEvent) {
      if (wrapRef.current && !wrapRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [open]);

  function buildUpdatedPlannerRole(
    providerId: string,
    modelSlug: string,
    effort: string,
  ): RoleRecord[] {
    return roles.map((r) =>
      r.key === "planner"
        ? { ...r, provider: providerId, model: modelSlug, effort }
        : r,
    );
  }

  function handleSelectModel(providerId: string, modelSlug: string) {
    if (!plannerRole) return;
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) return;
    const model = allModels(provider).find((m) => m.slug === modelSlug);
    if (!model) return;
    const effort = model.reasoningEfforts.includes(currentEffort)
      ? currentEffort
      : (model.reasoningEfforts[0] ?? "");
    const newRoles = buildUpdatedPlannerRole(providerId, modelSlug, effort);
    setRoles(newRoles);
    onSaveProviderSettings(newRoles);
    setOpen(false);
  }

  function handleSelectEffort(effort: string) {
    if (!plannerRole) return;
    const newRoles = roles.map((r) =>
      r.key === "planner" ? { ...r, effort } : r,
    );
    setRoles(newRoles);
    onSaveProviderSettings(newRoles);
  }

  const grouped: { provider: ProviderRecord; models: ModelRecord[] }[] = [];
  for (const provider of providers) {
    const models = allModels(provider);
    if (models.length > 0) {
      grouped.push({ provider, models });
    }
  }

  return (
    <div className="model-selector-wrap" ref={wrapRef}>
      <button
        className="status-item status-model"
        type="button"
        onClick={() => setOpen((v) => !v)}
      >
        <Cpu size={14} />
        <span>{t("statusBar.model")}</span>
        <strong>{currentModelInfo?.model.displayName ?? currentModelSlug ?? t("statusBar.noModel")}</strong>
        <ChevronDown size={13} />
      </button>
      {open && (
        <div className="model-selector-dropdown">
          <div className="model-selector-list">
            {grouped.map((group) => (
              <div className="model-selector-group" key={group.provider.id}>
                <div className="model-selector-provider-label">
                  {group.provider.name || group.provider.id}
                </div>
                {group.models.map((model) => (
                  <button
                    key={model.slug}
                    className={`model-selector-option${model.slug === currentModelSlug ? " active" : ""}`}
                    onClick={() => handleSelectModel(group.provider.id, model.slug)}
                  >
                    <span className="model-selector-name">{model.displayName || model.slug}</span>
                    {model.slug === currentModelSlug && currentEfforts.length > 0 ? (
                      <div className="model-selector-efforts" onClick={(e) => e.stopPropagation()}>
                        {currentEfforts.map((effort) => (
                          <button
                            key={effort}
                            className={`model-selector-effort${effort === currentEffort ? " active" : ""}`}
                            onClick={() => handleSelectEffort(effort)}
                          >
                            {effort}
                          </button>
                        ))}
                      </div>
                    ) : null}
                  </button>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export function SessionStatusBar({
  runtime,
  selectedSession,
  selectedProject,
  providers,
  roles,
  setRoles,
  onSaveProviderSettings,
}: SessionStatusBarProps) {
  const { t } = useTranslation();
  const contextLabel = `${formatTokenCount(runtime?.latestContextTokens)} / ${formatTokenCount(runtime?.contextWindow)}`;
  const costLabel = formatCost(runtime, t("statusBar.costNotConfigured"));
  const contextWidth = `${contextPercent(runtime)}%`;
  const skills = runtime?.activeSkills ?? [];
  const mcpServers = runtime?.activeMcpServers ?? [];

  return (
    <div className="session-status-bar" aria-label={t("statusBar.label")}>
      <div className="status-group status-left">
        <ModelSelector
          runtime={runtime}
          providers={providers}
          roles={roles}
          setRoles={setRoles}
          onSaveProviderSettings={onSaveProviderSettings}
        />

        <div className="status-item status-session">
          <FolderGit2 size={14} />
          <span>{t("statusBar.session")}</span>
          <strong>{selectedSession?.title ?? t("conversation.defaultTitle")}</strong>
        </div>
        <div className="status-item status-workspace">
          <Database size={14} />
          <strong>{selectedProject?.name ?? t("context.noProject")}</strong>
        </div>
      </div>

      <div className="status-group status-center">
        <StatusPopover
          className="context-popover-wrap"
          trigger={
            <button className="status-item status-context" type="button">
              <span>{t("statusBar.context")}</span>
              <strong>{contextLabel}</strong>
              <span className="context-meter" aria-hidden="true">
                <span style={{ width: contextWidth }} />
              </span>
            </button>
          }
        >
          <div className="status-usage-popover">
            <span>
              {t("statusBar.cacheHit")} <strong>{formatPercent(runtime?.cacheHitRate)}</strong>
            </span>
            <span>
              {t("statusBar.input")} <strong>{formatTokenCount(runtime?.promptTokens)}</strong>
            </span>
            <span>
              {t("statusBar.output")} <strong>{formatTokenCount(runtime?.completionTokens)}</strong>
            </span>
            <span>
              {t("statusBar.cacheRead")}{" "}
              <strong>{formatTokenCount(runtime?.cachedPromptTokens)}</strong>
            </span>
            <hr />
            <span>
              {t("statusBar.cost")} <strong>{costLabel}</strong>
            </span>
            <small>{t("statusBar.costHint")}</small>
          </div>
        </StatusPopover>

        <div className="status-item status-cost">
          <span>{t("statusBar.cost")}</span>
          <strong>{costLabel}</strong>
        </div>
      </div>

      <div className="status-group status-right">
        <StatusPopover
          trigger={
            <button className="status-item status-count" type="button">
              <Boxes size={14} />
              <strong>Skills {skills.length}</strong>
              <ChevronDown size={13} />
            </button>
          }
        >
          <ListPopover title="Skills" items={skills} />
        </StatusPopover>
        <StatusPopover
          trigger={
            <button className="status-item status-count" type="button">
              <Database size={14} />
              <strong>MCP {mcpServers.length}</strong>
              <ChevronDown size={13} />
            </button>
          }
        >
          <ListPopover title="MCP" items={mcpServers} />
        </StatusPopover>
      </div>
    </div>
  );
}
