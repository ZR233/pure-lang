import type { InteractionRequest, PlanState, TimelineItem } from "../types";
import type { StudioState } from "./studio-state";

export type ToolGroupSummaryKind =
  | "readFiles"
  | "editFiles"
  | "runCommands"
  | "coordinateAgents"
  | "useTools";

export type ToolGroupSummaryPart = {
  kind: ToolGroupSummaryKind;
  count: number;
};

export type TimelineEntry =
  | {
      kind: "message";
      key: string;
      role: "user" | "assistant";
      content: string;
      attachments?: TimelineItem["attachments"];
    }
  | {
      kind: "commentary";
      key: string;
      content: string;
      status: TimelineItem["status"];
      item: TimelineItem;
    }
  | { kind: "plan"; key: string; content: string; item: TimelineItem; planState?: PlanState }
  | {
      kind: "thought";
      key: string;
      content: string;
      status: TimelineItem["status"];
      startedAt: number;
      updatedAt: number;
      durationSeconds: number;
    }
  | {
      kind: "status";
      key: string;
      status: TimelineItem["status"];
      content: string;
      item: TimelineItem;
    }
  | { kind: "tool"; key: string; item: TimelineItem }
  | {
      kind: "toolGroup";
      key: string;
      turnId: string;
      items: TimelineItem[];
      summaryParts: ToolGroupSummaryPart[];
      status: TimelineItem["status"];
    }
  | { kind: "agent"; key: string; item: TimelineItem }
  | { kind: "trace"; key: string; item: TimelineItem };

type ThoughtEntry = Extract<TimelineEntry, { kind: "thought" }>;
type ToolGroupEntry = Extract<TimelineEntry, { kind: "toolGroup" }>;

export function selectSelectedProject(state: StudioState) {
  return state.projects.find((project) => project.id === state.selectedProjectId) ?? null;
}

export function selectSelectedSession(state: StudioState) {
  return state.sessions.find((session) => session.id === state.selectedSessionId) ?? null;
}

export function selectActiveInteraction(state: StudioState): InteractionRequest | null {
  return state.activeInteractionId
    ? state.interactions.get(state.activeInteractionId) ?? null
    : null;
}

export function selectTimelineEntries(state: StudioState): TimelineEntry[] {
  const entries: TimelineEntry[] = [];
  let activeThought: ThoughtEntry | null = null;
  let activeThoughtTurnId: string | null = null;
  let activeToolGroup: ToolGroupEntry | null = null;

  const closeThought = () => {
    activeThought = null;
    activeThoughtTurnId = null;
  };

  const closeToolGroup = () => {
    activeToolGroup = null;
  };

  const closeDisplaySegment = () => {
    closeThought();
    closeToolGroup();
  };

  const closeCrossTurnSegment = (turnId: string) => {
    const thoughtCrossedTurn = activeThoughtTurnId !== null && activeThoughtTurnId !== turnId;
    const toolCrossedTurn = activeToolGroup !== null && activeToolGroup.turnId !== turnId;
    if (thoughtCrossedTurn || toolCrossedTurn) {
      closeDisplaySegment();
    }
  };

  const appendThinkingItem = (item: TimelineItem) => {
    closeCrossTurnSegment(item.turnId);
    const content = thinkingContent(item);
    if (!content.trim()) return;
    if (activeThought && activeThoughtTurnId === item.turnId) {
      activeThought.content = appendThoughtContent(activeThought.content, content);
      activeThought.status = mergeThoughtStatus(activeThought.status, item.status);
      activeThought.startedAt = Math.min(activeThought.startedAt, item.createdAt);
      activeThought.updatedAt = Math.max(activeThought.updatedAt, item.updatedAt);
      activeThought.durationSeconds = thoughtDurationSeconds(activeThought.startedAt, activeThought.updatedAt);
      return;
    }
    activeThought = {
      kind: "thought",
      key: `thinking-${item.itemId}`,
      content,
      status: item.status,
      startedAt: item.createdAt,
      updatedAt: item.updatedAt,
      durationSeconds: thoughtDurationSeconds(item.createdAt, item.updatedAt),
    };
    activeThoughtTurnId = item.turnId;
    entries.push(activeThought);
  };

  const appendToolItem = (item: TimelineItem) => {
    closeCrossTurnSegment(item.turnId);
    if (!activeToolGroup || activeToolGroup.turnId !== item.turnId) {
      activeToolGroup = buildToolGroupEntry([item]);
      entries.push(activeToolGroup);
      return;
    }
    activeToolGroup.items.push(item);
    refreshToolGroupEntry(activeToolGroup);
  };

  for (const itemId of state.timelineOrder) {
    const rawItem = state.timelineItems.get(itemId);
    const item = rawItem ? normalizeTimelineItem(rawItem) : null;
    if (!item) continue;
    switch (item.kind) {
      case "text":
        closeDisplaySegment();
        if (!item.content.trim() && !(item.attachments?.length)) break;
        if (item.textChannel === "user" || item.textChannel === "final") {
          entries.push({
            kind: "message",
            key: `text-${item.itemId}`,
            role: item.textChannel === "user" ? "user" : "assistant",
            content: item.content,
            attachments: item.attachments,
          });
          break;
        }
        if (item.textChannel === "commentary") {
          entries.push({
            kind: "commentary",
            key: `commentary-${item.itemId}`,
            content: item.content,
            status: item.status,
            item,
          });
        }
        break;
      case "plan":
        closeDisplaySegment();
        if (!item.content.trim()) break;
        entries.push({
          kind: "plan",
          key: `plan-${item.itemId}`,
          content: item.content,
          item,
          planState: state.planStates.get(item.itemId),
        });
        break;
      case "thinking": {
        appendThinkingItem(item);
        break;
      }
      case "tool":
        appendToolItem(item);
        break;
      case "agent":
        closeDisplaySegment();
        entries.push({ kind: "agent", key: `agent-${item.itemId}`, item });
        break;
      case "turn":
        closeDisplaySegment();
        if (shouldShowStatusEntry(item)) {
          entries.push({
            kind: "status",
            key: `status-${item.itemId}`,
            status: item.status,
            content: item.content,
            item,
          });
          break;
        }
        if (shouldShowTurnTrace(item)) {
          entries.push({ kind: "trace", key: `trace-${item.itemId}`, item });
        }
        break;
      case "inference":
        break;
    }
  }
  return entries;
}

