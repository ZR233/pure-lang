// SPDX-License-Identifier: MIT
// Adapted from opencode packages/ui/src/components/message-part.tsx.

import type { StudioPart, StudioPartStatus } from "../../types";
import { readPartText, readPlanContent } from "./message-part-text";

const CONTEXT_GROUP_TOOLS = new Set(["read_file", "list_files", "list_file", "search_files", "stat_path"]);
const HIDDEN_TOOLS = new Set(["todowrite"]);
const QUESTION_TOOLS = new Set(["question", "request_user_input"]);

export type PartRef = {
  messageID: string;
  partID: string;
};

export type PartGroup =
  | {
      key: string;
      type: "part";
      ref: PartRef;
    }
  | {
      key: string;
      type: "context";
      refs: PartRef[];
    };

function sameRef(a: PartRef, b: PartRef) {
  return a.messageID === b.messageID && a.partID === b.partID;
}

function sameGroup(a: PartGroup, b: PartGroup) {
  if (a === b) return true;
  if (a.key !== b.key || a.type !== b.type) return false;
  if (a.type === "part") {
    if (b.type !== "part") return false;
    return sameRef(a.ref, b.ref);
  }
  if (b.type !== "context") return false;
  if (a.refs.length !== b.refs.length) return false;
  return a.refs.every((ref, index) => sameRef(ref, b.refs[index]!));
}

export function sameGroups(a: readonly PartGroup[] | undefined, b: readonly PartGroup[] | undefined) {
  if (a === b) return true;
  if (!a || !b) return false;
  if (a.length !== b.length) return false;
  return a.every((group, index) => sameGroup(group, b[index]!));
}

export function groupParts(parts: { messageID: string; part: StudioPart }[]) {
  const result: PartGroup[] = [];
  let start = -1;

  const flush = (end: number) => {
    if (start < 0) return;
    const first = parts[start];
    if (!first) {
      start = -1;
      return;
    }
    result.push({
      key: `context:${first.part.partId}`,
      type: "context",
      refs: parts.slice(start, end + 1).map((item) => ({
        messageID: item.messageID,
        partID: item.part.partId,
      })),
    });
    start = -1;
  };

  parts.forEach((item, index) => {
    if (isContextGroupTool(item.part)) {
      if (start < 0) start = index;
      return;
    }
    flush(index - 1);
    result.push({
      key: `part:${item.messageID}:${item.part.partId}`,
      type: "part",
      ref: {
        messageID: item.messageID,
        partID: item.part.partId,
      },
    });
  });

  flush(parts.length - 1);
  return result;
}

export function renderable(part: StudioPart, showReasoningSummaries = true) {
  if (part.ignored) return false;
  if (part.partType === "tool") {
    const name = part.tool?.name?.toLowerCase() ?? "";
    if (HIDDEN_TOOLS.has(name)) return false;
    if (QUESTION_TOOLS.has(name) && isActiveStatus(part.status)) return false;
    return true;
  }
  if (part.partType === "text") return Boolean(part.text?.trim() || part.attachments?.length);
  if (part.partType === "reasoning") return showReasoningSummaries && Boolean(part.text?.trim());
  if (part.partType === "plan") return Boolean((part.plan?.content ?? part.text).trim());
  if (part.partType === "agent") return true;
  if (part.partType === "turn") return isTerminalStatus(part.status);
  return false;
}

export function renderableWithDelta(
  part: StudioPart,
  deltaText: string | undefined,
  showReasoningSummaries = true,
) {
  if (part.ignored) return false;
  if (part.partType === "tool") {
    const name = part.tool?.name?.toLowerCase() ?? "";
    if (HIDDEN_TOOLS.has(name)) return false;
    if (QUESTION_TOOLS.has(name) && isActiveStatus(part.status)) return false;
    return true;
  }
  if (part.partType === "text") return Boolean(readPartText(deltaText, part).trim() || part.attachments?.length);
  if (part.partType === "reasoning") return showReasoningSummaries && Boolean(readPartText(deltaText, part).trim());
  if (part.partType === "plan") return Boolean(readPlanContent(deltaText, part).trim());
  if (part.partType === "agent") return true;
  if (part.partType === "turn") return isTerminalStatus(part.status);
  return false;
}

export function partDefaultOpen(part: StudioPart, shell = false, edit = false) {
  if (part.partType !== "tool") return undefined;
  const name = part.tool?.name?.toLowerCase();
  if (name === "bash") return shell;
  if (name === "write_file" || name === "apply_patch" || name === "delete_path" || name === "move_path") return edit;
}

export function reasoningDefaultOpen(part: StudioPart, defaultOpen?: boolean) {
  return part.partType === "reasoning" && !isActiveStatus(part.status) && defaultOpen === true;
}

function isContextGroupTool(part: StudioPart) {
  return part.partType === "tool" && CONTEXT_GROUP_TOOLS.has(part.tool?.name?.toLowerCase() ?? "");
}

export function isActiveStatus(status: StudioPartStatus) {
  return status === "started" || status === "streaming" || status === "running" || status === "awaitingApproval" || status === "approved";
}

export function isTerminalStatus(status: StudioPartStatus) {
  return status === "failed" || status === "interrupted" || status === "budgetLimited" || status === "denied";
}

export function isTerminalProblem(status: StudioPartStatus) {
  return status === "failed" || status === "interrupted" || status === "budgetLimited" || status === "denied";
}

export function isQuestionTool(part: StudioPart) {
  return part.partType === "tool" && QUESTION_TOOLS.has(part.tool?.name?.toLowerCase() ?? "");
}
