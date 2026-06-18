import {
  Boxes,
  Bot,
  Brain,
  Circle,
  Clock,
  Cpu,
  Loader2,
  Users,
} from "lucide-solid";
import { For, Show, createMemo, type JSX } from "solid-js";
import type {
  AgentDto,
  CompileMode,
  InteractionRequest,
  LspServerRecord,
  McpServerRecord,
  ProviderRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
} from "../../types";
import i18n from "../../i18n";
import { StatusDetailsPopover } from "./status-details-popover";
import { StatusSelect, type StatusSelectOption } from "./status-select";
import {
  activeWaitingPhase,
  agentStatusLabel,
  allModels,
  contextPercent,
  elapsedLabel,
  findModel,
  formatCost,
  formatCostNumber,
  formatPercent,
  formatTokenCount,
  interactionLabel,
  lspAvailabilityLabel,
  lspDotClass,
  mcpAvailabilityLabel,
  mcpDotClass,
  phaseLabel,
  phaseName,
  phaseSummaryLabel,
  pricedCosts,
} from "./status-format";
export { activeWaitingPhase } from "./status-format";

const activeAgentStatuses = new Set(["queued", "running", "waiting"]);
const modes: CompileMode[] = ["auto", "plan"];

export function SessionStatusBar(props: {
  runtime: SessionRuntime | null | undefined;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  currentMode: CompileMode;
  selectedSession: SessionRecord | undefined;
  busy: boolean;
  turnPhase: string | undefined;
  activeInteraction: InteractionRequest | null | undefined;
  turnStartedAt: number | null | undefined;
  mcpServers: McpServerRecord[];
  activeMcpServers: string[];
  lspServers: LspServerRecord[];
  activeLspServers: string[];
  agents: AgentDto[];
  onSetSessionMode: (mode: CompileMode) => void;
  onSaveRoles: (roles: RoleRecord[]) => void;
}) {
  const usage = () => props.runtime?.usage ?? null;
  const plannerRole = createMemo(() => props.roles.find((role) => role.key === "planner") ?? props.roles[0]);
  const modelInfo = createMemo(() => findModel(props.providers, plannerRole()?.model ?? usage()?.model ?? ""));
  const contextWindow = createMemo(() => modelInfo()?.contextWindow ?? modelInfo()?.maxContextWindow ?? usage()?.contextWindow ?? null);
  const contextLabel = createMemo(() => `${formatTokenCount(usage()?.latestContextTokens)} / ${formatTokenCount(contextWindow())}`);
  const contextUsagePercent = createMemo(() => contextPercent(usage()?.latestContextTokens, contextWindow()));
  const costLabel = createMemo(() => formatCost(usage()));
  const visibleAgents = createMemo(() => props.agents.filter((agent) => activeAgentStatuses.has(agent.status)));
  const capabilityCount = createMemo(() =>
    (props.runtime?.activeSkills.length ?? 0) + props.activeMcpServers.length + props.activeLspServers.length,
  );
  const efforts = createMemo(() => modelInfo()?.reasoningEfforts ?? []);
  const waitingPhase = createMemo(() => activeWaitingPhase(props.activeInteraction));
  const visiblePhase = createMemo(() => waitingPhase() ?? props.turnPhase ?? "idle");
  const modeOptions = createMemo<StatusSelectOption[]>(() => modes.map((mode) => ({
    value: mode,
    label: mode === "auto" ? i18n.t("conversation.autoMode") : i18n.t("conversation.planMode"),
  })));
  const modelOptions = createMemo<StatusSelectOption[]>(() => props.providers.flatMap((provider) =>
    allModels(provider).map((model) => ({
      value: `${provider.id}::${model.slug}`,
      label: `${model.displayName || model.slug} · ${provider.name || provider.id}`,
    })),
  ));
  const effortOptions = createMemo<StatusSelectOption[]>(() => efforts().map((effort) => ({ value: effort, label: effort })));

  function selectModel(value: string) {
    const [providerId, model] = value.split("::");
    const role = plannerRole();
    if (!role || !providerId || !model) return;
    const provider = props.providers.find((item) => item.id === providerId);
    const info = provider?.models.find((item) => item.slug === model)
      ?? provider?.defaultModels.find((item) => item.slug === model)
      ?? provider?.customModels.find((item) => item.slug === model);
    const effort = info?.reasoningEfforts.includes(role.effort)
      ? role.effort
      : info?.reasoningEfforts[0] ?? "";
    props.onSaveRoles(props.roles.map((item) =>
      item.key === role.key ? { ...item, provider: providerId, model, effort } : item,
    ));
  }

  function selectEffort(effort: string) {
    const role = plannerRole();
    if (!role) return;
    props.onSaveRoles(props.roles.map((item) => item.key === role.key ? { ...item, effort } : item));
  }

  return (
    <div class="session-status-bar">
      <div class="status-controls">
        <StatusSelect
          value={props.currentMode}
          options={modeOptions()}
          disabled={props.busy || !props.selectedSession}
          onChange={(value) => props.onSetSessionMode(value as CompileMode)}
          aria-label={i18n.t("statusBar.sessionMode")}
          class="mode"
          icon={props.currentMode === "plan" ? <Brain size={14} /> : <Bot size={14} />}
        />

        <StatusSelect
          value={`${plannerRole()?.provider ?? ""}::${plannerRole()?.model ?? ""}`}
          options={modelOptions()}
          onChange={selectModel}
          aria-label={i18n.t("statusBar.plannerModel")}
          class="wide"
          icon={<Cpu size={14} />}
        />

        <Show when={efforts().length > 0}>
          <StatusSelect
            value={plannerRole()?.effort ?? efforts()[0] ?? ""}
            options={effortOptions()}
            onChange={selectEffort}
            aria-label={i18n.t("roleRoute.effort")}
            class="compact"
            icon={<Clock size={14} />}
          />
        </Show>
      </div>

      <div class="status-readouts">
        <StatusDetailsPopover icon={<Clock size={14} />} label={phaseSummaryLabel(visiblePhase(), props.turnStartedAt)}>
          <StatusLine label={i18n.t("statusBar.status")} value={phaseLabel(visiblePhase(), props.turnStartedAt)} />
          <StatusLine label={i18n.t("statusBar.phase")} value={phaseName(visiblePhase())} />
          <StatusLine label={i18n.t("statusBar.elapsed")} value={elapsedLabel(visiblePhase(), props.turnStartedAt)} />
          <StatusLine label={i18n.t("statusBar.busy")} value={props.busy ? i18n.t("common.yes") : i18n.t("common.no")} />
          <StatusLine label={i18n.t("statusBar.blocked")} value={waitingPhase() ? i18n.t("common.yes") : i18n.t("common.no")} />
          <Show when={waitingPhase()}>
            {(phase) => <StatusLine label={i18n.t("statusBar.waitingFor")} value={interactionLabel(phase())} />}
          </Show>
        </StatusDetailsPopover>

        <StatusDetailsPopover
          class="status-context-details"
          icon={<ContextRing percent={contextUsagePercent()} />}
        >
          <div class="context-detail-meter"><span style={{ width: `${contextUsagePercent()}%` }} /></div>
          <StatusLine label={i18n.t("statusBar.used")} value={contextLabel()} />
          <StatusLine label={i18n.t("statusBar.window")} value={formatTokenCount(contextWindow())} />
          <StatusLine label={i18n.t("statusBar.usage")} value={`${contextUsagePercent()}%`} />
          <StatusLine label={i18n.t("statusBar.input")} value={formatTokenCount(usage()?.promptTokens)} />
          <StatusLine label={i18n.t("statusBar.output")} value={formatTokenCount(usage()?.completionTokens)} />
          <StatusLine label={i18n.t("statusBar.cacheRead")} value={formatTokenCount(usage()?.cachedPromptTokens)} />
          <StatusLine label={i18n.t("statusBar.cacheHit")} value={formatPercent(usage()?.cacheHitRate)} />
        </StatusDetailsPopover>

        <StatusDetailsPopover class="status-cost-details" label={costLabel()}>
          <Show
            when={pricedCosts(usage()?.estimatedCosts).length > 0}
            fallback={<StatusLine label={i18n.t("statusBar.estimated")} value={costLabel()} />}
          >
            <For each={pricedCosts(usage()?.estimatedCosts)}>
              {(cost) => <StatusLine label={cost.currency} value={formatCostNumber(cost.amount)} />}
            </For>
          </Show>
          <Show when={usage()?.hasUnpricedUsage}>
            <StatusLine label={i18n.t("statusBar.unpriced")} value={i18n.t("common.yes")} />
          </Show>
        </StatusDetailsPopover>

        <StatusDetailsPopover icon={<Boxes size={14} />} label={String(capabilityCount())}>
          <StatusGroup title={i18n.t("statusBar.skills")} items={props.runtime?.activeSkills ?? []} />
          <div class="status-list-title">{i18n.t("statusBar.mcpServers")}</div>
          <For each={props.mcpServers} fallback={<span class="status-muted">{i18n.t("common.none")}</span>}>
            {(server) => <McpRow server={server} active={props.activeMcpServers.includes(server.id)} />}
          </For>
          <div class="status-list-title">{i18n.t("statusBar.lspServers")}</div>
          <For each={props.lspServers} fallback={<span class="status-muted">{i18n.t("common.none")}</span>}>
            {(server) => <LspRow server={server} active={props.activeLspServers.includes(server.id)} />}
          </For>
        </StatusDetailsPopover>

        <StatusDetailsPopover icon={<Users size={14} />} label={String(visibleAgents().length)}>
          <For each={visibleAgents()} fallback={<span class="status-muted">{i18n.t("statusBar.noSubagents")}</span>}>
            {(agent) => <AgentRow agent={agent} />}
          </For>
        </StatusDetailsPopover>
      </div>
    </div>
  );
}

