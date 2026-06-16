import {
  Boxes,
  Bot,
  Brain,
  Circle,
  Clock,
  Cpu,
  DollarSign,
  Loader2,
  ShieldCheck,
  Users,
} from "lucide-solid";
import { For, Show, createMemo, type JSX } from "solid-js";
import type {
  AgentDto,
  CompileMode,
  InteractionRequest,
  LspServerRecord,
  McpServerRecord,
  PermissionMode,
  ProviderRecord,
  RoleRecord,
  RuntimeUsage,
  SessionRecord,
  SessionRuntime,
} from "../../types";

const activeAgentStatuses = new Set(["queued", "running", "waiting"]);
const permissionModes: PermissionMode[] = ["request-approval", "auto-review", "full-access"];
const modes: CompileMode[] = ["auto", "plan"];

export function SessionStatusBar(props: {
  runtime: SessionRuntime | null | undefined;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  permissionMode: PermissionMode;
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
  onSavePermissionMode: (mode: PermissionMode) => void;
  onSaveRoles: (roles: RoleRecord[]) => void;
}) {
  const usage = () => props.runtime?.usage ?? null;
  const plannerRole = createMemo(() => props.roles.find((role) => role.key === "planner") ?? props.roles[0]);
  const modelInfo = createMemo(() => findModel(props.providers, plannerRole()?.model ?? usage()?.model ?? ""));
  const contextWindow = createMemo(() => modelInfo()?.contextWindow ?? modelInfo()?.maxContextWindow ?? usage()?.contextWindow ?? null);
  const contextLabel = createMemo(() => `${formatTokenCount(usage()?.latestContextTokens)} / ${formatTokenCount(contextWindow())}`);
  const contextWidth = createMemo(() => `${contextPercent(usage()?.latestContextTokens, contextWindow())}%`);
  const costLabel = createMemo(() => formatCost(usage()));
  const visibleAgents = createMemo(() => props.agents.filter((agent) => activeAgentStatuses.has(agent.status)));
  const capabilityCount = createMemo(() =>
    (props.runtime?.activeSkills.length ?? 0) + props.activeMcpServers.length + props.activeLspServers.length,
  );
  const efforts = createMemo(() => modelInfo()?.reasoningEfforts ?? []);
  const waitingPhase = createMemo(() => activeWaitingPhase(props.activeInteraction));
  const visiblePhase = createMemo(() => waitingPhase() ?? props.turnPhase ?? "idle");

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
        <label class="status-select">
          <Show when={props.currentMode === "plan"} fallback={<Bot size={14} />}>
            <Brain size={14} />
          </Show>
          <select
            value={props.currentMode}
            disabled={props.busy || !props.selectedSession}
            onChange={(event) => props.onSetSessionMode(event.currentTarget.value as CompileMode)}
            aria-label="Session mode"
          >
            <For each={modes}>
              {(mode) => <option value={mode}>{mode === "auto" ? "Auto" : "Plan"}</option>}
            </For>
          </select>
        </label>

        <label class="status-select wide">
          <Cpu size={14} />
          <select
            value={`${plannerRole()?.provider ?? ""}::${plannerRole()?.model ?? ""}`}
            onChange={(event) => selectModel(event.currentTarget.value)}
            aria-label="Planner model"
          >
            <For each={props.providers}>
              {(provider) => (
                <optgroup label={provider.name || provider.id}>
                  <For each={allModels(provider)}>
                    {(model) => <option value={`${provider.id}::${model.slug}`}>{model.displayName || model.slug}</option>}
                  </For>
                </optgroup>
              )}
            </For>
          </select>
        </label>

        <Show when={efforts().length > 0}>
          <label class="status-select compact">
            <Clock size={14} />
            <select
              value={plannerRole()?.effort ?? efforts()[0] ?? ""}
              onChange={(event) => selectEffort(event.currentTarget.value)}
              aria-label="Reasoning effort"
            >
              <For each={efforts()}>
                {(effort) => <option value={effort}>{effort}</option>}
              </For>
            </select>
          </label>
        </Show>

        <label class="status-select wide">
          <ShieldCheck size={14} />
          <select
            value={props.permissionMode}
            onChange={(event) => props.onSavePermissionMode(event.currentTarget.value as PermissionMode)}
            aria-label="Permission mode"
          >
            <For each={permissionModes}>
              {(mode) => <option value={mode}>{permissionLabel(mode)}</option>}
            </For>
          </select>
        </label>
      </div>

      <div class="status-readouts">
        <StatusDetails icon={<Clock size={14} />} label={phaseLabel(visiblePhase(), props.turnStartedAt)}>
          <StatusLine label="Phase" value={visiblePhase()} />
          <StatusLine label="Busy" value={props.busy ? "yes" : "no"} />
          <StatusLine label="Blocked" value={waitingPhase() ? "yes" : "no"} />
          <Show when={waitingPhase()}>
            {(phase) => <StatusLine label="Waiting for" value={interactionLabel(phase())} />}
          </Show>
        </StatusDetails>

        <StatusDetails label={contextLabel()}>
          <div class="context-meter"><span style={{ width: contextWidth() }} /></div>
          <StatusLine label="Input" value={formatTokenCount(usage()?.promptTokens)} />
          <StatusLine label="Output" value={formatTokenCount(usage()?.completionTokens)} />
          <StatusLine label="Cache read" value={formatTokenCount(usage()?.cachedPromptTokens)} />
          <StatusLine label="Cache hit" value={formatPercent(usage()?.cacheHitRate)} />
        </StatusDetails>

        <StatusDetails icon={<DollarSign size={14} />} label={costLabel()}>
          <For each={pricedCosts(usage()?.estimatedCosts)}>
            {(cost) => <StatusLine label={cost.currency} value={formatCostNumber(cost.amount)} />}
          </For>
          <Show when={usage()?.hasUnpricedUsage}>
            <StatusLine label="Unpriced" value="yes" />
          </Show>
        </StatusDetails>

        <StatusDetails icon={<Boxes size={14} />} label={String(capabilityCount())}>
          <StatusGroup title="Skills" items={props.runtime?.activeSkills ?? []} />
          <div class="status-list-title">MCP</div>
          <For each={props.mcpServers} fallback={<span class="status-muted">None</span>}>
            {(server) => <McpRow server={server} active={props.activeMcpServers.includes(server.id)} />}
          </For>
          <div class="status-list-title">LSP</div>
          <For each={props.lspServers} fallback={<span class="status-muted">None</span>}>
            {(server) => <LspRow server={server} active={props.activeLspServers.includes(server.id)} />}
          </For>
        </StatusDetails>

        <StatusDetails icon={<Users size={14} />} label={String(visibleAgents().length)}>
          <For each={visibleAgents()} fallback={<span class="status-muted">No active subagents</span>}>
            {(agent) => <AgentRow agent={agent} />}
          </For>
        </StatusDetails>
      </div>
    </div>
  );
}

