import {
  Activity,
  Boxes,
  Brain,
  Bot,
  ChevronDown,
  Clock,
  Circle,
  Cpu,
  DollarSign,
  Loader2,
  MoreVertical,
  Send,
  Settings,
  ShieldCheck,
  Smile,
  Users,
} from "lucide-react";
import type { CSSProperties, Dispatch, ReactNode, SetStateAction } from "react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type {
  CompileMode,
  ModelRecord,
  AgentDto,
  AgentStatus,
  PermissionMode,
  ProjectRecord,
  ProviderRecord,
  RoleRecord,
  RuntimeCostAmount,
  RuntimeUsage,
  SessionRecord,
  SessionRuntime,
  TurnPhase,
} from "../types";
import { allModels } from "../lib/utils";

type SessionStatusBarProps = {
  runtime: SessionRuntime | null;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveProviderSettings: (explicitRoles?: RoleRecord[]) => void;
  onSavePermissionMode: (mode: PermissionMode) => void;
  onSetSessionMode: (mode: CompileMode) => void;
  currentMode: CompileMode;
  isBusy: boolean;
  selectedSession: SessionRecord | null;
  turnPhase: TurnPhase;
  turnStartedAt: number | null;
  permissionMode: PermissionMode;
  agents: AgentDto[];
};

type DropdownPosition = CSSProperties & {
  "--dropdown-max-height"?: string;
};

const turnPhaseKeys: Record<TurnPhase, string> = {
  idle: "turnPhase.idle",
  running: "turnPhase.running",
  thinking: "turnPhase.thinking",
  tool: "turnPhase.tool",
  subagent: "turnPhase.subagent",
  approval: "turnPhase.approval",
  userInput: "turnPhase.userInput",
  stopping: "turnPhase.stopping",
  completed: "turnPhase.completed",
  aborted: "turnPhase.interrupted",
  interrupted: "turnPhase.interrupted",
  budgetLimited: "turnPhase.budgetLimited",
  errored: "turnPhase.failed",
  failed: "turnPhase.failed",
};

const permissionModes: PermissionMode[] = [
  "request-approval",
  "auto-review",
  "full-access",
];

const activeAgentStatuses = new Set<AgentStatus>([
  "running",
  "waiting",
  "queued",
]);

/* ===== Helpers ===== */

function formatTokenCount(value?: number | null): string {
  const tokens = value ?? 0;
  if (tokens >= 1_000_000) return `${formatNumber(tokens / 1_000_000)}m`;
  if (tokens >= 1_000) return `${formatNumber(tokens / 1_000)}k`;
  return tokens.toString();
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? value.toFixed(0) : value.toFixed(1);
}

function formatPercent(value?: number | null): string {
  if (value == null) return "0%";
  return `${Math.round(value * 100)}%`;
}

function pricedCosts(costs: RuntimeCostAmount[] | null | undefined): RuntimeCostAmount[] {
  return (costs ?? []).filter((cost) => cost.currency && Number.isFinite(cost.amount));
}

function formatCostNumber(value: number): string {
  const absolute = Math.abs(value);
  if (absolute > 0 && absolute < 0.01) return value.toFixed(4);
  return value.toFixed(2);
}

function formatCostAmounts(
  costs: RuntimeCostAmount[] | null | undefined,
  hasUnpricedUsage: boolean | undefined,
  fallback: string,
  unpriced: string,
): string {
  const priced = pricedCosts(costs);
  if (priced.length === 0) return hasUnpricedUsage ? unpriced : fallback;
  const label = priced
    .map((cost) => `${cost.currency} ${formatCostNumber(cost.amount)}`)
    .join(" + ");
  return hasUnpricedUsage ? `${label} + ${unpriced}` : label;
}

function formatCost(runtime: SessionRuntime | null, fallback: string, unpriced: string): string {
  if (!runtime?.usage) return fallback;
  return formatCostAmounts(
    runtime.usage.estimatedCosts,
    runtime.usage.hasUnpricedUsage,
    fallback,
    unpriced,
  );
}

