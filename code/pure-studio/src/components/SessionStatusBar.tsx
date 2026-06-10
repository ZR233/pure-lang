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
  LoaderCircle,
  MoreVertical,
  ShieldCheck,
  Users,
} from "lucide-react";
import type { Dispatch, ReactNode, SetStateAction } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  CompileMode,
  LspServerRecord,
  ModelRecord,
  AgentDto,
  AgentStatus,
  PermissionMode,
  ProviderRecord,
  RoleRecord,
  RuntimeCostAmount,
  RuntimeUsage,
  SessionRecord,
  SessionRuntime,
  TurnPhase,
} from "../types";
import { allModels } from "../lib/utils";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuLabel,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";

type SessionStatusBarProps = {
  runtime: SessionRuntime | null;
  lspServers: LspServerRecord[];
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

const liveAgentTurnPhases = new Set<TurnPhase>([
  "running",
  "thinking",
  "tool",
  "subagent",
  "approval",
  "userInput",
  "stopping",
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

/* ===== Display helpers ===== */

function ListPopover({ title, items }: { title: string; items: string[] }) {
  return (
    <div className="space-y-1">
      <StatusListContent title={title} items={items} />
    </div>
  );
}

function StatusListContent({ title, items }: { title: string; items: string[] }) {
  const { t } = useTranslation();

  return (
    <>
      <strong className="text-xs font-medium">{title}</strong>
      {items.length === 0 ? (
        <span className="text-xs text-muted-foreground">{t("statusBar.notConfigured")}</span>
      ) : (
        items.map((item) => (
          <span key={item} className="flex items-center gap-1.5 text-xs">
            <span className="inline-flex items-center justify-center w-4 h-4 rounded bg-muted text-[10px] font-medium">
              {item.slice(0, 1).toUpperCase()}
            </span>
            {item}
          </span>
        ))
      )}
    </>
  );
}

type Translate = (key: string, options?: Record<string, unknown>) => string;

function lspStatusLabel(server: LspServerRecord, t: Translate): string {
  if (server.availabilityKind !== "available") {
    const map: Record<LspServerRecord["availabilityKind"], string> = {
      checking: "statusBar.lspChecking",
      available: "statusBar.lspReady",
      unavailable: "statusBar.lspUnavailable",
      missingCommand: "statusBar.lspMissingCommand",
      disabled: "statusBar.lspDisabled",
    };
    return t(map[server.availabilityKind]);
  }
  if (server.activityKind === "indexing") return t("statusBar.lspIndexing");
  if (server.activityKind === "busy") return t("statusBar.lspBusy");
  return t("statusBar.lspReady");
}

function lspStatusDotClass(server: LspServerRecord): string {
  if (server.availabilityKind === "available") {
    if (server.activityKind === "indexing") return "text-primary";
    if (server.activityKind === "busy") return "text-amber-500";
    return "text-emerald-600";
  }
  if (server.availabilityKind === "checking") return "text-amber-500";
  if (server.availabilityKind === "missingCommand" || server.availabilityKind === "unavailable") {
    return "text-red-500";
  }
  return "text-muted-foreground";
}

function lspDetailText(server: LspServerRecord): string | null {
  if (server.availabilityKind !== "available") {
    return server.availabilityMessage ?? null;
  }
  if (server.activityKind !== "idle") {
    const message = server.activityMessage ?? server.activityTitle ?? null;
    if (server.activityPercentage == null) return message;
    return message ? `${server.activityPercentage}% ${message}` : `${server.activityPercentage}%`;
  }
  return server.lastError ?? null;
}

function LspListContent({ title, servers }: { title: string; servers: LspServerRecord[] }) {
  const { t } = useTranslation();

  return (
    <>
      <strong className="text-xs font-medium">{title}</strong>
      {servers.length === 0 ? (
        <span className="text-xs text-muted-foreground">{t("statusBar.notConfigured")}</span>
      ) : (
        servers.map((server) => {
          const active = server.availabilityKind === "available" && server.activityKind !== "idle";
          const detail = lspDetailText(server);
          const detailIsError = server.availabilityKind === "available" && server.activityKind === "idle" && !!server.lastError;
          return (
            <div key={server.id} className="flex items-center gap-2 py-1.5 text-xs">
              {active ? (
                <LoaderCircle size={12} className={cn("shrink-0 animate-spin", lspStatusDotClass(server))} />
              ) : (
                <Circle size={8} className={cn("fill-current shrink-0", lspStatusDotClass(server))} />
              )}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 min-w-0">
                  <span className="font-medium truncate">{server.displayName}</span>
                  <Badge variant="outline" className="h-5 text-[10px] shrink-0">
                    {lspStatusLabel(server, t)}
                  </Badge>
                </div>
                {detail ? (
                  <div className={cn("truncate", detailIsError ? "text-red-500" : "text-muted-foreground")}>
                    {detail}
                  </div>
                ) : null}
              </div>
              <span className="text-[10px] text-muted-foreground shrink-0">
                {t("statusBar.lspDiagnosticsCount", { count: server.diagnosticCount })}
              </span>
            </div>
          );
        })
      )}
    </>
  );
}

function UsageMetricRows({ usage }: { usage: RuntimeUsage | null }) {
  const { t } = useTranslation();

  return (
    <>
      <span className="flex justify-between text-xs">
        {t("statusBar.cacheHit")} <strong>{formatPercent(usage?.cacheHitRate)}</strong>
      </span>
      <span className="flex justify-between text-xs">
        {t("statusBar.input")} <strong>{formatTokenCount(usage?.promptTokens)}</strong>
      </span>
      <span className="flex justify-between text-xs">
        {t("statusBar.output")} <strong>{formatTokenCount(usage?.completionTokens)}</strong>
      </span>
      <span className="flex justify-between text-xs">
        {t("statusBar.cacheRead")}{" "}
        <strong>{formatTokenCount(usage?.cachedPromptTokens)}</strong>
      </span>
    </>
  );
}

function UsagePopoverContent({ usage }: { usage: RuntimeUsage | null }) {
  const { t } = useTranslation();

  return (
    <div className="space-y-2">
      <strong className="text-xs">{t("statusBar.context")}</strong>
      <UsageMetricRows usage={usage} />
    </div>
  );
}

function CostRows({ usage }: { usage: RuntimeUsage | null }) {
  const { t } = useTranslation();
  const rows = pricedCosts(usage?.estimatedCosts);
  const hasUnpricedUsage = usage?.hasUnpricedUsage ?? false;

  if (rows.length === 0) {
    return (
      <span className="text-xs">
        {t("statusBar.cost")}{" "}
        <strong>{hasUnpricedUsage ? t("statusBar.costUnpriced") : t("statusBar.costNotConfigured")}</strong>
      </span>
    );
  }

  return (
    <>
      {rows.map((cost) => (
        <span className="flex justify-between text-xs" key={cost.currency}>
          {cost.currency} <strong>{formatCostNumber(cost.amount)}</strong>
        </span>
      ))}
      {hasUnpricedUsage ? (
        <span className="flex justify-between text-xs text-muted-foreground">
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
    return <span className="text-xs text-muted-foreground">{t("statusBar.noSubagents")}</span>;
  }

  return (
    <>
      {items.map((activity) => (
        <div key={activity.id} className="flex items-center gap-2 px-3 py-2 border-b last:border-0 text-xs">
          <Circle size={8} className={cn(
            "fill-current shrink-0",
            activity.status === "running" && "text-primary",
            activity.status === "waiting" && "text-amber-500",
            activity.status === "queued" && "text-muted-foreground",
            activity.status === "completed" && "text-emerald-600",
            activity.status === "errored" && "text-red-500",
            activity.status === "interrupted" && "text-orange-500",
            activity.status === "shutdown" && "text-muted-foreground",
            activity.status === "notFound" && "text-muted-foreground",
          )} />
          <div className="flex-1 min-w-0">
            <div className="font-medium truncate">{activity.role}</div>
            <div className="text-muted-foreground truncate">{agentPopoverDetail(activity, t)}</div>
          </div>
          <span className="text-muted-foreground shrink-0">
            {formatRuntimeCost(activity.runtimeUsage, fallback, unpriced)}
          </span>
          <Badge variant="outline" className="text-[10px] shrink-0">
            {t(agentStatusKeys(activity.status))}
          </Badge>
        </div>
      ))}
    </>
  );
}

function AgentPopover({ agents, count }: { agents: AgentDto[]; count: number }) {
  const { t } = useTranslation();

  return (
    <Popover>
      <PopoverTrigger asChild>
        <Button variant="ghost" size="sm" className="h-7 gap-1 px-2 text-xs text-muted-foreground">
          <Users size={14} />
          <span>{count}</span>
        </Button>
      </PopoverTrigger>
      <PopoverContent side="top" className="w-80 p-0" align="end">
        <div className="px-3 py-2 font-medium text-xs border-b">{t("statusBar.subagents")}</div>
        <ScrollArea className="max-h-64">
          <SubagentRows agents={agents} />
        </ScrollArea>
      </PopoverContent>
    </Popover>
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
    ? <Bot size={14} />
    : <Brain size={14} />;

  const modeLabel = currentMode === "auto" ? t("conversation.autoMode") : t("conversation.planMode");

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="h-7 gap-1.5 px-2 text-xs">
          {modeIcon}
          <span className="truncate max-w-[100px]">{modeLabel}</span>
          <ChevronDown size={12} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-56">
        <DropdownMenuItem
          onSelect={() => onSetSessionMode("auto")}
          disabled={isBusy || !selectedSession}
        >
          <Bot size={16} className="mr-2" />
          <div>
            <div className="font-medium">{t("conversation.autoMode")}</div>
            <div className="text-xs text-muted-foreground">自动执行，直接生成代码</div>
          </div>
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() => onSetSessionMode("plan")}
          disabled={isBusy || !selectedSession}
        >
          <Brain size={16} className="mr-2" />
          <div>
            <div className="font-medium">{t("conversation.planMode")}</div>
            <div className="text-xs text-muted-foreground">先规划方案，确认后执行</div>
          </div>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
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
  const [searchQuery, setSearchQuery] = useState("");
  const plannerRole = findPlannerRole(roles);
  const currentModelSlug = plannerRole?.model ?? runtime?.usage.model ?? "";
  const currentModelInfo = findModelInProviders(providers, currentModelSlug);
  const currentEffort = plannerRole?.effort ?? "";
  const currentEfforts = currentModelInfo?.model.reasoningEfforts ?? [];

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

  const grouped: { provider: ProviderRecord; models: ModelRecord[] }[] = [];
  for (const provider of providers) {
    const models = allModels(provider);
    if (models.length > 0) {
      grouped.push({ provider, models });
    }
  }

  const filtered = grouped
    .map((g) => ({
      ...g,
      models: searchQuery
        ? g.models.filter((m) =>
            (m.displayName ?? m.slug).toLowerCase().includes(searchQuery.toLowerCase()),
          )
        : g.models,
    }))
    .filter((g) => g.models.length > 0);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="h-7 gap-1.5 px-2 text-xs">
          <Cpu size={14} />
          <span className="truncate max-w-[100px]">
            {currentModelInfo?.model.displayName ?? currentModelSlug ?? t("statusBar.noModel")}
          </span>
          <ChevronDown size={12} />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-72 p-0" align="start">
        <Command shouldFilter={false}>
          <CommandInput
            placeholder={t("statusBar.searchModels") ?? "搜索模型..."}
            value={searchQuery}
            onValueChange={setSearchQuery}
          />
          <CommandList>
            <CommandEmpty>{t("statusBar.noModel")}</CommandEmpty>
            {filtered.map((group) => (
              <CommandGroup key={group.provider.id} heading={group.provider.name || group.provider.id}>
                {group.models.map((model) => (
                  <CommandItem
                    key={model.slug}
                    onSelect={() => handleSelectModel(group.provider.id, model.slug)}
                  >
                    <div className="flex-1 min-w-0">
                      <span className={cn(
                        "text-sm",
                        model.slug === currentModelSlug && "font-medium",
                      )}>
                        {model.displayName || model.slug}
                      </span>
                      {model.slug === currentModelSlug && currentEfforts.length > 0 && (
                        <div className="flex gap-1 mt-1" onClick={(e) => e.stopPropagation()}>
                          {currentEfforts.map((effort) => (
                            <button
                              key={effort}
                              className={cn(
                                "text-[10px] px-1.5 py-0.5 rounded border",
                                effort === currentEffort
                                  ? "border-primary bg-primary/10 text-primary"
                                  : "border-border text-muted-foreground",
                              )}
                              onClick={(e) => {
                                e.stopPropagation();
                                if (!plannerRole) return;
                                const newRoles = roles.map((r) =>
                                  r.key === "planner" ? { ...r, effort } : r,
                                );
                                setRoles(newRoles);
                                onSaveProviderSettings(newRoles);
                              }}
                            >
                              {effort}
                            </button>
                          ))}
                        </div>
                      )}
                    </div>
                  </CommandItem>
                ))}
              </CommandGroup>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
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
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="h-7 gap-1.5 px-2 text-xs">
          <Clock size={14} />
          <span className="truncate max-w-[100px]">{currentEffort || t("roleRoute.effort")}</span>
          <ChevronDown size={12} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-40">
        <DropdownMenuLabel className="text-xs text-muted-foreground">{t("roleRoute.effort")}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuRadioGroup value={currentEffort} onValueChange={handleSelectEffort}>
          {efforts.map((effort) => (
            <DropdownMenuRadioItem key={effort} value={effort}>
              {effort}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
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
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="outline" size="sm" className="h-7 gap-1.5 px-2 text-xs">
          <ShieldCheck size={14} />
          <span className="truncate max-w-[100px]">{t(`permissionMode.${permissionMode}`)}</span>
          <ChevronDown size={12} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-48">
        <DropdownMenuLabel className="text-xs text-muted-foreground">{t("statusBar.permissionMode")}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuRadioGroup value={permissionMode} onValueChange={(value) => onSavePermissionMode(value as PermissionMode)}>
          {permissionModes.map((mode) => (
            <DropdownMenuRadioItem key={mode} value={mode}>
              <strong>{t(`permissionMode.${mode}`)}</strong>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

type StatusReadoutsProps = {
  usage: RuntimeUsage | null;
  contextLabel: string;
  contextWidth: string;
  costLabel: string;
  skills: string[];
  mcpServers: string[];
  lspServers: LspServerRecord[];
  capabilityCount: number;
  agents: AgentDto[];
  activeAgentCount: number;
};

/* ===== Status Readout Popovers ===== */

function StatusReadoutPopovers({
  usage,
  contextLabel,
  contextWidth,
  costLabel,
  skills,
  mcpServers,
  lspServers,
  capabilityCount,
  agents,
  activeAgentCount,
}: StatusReadoutsProps) {
  const { t } = useTranslation();

  return (
    <>
      <Popover>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="sm" className="h-7 gap-1 px-2 text-xs text-muted-foreground">
            <div className="flex items-center gap-1">
              <span>{contextLabel}</span>
              <span className="w-12 h-1.5 bg-muted rounded-full overflow-hidden inline-block">
                <span className="block h-full bg-primary rounded-full" style={{ width: contextWidth }} />
              </span>
            </div>
          </Button>
        </PopoverTrigger>
        <PopoverContent side="top" className="w-64">
          <UsagePopoverContent usage={usage} />
        </PopoverContent>
      </Popover>

      <Popover>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="sm" className="h-7 gap-1 px-2 text-xs text-muted-foreground">
            <DollarSign size={14} />
            <span>{costLabel}</span>
          </Button>
        </PopoverTrigger>
        <PopoverContent side="top" className="w-64">
          <div className="space-y-2">
            <CostRows usage={usage} />
            <small className="text-xs text-muted-foreground block">{t("statusBar.costHint")}</small>
          </div>
        </PopoverContent>
      </Popover>

      <Popover>
        <PopoverTrigger asChild>
          <Button variant="ghost" size="sm" className="h-7 gap-1 px-2 text-xs text-muted-foreground">
            <Boxes size={14} />
            <span>{capabilityCount}</span>
          </Button>
        </PopoverTrigger>
        <PopoverContent side="top" className="w-80 space-y-3">
          <ListPopover title={t("statusBar.skills")} items={skills} />
          <ListPopover title={t("statusBar.mcpServers")} items={mcpServers} />
          <LspListContent title={t("statusBar.lspServers")} servers={lspServers} />
        </PopoverContent>
      </Popover>

      <AgentPopover agents={agents} count={activeAgentCount} />
    </>
  );
}

/* ===== More Status Menu ===== */

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
    <section className="px-3 py-2" data-priority={priority} aria-label={label}>
      <div className="flex items-center gap-2 mb-2">
        <span className="text-muted-foreground" aria-hidden="true">
          {icon}
        </span>
        <div className="flex-1 min-w-0">
          <span className="text-xs text-muted-foreground">{label}</span>
          <div className="text-sm font-medium">{summary}</div>
        </div>
      </div>
      <div className="space-y-1">{children}</div>
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
  lspServers,
  capabilityCount,
  agents,
  activeAgentCount,
}: StatusReadoutsProps) {
  const { t } = useTranslation();

  const menuContent = (
    <>
      <StatusMoreSection
        priority="1"
        icon={<Activity size={14} />}
        label={t("statusBar.context")}
        summary={
          <span className="flex items-center gap-2">
            {contextLabel}
            <span className="w-16 h-1.5 bg-muted rounded-full overflow-hidden inline-block">
              <span className="block h-full bg-primary rounded-full" style={{ width: contextWidth }} />
            </span>
          </span>
        }
      >
        <UsageMetricRows usage={usage} />
        <Separator className="my-1" />
        <CostRows usage={usage} />
      </StatusMoreSection>

      <Separator />

      <StatusMoreSection
        priority="2"
        icon={<DollarSign size={14} />}
        label={t("statusBar.cost")}
        summary={costLabel}
      >
        <CostRows usage={usage} />
        <small className="text-xs text-muted-foreground block mt-1">{t("statusBar.costHint")}</small>
      </StatusMoreSection>

      <Separator />

      <StatusMoreSection
        priority="3"
        icon={<Boxes size={14} />}
        label={t("statusBar.capabilities")}
        summary={capabilityCount}
      >
        <div className="space-y-2">
          <div>
            <StatusListContent title={t("statusBar.skills")} items={skills} />
          </div>
          <div>
            <StatusListContent title={t("statusBar.mcpServers")} items={mcpServers} />
          </div>
          <div>
            <LspListContent title={t("statusBar.lspServers")} servers={lspServers} />
          </div>
        </div>
      </StatusMoreSection>

      <Separator />

      <StatusMoreSection
        priority="4"
        icon={<Users size={14} />}
        label={t("statusBar.subagents")}
        summary={activeAgentCount}
      >
        <SubagentRows agents={agents} />
      </StatusMoreSection>
    </>
  );

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" className="h-7 w-7 p-0">
          <MoreVertical size={16} />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        side="top"
        className="w-80 max-h-[70vh] overflow-y-auto"
      >
        {menuContent}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/* ===== Main: SessionStatusBar ===== */

export function SessionStatusBar({
  runtime,
  lspServers: lspServerSnapshots,
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
  const activeLspServers = runtime?.activeLspServers ?? [];
  const lspServers = lspServerSnapshots;
  const capabilityCount = skills.length + mcpServers.length + activeLspServers.length;
  const shouldShowAgents = isBusy && liveAgentTurnPhases.has(turnPhase);
  const visibleAgents = shouldShowAgents
    ? agents.filter((agent) => activeAgentStatuses.has(agent.status))
    : [];
  const activeAgentCount = visibleAgents.length;
  const statusReadouts = {
    usage,
    contextLabel,
    contextWidth,
    costLabel,
    skills,
    mcpServers,
    lspServers,
    capabilityCount,
    agents: visibleAgents,
    activeAgentCount,
  };

  return (
    <div className="flex items-center justify-between gap-2 px-3 h-10 border-t border-border bg-card">
      {/* Left: Interactive/selectable */}
      <div className="flex items-center gap-1 min-w-0 overflow-x-auto">
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

        <MoreStatusMenu {...statusReadouts} />
      </div>

      {/* Right: Read-only status */}
      <div className="flex items-center gap-1 shrink-0">
        <StatusReadoutPopovers {...statusReadouts} />
      </div>
    </div>
  );
}
