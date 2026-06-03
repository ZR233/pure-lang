import type { TimelineItem } from "../types";
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
    }
  | { kind: "thought"; key: string; content: string }
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

export function selectSelectedProject(state: StudioState) {
  return state.projects.find((project) => project.id === state.selectedProjectId) ?? null;
}

export function selectSelectedSession(state: StudioState) {
  return state.sessions.find((session) => session.id === state.selectedSessionId) ?? null;
}

export function selectTimelineEntries(state: StudioState): TimelineEntry[] {
  const entries: TimelineEntry[] = [];
  let toolGroup: TimelineItem[] = [];

  const flushToolGroup = () => {
    if (toolGroup.length === 0) return;
    entries.push(buildToolGroupEntry(toolGroup));
    toolGroup = [];
  };

  for (const itemId of state.timelineOrder) {
    const rawItem = state.timelineItems.get(itemId);
    const item = rawItem ? normalizeTimelineItem(rawItem) : null;
    if (!item) continue;
    switch (item.kind) {
      case "text":
        flushToolGroup();
        if (!item.content.trim()) break;
        entries.push({
          kind: "message",
          key: `text-${item.itemId}`,
          role: item.role === "user" ? "user" : "assistant",
          content: item.content,
        });
        break;
      case "thinking": {
        flushToolGroup();
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
        if (toolGroup.length > 0 && toolGroup[0].turnId !== item.turnId) {
          flushToolGroup();
        }
        toolGroup.push(item);
        break;
      case "agent":
        flushToolGroup();
        entries.push({ kind: "agent", key: `agent-${item.itemId}`, item });
        break;
      case "turn":
        flushToolGroup();
        if (shouldShowTurnTrace(item)) {
          entries.push({ kind: "trace", key: `trace-${item.itemId}`, item });
        }
        break;
      case "inference":
        flushToolGroup();
        break;
    }
  }
  flushToolGroup();
  return entries;
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
