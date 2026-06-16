import { marked } from "marked";
import { For } from "solid-js";
import { streamMarkdown } from "./markdown-stream";

export function MarkdownContent(props: { content: string; class?: string; live?: boolean }) {
  const blocks = () => streamMarkdown(props.content || "", props.live === true);
  const html = (content: string) => marked.parse(content, { async: false }) as string;
  return (
    <div class={`markdown-content ${props.class ?? ""}`} data-live={props.live || undefined}>
      <For each={blocks()}>
        {(block) => <div class="markdown-block" data-markdown-block={block.mode} innerHTML={html(block.src)} />}
      </For>
    </div>
  );
}