function formatRuntimeCost(usage: RuntimeUsage | null | undefined, fallback: string, unpriced: string): string {
  if (!usage || usage.totalTokens === 0) return fallback;
  return formatCostAmounts(usage.estimatedCosts, usage.hasUnpricedUsage, fallback, unpriced);
}

function contextPercent(latestContextTokens: number | null | undefined, contextWindow: number | null): number {
  if (!contextWindow) return 0;
  return Math.min(100, ((latestContextTokens ?? 0) / contextWindow) * 100);
}

function formatElapsed(startedAt: number | null, now: number): string | null {
  if (!startedAt) return null;
  const seconds = Math.max(0, Math.floor((now - startedAt) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${seconds % 60}s`;
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

export function selectedContextWindow(
  providers: ProviderRecord[],
  roles: RoleRecord[],
  runtime: SessionRuntime | null,
): number | null {
  const plannerRole = findPlannerRole(roles);
  const currentModelSlug = plannerRole?.model ?? runtime?.usage.model ?? "";
  const currentModelInfo = currentModelSlug ? findModelInProviders(providers, currentModelSlug) : null;
  return currentModelInfo?.model.contextWindow
    ?? currentModelInfo?.model.maxContextWindow
    ?? runtime?.usage.contextWindow
    ?? null;
}

function agentStatusKeys(status: AgentStatus): string {
  const map: Record<AgentStatus, string> = {
    queued: "subagent.queued",
    running: "subagent.running",
    waiting: "subagent.awaitingTool",
    completed: "turnPhase.completed",
    errored: "subagent.failed",
    interrupted: "turnPhase.interrupted",
    shutdown: "status.done",
    notFound: "subagent.notFound",
  };
  return map[status];
}

function StatusPopover({
  children,
  priority,
  trigger,
}: {
  children: ReactNode;
  priority?: string;
  trigger: ReactNode;
}) {
  const [open, setOpen] = useState(false);

  return (
    <div
      className={`status-popover-wrap${open ? " open" : ""}`}
      data-priority={priority}
      tabIndex={0}
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={() => setOpen(false)}
      onFocusCapture={() => setOpen(true)}
      onBlurCapture={(event) => {
        const nextFocus = event.relatedTarget;
        if (!(nextFocus instanceof Node) || !event.currentTarget.contains(nextFocus)) {
          setOpen(false);
        }
      }}
    >
      {trigger}
      <div className={`status-popover${open ? " open" : ""}`}>{children}</div>
    </div>
  );
}

function ListPopover({ title, items }: { title: string; items: string[] }) {
  return (
    <div className="status-list-popover">
      <StatusListContent title={title} items={items} />
    </div>
  );
}

function StatusListContent({ title, items }: { title: string; items: string[] }) {
  const { t } = useTranslation();

  return (
    <>
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
    </>
  );
}

function UsageMetricRows({ usage }: { usage: RuntimeUsage | null }) {
  const { t } = useTranslation();

  return (
    <>
      <span>
        {t("statusBar.cacheHit")} <strong>{formatPercent(usage?.cacheHitRate)}</strong>
      </span>
      <span>
        {t("statusBar.input")} <strong>{formatTokenCount(usage?.promptTokens)}</strong>
      </span>
      <span>
        {t("statusBar.output")} <strong>{formatTokenCount(usage?.completionTokens)}</strong>
      </span>
      <span>
        {t("statusBar.cacheRead")}{" "}
        <strong>{formatTokenCount(usage?.cachedPromptTokens)}</strong>
      </span>
    </>
  );
}

function UsagePopoverContent({ usage }: { usage: RuntimeUsage | null }) {
  const { t } = useTranslation();

  return (
    <div className="status-usage-popover">
      <UsageMetricRows usage={usage} />
      <hr />
      <CostRows usage={usage} />
      <small>{t("statusBar.costHint")}</small>
    </div>
  );
}

function CostRows({ usage }: { usage: RuntimeUsage | null }) {
  const { t } = useTranslation();
  const rows = pricedCosts(usage?.estimatedCosts);
  const hasUnpricedUsage = usage?.hasUnpricedUsage ?? false;

  if (rows.length === 0) {
    return (
      <span>
        {t("statusBar.cost")}{" "}
        <strong>{hasUnpricedUsage ? t("statusBar.costUnpriced") : t("statusBar.costNotConfigured")}</strong>
      </span>
    );
  }

  return (
    <>
      {rows.map((cost) => (
        <span className="status-cost-row" key={cost.currency}>
          {cost.currency} <strong>{formatCostNumber(cost.amount)}</strong>
        </span>
      ))}
      {hasUnpricedUsage ? (
        <span className="status-cost-row status-cost-unpriced">
          {t("statusBar.cost")} <strong>{t("statusBar.costUnpriced")}</strong>
        </span>
      ) : null}
    </>
  );
}

function sortAgents(agents: AgentDto[]): AgentDto[] {
  return [...agents].sort((left, right) => {
    const leftActive = activeAgentStatuses.has(left.status) ? 1 : 0;
    const rightActive = activeAgentStatuses.has(right.status) ? 1 : 0;
    if (leftActive !== rightActive) {
      return rightActive - leftActive;
    }
    if (right.updatedAt !== left.updatedAt) {
      return right.updatedAt - left.updatedAt;
    }
    return left.role.localeCompare(right.role);
  });
}

function agentPopoverDetail(agent: AgentDto, t: (key: string) => string): string {
  if (["errored", "interrupted", "shutdown", "notFound"].includes(agent.status)) {
    if (agent.error?.trim()) {
      return agent.error;
    }
    if (agent.reason?.trim()) {
      return translateAgentReason(agent.reason, t);
    }
  }
  return agent.task;
}

function translateAgentReason(reason: string, t: (key: string) => string): string {
  switch (reason) {
    case "providerError":
      return t("subagent.providerError");
    case "toolError":
      return t("subagent.toolError");
    case "budgetLimited":
      return t("subagent.budgetLimited");
    case "interrupted":
      return t("subagent.interrupted");
    case "shutdown":
      return t("subagent.shutdown");
    default:
      return reason;
  }
}

function SubagentRows({ agents }: { agents: AgentDto[] }) {
  const { t } = useTranslation();
  const items = sortAgents(agents);
  const fallback = t("statusBar.costNotConfigured");
  const unpriced = t("statusBar.costUnpriced");

  if (items.length === 0) {
    return <span className="status-empty">{t("statusBar.noSubagents")}</span>;
  }

  return (
    <>
      {items.map((activity) => (
        <div key={activity.id} className={`status-subagent-row status-${activity.status}`}>
          <span className="status-subagent-dot" aria-hidden="true" />
          <div>
            <span className="status-subagent-role">{activity.role}</span>
            <p>{agentPopoverDetail(activity, t)}</p>
          </div>
          <span className="status-subagent-cost">
            {formatRuntimeCost(activity.runtimeUsage, fallback, unpriced)}
          </span>
          <span className="status-subagent-badge">{t(agentStatusKeys(activity.status))}</span>
        </div>
      ))}
    </>
  );
}

function AgentPopover({ agents, count }: { agents: AgentDto[]; count: number }) {
  const { t } = useTranslation();

  return (
    <StatusPopover
      priority="4"
      trigger={
        <button className="status-readonly status-readonly-trigger" type="button">
          <Users size={12} />
          {count}
        </button>
      }
    >
      <div className="status-subagent-popover">
        <strong>{t("statusBar.subagents")}</strong>
        <SubagentRows agents={agents} />
      </div>
    </StatusPopover>
  );
}

/* ===== Dropdown primitive ===== */

function Dropdown({
  trigger,
  children,
  className,
  wrapClassName,
  buttonClassName,
  dropdownClassName,
  align = "left",
  hideChevron = false,
  ariaLabel,
  ariaHaspopup = "menu",
  menuRole,
}: {
  trigger: ReactNode;
  children: ReactNode;
  className?: string;
  wrapClassName?: string;
  buttonClassName?: string;
  dropdownClassName?: string;
  align?: "left" | "right";
  hideChevron?: boolean;
  ariaLabel?: string;
  ariaHaspopup?: "menu" | "dialog";
  menuRole?: "menu" | "dialog";
}) {
  const [open, setOpen] = useState(false);
  const [dropdownStyle, setDropdownStyle] = useState<DropdownPosition | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);

  function getDropdownStyle(): DropdownPosition | null {
    const button = buttonRef.current;
    if (!button) return null;
    const rect = button.getBoundingClientRect();
    const dropdownWidth = Math.min(280, window.innerWidth - 24);
    const left = align === "right"
      ? Math.max(12, Math.min(rect.right - dropdownWidth, window.innerWidth - dropdownWidth - 12))
      : Math.max(12, Math.min(rect.left, window.innerWidth - dropdownWidth - 12));
    const bottom = Math.max(12, window.innerHeight - rect.top + 6);
    const maxHeight = Math.max(140, Math.min(360, rect.top - 18));
    return {
      bottom: `${bottom}px`,
      left: `${left}px`,
      position: "fixed",
      right: "auto",
      top: "auto",
      width: `${dropdownWidth}px`,
      "--dropdown-max-height": `${maxHeight}px`,
    };
  }

  useEffect(() => {
    if (!open) return;
    function updatePosition() {
      setDropdownStyle(getDropdownStyle());
    }
    function handleClick(event: MouseEvent) {
      const target = event.target as Node;
      if (ref.current?.contains(target) || dropdownRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }
    updatePosition();
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleKeyDown);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [open]);

  const dropdown = (
    <div
      ref={dropdownRef}
      className={`status-dropdown${align === "right" ? " right" : ""}${dropdownClassName ? ` ${dropdownClassName}` : ""} ${open ? "open" : ""}`}
      role={menuRole}
      style={dropdownStyle ?? undefined}
      onClick={(event) => {
        const target = event.target;
        const button = target instanceof Element ? target.closest("button") : null;
        if (button instanceof HTMLButtonElement && !button.disabled) {
          setOpen(false);
        }
      }}
    >
      {children}
    </div>
  );

  return (
    <div
      ref={ref}
      className={`status-dropdown-wrap${open ? " open" : ""}${align === "right" ? " align-right" : ""}${wrapClassName ? ` ${wrapClassName}` : ""}`}
      style={{ position: "relative", display: "inline-flex" }}
    >
      <button
        ref={buttonRef}
        className={buttonClassName ?? `status-chip selectable ${className ?? ""}`}
        style={buttonClassName ? undefined : { display: "inline-flex", alignItems: "center", gap: 4 }}
        type="button"
        aria-label={ariaLabel}
        aria-haspopup={ariaHaspopup}
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {trigger}
        {hideChevron ? null : <ChevronDown size={12} />}
      </button>
      {open && dropdownStyle && typeof document !== "undefined"
        ? createPortal(dropdown, document.body)
        : null}
    </div>
  );
}

/* ===== Mode Selector ===== */

function ModeSelector({
  currentMode,
  onSetSessionMode,
  isBusy,
  selectedSession,
}: {
  currentMode: CompileMode;
  onSetSessionMode: (mode: CompileMode) => void;
  isBusy: boolean;
  selectedSession: SessionRecord | null;
}) {
  const { t } = useTranslation();

  const modeIcon = currentMode === "auto"
    ? <Bot size={13} />
    : <Brain size={13} />;

  const modeLabel = currentMode === "auto" ? t("conversation.autoMode") : t("conversation.planMode");

  return (
    <Dropdown
      className="status-chip mode-chip selectable"
      trigger={<>{modeIcon} {modeLabel}</>}
    >
      <button
        className={`status-dropdown-item ${currentMode === "auto" ? "active" : ""}`}
        onClick={() => {
          onSetSessionMode("auto");
        }}
        disabled={isBusy || !selectedSession}
      >
        <Bot size={16} />
        <span>
          <strong>{t("conversation.autoMode")}</strong>
          <small>自动执行，直接生成代码</small>
        </span>
      </button>
      <button
        className={`status-dropdown-item ${currentMode === "plan" ? "active" : ""}`}
        onClick={() => {
          onSetSessionMode("plan");
        }}
        disabled={isBusy || !selectedSession}
      >
        <Brain size={16} />
        <span>
          <strong>{t("conversation.planMode")}</strong>
          <small>先规划方案，确认后执行</small>
        </span>
      </button>
    </Dropdown>
  );
}

/* ===== Model Selector ===== */

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
  const [dropdownStyle, setDropdownStyle] = useState<DropdownPosition | null>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const plannerRole = findPlannerRole(roles);
  const currentModelSlug = plannerRole?.model ?? runtime?.usage.model ?? "";
  const currentModelInfo = findModelInProviders(providers, currentModelSlug);
  const currentEffort = plannerRole?.effort ?? "";
  const currentEfforts = currentModelInfo?.model.reasoningEfforts ?? [];

  function getDropdownStyle(): DropdownPosition | null {
    const button = buttonRef.current;
    if (!button) return null;
    const rect = button.getBoundingClientRect();
    const dropdownWidth = Math.min(340, window.innerWidth - 24);
    const left = Math.max(12, Math.min(rect.left, window.innerWidth - dropdownWidth - 12));
    const maxHeight = Math.max(160, Math.min(360, rect.top - 18));
    return {
      bottom: `${Math.max(12, window.innerHeight - rect.top + 6)}px`,
      left: `${left}px`,
      position: "fixed",
      right: "auto",
      top: "auto",
      width: `${dropdownWidth}px`,
      "--dropdown-max-height": `${maxHeight}px`,
    };
  }

  useEffect(() => {
    if (!open) return;
    function updatePosition() {
      setDropdownStyle(getDropdownStyle());
    }
    function handleClickOutside(event: MouseEvent) {
      const target = event.target as Node;
      if (wrapRef.current?.contains(target) || dropdownRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }
    updatePosition();
    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("keydown", handleKeyDown);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [open]);

  function handleSelectModel(providerId: string, modelSlug: string) {
    if (!plannerRole) return;
    const provider = providers.find((p) => p.id === providerId);
    if (!provider) return;
    const model = allModels(provider).find((m) => m.slug === modelSlug);
    if (!model) return;
    const effort = model.reasoningEfforts.includes(currentEffort)
      ? currentEffort
      : (model.reasoningEfforts[0] ?? "");
    const newRoles = roles.map((r) =>
      r.key === "planner"
        ? { ...r, provider: providerId, model: modelSlug, effort }
        : r,
    );
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
    setOpen(false);
  }

  const grouped: { provider: ProviderRecord; models: ModelRecord[] }[] = [];
  for (const provider of providers) {
    const models = allModels(provider);
    if (models.length > 0) {
      grouped.push({ provider, models });
    }
  }

  const dropdown = (
    <div ref={dropdownRef} className="model-selector-dropdown" style={dropdownStyle ?? undefined}>
      <div className="model-selector-list">
        {grouped.map((group) => (
          <div className="model-selector-group" key={group.provider.id}>
            <div className="model-selector-provider-label">
              {group.provider.name || group.provider.id}
            </div>
            {group.models.map((model) => (
              <div
                key={model.slug}
                className={`model-selector-option${model.slug === currentModelSlug ? " active" : ""}`}
                role="button"
                tabIndex={0}
                onClick={() => handleSelectModel(group.provider.id, model.slug)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    handleSelectModel(group.provider.id, model.slug);
                  }
                }}
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
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );

  return (
    <div className={`model-selector-wrap${open ? " open" : ""}`} ref={wrapRef}>
      <button
        ref={buttonRef}
        className="status-chip selectable model-chip"
        type="button"
        onClick={() => {
          if (!open) {
            setDropdownStyle(getDropdownStyle());
          }
          setOpen((v) => !v);
        }}
      >
        <Cpu size={13} />
        {currentModelInfo?.model.displayName ?? currentModelSlug ?? t("statusBar.noModel")}
        <ChevronDown size={12} />
      </button>
      {open && dropdownStyle && typeof document !== "undefined"
        ? createPortal(dropdown, document.body)
        : null}
    </div>
  );
}

/* ===== Effort Selector ===== */

function EffortSelector({
  providers,
  roles,
  setRoles,
  onSaveProviderSettings,
}: {
  providers: ProviderRecord[];
  roles: RoleRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveProviderSettings: (explicitRoles?: RoleRecord[]) => void;
}) {
  const { t } = useTranslation();
  const plannerRole = findPlannerRole(roles);
  const currentModelSlug = plannerRole?.model ?? "";
  const currentModelInfo = currentModelSlug ? findModelInProviders(providers, currentModelSlug) : null;
  const efforts = currentModelInfo?.model.reasoningEfforts ?? [];
  const currentEffort = plannerRole?.effort && efforts.includes(plannerRole.effort)
    ? plannerRole.effort
    : (efforts[0] ?? "");

  if (!plannerRole || efforts.length === 0) {
    return null;
  }

  function handleSelectEffort(effort: string) {
    const newRoles = roles.map((role) =>
      role.key === "planner" ? { ...role, effort } : role,
    );
    setRoles(newRoles);
    onSaveProviderSettings(newRoles);
  }

  return (
    <Dropdown
      className="status-chip effort-chip selectable"
      trigger={<><Clock size={13} /> {currentEffort || t("roleRoute.effort")}</>}
    >
      <div className="status-dropdown-title">{t("roleRoute.effort")}</div>
      {efforts.map((effort) => (
        <button
          key={effort}
          className={`status-dropdown-item ${effort === currentEffort ? "active" : ""}`}
          onClick={() => handleSelectEffort(effort)}
        >
          <strong>{effort}</strong>
        </button>
      ))}
    </Dropdown>
  );
}

/* ===== Permission Selector ===== */

function PermissionSelector({
  permissionMode,
  onSavePermissionMode,
}: {
  permissionMode: PermissionMode;
  onSavePermissionMode: (mode: PermissionMode) => void;
}) {
  const { t } = useTranslation();

  return (
    <Dropdown
      className="status-chip permission-chip selectable"
      trigger={
        <>
          <ShieldCheck size={13} />
          {t(`permissionMode.${permissionMode}`)}
        </>
      }
    >
      <div className="status-dropdown-title">{t("statusBar.permissionMode")}</div>
      {permissionModes.map((mode) => (
        <button
          key={mode}
          className={`status-dropdown-item ${mode === permissionMode ? "active" : ""}`}
          onClick={() => {
            if (mode !== permissionMode) onSavePermissionMode(mode);
          }}
        >
          <strong>{t(`permissionMode.${mode}`)}</strong>
        </button>
      ))}
    </Dropdown>
  );
}

type StatusReadoutsProps = {
  usage: RuntimeUsage | null;
  contextLabel: string;
  contextWidth: string;
  costLabel: string;
  skills: string[];
  mcpServers: string[];
  capabilityCount: number;
  agents: AgentDto[];
  activeAgentCount: number;
};

function StatusReadoutPopovers({
  usage,
  contextLabel,
  contextWidth,
  costLabel,
  skills,
  mcpServers,
  capabilityCount,
  agents,
  activeAgentCount,
}: StatusReadoutsProps) {
  const { t } = useTranslation();

  return (
    <>
      <StatusPopover
        priority="1"
        trigger={
          <button className="status-readonly status-readonly-trigger" type="button">
            <div className="context-meter-inline">
              <span>{contextLabel}</span>
              <span className="context-meter-bar">
                <span className="context-meter-fill" style={{ width: contextWidth }} />
              </span>
            </div>
          </button>
        }
      >
        <UsagePopoverContent usage={usage} />
      </StatusPopover>

      <StatusPopover
        priority="2"
        trigger={
          <button className="status-readonly status-readonly-trigger" type="button">
            <DollarSign size={12} />
            {costLabel}
          </button>
        }
      >
        <div className="status-usage-popover">
          <CostRows usage={usage} />
          <small>{t("statusBar.costHint")}</small>
        </div>
      </StatusPopover>

      <StatusPopover
        priority="3"
        trigger={
          <button className="status-readonly status-readonly-trigger" type="button">
            <Boxes size={12} />
            {capabilityCount}
          </button>
        }
      >
        <div className="status-extensions-popover">
          <ListPopover title={t("statusBar.skills")} items={skills} />
          <ListPopover title={t("statusBar.mcpServers")} items={mcpServers} />
        </div>
      </StatusPopover>

      <AgentPopover agents={agents} count={activeAgentCount} />
    </>
  );
}

function StatusMoreSection({
  priority,
  icon,
  label,
  summary,
  children,
}: {
  priority: string;
  icon: ReactNode;
  label: string;
  summary: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="status-more-section" data-priority={priority} aria-label={label}>
      <div className="status-more-section-head">
        <span className="status-more-section-icon" aria-hidden="true">
          {icon}
        </span>
        <div className="status-more-section-copy">
          <span>{label}</span>
          <strong>{summary}</strong>
        </div>
      </div>
      <div className="status-more-section-body">{children}</div>
    </section>
  );
}

function MoreStatusMenu({
  usage,
  contextLabel,
  contextWidth,
  costLabel,
  skills,
  mcpServers,
  capabilityCount,
  agents,
  activeAgentCount,
}: StatusReadoutsProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [menuStyle, setMenuStyle] = useState<CSSProperties | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  function getMenuStyle(): CSSProperties | null {
    const button = buttonRef.current;
    if (!button) return null;
    const rect = button.getBoundingClientRect();
    const menuWidth = Math.min(360, window.innerWidth - 24);
    const maxLeft = Math.max(12, window.innerWidth - menuWidth - 12);
    const left = Math.min(Math.max(12, rect.right - menuWidth), maxLeft);
    return {
      bottom: `${Math.max(12, window.innerHeight - rect.top + 6)}px`,
      left: `${left}px`,
      position: "fixed",
      right: "auto",
      top: "auto",
    };
  }

  useEffect(() => {
    if (!open) return;

    function updatePosition() {
      setMenuStyle(getMenuStyle());
    }
    function handleClick(event: MouseEvent) {
      const target = event.target as Node;
      if (buttonRef.current?.contains(target) || menuRef.current?.contains(target)) {
        return;
      }
      setOpen(false);
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
        buttonRef.current?.focus();
      }
    }

    updatePosition();
    document.addEventListener("mousedown", handleClick);
    document.addEventListener("keydown", handleKeyDown);
    window.addEventListener("resize", updatePosition);
    window.addEventListener("scroll", updatePosition, true);
    return () => {
      document.removeEventListener("mousedown", handleClick);
      document.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("resize", updatePosition);
      window.removeEventListener("scroll", updatePosition, true);
    };
  }, [open]);

  const menu = (
    <div
      ref={menuRef}
      className="status-dropdown more-dropdown status-more-menu status-more-menu-fixed open"
      role="dialog"
      aria-label={t("statusBar.more")}
      style={menuStyle ?? undefined}
    >
      <StatusMoreSection
        priority="1"
        icon={<Activity size={14} />}
        label={t("statusBar.context")}
        summary={
          <span className="status-more-context-summary">
            {contextLabel}
            <span className="context-meter-bar">
              <span className="context-meter-fill" style={{ width: contextWidth }} />
            </span>
          </span>
        }
      >
        <div className="status-more-metrics">
          <UsageMetricRows usage={usage} />
        </div>
        <div className="status-more-cost-lines">
          <CostRows usage={usage} />
        </div>
      </StatusMoreSection>

      <StatusMoreSection
        priority="2"
        icon={<DollarSign size={14} />}
        label={t("statusBar.cost")}
        summary={costLabel}
      >
        <div className="status-more-cost-lines">
          <CostRows usage={usage} />
        </div>
        <small>{t("statusBar.costHint")}</small>
      </StatusMoreSection>

      <StatusMoreSection
        priority="3"
        icon={<Boxes size={14} />}
        label={t("statusBar.capabilities")}
        summary={capabilityCount}
      >
        <div className="status-more-lists">
          <div className="status-more-list-block">
            <StatusListContent title={t("statusBar.skills")} items={skills} />
          </div>
          <div className="status-more-list-block">
            <StatusListContent title={t("statusBar.mcpServers")} items={mcpServers} />
          </div>
        </div>
      </StatusMoreSection>

      <StatusMoreSection
        priority="4"
        icon={<Users size={14} />}
        label={t("statusBar.subagents")}
        summary={activeAgentCount}
      >
        <div className="status-more-subagents">
          <SubagentRows agents={agents} />
        </div>
      </StatusMoreSection>
    </div>
  );

  return (
    <>
      <button
        ref={buttonRef}
        className="status-more-btn"
        type="button"
        aria-label={t("statusBar.more")}
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => {
          if (!open) {
            setMenuStyle(getMenuStyle());
          }
          setOpen((value) => !value);
        }}
      >
        <MoreVertical size={16} aria-hidden="true" />
      </button>
      {open && menuStyle && typeof document !== "undefined" ? createPortal(menu, document.body) : null}
    </>
  );
}

/* ===== Main: SessionStatusBar ===== */

export function SessionStatusBar({
  runtime,
  providers,
  roles,
  setRoles,
  onSaveProviderSettings,
  onSavePermissionMode,
  onSetSessionMode,
  currentMode,
  isBusy,
  selectedSession,
  turnPhase,
  turnStartedAt,
  permissionMode,
  agents,
}: SessionStatusBarProps) {
  const { t } = useTranslation();
  const usage = runtime?.usage ?? null;
  const currentContextWindow = selectedContextWindow(providers, roles, runtime);
  const contextLabel = `${formatTokenCount(usage?.latestContextTokens)} / ${formatTokenCount(currentContextWindow)}`;
  const costLabel = formatCost(runtime, t("statusBar.costNotConfigured"), t("statusBar.costUnpriced"));
  const contextWidth = `${contextPercent(usage?.latestContextTokens, currentContextWindow)}%`;
  const skills = runtime?.activeSkills ?? [];
  const mcpServers = runtime?.activeMcpServers ?? [];
  const capabilityCount = skills.length;
  const activeAgentCount = agents.filter(
    (a) => a.status === "running" || a.status === "waiting" || a.status === "queued",
  ).length;
  const statusReadouts = {
    usage,
    contextLabel,
    contextWidth,
    costLabel,
    skills,
    mcpServers,
    capabilityCount,
    agents,
    activeAgentCount,
  };

  return (
    <div className="bottom-status-bar">
      {/* Left: Interactive/selectable */}
      <div className="bottom-status-left">
        <div className="bottom-status-controls">
          <ModeSelector
            currentMode={currentMode}
            onSetSessionMode={onSetSessionMode}
            isBusy={isBusy}
            selectedSession={selectedSession}
          />

          <ModelSelector
            runtime={runtime}
            providers={providers}
            roles={roles}
            setRoles={setRoles}
            onSaveProviderSettings={onSaveProviderSettings}
          />

          <EffortSelector
            providers={providers}
            roles={roles}
            setRoles={setRoles}
            onSaveProviderSettings={onSaveProviderSettings}
          />

          <PermissionSelector
            permissionMode={permissionMode}
            onSavePermissionMode={onSavePermissionMode}
          />
        </div>

        <MoreStatusMenu {...statusReadouts} />
      </div>

      {/* Right: Read-only status */}
      <div className="bottom-status-right">
        <StatusReadoutPopovers {...statusReadouts} />
      </div>

    </div>
  );
}
