import type { TimelineItem } from "../types";
import type { StudioState } from "./studio-state";

export type TimelineEntry =
  | {
      kind: "message";
      key: string;
      role: "user" | "assistant";
      content: string;
    }
  | { kind: "thought"; key: string; content: string }
  | { kind: "tool"; key: string; item: TimelineItem }
  | { kind: "agent"; key: string; item: TimelineItem }
  | { kind: "trace"; key: string; item: TimelineItem };

export function selectSelectedProject(state: StudioState) {
  return state.projects.find((project) => project.id === state.selectedProjectId) ?? null;
}

export function selectSelectedSession(state: StudioState) {
  return state.sessions.find((session) => session.id === state.selectedSessionId) ?? null;
}

export function selectTimelineEntries(state: StudioState): TimelineEntry[] {
  const entries: TimelineEntry[] = [];
  for (const itemId of state.timelineOrder) {
    const item = state.timelineItems.get(itemId);
    if (!item) continue;
    switch (item.kind) {
      case "text":
        if (!item.content.trim()) break;
        entries.push({
          kind: "message",
          key: `text-${item.itemId}`,
          role: item.role === "user" ? "user" : "assistant",
          content: item.content,
        });
        break;
      case "thinking": {
        const content = item.thinkingChunks
          .slice()
          .sort((left, right) => left.chunkIndex - right.chunkIndex)
          .map((chunk) => chunk.content)
          .join("");
        if (content.trim()) {
          entries.push({ kind: "thought", key: `thinking-${item.itemId}`, content });
        }
        break;
      }
      case "tool":
        entries.push({ kind: "tool", key: `tool-${item.itemId}`, item });
        break;
      case "agent":
        entries.push({ kind: "agent", key: `agent-${item.itemId}`, item });
        break;
      case "turn":
      case "inference":
        entries.push({ kind: "trace", key: `trace-${item.itemId}`, item });
        break;
    }
  }
  return entries;
}
