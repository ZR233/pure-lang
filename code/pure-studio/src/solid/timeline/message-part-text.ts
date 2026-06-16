// SPDX-License-Identifier: MIT
// Adapted from opencode packages/ui/src/components/message-part-text.ts.

import type { StudioPart } from "../../types";

export function readPartText(deltaText: string | undefined, part: { text?: string | null }) {
  return deltaText ?? part.text ?? "";
}

export function readPlanContent(deltaText: string | undefined, part: StudioPart) {
  return deltaText ?? part.plan?.content ?? part.text ?? "";
}

export function readToolArguments(deltaText: string | undefined, part: StudioPart) {
  if (!isToolResultOverlay(part, deltaText)) return deltaText ?? part.tool?.arguments ?? "";
  return part.tool?.arguments ?? "";
}

export function readToolResult(deltaText: string | undefined, part: StudioPart) {
  if (isToolResultOverlay(part, deltaText)) return deltaText ?? part.tool?.result ?? "";
  return part.tool?.result ?? "";
}

export function reasoningHeading(text: string) {
  const markdown = text.replace(/\r\n?/g, "\n");
  const html = markdown.match(/<h[1-6][^>]*>([\s\S]*?)<\/h[1-6]>/i);
  if (html?.[1]) {
    const value = cleanHeading(html[1].replace(/<[^>]+>/g, " "));
    if (value) return value;
  }

  const atx = markdown.match(/^\s{0,3}#{1,6}[ \t]+(.+?)(?:[ \t]+#+[ \t]*)?$/m);
  if (atx?.[1]) {
    const value = cleanHeading(atx[1]);
    if (value) return value;
  }

  const setext = markdown.match(/^([^\n]+)\n(?:=+|-+)\s*$/m);
  if (setext?.[1]) {
    const value = cleanHeading(setext[1]);
    if (value) return value;
  }

  const strong = markdown.match(/^\s*(?:\*\*|__)(.+?)(?:\*\*|__)\s*$/m);
  if (strong?.[1]) {
    const value = cleanHeading(strong[1]);
    if (value) return value;
  }
}

function cleanHeading(value: string) {
  return value
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]+\)/g, "$1")
    .replace(/[*_~]+/g, "")
    .trim();
}

function isToolResultOverlay(part: StudioPart, deltaText: string | undefined) {
  if (deltaText === undefined) return false;
  if (part.partType !== "tool") return false;
  if (part.tool?.result !== undefined && part.tool.result !== null) return true;
  return part.status === "running"
    || part.status === "completed"
    || part.status === "failed"
    || part.status === "interrupted"
    || part.status === "budgetLimited";
}
