// SPDX-License-Identifier: MIT
// Adapted from opencode packages/ui/src/components/markdown-stream.ts.

import { marked, type Tokens } from "marked";
import remend from "remend";

export type MarkdownStreamBlock = {
  raw: string;
  src: string;
  mode: "full" | "live";
};

function hasReferences(text: string) {
  return /^\[[^\]]+\]:\s+\S+/m.test(text) || /^\[\^[^\]]+\]:\s+/m.test(text);
}

function hasOpenFence(raw: string) {
  const match = raw.match(/^[ \t]{0,3}(`{3,}|~{3,})/);
  if (!match) return false;
  const mark = match[1];
  if (!mark) return false;
  const char = mark[0];
  const size = mark.length;
  const lines = raw.trimEnd().split("\n");
  const last = lines.length > 0 ? (lines[lines.length - 1]?.trim() ?? "") : "";
  return !new RegExp(`^[\\t ]{0,3}${char}{${size},}[\\t ]*$`).test(last);
}

function heal(text: string) {
  return remend(text, { linkMode: "text-only" });
}

export function streamMarkdown(text: string, live: boolean): MarkdownStreamBlock[] {
  if (!live) return [{ raw: text, src: text, mode: "full" }];
  const src = heal(text);
  if (hasReferences(text)) return [{ raw: text, src, mode: "live" }];

  const tokens = marked.lexer(text);
  let tail = -1;
  for (let index = tokens.length - 1; index >= 0; index -= 1) {
    if (tokens[index]?.type !== "space") {
      tail = index;
      break;
    }
  }
  if (tail < 0) return [{ raw: text, src, mode: "live" }];

  const last = tokens[tail];
  if (!last || last.type !== "code") return [{ raw: text, src, mode: "live" }];
  const code = last as Tokens.Code;
  if (!hasOpenFence(code.raw)) return [{ raw: text, src, mode: "live" }];

  const head = tokens
    .slice(0, tail)
    .map((token) => token.raw)
    .join("");
  if (!head) return [{ raw: code.raw, src: code.raw, mode: "live" }];
  return [
    { raw: head, src: heal(head), mode: "live" },
    { raw: code.raw, src: code.raw, mode: "live" },
  ];
}
