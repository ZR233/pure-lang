import type { ChatItem } from "../types";
import type { StudioStateLike } from "./types";

export function selectSelectedProject(state: StudioStateLike) {
  return state.projects.find((project) => project.id === state.selectedProjectId) ?? null;
}

export function selectSelectedSession(state: StudioStateLike) {
  return state.sessions.find((session) => session.id === state.selectedSessionId) ?? null;
}

export function selectChatItems(
  state: StudioStateLike,
  thinkingFallbackText: string,
): ChatItem[] {
  const items: ChatItem[] = [];
  let index = 0;

  for (const msg of state.messages) {
    if (msg.role === "tool" && msg.metadata?.tool_call_id) {
      items.push({
        kind: "tool_call",
        toolCall: {
          id: msg.metadata.tool_call_id,
          name: msg.metadata.tool_name ?? "tool",
          arguments: msg.metadata.tool_call_arguments ?? "",
          status: "result_ready",
          result: msg.content,
          startedAt: 0,
        },
        key: `tc-${msg.metadata.tool_call_id}`,
      });
    } else {
      items.push({ kind: "message", message: msg, key: `msg-${index}` });
    }
    index++;
  }

  for (const tc of state.toolCalls.values()) {
    if (!items.some((item) => item.kind === "tool_call" && item.key === `tc-${tc.id}`)) {
      items.push({ kind: "tool_call", toolCall: tc, key: `tc-${tc.id}` });
    }
  }

  if (state.thinkingText || state.streamingText) {
    items.push({
      kind: "message",
      message: {
        role: "assistant",
        content: state.streamingText || thinkingFallbackText,
        reasoningContent: state.thinkingText || null,
      },
      key: "streaming",
    });
  }

  return items;
}
