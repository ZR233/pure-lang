import type {
  AgentDto,
  InteractionRequest,
  LspServerRecord,
  McpServerRecord,
  ProviderRecord,
  RuntimeUsage,
} from "../../types";
import i18n from "../../i18n";

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

export function findModel(providers: ProviderRecord[], slug: string) {
  for (const provider of providers) {
    const model = allModels(provider).find((item) => item.slug === slug);
    if (model) return model;
  }
}

export function allModels(provider: ProviderRecord) {
  const seen = new Set<string>();
  return [...provider.defaultModels, ...provider.customModels, ...provider.models].filter((model) => {
    if (seen.has(model.slug)) return false;
    seen.add(model.slug);
    return true;
  });
}

export function formatTokenCount(value: number | null | undefined) {
  if (!value) return "0";
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k`;
  return String(value);
}

export function formatPercent(value: number | null | undefined) {
  if (value === null || value === undefined) return i18n.t("common.notAvailable");
  return `${Math.round(value * 100)}%`;
}

export function contextPercent(tokens: number | null | undefined, contextWindow: number | null | undefined) {
  if (!tokens || !contextWindow) return 0;
  return Math.max(0, Math.min(100, Math.round((tokens / contextWindow) * 100)));
}

export function pricedCosts(costs: RuntimeUsage["estimatedCosts"] | undefined) {
  return (costs ?? []).filter((cost) => Number.isFinite(cost.amount));
}

export function formatCost(usage: RuntimeUsage | null) {
  const costs = pricedCosts(usage?.estimatedCosts);
  if (costs.length === 0) return usage?.hasUnpricedUsage ? i18n.t("statusBar.costUnpriced") : i18n.t("statusBar.noCost");
  return costs.map((cost) => `${cost.currency} ${formatCostNumber(cost.amount)}`).join(" / ");
}

export function formatCostNumber(value: number) {
  if (value === 0) return "0";
  if (Math.abs(value) < 0.01) return value.toFixed(4);
  return value.toFixed(2);
}

export function phaseLabel(phase: string | undefined, startedAt: number | null | undefined) {
  if (!phase || phase === "idle") return i18n.t("turnPhase.idle");
  if (isWaitingPhase(phase)) return i18n.t("statusBar.blockedPhase", { phase: interactionLabel(phase) });
  if (!startedAt) return phaseName(phase);
  return i18n.t("statusBar.phaseElapsed", { phase: phaseName(phase), seconds: elapsedSeconds(startedAt) });
}

export function phaseSummaryLabel(phase: string | undefined, startedAt: number | null | undefined) {
  if (!phase || phase === "idle") return i18n.t("turnPhase.idle");
  if (isWaitingPhase(phase)) return i18n.t("statusBar.blocked");
  if (!startedAt) return phaseName(phase);
  return i18n.t("statusBar.phaseElapsed", { phase: phaseName(phase), seconds: elapsedSeconds(startedAt) });
}

export function elapsedLabel(phase: string | undefined, startedAt: number | null | undefined) {
  if (!phase || phase === "idle" || !startedAt) return i18n.t("common.notAvailable");
  return i18n.t("statusBar.seconds", { seconds: elapsedSeconds(startedAt) });
}

export function interactionLabel(phase: WaitingPhase) {
  switch (phase) {
    case "toolApproval":
      return i18n.t("approval.title");
    case "userInput":
      return i18n.t("askUser.awaiting");
    case "planConfirmation":
      return i18n.t("planConfirm.title");
  }
}

export function phaseName(phase: string | undefined) {
  if (!phase) return i18n.t("turnPhase.idle");
  return i18n.exists(`turnPhase.${phase}`) ? i18n.t(`turnPhase.${phase}`) : phase;
}

export function lspAvailabilityLabel(value: string) {
  const key = lspStatusKey(value);
  return i18n.exists(`statusBar.${key}`) ? i18n.t(`statusBar.${key}`) : value;
}

export function mcpAvailabilityLabel(value: string) {
  return i18n.exists(`settings.mcp.availability.${value}`) ? i18n.t(`settings.mcp.availability.${value}`) : value;
}

export function lspDotClass(server: LspServerRecord) {
  if (server.availabilityKind === "available") return "lsp-dot available";
  if (server.availabilityKind === "checking") return "lsp-dot checking";
  return "lsp-dot unavailable";
}

export function mcpDotClass(server: McpServerRecord, active: boolean) {
  if (active && server.availabilityKind === "available") return "lsp-dot available";
  if (server.availabilityKind === "checking") return "lsp-dot checking";
  return "lsp-dot unavailable";
}

export function agentStatusLabel(agent: AgentDto) {
  if (!hasVisibleCost(agent.runtimeUsage ?? null)) return agent.status;
  const cost = formatCost(agent.runtimeUsage ?? null);
  return `${agent.status} / ${cost}`;
}

function isWaitingPhase(phase: string): phase is WaitingPhase {
  return phase === "toolApproval" || phase === "userInput" || phase === "planConfirmation";
}

function elapsedSeconds(startedAt: number) {
  return Math.max(0, Math.floor(Date.now() / 1000) - startedAt);
}

function lspStatusKey(value: string) {
  switch (value) {
    case "available":
      return "lspReady";
    case "checking":
      return "lspChecking";
    case "unavailable":
      return "lspUnavailable";
    case "missingCommand":
      return "lspMissingCommand";
    case "disabled":
      return "lspDisabled";
    default:
      return value;
  }
}

function hasVisibleCost(usage: RuntimeUsage | null) {
  return pricedCosts(usage?.estimatedCosts).length > 0 || Boolean(usage?.hasUnpricedUsage);
}
