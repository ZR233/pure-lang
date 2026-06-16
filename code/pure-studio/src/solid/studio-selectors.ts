import type { MessageStore } from "./studio-store";
import type { InteractionRequest, SessionRuntime } from "../types";

export type SelectedSessionView = {
  sessionId: string | null;
  session: MessageStore["sessions"][number] | undefined;
  messages: NonNullable<MessageStore["messages"][string]>;
  runtime: SessionRuntime | null | undefined;
  agents: NonNullable<MessageStore["agents"][string]>;
  activeInteraction: InteractionRequest | null;
  busy: boolean;
  turnPhase: string | undefined;
  turnStartedAt: number | null | undefined;
  activeMcpServers: string[];
  activeLspServers: string[];
};

export function selectedSessionView(store: MessageStore): SelectedSessionView {
  const sessionId = store.selectedSessionId;
  const runtime = sessionId ? store.sessionRuntime[sessionId] : null;
  return {
    sessionId,
    session: store.sessions.find((session) => session.id === sessionId),
    messages: sessionId ? store.messages[sessionId] ?? [] : [],
    runtime,
    agents: sessionId ? store.agents[sessionId] ?? [] : [],
    activeInteraction: store.activeInteractionId ? store.interactions[store.activeInteractionId] ?? null : null,
    busy: selectedSessionBusy(store, sessionId),
    turnPhase: sessionId ? store.turnPhase[sessionId] : undefined,
    turnStartedAt: sessionId ? store.turnStartedAt[sessionId] : null,
    activeMcpServers: runtime?.activeMcpServers ?? [],
    activeLspServers: runtime?.activeLspServers ?? [],
  };
}

export function visibleProjectSessions(store: MessageStore) {
  const selectedProjectId = store.selectedProjectId;
  const byId = new Map<string, MessageStore["sessions"][number]>();
  for (const session of store.sessions) {
    if (session.projectId !== selectedProjectId || session.visibility !== "active" || session.parentSessionId) continue;
    const existing = byId.get(session.id);
    if (!existing || existing.updatedAt < session.updatedAt) byId.set(session.id, session);
  }
  return [...byId.values()].sort(compareSessions);
}

export function selectedSessionBusy(store: Pick<MessageStore, "sessionBusy">, sessionId: string | null) {
  return sessionId ? store.sessionBusy[sessionId] ?? false : false;
}

function compareSessions(left: MessageStore["sessions"][number], right: MessageStore["sessions"][number]) {
  if (left.updatedAt !== right.updatedAt) return right.updatedAt - left.updatedAt;
  return left.id.localeCompare(right.id);
}