function ContextRing(props: { percent: number }) {
  return (
    <span
      class="context-ring"
      style={{ "--context-percent": `${props.percent}%` } as JSX.CSSProperties}
      aria-label={i18n.t("statusBar.contextPercent", { percent: props.percent })}
    >
      <span>{props.percent}%</span>
    </span>
  );
}

function StatusLine(props: { label: string; value: string }) {
  return (
    <div class="status-line">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  );
}

function StatusGroup(props: { title: string; items: string[] }) {
  return (
    <div>
      <div class="status-list-title">{props.title}</div>
      <Show when={props.items.length > 0} fallback={<span class="status-muted">{i18n.t("common.none")}</span>}>
        <For each={props.items}>
          {(item) => <div class="status-list-item">{item}</div>}
        </For>
      </Show>
    </div>
  );
}

function LspRow(props: { server: LspServerRecord; active: boolean }) {
  return (
    <div class="status-list-item">
      <Show when={props.active} fallback={<Circle size={8} class={lspDotClass(props.server)} />}>
        <Loader2 size={12} class="spin" />
      </Show>
      <span>{props.server.displayName}</span>
      <small>{props.server.activityKind === "idle" ? lspAvailabilityLabel(props.server.availabilityKind) : props.server.activityKind}</small>
    </div>
  );
}

function McpRow(props: { server: McpServerRecord; active: boolean }) {
  return (
    <div class="status-list-item">
      <Circle size={8} class={mcpDotClass(props.server, props.active)} />
      <span>{props.server.id}</span>
      <small>{props.active ? i18n.t("statusBar.mcpActive", { status: mcpAvailabilityLabel(props.server.availabilityKind) }) : mcpAvailabilityLabel(props.server.availabilityKind)}</small>
    </div>
  );
}

function AgentRow(props: { agent: AgentDto }) {
  return (
    <div class="status-list-item">
      <Circle size={8} class={`agent-dot ${props.agent.status}`} />
      <span title={props.agent.task}>{props.agent.role}: {props.agent.task}</span>
      <small>{agentStatusLabel(props.agent)}</small>
    </div>
  );
}