function shouldShowStatusEntry(item: TimelineItem): boolean {
  return item.itemId.startsWith("optimistic-") && item.status === "running";
}

function mergeThoughtStatus(
  current: TimelineItem["status"],
  next: TimelineItem["status"],
): TimelineItem["status"] {
  return toolStatusPriority(next) > toolStatusPriority(current) ? next : current;
}

function thoughtDurationSeconds(startedAt: number, updatedAt: number): number {
  return Math.max(0, updatedAt - startedAt);
}

function thinkingContent(item: TimelineItem): string {
  return item.thinkingChunks
    .slice()
    .sort((left, right) => left.chunkIndex - right.chunkIndex)
    .map((chunk) => chunk.content)
    .join("");
}

function appendThoughtContent(current: string, next: string): string {
  const left = current.trimEnd();
  const right = next.trimStart();
  if (!left) return next;
  if (!right) return current;
  return `${left}\n\n${right}`;
}

function shouldShowTurnTrace(item: TimelineItem): boolean {
  switch (item.status) {
    case "failed":
    case "interrupted":
    case "budgetLimited":
    case "denied":
      return true;
    case "started":
    case "streaming":
    case "awaitingApproval":
    case "approved":
    case "running":
    case "completed":
      return false;
  }
}

function buildToolGroupEntry(items: TimelineItem[]): Extract<TimelineEntry, { kind: "toolGroup" }> {
  const first = items[0];
  const last = items[items.length - 1];
  return {
    kind: "toolGroup",
    key: `tool-group-${first.itemId}-${last.itemId}`,
    turnId: first.turnId,
    items,
    summaryParts: summarizeToolGroup(items),
    status: aggregateToolStatus(items),
  };
}

function refreshToolGroupEntry(entry: ToolGroupEntry) {
  const first = entry.items[0];
  const last = entry.items[entry.items.length - 1];
  entry.key = `tool-group-${first.itemId}-${last.itemId}`;
  entry.summaryParts = summarizeToolGroup(entry.items);
  entry.status = aggregateToolStatus(entry.items);
}

function summarizeToolGroup(items: TimelineItem[]): ToolGroupSummaryPart[] {
  const counts: Record<ToolGroupSummaryKind, number> = {
    readFiles: 0,
    editFiles: 0,
    runCommands: 0,
    coordinateAgents: 0,
    useTools: 0,
  };
  for (const item of items) {
    const category = toolCategory(item.tool?.name);
    counts[category] += toolSummaryCount(item, category);
  }
  return (["readFiles", "editFiles", "runCommands", "coordinateAgents", "useTools"] as const)
    .filter((kind) => counts[kind] > 0)
    .map((kind) => ({ kind, count: counts[kind] }));
}

