import { Activity, Boxes, Bot, ChevronDown, Cpu, Loader2 } from "lucide-react";
import type { Dispatch, ReactNode, SetStateAction } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ModelRecord,
  ProviderRecord,
  RoleRecord,
  SessionRuntime,
  SubagentActivity,
  SubagentStatus,
  TurnPhase,
} from "../types";
import { allModels } from "../lib/utils";

type SessionStatusBarProps = {
  runtime: SessionRuntime | null;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveProviderSettings: (explicitRoles?: RoleRecord[]) => void;
  turnPhase: TurnPhase;
  turnStartedAt: number | null;
  subagentActivities: SubagentActivity[];
};

const turnPhaseKeys: Record<TurnPhase, string> = {
  idle: "turnPhase.idle",
  running: "turnPhase.running",
  thinking: "turnPhase.thinking",
  tool: "turnPhase.tool",
  subagent: "turnPhase.subagent",
  approval: "turnPhase.approval",
  stopping: "turnPhase.stopping",
  completed: "turnPhase.completed",
  interrupted: "turnPhase.interrupted",
  failed: "turnPhase.failed",
};

const activeSubagentStatuses = new Set<SubagentStatus>([
  "running",
  "awaitingApproval",
  "awaitingToolApproval",
  "queued",
]);

const subagentStatusKeys: Record<SubagentStatus, string> = {
  queued: "subagent.queued",
  awaitingApproval: "subagent.awaitingApproval",
  running: "subagent.running",
  awaitingToolApproval: "subagent.awaitingTool",
  succeeded: "subagent.succeeded",
  failed: "subagent.failed",
  denied: "subagent.denied",
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

function formatElapsed(startedAt: number | null, now: number): string | null {
  if (!startedAt) {
    return null;
  }
  const seconds = Math.max(0, Math.floor((now - startedAt) / 1000));
  if (seconds < 60) {
    return `${seconds}s`;
  }
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${seconds % 60}s`;
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

function sortSubagents(activities: SubagentActivity[]): SubagentActivity[] {
  return [...activities].sort((left, right) => {
    const leftActive = activeSubagentStatuses.has(left.status) ? 1 : 0;
    const rightActive = activeSubagentStatuses.has(right.status) ? 1 : 0;
    if (leftActive !== rightActive) {
      return rightActive - leftActive;
    }
    if (right.updatedAt !== left.updatedAt) {
      return right.updatedAt - left.updatedAt;
    }
    return left.role.localeCompare(right.role);
  });
}

function TurnStatusIndicator({
  turnPhase,
  turnStartedAt,
}: {
  turnPhase: TurnPhase;
  turnStartedAt: number | null;
}) {
  const { t } = useTranslation();
  const [now, setNow] = useState(Date.now());
  const elapsed = formatElapsed(turnStartedAt, now);
  const isActive = ["running", "thinking", "tool", "subagent", "approval", "stopping"].includes(turnPhase);

  useEffect(() => {
    if (!turnStartedAt || !isActive) {
      return;
    }
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [turnStartedAt, isActive]);

  return (
    <div className={`turn-status-indicator phase-${turnPhase}`} aria-live="polite">
      {isActive ? <Loader2 size={14} className="spin" /> : <Activity size={14} />}
      <strong>{t(turnPhaseKeys[turnPhase])}</strong>
      {elapsed ? <span>{elapsed}</span> : null}
    </div>
  );
}

function SubagentPopover({ activities }: { activities: SubagentActivity[] }) {
  const { t } = useTranslation();
  const items = sortSubagents(activities);

  return (
    <StatusPopover
      className="subagent-popover-wrap"
      trigger={
        <button className="status-item status-subagents" type="button">
          <Bot size={14} />
          <strong>{t("statusBar.subagents")} {activities.length}</strong>
          <ChevronDown size={13} />
        </button>
      }
    >
      <div className="status-subagent-popover">
        <strong>{t("statusBar.subagents")}</strong>
        {items.length === 0 ? (
          <span className="status-empty">{t("statusBar.noSubagents")}</span>
        ) : (
          items.map((activity) => (
            <div key={activity.id} className={`status-subagent-row status-${activity.status}`}>
              <span className="status-subagent-dot" aria-hidden="true" />
              <div>
                <span className="status-subagent-role">{activity.role}</span>
                <p>{activity.task}</p>
              </div>
              <span className="status-subagent-badge">{t(subagentStatusKeys[activity.status])}</span>
            </div>
          ))
        )}
      </div>
    </StatusPopover>
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
  providers,
  roles,
  setRoles,
  onSaveProviderSettings,
  turnPhase,
  turnStartedAt,
  subagentActivities,
}: SessionStatusBarProps) {
  const { t } = useTranslation();
  const contextLabel = `${formatTokenCount(runtime?.latestContextTokens)} / ${formatTokenCount(runtime?.contextWindow)}`;
  const costLabel = formatCost(runtime, t("statusBar.costNotConfigured"));
  const contextWidth = `${contextPercent(runtime)}%`;
  const skills = runtime?.activeSkills ?? [];
  const mcpServers = runtime?.activeMcpServers ?? [];
  const capabilityCount = skills.length;

  return (
    <div className="session-status-bar" aria-label={t("statusBar.label")}>
      <div className="status-group status-left">
        <TurnStatusIndicator turnPhase={turnPhase} turnStartedAt={turnStartedAt} />
      </div>

      <div className="status-group status-right">
        <ModelSelector
          runtime={runtime}
          providers={providers}
          roles={roles}
          setRoles={setRoles}
          onSaveProviderSettings={onSaveProviderSettings}
        />

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

        <StatusPopover
          trigger={
            <button className="status-item status-count" type="button">
              <Boxes size={14} />
              <strong>{t("statusBar.capabilities")} {capabilityCount}</strong>
              <ChevronDown size={13} />
            </button>
          }
        >
          <div className="status-extensions-popover">
            <ListPopover title="Skills" items={skills} />
            <ListPopover title="MCP" items={mcpServers} />
          </div>
        </StatusPopover>

        <SubagentPopover activities={subagentActivities} />
      </div>
    </div>
  );
}