function StatusDetails(props: { icon?: JSX.Element; label: string; children: JSX.Element }) {
  return (
    <details class="status-details">
      <summary>
        {props.icon}
        <span>{props.label}</span>
      </summary>
      <div class="status-popover">{props.children}</div>
    </details>
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
      <Show when={props.items.length > 0} fallback={<span class="status-muted">None</span>}>
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
      <small>{props.server.activityKind === "idle" ? props.server.availabilityKind : props.server.activityKind}</small>
    </div>
  );
}

function McpRow(props: { server: McpServerRecord; active: boolean }) {
  return (
    <div class="status-list-item">
      <Circle size={8} class={mcpDotClass(props.server, props.active)} />
      <span>{props.server.id}</span>
      <small>{props.active ? `${props.server.availabilityKind} active` : props.server.availabilityKind}</small>
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

function findModel(providers: ProviderRecord[], slug: string) {
  for (const provider of providers) {
    const model = allModels(provider).find((item) => item.slug === slug);
    if (model) return model;
  }
}

function allModels(provider: ProviderRecord) {
  const seen = new Set<string>();
  return [...provider.defaultModels, ...provider.customModels, ...provider.models].filter((model) => {
    if (seen.has(model.slug)) return false;
    seen.add(model.slug);
    return true;
  });
}

function formatTokenCount(value: number | null | undefined) {
  if (!value) return "0";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k`;
  return String(value);
}

function formatPercent(value: number | null | undefined) {
  if (value === null || value === undefined) return "-";
  return `${Math.round(value * 100)}%`;
}

function contextPercent(tokens: number | null | undefined, contextWindow: number | null | undefined) {
  if (!tokens || !contextWindow) return 0;
  return Math.max(0, Math.min(100, Math.round((tokens / contextWindow) * 100)));
}

function pricedCosts(costs: RuntimeUsage["estimatedCosts"] | undefined) {
  return (costs ?? []).filter((cost) => Number.isFinite(cost.amount));
}

function formatCost(usage: RuntimeUsage | null) {
  const costs = pricedCosts(usage?.estimatedCosts);
  if (costs.length === 0) return usage?.hasUnpricedUsage ? "unpriced" : "no cost";
  return costs.map((cost) => `${cost.currency} ${formatCostNumber(cost.amount)}`).join(" / ");
}

function formatCostNumber(value: number) {
  if (value === 0) return "0";
  if (Math.abs(value) < 0.01) return value.toFixed(4);
  return value.toFixed(2);
}

function permissionLabel(mode: PermissionMode) {
  switch (mode) {
    case "request-approval":
      return "Request approval";
    case "auto-review":
      return "Auto review";
    case "full-access":
      return "Full access";
  }
}

function phaseLabel(phase: string | undefined, startedAt: number | null | undefined) {
  if (!phase || phase === "idle") return "Idle";
  if (isWaitingPhase(phase)) return `Blocked: ${interactionLabel(phase)}`;
  if (!startedAt) return phase;
  const elapsed = Math.max(0, Math.floor(Date.now() / 1000) - startedAt);
  return `${phase} ${elapsed}s`;
}

export type WaitingPhase = "toolApproval" | "userInput" | "planConfirmation";

export function activeWaitingPhase(interaction: InteractionRequest | null | undefined): WaitingPhase | null {
  if (!interaction || interaction.status !== "pending") return null;
  switch (interaction.kind) {
    case "toolApproval":
      return "toolApproval";
    case "userInput":
      return "userInput";
    case "planConfirmation":
      return "planConfirmation";
  }
}

function isWaitingPhase(phase: string): phase is WaitingPhase {
  return phase === "toolApproval" || phase === "userInput" || phase === "planConfirmation";
}

function interactionLabel(phase: WaitingPhase) {
  switch (phase) {
    case "toolApproval":
      return "tool approval";
    case "userInput":
      return "user input";
    case "planConfirmation":
      return "plan confirmation";
  }
}

function lspDotClass(server: LspServerRecord) {
  if (server.availabilityKind === "available") return "lsp-dot available";
  if (server.availabilityKind === "checking") return "lsp-dot checking";
  return "lsp-dot unavailable";
}

function mcpDotClass(server: McpServerRecord, active: boolean) {
  if (active && server.availabilityKind === "available") return "lsp-dot available";
  if (server.availabilityKind === "checking") return "lsp-dot checking";
  return "lsp-dot unavailable";
}

function agentStatusLabel(agent: AgentDto) {
  const cost = formatCost(agent.runtimeUsage ?? null);
  return cost === "no cost" ? agent.status : `${agent.status} / ${cost}`;
}