function aggregateToolStatus(items: TimelineItem[]): TimelineItem["status"] {
  return items.reduce<TimelineItem["status"]>((status, item) => {
    return toolStatusPriority(item.status) > toolStatusPriority(status) ? item.status : status;
  }, "completed");
}

function toolStatusPriority(status: TimelineItem["status"]): number {
  switch (status) {
    case "failed":
    case "denied":
    case "interrupted":
    case "budgetLimited":
      return 3;
    case "awaitingApproval":
      return 2;
    case "started":
    case "streaming":
    case "running":
    case "approved":
      return 1;
    case "completed":
    default:
      return 0;
  }
}

function toolCategory(name: string | null | undefined): ToolGroupSummaryKind {
  const normalized = name?.toLowerCase();
  if (
    normalized &&
    ["read_file", "list_files", "list_file", "search_files", "stat_path"].includes(normalized)
  ) {
    return "readFiles";
  }
  if (
    normalized &&
    [
      "write_file",
      "create_directory",
      "delete_path",
      "copy_path",
      "move_path",
      "apply_patch",
      "skill_manage",
    ].includes(normalized)
  ) {
    return "editFiles";
  }
  if (normalized === "bash") {
    return "runCommands";
  }
  if (
    normalized &&
    [
      "subagent",
      "spawn_agent",
      "wait_agent",
      "list_agents",
      "send_message",
      "followup_task",
      "close_agent",
    ].includes(normalized)
  ) {
    return "coordinateAgents";
  }
  return "useTools";
}

function toolSummaryCount(item: TimelineItem, category: ToolGroupSummaryKind): number {
  if (category === "runCommands" || category === "coordinateAgents" || category === "useTools") {
    return 1;
  }
  const name = item.tool?.name?.toLowerCase();
  if (name === "apply_patch") {
    return applyPatchFileCount(item.tool?.result, item.tool?.arguments);
  }
  return Math.max(toolArgumentPaths(item.tool?.arguments).length, 1);
}

function applyPatchFileCount(
  resultText: string | null | undefined,
  argumentsText: string | null | undefined,
): number {
  const resultPaths = new Set<string>();
  for (const line of resultText?.split(/\r?\n/) ?? []) {
    const match = line.trim().match(/^[AMD]\s+(.+)$/);
    if (match?.[1]?.trim()) {
      resultPaths.add(match[1].trim());
    }
  }
  if (resultPaths.size > 0) {
    return resultPaths.size;
  }

  const args = parseToolArguments(argumentsText);
  const patch = args?.patch ?? args?.input;
  if (typeof patch !== "string") {
    return 1;
  }
  const patchPaths = new Set<string>();
  for (const line of patch.split(/\r?\n/)) {
    const match = line
      .trim()
      .match(/^\*\*\* (?:Add File|Update File|Delete File):\s+(.+)$/);
    if (match?.[1]?.trim()) {
      patchPaths.add(match[1].trim());
    }
  }
  return Math.max(patchPaths.size, 1);
}

function toolArgumentPaths(argumentsText: string | null | undefined): string[] {
  const args = parseToolArguments(argumentsText);
  if (!args) return [];
  const values = [
    args.path,
    args.paths,
    args.filePath,
    args.file_path,
    args.targetPath,
    args.target_path,
    args.directory,
    args.root,
    args.from,
    args.to,
    args.projectDir,
    args.project_dir,
  ];
  return [...new Set(values.flatMap(pathStrings))];
}

function pathStrings(value: unknown): string[] {
  if (typeof value === "string" && value.trim()) {
    return [value.trim()];
  }
  if (Array.isArray(value)) {
    return value.flatMap(pathStrings);
  }
  return [];
}

function parseToolArguments(argumentsText: string | null | undefined): Record<string, unknown> | null {
  if (!argumentsText?.trim()) return null;
  try {
    const parsed = JSON.parse(argumentsText);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function normalizeTimelineItem(item: TimelineItem): TimelineItem {
  return {
    ...item,
    content: item.content ?? "",
    thinkingChunks: item.thinkingChunks ?? [],
    tool: item.tool ?? null,
    agent: item.agent ?? null,
    inference: item.inference ?? null,
    usage: item.usage ?? null,
  };
}
